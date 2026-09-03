use std::collections::HashMap;
use std::path::Path;

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};

use crate::db::{Db, PackEntry};

pub const FORMAT: u32 = 1;
pub const EXT: &str = "sbpack";

#[derive(Debug, Serialize, Deserialize)]
pub struct Tag {
    pub name: String,
    #[serde(default)]
    pub color: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Pack {
    pub format: u32,
    pub profile: String,
    pub exported_at: i64,
    #[serde(default)]
    pub tags: Vec<Tag>,
    pub entries: Vec<PackEntry>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// Union of tags; a local favourite is never cleared.
    Merge,
    /// Incoming values replace local ones for matched sounds.
    Overwrite,
}

#[derive(Debug, Default, Serialize)]
pub struct Preview {
    pub total: usize,
    /// Matched by content hash: the same bytes, so certainly the same sound.
    pub exact: usize,
    /// Matched by filename and size only; could be a re-encode.
    pub fuzzy: usize,
    pub missing: usize,
    pub sample_missing: Vec<String>,
}

pub fn export(db: &Db, profile: &str) -> Result<Pack> {
    Ok(Pack {
        format: FORMAT,
        profile: profile.to_string(),
        exported_at: crate::db::now(),
        tags: db.tag_colors()?.into_iter().map(|(name, color)| Tag { name, color }).collect(),
        entries: db.export_entries()?,
    })
}

pub fn write(pack: &Pack, path: &Path) -> Result<()> {
    let json = serde_json::to_vec(pack)?;
    // Packs are highly repetitive JSON; gzip cuts them by roughly 10x.
    let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    std::io::Write::write_all(&mut enc, &json)?;
    std::fs::write(path, enc.finish()?)?;
    Ok(())
}

pub fn read(path: &Path) -> Result<Pack> {
    let raw = std::fs::read(path)?;
    // Accept plain JSON too, so a hand-edited pack still imports.
    let json = if raw.starts_with(&[0x1f, 0x8b]) {
        let mut out = Vec::new();
        std::io::Read::read_to_end(&mut flate2::read::GzDecoder::new(&raw[..]), &mut out)?;
        out
    } else {
        raw
    };
    let pack: Pack = serde_json::from_slice(&json)?;
    if pack.format > FORMAT {
        return Err(anyhow!(
            "this pack was made by a newer version of SoundBox (format {})",
            pack.format
        ));
    }
    Ok(pack)
}

/// Local lookup tables: exact by content hash, fallback by name and size.
struct Local {
    by_key: HashMap<String, ()>,
    by_name_size: HashMap<(String, u64), String>,
}

fn local_index(db: &Db) -> Result<Local> {
    let mut by_key = HashMap::new();
    let mut by_name_size = HashMap::new();
    for (key, filename, size) in db.content_fingerprints()? {
        by_name_size.insert((filename.to_lowercase(), size), key.clone());
        by_key.insert(key, ());
    }
    Ok(Local { by_key, by_name_size })
}

fn resolve<'a>(local: &'a Local, e: &'a PackEntry) -> Option<(&'a str, bool)> {
    if local.by_key.contains_key(&e.content_key) {
        return Some((&e.content_key, true));
    }
    local.by_name_size.get(&(e.filename.to_lowercase(), e.size)).map(|k| (k.as_str(), false))
}

pub fn preview(db: &Db, pack: &Pack) -> Result<Preview> {
    let local = local_index(db)?;
    let mut p = Preview { total: pack.entries.len(), ..Default::default() };
    for e in &pack.entries {
        match resolve(&local, e) {
            Some((_, true)) => p.exact += 1,
            Some((_, false)) => p.fuzzy += 1,
            None => {
                p.missing += 1;
                if p.sample_missing.len() < 10 {
                    p.sample_missing.push(e.filename.clone());
                }
            }
        }
    }
    Ok(p)
}

/// Applies a pack. `include_fuzzy` decides whether name+size matches count.
///
/// Runs in one transaction: a partially imported library is worse than none.
pub fn import(db: &Db, pack: &Pack, mode: Mode, include_fuzzy: bool) -> Result<Preview> {
    let local = local_index(db)?;
    let mut applied = Preview { total: pack.entries.len(), ..Default::default() };

    db.transaction(|| {
        for t in &pack.tags {
            if let Some(c) = &t.color {
                db.set_tag_color(&t.name, c)?;
            }
        }
        for e in &pack.entries {
            match resolve(&local, e) {
                Some((key, exact)) => {
                    if !exact && !include_fuzzy {
                        applied.missing += 1;
                        continue;
                    }
                    db.apply_pack_entry(key, &e.tags, e.favorite, mode == Mode::Overwrite)?;
                    if exact {
                        applied.exact += 1;
                    } else {
                        applied.fuzzy += 1;
                    }
                }
                None => applied.missing += 1,
            }
        }
        Ok(())
    })?;

    Ok(applied)
}

/// A pack shipped alongside the sounds, so metadata survives zipping or syncing.
pub fn find_in_root(root: &Path) -> Option<std::path::PathBuf> {
    std::fs::read_dir(root)
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .find(|p| p.extension().and_then(|e| e.to_str()) == Some(EXT))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::FileRow;

    fn db_with(files: &[(&str, &str, u64)]) -> Db {
        let db = Db::open_in_memory().unwrap();
        let root = db.add_root(Path::new("/lib"), "lib").unwrap();
        for (key, name, size) in files {
            db.upsert(&FileRow {
                content_key: key.to_string(),
                root_id: root,
                rel_path: format!("sub/{name}"),
                filename: name.to_string(),
                ext: "wav".into(),
                size: *size,
                mtime: 0,
                duration_ms: 1000,
                sample_rate: 48000,
                channels: 2,
                lufs: None,
                peak_db: -1.0,
                features: Vec::new(),
                status: "ok".into(),
            })
            .unwrap();
        }
        db
    }

    fn id_of(db: &Db, name: &str) -> i64 {
        db.all_items().unwrap().into_iter().find(|i| i.filename == name).unwrap().id
    }

    /// Tags the user actually chose, without the folder-derived ones.
    fn user_tags(db: &Db) -> Vec<String> {
        let item = db.all_items().unwrap().remove(0);
        let mut t: Vec<String> =
            item.tags.into_iter().filter(|x| !item.folder_tags.contains(x)).collect();
        t.sort();
        t
    }

    #[test]
    fn export_only_includes_sounds_with_metadata() {
        let db = db_with(&[("k1", "a.wav", 10), ("k2", "b.wav", 20), ("k3", "c.wav", 30)]);
        db.tag_file(id_of(&db, "a.wav"), "whoosh").unwrap();
        db.toggle_favorite(id_of(&db, "b.wav")).unwrap();

        let pack = export(&db, "Test").unwrap();
        let keys: Vec<&str> = pack.entries.iter().map(|e| e.content_key.as_str()).collect();
        assert_eq!(keys.len(), 2, "the untagged sound should not be exported");
        assert!(keys.contains(&"k1") && keys.contains(&"k2"));
    }

    #[test]
    fn folder_tags_do_not_travel_in_a_pack() {
        // They belong to whatever machine holds the files, not to the sound.
        let db = db_with(&[("k1", "a.wav", 10)]);
        db.toggle_favorite(id_of(&db, "a.wav")).unwrap();
        assert!(db.all_items().unwrap()[0].tags.contains(&"sub".to_string()));

        let pack = export(&db, "Test").unwrap();
        assert!(pack.entries[0].tags.is_empty(), "only user-authored tags are exported");
    }

    #[test]
    fn pack_round_trips_through_a_file() {
        let db = db_with(&[("k1", "a.wav", 10)]);
        db.tag_file(id_of(&db, "a.wav"), "metal").unwrap();

        let pack = export(&db, "Test").unwrap();
        let path = std::env::temp_dir().join(format!("sb_{}.sbpack", std::process::id()));
        write(&pack, &path).unwrap();

        let back = read(&path).unwrap();
        assert_eq!(back.format, FORMAT);
        assert_eq!(back.entries.len(), 1);
        assert_eq!(back.entries[0].tags, vec!["metal"]);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn import_matches_by_content_hash_across_different_names() {
        // The same bytes filed under a different name must still match.
        let source = db_with(&[("shared", "his_name.wav", 100)]);
        source.tag_file(id_of(&source, "his_name.wav"), "whoosh").unwrap();
        let pack = export(&source, "His").unwrap();

        let mine = db_with(&[("shared", "my_name.wav", 100)]);
        let r = import(&mine, &pack, Mode::Merge, false).unwrap();
        assert_eq!((r.exact, r.fuzzy, r.missing), (1, 0, 0));

        assert_eq!(user_tags(&mine), vec!["whoosh"]);
    }

    #[test]
    fn fuzzy_matches_are_opt_in() {
        let source = db_with(&[("theirs", "shared.wav", 100)]);
        source.tag_file(id_of(&source, "shared.wav"), "boom").unwrap();
        let pack = export(&source, "His").unwrap();

        // Same name and size, different bytes: a re-encode, so only a guess.
        let mine = db_with(&[("mine", "shared.wav", 100)]);
        let preview = preview(&mine, &pack).unwrap();
        assert_eq!((preview.exact, preview.fuzzy), (0, 1));

        let skipped = import(&mine, &pack, Mode::Merge, false).unwrap();
        assert_eq!(skipped.missing, 1, "fuzzy matches must not apply unless asked");
        assert!(user_tags(&mine).is_empty());

        let applied = import(&mine, &pack, Mode::Merge, true).unwrap();
        assert_eq!(applied.fuzzy, 1);
        assert_eq!(user_tags(&mine), vec!["boom"]);
    }

    #[test]
    fn merge_keeps_local_tags_and_favorites() {
        let source = db_with(&[("k", "a.wav", 1)]);
        source.tag_file(id_of(&source, "a.wav"), "theirs").unwrap();
        let pack = export(&source, "His").unwrap();

        let mine = db_with(&[("k", "a.wav", 1)]);
        mine.tag_file(id_of(&mine, "a.wav"), "mine").unwrap();
        mine.toggle_favorite(id_of(&mine, "a.wav")).unwrap();

        import(&mine, &pack, Mode::Merge, false).unwrap();
        assert_eq!(user_tags(&mine), vec!["mine", "theirs"], "merge is a union");
        assert!(mine.all_items().unwrap()[0].favorite, "merge must not clear a local favourite");
    }

    #[test]
    fn overwrite_replaces_local_metadata() {
        let source = db_with(&[("k", "a.wav", 1)]);
        source.tag_file(id_of(&source, "a.wav"), "theirs").unwrap();
        let pack = export(&source, "His").unwrap();

        let mine = db_with(&[("k", "a.wav", 1)]);
        mine.tag_file(id_of(&mine, "a.wav"), "mine").unwrap();
        mine.toggle_favorite(id_of(&mine, "a.wav")).unwrap();

        import(&mine, &pack, Mode::Overwrite, false).unwrap();
        assert_eq!(user_tags(&mine), vec!["theirs"]);
        assert!(!mine.all_items().unwrap()[0].favorite, "the incoming entry was not a favourite");
    }

    #[test]
    fn unmatched_entries_are_reported_not_invented() {
        let source = db_with(&[("gone", "absent.wav", 5)]);
        source.tag_file(id_of(&source, "absent.wav"), "x").unwrap();
        let pack = export(&source, "His").unwrap();

        let mine = db_with(&[("here", "present.wav", 9)]);
        let r = import(&mine, &pack, Mode::Merge, true).unwrap();
        assert_eq!(r.missing, 1);
        assert_eq!(mine.count().unwrap(), 1, "import must never add files");
    }

    #[test]
    fn a_newer_format_is_refused() {
        let path = std::env::temp_dir().join(format!("sb_future_{}.sbpack", std::process::id()));
        std::fs::write(&path, br#"{"format":99,"profile":"x","exported_at":0,"entries":[]}"#)
            .unwrap();
        assert!(read(&path).is_err());
        std::fs::remove_file(path).unwrap();
    }
}
