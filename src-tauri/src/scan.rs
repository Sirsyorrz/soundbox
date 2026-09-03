use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use anyhow::Result;
use rayon::prelude::*;
use walkdir::WalkDir;

use crate::db::{Db, FileRow};
use crate::{analysis, audio, cache, ident};

/// mp4 and its relatives are video containers, but the audio track decodes the
/// same way and clips are often filed as mp4.
pub const AUDIO_EXTS: &[&str] =
    &["wav", "flac", "mp3", "ogg", "oga", "opus", "m4a", "aac", "alac", "mp4", "m4v", "mov"];

#[derive(Debug, Default, Clone)]
pub struct ScanStats {
    pub total: usize,
    pub analysed: usize,
    pub skipped: usize,
    pub failed: usize,
    pub cancelled: bool,
    pub elapsed_ms: u128,
    /// (path, reason) for files that produced no row at all. These are retried
    /// on every scan, so they must be surfaced rather than silently counted.
    pub errors: Vec<(String, String)>,
}

pub struct Progress {
    pub done: usize,
    pub total: usize,
    pub path: String,
}

pub fn candidates(root: &Path) -> Vec<PathBuf> {
    WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .map(|e| e.into_path())
        .filter(|p| {
            p.extension()
                .and_then(|x| x.to_str())
                .map(|x| AUDIO_EXTS.contains(&x.to_ascii_lowercase().as_str()))
                .unwrap_or(false)
        })
        .collect()
}

enum Outcome {
    Skipped,
    Row(Box<FileRow>),
}

fn analyse(
    root: &Path,
    root_id: i64,
    path: &Path,
    cache_dir: &Path,
    db_snapshot: Option<(u64, i64, u16)>,
) -> Result<Outcome> {
    let meta = std::fs::metadata(path)?;
    let size = meta.len();
    let mtime = meta
        .modified()
        .ok()
        .and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    // A cache version bump must force re-analysis even when the file itself is
    // untouched, or stale blobs survive forever and the UI silently gets nothing.
    if let Some((s, m, rev)) = db_snapshot {
        if s == size && m == mtime && rev == crate::cache::VERSION {
            return Ok(Outcome::Skipped);
        }
    }

    let rel_path = path.strip_prefix(root).unwrap_or(path).to_string_lossy().to_string();
    let filename = path.file_name().unwrap_or_default().to_string_lossy().to_string();
    let ext = path.extension().and_then(|x| x.to_str()).unwrap_or("").to_ascii_lowercase();
    let content_key = ident::content_key(path)?;

    let mut row = FileRow {
        content_key,
        root_id,
        rel_path,
        filename,
        ext,
        size,
        mtime,
        duration_ms: 0,
        sample_rate: 0,
        channels: 0,
        lufs: None,
        peak_db: -120.0,
        features: Vec::new(),
        status: "ok".into(),
    };

    // A file that fails to decode is still recorded, so it is not retried on
    // every rescan and can be surfaced in the UI.
    let decoded = match audio::decode_file(path) {
        Ok(d) => d,
        Err(_) => {
            row.status = "error".into();
            return Ok(Outcome::Row(Box::new(row)));
        }
    };

    let peaks = cache::build(&decoded);
    cache::write(cache_dir, &row.content_key, &peaks)?;

    row.duration_ms = decoded.duration_ms();
    row.sample_rate = decoded.sample_rate;
    row.channels = decoded.channels;
    row.lufs = analysis::loudness(&decoded);
    row.peak_db = analysis::peak_db(&decoded);
    row.features = analysis::features_to_blob(&analysis::features(&decoded));

    Ok(Outcome::Row(Box::new(row)))
}

pub fn scan_root<F>(
    db: &Db,
    cache_dir: &Path,
    root_id: i64,
    root: &Path,
    on_progress: F,
) -> Result<ScanStats>
where
    F: Fn(Progress) + Sync + Send,
{
    scan_root_cancellable(db, cache_dir, root_id, root, on_progress, &AtomicBool::new(false))
}

/// `cancel` is checked per file. Whatever was analysed before cancelling is
/// still committed, so stopping a long scan keeps the work already done and a
/// later rescan picks up where it left off.
pub fn scan_root_cancellable<F>(
    db: &Db,
    cache_dir: &Path,
    root_id: i64,
    root: &Path,
    on_progress: F,
    cancel: &AtomicBool,
) -> Result<ScanStats>
where
    F: Fn(Progress) + Sync + Send,
{
    let started = std::time::Instant::now();
    let files = candidates(root);
    let total = files.len();

    // Read the existing index up front: SQLite reads from many rayon threads
    // would serialise on the connection lock anyway.
    let snapshots: Vec<Option<(u64, i64, u16)>> = files
        .iter()
        .map(|p| {
            let rel = p.strip_prefix(root).unwrap_or(p).to_string_lossy().to_string();
            db.known(root_id, &rel).ok().flatten()
        })
        .collect();

    let done = AtomicUsize::new(0);
    let results: Vec<(PathBuf, Result<Outcome>)> = files
        .par_iter()
        .zip(snapshots.par_iter())
        .map(|(path, snap)| {
            if cancel.load(Ordering::Relaxed) {
                return (path.clone(), Ok(Outcome::Skipped));
            }
            let r = analyse(root, root_id, path, cache_dir, *snap);
            let n = done.fetch_add(1, Ordering::Relaxed) + 1;
            on_progress(Progress { done: n, total, path: path.to_string_lossy().to_string() });
            (path.clone(), r)
        })
        .collect();

    let mut stats =
        ScanStats { total, cancelled: cancel.load(Ordering::Relaxed), ..Default::default() };
    let tx = db.conn.unchecked_transaction()?;
    for (path, r) in results {
        match r {
            Ok(Outcome::Skipped) => stats.skipped += 1,
            Ok(Outcome::Row(row)) => {
                if row.status == "error" {
                    stats.failed += 1;
                } else {
                    stats.analysed += 1;
                }
                db.upsert(&row)?;
            }
            Err(e) => {
                stats.failed += 1;
                stats.errors.push((path.to_string_lossy().to_string(), e.to_string()));
            }
        }
    }
    tx.commit()?;

    stats.elapsed_ms = started.elapsed().as_millis();
    Ok(stats)
}
