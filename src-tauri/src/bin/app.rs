#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde::Serialize;
use soundbox::db::Db;
use soundbox::player::{normalise_gain, Cmd, Player};
use soundbox::profiles::{self, Registry};
use soundbox::search::{Filter, Hit, Index, Sort};
use soundbox::similar::{SimilarIndex, DEFAULT_DURATION_RATIO};
use soundbox::{audio, cache, pack, rename, scan};
use tauri::{Emitter, Manager, State};
use tauri_plugin_dialog::DialogExt;

struct App {
    db: Mutex<Db>,
    index: Mutex<Index>,
    similar: Mutex<SimilarIndex>,
    player: Option<Player>,
    current: Mutex<Option<CurrentFile>>,
    base: PathBuf,
    reg: Mutex<Registry>,
    cancel_scan: std::sync::atomic::AtomicBool,
}

struct CurrentFile {
    frames: usize,
    /// Retained so the detail view can rebuild peaks for any zoom range
    /// without decoding again. Shared with the audio thread.
    audio: Arc<audio::Decoded>,
}

/// Enough detail for a full-width waveform without sending megabytes to the UI.
const DETAIL_BUCKETS: usize = 4000;

fn base_dir() -> PathBuf {
    dirs::data_dir().unwrap_or_else(|| PathBuf::from(".")).join("SoundBox")
}

#[derive(Serialize, Clone)]
struct ItemDto {
    id: i64,
    filename: String,
    folder: String,
    duration_ms: u64,
    channels: usize,
    sample_rate: u32,
    ext: String,
    mtime: i64,
    added_at: i64,
    last_played: i64,
    favorite: bool,
    tags: Vec<String>,
    /// Which of `tags` came from the folder path, so the UI can show them as
    /// fixed rather than removable.
    folder_tags: Vec<String>,
}

#[derive(Serialize, Clone)]
struct ScanEvent {
    done: usize,
    total: usize,
}

#[derive(Serialize)]
struct LoadedDto {
    id: i64,
    path: String,
    frames: usize,
    duration_ms: u64,
    sample_rate: u32,
    channels: usize,
    peaks: Vec<Vec<(f32, f32)>>,
    lufs: Option<f64>,
}

#[derive(Serialize)]
struct StatusDto {
    pos: u64,
    playing: bool,
    frames: usize,
}

fn reload_index(app: &App) -> Result<usize, String> {
    let (items, feats) = {
        let db = app.db.lock().unwrap();
        (db.all_items().map_err(|e| e.to_string())?, db.all_features().map_err(|e| e.to_string())?)
    };
    let n = items.len();
    *app.index.lock().unwrap() = Index::new(items);
    *app.similar.lock().unwrap() = SimilarIndex::build(feats);
    Ok(n)
}

#[tauri::command]
async fn pick_folder(app: tauri::AppHandle) -> Option<String> {
    let (tx, rx) = std::sync::mpsc::channel();
    app.dialog().file().pick_folder(move |p| {
        let _ = tx.send(p);
    });
    tauri::async_runtime::spawn_blocking(move || rx.recv().ok().flatten())
        .await
        .ok()
        .flatten()
        .and_then(|p| p.into_path().ok())
        .map(|p| p.to_string_lossy().to_string())
}

#[tauri::command]
async fn save_pack_dialog(app: tauri::AppHandle) -> Option<String> {
    let (tx, rx) = std::sync::mpsc::channel();
    app.dialog()
        .file()
        .add_filter("SoundBox pack", &[pack::EXT])
        .set_file_name("library.sbpack")
        .save_file(move |p| {
            let _ = tx.send(p);
        });
    tauri::async_runtime::spawn_blocking(move || rx.recv().ok().flatten())
        .await
        .ok()
        .flatten()
        .and_then(|p| p.into_path().ok())
        .map(|p| p.to_string_lossy().to_string())
}

#[tauri::command]
async fn open_pack_dialog(app: tauri::AppHandle) -> Option<String> {
    let (tx, rx) = std::sync::mpsc::channel();
    app.dialog().file().add_filter("SoundBox pack", &[pack::EXT]).pick_file(move |p| {
        let _ = tx.send(p);
    });
    tauri::async_runtime::spawn_blocking(move || rx.recv().ok().flatten())
        .await
        .ok()
        .flatten()
        .and_then(|p| p.into_path().ok())
        .map(|p| p.to_string_lossy().to_string())
}

#[tauri::command]
async fn add_root(
    handle: tauri::AppHandle,
    state: State<'_, App>,
    path: String,
) -> Result<usize, String> {
    let dir = PathBuf::from(&path);
    if !dir.is_dir() {
        return Err(format!("{path} is not a directory"));
    }
    let label = dir.file_name().unwrap_or_default().to_string_lossy().to_string();
    let root_id = {
        let db = state.db.lock().unwrap();
        db.add_root(&dir, &label).map_err(|e| e.to_string())?
    };

    let stats = {
        let db = state.db.lock().unwrap();
        state.cancel_scan.store(false, std::sync::atomic::Ordering::Relaxed);
        scan::scan_root_cancellable(
            &db,
            &state.base,
            root_id,
            &dir,
            |p| {
                // Throttle: one event per 1% is plenty for a progress bar.
                if p.total < 100 || p.done % (p.total / 100).max(1) == 0 {
                    let _ =
                        handle.emit("scan:progress", ScanEvent { done: p.done, total: p.total });
                }
            },
            &state.cancel_scan,
        )
        .map_err(|e| e.to_string())?
    };
    let _ = handle.emit("scan:done", stats.total);
    reload_index(&state)
}

#[tauri::command]
async fn rescan_root(
    handle: tauri::AppHandle,
    state: State<'_, App>,
    id: i64,
) -> Result<usize, String> {
    let path = {
        let db = state.db.lock().unwrap();
        db.roots()
            .map_err(|e| e.to_string())?
            .into_iter()
            .find(|(rid, _, _)| *rid == id)
            .map(|(_, p, _)| p)
            .ok_or_else(|| "no such folder".to_string())?
    };
    let dir = PathBuf::from(&path);
    if !dir.is_dir() {
        return Err(format!("{path} is no longer reachable"));
    }
    {
        let db = state.db.lock().unwrap();
        state.cancel_scan.store(false, std::sync::atomic::Ordering::Relaxed);
        scan::scan_root_cancellable(
            &db,
            &state.base,
            id,
            &dir,
            |p| {
                if p.total < 100 || p.done % (p.total / 100).max(1) == 0 {
                    let _ =
                        handle.emit("scan:progress", ScanEvent { done: p.done, total: p.total });
                }
            },
            &state.cancel_scan,
        )
        .map_err(|e| e.to_string())?;
    }
    let _ = handle.emit("scan:done", 0usize);
    reload_index(&state)
}

#[tauri::command]
fn roots(state: State<'_, App>) -> Result<Vec<(i64, String, String)>, String> {
    state.db.lock().unwrap().roots().map_err(|e| e.to_string())
}

#[tauri::command]
fn library_size(state: State<'_, App>) -> usize {
    state.index.lock().unwrap().len()
}

#[tauri::command]
fn search(
    state: State<'_, App>,
    query: String,
    limit: usize,
    sort: Sort,
    desc: bool,
    filter: Filter,
) -> Vec<(Hit, ItemDto)> {
    let mut ix = state.index.lock().unwrap();
    let hits = ix.search(&query, limit, sort, desc, &filter);
    hits.into_iter()
        .filter_map(|h| {
            ix.get(h.id).map(|i| {
                (
                    h.clone(),
                    ItemDto {
                        id: i.id,
                        filename: i.filename.clone(),
                        folder: i.folder.clone(),
                        duration_ms: i.duration_ms,
                        channels: i.channels,
                        sample_rate: i.sample_rate,
                        ext: i.ext.clone(),
                        mtime: i.mtime,
                        added_at: i.added_at,
                        last_played: i.last_played,
                        favorite: i.favorite,
                        tags: i.tags.clone(),
                        folder_tags: i.folder_tags.clone(),
                    },
                )
            })
        })
        .collect()
}

#[derive(Serialize)]
struct SimilarDto {
    item: ItemDto,
    score: f32,
}

#[tauri::command]
fn similar(
    state: State<'_, App>,
    id: i64,
    limit: usize,
    gate: bool,
    filter: soundbox::search::Filter,
) -> Vec<SimilarDto> {
    let ratio = if gate { DEFAULT_DURATION_RATIO } else { 0.0 };
    // Neighbours are ranked before filtering, so over-fetch or a restrictive
    // filter would leave the panel short of results.
    let want = if filter.is_active() { (limit * 20).min(2000) } else { limit };
    let neighbours = state.similar.lock().unwrap().query(id, want, ratio);
    let ix = state.index.lock().unwrap();
    neighbours
        .into_iter()
        .filter(|n| ix.get(n.id).map(|i| filter.keeps(i)).unwrap_or(false))
        .take(limit)
        .filter_map(|n| {
            ix.get(n.id).map(|i| SimilarDto {
                item: ItemDto {
                    id: i.id,
                    filename: i.filename.clone(),
                    folder: i.folder.clone(),
                    duration_ms: i.duration_ms,
                    channels: i.channels,
                    sample_rate: i.sample_rate,
                    ext: i.ext.clone(),
                    mtime: i.mtime,
                    added_at: i.added_at,
                    last_played: i.last_played,
                    favorite: i.favorite,
                    tags: i.tags.clone(),
                    folder_tags: i.folder_tags.clone(),
                },
                score: n.score,
            })
        })
        .collect()
}

/// Sparklines for the rows currently on screen. Reads cached peak blobs, so no
/// decoding happens and scrolling stays cheap.
#[tauri::command]
fn sparklines(state: State<'_, App>, ids: Vec<i64>, width: usize) -> Vec<(i64, Vec<(f32, f32)>)> {
    let keys: Vec<(i64, String)> = {
        let ix = state.index.lock().unwrap();
        ids.iter().filter_map(|id| ix.get(*id).map(|i| (*id, i.content_key.clone()))).collect()
    };
    keys.into_iter()
        .map(|(id, key)| {
            let peaks = cache::read(&state.base, &key)
                .map(|p| p.mono_downsample(width))
                .unwrap_or_default();
            (id, peaks)
        })
        .collect()
}

#[tauri::command]
async fn toggle_favorite(state: State<'_, App>, id: i64) -> Result<bool, String> {
    let now_fav = {
        let db = state.db.lock().unwrap();
        db.toggle_favorite(id).map_err(|e| e.to_string())?
    };
    reload_index(&state)?;
    Ok(now_fav)
}

#[tauri::command]
async fn tag_file(state: State<'_, App>, id: i64, tag: String) -> Result<(), String> {
    {
        let db = state.db.lock().unwrap();
        db.tag_file(id, &tag).map_err(|e| e.to_string())?;
    }
    reload_index(&state).map(|_| ())
}

#[tauri::command]
async fn untag_file(state: State<'_, App>, id: i64, tag: String) -> Result<(), String> {
    {
        let db = state.db.lock().unwrap();
        db.untag_file(id, &tag).map_err(|e| e.to_string())?;
    }
    reload_index(&state).map(|_| ())
}

/// Counts come from the index, so folder-derived tags are included alongside
/// the user's own.
#[tauri::command]
fn tags(state: State<'_, App>) -> Vec<(String, i64)> {
    let ix = state.index.lock().unwrap();
    let mut counts: std::collections::HashMap<&str, i64> = std::collections::HashMap::new();
    for item in ix.items() {
        for t in &item.tags {
            *counts.entry(t.as_str()).or_default() += 1;
        }
    }
    let mut out: Vec<(String, i64)> = counts.into_iter().map(|(k, v)| (k.to_string(), v)).collect();
    out.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    out
}

/// Blobs are shared between profiles, so every profile has to be consulted
/// before deciding a blob is unreferenced.
#[tauri::command]
async fn prune_cache(state: State<'_, App>) -> Result<cache::Pruned, String> {
    let (ids, active) = {
        let reg = state.reg.lock().unwrap();
        (reg.profiles.iter().map(|p| p.id.clone()).collect::<Vec<_>>(), reg.active.clone())
    };

    let mut keep = std::collections::HashSet::new();
    for id in ids {
        let keys = if id == active {
            state.db.lock().unwrap().all_content_keys().map_err(|e| e.to_string())?
        } else {
            // Opened read-only-ish and dropped immediately; the active profile's
            // connection is the one held open.
            match Db::open(&profiles::db_path(&state.base, &id)) {
                Ok(db) => db.all_content_keys().map_err(|e| e.to_string())?,
                Err(e) => return Err(format!("could not read profile {id}: {e}")),
            }
        };
        keep.extend(keys);
    }

    cache::prune(&state.base, &keep).map_err(|e| e.to_string())
}

#[tauri::command]
fn cancel_scan(state: State<'_, App>) {
    state.cancel_scan.store(true, std::sync::atomic::Ordering::Relaxed);
}

#[tauri::command]
fn failed_files(state: State<'_, App>) -> Result<Vec<(String, String)>, String> {
    state.db.lock().unwrap().failed_files().map_err(|e| e.to_string())
}

#[derive(Serialize)]
struct RenameResult {
    filename: String,
    suffixed: bool,
}

#[tauri::command]
async fn rename_file(
    state: State<'_, App>,
    id: i64,
    stem: String,
    allow_suffix: bool,
) -> Result<RenameResult, String> {
    let out = {
        let db = state.db.lock().unwrap();
        rename::perform(&db, id, &stem, allow_suffix).map_err(|e| e.to_string())?
    };
    reload_index(&state)?;
    Ok(RenameResult { filename: out.filename, suffixed: out.suffixed })
}

#[tauri::command]
async fn undo_rename(state: State<'_, App>) -> Result<Option<String>, String> {
    let name = {
        let db = state.db.lock().unwrap();
        rename::undo_last(&db).map_err(|e| e.to_string())?
    };
    reload_index(&state)?;
    Ok(name)
}

#[tauri::command]
fn profiles_list(state: State<'_, App>) -> Registry {
    state.reg.lock().unwrap().clone()
}

/// Swaps the open database and rebuilds the in-memory indexes.
fn open_profile(state: &State<'_, App>, id: &str) -> Result<usize, String> {
    let path = profiles::db_path(&state.base, id);
    let db = Db::open(&path).map_err(|e| e.to_string())?;
    *state.db.lock().unwrap() = db;
    // Selection belongs to the profile that was open, not this one.
    *state.current.lock().unwrap() = None;
    reload_index(state)
}

#[tauri::command]
async fn profile_switch(state: State<'_, App>, id: String) -> Result<usize, String> {
    {
        let mut reg = state.reg.lock().unwrap();
        reg.switch(&state.base, &id).map_err(|e| e.to_string())?;
    }
    open_profile(&state, &id)
}

#[tauri::command]
async fn profile_create(state: State<'_, App>, name: String) -> Result<Registry, String> {
    let mut reg = state.reg.lock().unwrap();
    reg.create(&state.base, &name).map_err(|e| e.to_string())?;
    Ok(reg.clone())
}

#[tauri::command]
async fn profile_rename(
    state: State<'_, App>,
    id: String,
    name: String,
) -> Result<Registry, String> {
    let mut reg = state.reg.lock().unwrap();
    reg.rename(&state.base, &id, &name).map_err(|e| e.to_string())?;
    Ok(reg.clone())
}

#[tauri::command]
async fn profile_delete(state: State<'_, App>, id: String) -> Result<Registry, String> {
    let active = {
        let mut reg = state.reg.lock().unwrap();
        reg.delete(&state.base, &id).map_err(|e| e.to_string())?
    };
    open_profile(&state, &active)?;
    Ok(state.reg.lock().unwrap().clone())
}

/// Packs travel with the sounds, so adding a root offers whatever it contains.
#[tauri::command]
fn pack_in_root(path: String) -> Option<String> {
    pack::find_in_root(std::path::Path::new(&path)).map(|p| p.to_string_lossy().to_string())
}

#[tauri::command]
async fn pack_export(state: State<'_, App>, path: String) -> Result<usize, String> {
    let name =
        state.reg.lock().unwrap().active_profile().map(|p| p.name.clone()).unwrap_or_default();
    let p = {
        let db = state.db.lock().unwrap();
        pack::export(&db, &name).map_err(|e| e.to_string())?
    };
    let n = p.entries.len();
    pack::write(&p, std::path::Path::new(&path)).map_err(|e| e.to_string())?;
    Ok(n)
}

#[tauri::command]
async fn pack_preview(state: State<'_, App>, path: String) -> Result<pack::Preview, String> {
    let p = pack::read(std::path::Path::new(&path)).map_err(|e| e.to_string())?;
    let db = state.db.lock().unwrap();
    pack::preview(&db, &p).map_err(|e| e.to_string())
}

#[tauri::command]
async fn pack_import(
    state: State<'_, App>,
    path: String,
    overwrite: bool,
    include_fuzzy: bool,
) -> Result<pack::Preview, String> {
    let p = pack::read(std::path::Path::new(&path)).map_err(|e| e.to_string())?;
    let mode = if overwrite { pack::Mode::Overwrite } else { pack::Mode::Merge };
    let applied = {
        let db = state.db.lock().unwrap();
        pack::import(&db, &p, mode, include_fuzzy).map_err(|e| e.to_string())?
    };
    reload_index(&state)?;
    Ok(applied)
}

#[tauri::command]
fn file_path(state: State<'_, App>, id: i64) -> Result<String, String> {
    state.db.lock().unwrap().path_for(id).map_err(|e| e.to_string())
}

#[tauri::command]
async fn load(state: State<'_, App>, id: i64, normalise: bool) -> Result<LoadedDto, String> {
    let (path, key, lufs, peak_db) = {
        let db = state.db.lock().unwrap();
        let info = db.load_info(id).map_err(|e| e.to_string())?;
        let _ = db.record_play(id);
        info
    };

    let decoded =
        Arc::new(audio::decode_file(std::path::Path::new(&path)).map_err(|e| e.to_string())?);

    // The cached level is a coarse 2048 samples/bucket, which is far too blocky
    // for a short sound. The file is already decoded here, so build detail peaks
    // at display resolution instead.
    let peaks = cache::build_n(&decoded, DETAIL_BUCKETS).to_f32();

    // Heal a missing cache blob, so the list sparkline stops being blank. Costs
    // nothing here because the file is already decoded.
    if cache::read(&state.base, &key).is_err() {
        let _ = cache::write(&state.base, &key, &cache::build(&decoded));
    }

    let gain = if normalise { normalise_gain(lufs, -18.0, peak_db.unwrap_or(-1.0)) } else { 1.0 };

    let dto = LoadedDto {
        id,
        path,
        frames: decoded.frames(),
        duration_ms: decoded.duration_ms(),
        sample_rate: decoded.sample_rate,
        channels: decoded.channels,
        peaks,
        lufs,
    };

    *state.current.lock().unwrap() =
        Some(CurrentFile { frames: decoded.frames(), audio: decoded.clone() });
    if let Some(p) = &state.player {
        p.send(Cmd::Load { audio: decoded, gain });
    }
    Ok(dto)
}

/// Peaks for an arbitrary frame range at display resolution, for zooming.
#[tauri::command]
fn peaks_range(
    state: State<'_, App>,
    start: usize,
    end: usize,
    width: usize,
) -> Vec<Vec<(f32, f32)>> {
    let Some(cur) = state.current.lock().unwrap().as_ref().map(|c| c.audio.clone()) else {
        return Vec::new();
    };
    let frames = cur.frames();
    let start = start.min(frames);
    let end = end.clamp(start + 1, frames.max(1));

    let slice = audio::Decoded {
        samples: cur.samples[start * cur.channels..end * cur.channels].to_vec(),
        channels: cur.channels,
        sample_rate: cur.sample_rate,
    };
    cache::build_n(&slice, width.clamp(1, 8000)).to_f32()
}

#[tauri::command]
fn play(state: State<'_, App>, start: usize, end: usize, looping: bool) {
    if let Some(p) = &state.player {
        p.send(Cmd::Play { start, end, looping });
    }
}

#[tauri::command]
fn toggle(state: State<'_, App>) {
    if let Some(p) = &state.player {
        p.send(Cmd::Toggle);
    }
}

#[tauri::command]
fn stop(state: State<'_, App>) {
    if let Some(p) = &state.player {
        p.send(Cmd::Stop);
    }
}

#[tauri::command]
fn set_looping(state: State<'_, App>, looping: bool) {
    if let Some(p) = &state.player {
        p.send(Cmd::SetLooping(looping));
    }
}

#[tauri::command]
fn set_volume(state: State<'_, App>, volume: f32) {
    if let Some(p) = &state.player {
        p.send(Cmd::SetVolume(volume));
    }
}

#[tauri::command]
async fn remove_root(state: State<'_, App>, id: i64) -> Result<usize, String> {
    {
        let db = state.db.lock().unwrap();
        db.remove_root(id).map_err(|e| e.to_string())?;
    }
    reload_index(&state)
}

#[tauri::command]
fn status(state: State<'_, App>) -> StatusDto {
    let frames = state.current.lock().unwrap().as_ref().map(|c| c.frames).unwrap_or(0);
    match &state.player {
        Some(p) => StatusDto { pos: p.position(), playing: p.is_playing(), frames },
        None => StatusDto { pos: 0, playing: false, frames },
    }
}

#[tauri::command]
fn device_info(state: State<'_, App>) -> String {
    match state.player.as_ref().and_then(|p| p.output_info()) {
        Some((r, c)) => format!("{r} Hz / {c} ch"),
        None => "no audio device".into(),
    }
}

fn main() {
    // WebKitGTK's DMA-BUF renderer hard-crashes with a Wayland protocol error on
    // NVIDIA. Must be set before GTK initialises.
    #[cfg(target_os = "linux")]
    if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() {
        std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_drag::init())
        .setup(|app| {
            let base = base_dir();
            let reg = profiles::init(&base)?;
            let db = Db::open(&profiles::db_path(&base, &reg.active))?;
            let items = db.all_items().unwrap_or_default();
            let feats = db.all_features().unwrap_or_default();
            let player = match Player::spawn() {
                Ok(p) => Some(p),
                Err(e) => {
                    eprintln!("audio unavailable: {e}");
                    None
                }
            };
            app.manage(App {
                db: Mutex::new(db),
                index: Mutex::new(Index::new(items)),
                similar: Mutex::new(SimilarIndex::build(feats)),
                player,
                current: Mutex::new(None),
                base,
                reg: Mutex::new(reg),
                cancel_scan: std::sync::atomic::AtomicBool::new(false),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            pick_folder,
            save_pack_dialog,
            open_pack_dialog,
            add_root,
            roots,
            library_size,
            search,
            file_path,
            profiles_list,
            profile_switch,
            profile_create,
            profile_rename,
            profile_delete,
            pack_in_root,
            pack_export,
            pack_preview,
            pack_import,
            rename_file,
            undo_rename,
            toggle_favorite,
            tag_file,
            untag_file,
            tags,
            prune_cache,
            cancel_scan,
            failed_files,
            similar,
            sparklines,
            load,
            play,
            peaks_range,
            toggle,
            stop,
            set_looping,
            set_volume,
            remove_root,
            rescan_root,
            status,
            device_info
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
