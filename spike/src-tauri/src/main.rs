#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod audio;

use std::path::PathBuf;
use std::sync::Mutex;

use audio::{Decoded, Engine};
use serde::Serialize;
use tauri::{Manager, State};
use tauri_plugin_dialog::DialogExt;
use walkdir::WalkDir;

const EXTS: &[&str] = &["wav", "flac", "mp3", "ogg", "opus", "m4a", "aac", "mp4", "mkv", "oga"];

#[derive(Serialize)]
struct Entry {
    path: String,
    name: String,
    folder: String,
    size: u64,
}

#[derive(Serialize)]
struct Loaded {
    path: String,
    frames: usize,
    sample_rate: u32,
    channels: usize,
    duration_ms: u64,
    peaks: Vec<Vec<(f32, f32)>>,
}

struct App {
    engine: Mutex<Option<Engine>>,
    current: Mutex<Option<Decoded>>,
}

// Must be async: sync commands run on the main thread on Linux, and the dialog
// plugin's blocking_* API deadlocks the GTK event loop there.
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
async fn scan(dir: String) -> Vec<Entry> {
    let root = PathBuf::from(&dir);
    let mut out = Vec::new();
    for e in WalkDir::new(&root).follow_links(false).into_iter().filter_map(|e| e.ok()) {
        if !e.file_type().is_file() {
            continue;
        }
        let p = e.path();
        let ext = p.extension().and_then(|x| x.to_str()).unwrap_or("").to_ascii_lowercase();
        if !EXTS.contains(&ext.as_str()) {
            continue;
        }
        let folder = p
            .parent()
            .and_then(|f| f.strip_prefix(&root).ok())
            .map(|f| f.to_string_lossy().to_string())
            .unwrap_or_default();
        out.push(Entry {
            path: p.to_string_lossy().to_string(),
            name: p.file_name().unwrap_or_default().to_string_lossy().to_string(),
            folder,
            size: e.metadata().map(|m| m.len()).unwrap_or(0),
        });
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    out
}

#[tauri::command]
async fn load(state: State<'_, App>, path: String, buckets: usize) -> Result<Loaded, String> {
    let d = audio::decode_file(std::path::Path::new(&path)).map_err(|e| e.to_string())?;
    let peaks = audio::build_peaks(&d, buckets);
    let out = Loaded {
        path,
        frames: d.frames(),
        sample_rate: d.sample_rate,
        channels: d.channels,
        duration_ms: d.duration_ms(),
        peaks,
    };
    if let Some(eng) = state.engine.lock().unwrap().as_ref() {
        eng.load(&d, 1.0);
    }
    *state.current.lock().unwrap() = Some(d);
    Ok(out)
}

#[tauri::command]
fn play(state: State<'_, App>, start: usize, end: usize, looping: bool) {
    if let Some(eng) = state.engine.lock().unwrap().as_ref() {
        eng.play_region(start, end, looping);
    }
}

#[tauri::command]
fn toggle_play(state: State<'_, App>) -> bool {
    match state.engine.lock().unwrap().as_ref() {
        Some(e) => e.toggle(),
        None => false,
    }
}

#[tauri::command]
fn stop(state: State<'_, App>) {
    if let Some(eng) = state.engine.lock().unwrap().as_ref() {
        eng.stop();
    }
}

#[tauri::command]
fn set_loop(state: State<'_, App>, looping: bool) {
    if let Some(eng) = state.engine.lock().unwrap().as_ref() {
        eng.set_looping(looping);
    }
}

#[derive(Serialize)]
struct Status {
    pos: u64,
    playing: bool,
}

#[tauri::command]
fn status(state: State<'_, App>) -> Status {
    match state.engine.lock().unwrap().as_ref() {
        Some(e) => Status { pos: e.position(), playing: e.is_playing() },
        None => Status { pos: 0, playing: false },
    }
}

#[tauri::command]
fn device_info(state: State<'_, App>) -> String {
    match state.engine.lock().unwrap().as_ref() {
        Some(e) => {
            let (r, c) = e.output_info();
            format!("{r} Hz / {c} ch")
        }
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
        .plugin(tauri_plugin_drag::init())
        .setup(|app| {
            let engine = match Engine::new() {
                Ok(e) => Some(e),
                Err(e) => {
                    eprintln!("audio init failed: {e}");
                    None
                }
            };
            app.manage(App { engine: Mutex::new(engine), current: Mutex::new(None) });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            pick_folder, scan, load, play, toggle_play, stop, set_loop, status, device_info
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
