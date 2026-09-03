use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use anyhow::{bail, Result};
use soundbox::{analysis, audio, cache, db::Db, ident, scan};

/// `SOUNDBOX_DATA` overrides the library location, so a scratch index can be
/// built without disturbing the real one.
fn data_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("SOUNDBOX_DATA") {
        return PathBuf::from(d);
    }
    dirs::data_dir().unwrap_or_else(|| PathBuf::from(".")).join("SoundBox")
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(|s| s.as_str()) {
        Some("scan") => {
            let dir = args.get(1).map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
            cmd_scan(&dir)
        }
        Some("probe") => cmd_probe(&args[1..]),
        Some("stats") => cmd_stats(),
        Some("play") => cmd_play(&args[1..]),
        Some("search") => cmd_search(&args[1..]),
        Some("similar") => cmd_similar(&args[1..]),
        _ => {
            eprintln!("usage:");
            eprintln!("  soundbox-cli scan <dir>     index a folder");
            eprintln!("  soundbox-cli probe <file..> analyse without touching the db");
            eprintln!("  soundbox-cli stats          summarise the index");
            eprintln!("  soundbox-cli play <file>    audition through the audio thread");
            eprintln!("  soundbox-cli similar <query>  nearest neighbours of the first match");
            Ok(())
        }
    }
}

fn cmd_scan(dir: &Path) -> Result<()> {
    if !dir.is_dir() {
        bail!("{} is not a directory", dir.display());
    }
    let dir = dir.canonicalize()?;
    let base = data_dir();
    let db = Db::open(&base.join("library.db"))?;
    let root_id = db.add_root(&dir, &dir.file_name().unwrap_or_default().to_string_lossy())?;

    println!("scanning {}", dir.display());
    let last = AtomicUsize::new(0);
    let stats = scan::scan_root(&db, &base, root_id, &dir, |p| {
        let pct = p.done * 100 / p.total.max(1);
        if pct != last.swap(pct, Ordering::Relaxed) {
            eprint!("\r  {pct:3}%  {}/{}    ", p.done, p.total);
        }
    })?;
    eprintln!("\r                              ");

    println!(
        "{} files: {} analysed, {} unchanged, {} failed  in {:.1}s",
        stats.total,
        stats.analysed,
        stats.skipped,
        stats.failed,
        stats.elapsed_ms as f64 / 1000.0
    );
    for (path, why) in &stats.errors {
        println!("  ERROR {path}: {why}");
    }
    if stats.analysed > 0 {
        println!("  {:.1} ms/file", stats.elapsed_ms as f64 / stats.analysed as f64);
    }
    println!("db: {}", base.join("library.db").display());
    Ok(())
}

fn cmd_probe(paths: &[String]) -> Result<()> {
    for p in paths {
        let path = Path::new(p);
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        match audio::decode_file(path) {
            Ok(d) => {
                let t = std::time::Instant::now();
                let lufs = analysis::loudness(&d);
                let feats = analysis::features(&d);
                let pk = cache::build(&d);
                let key = ident::content_key(path)?;
                println!(
                    "{name}\n  {} Hz  {} ch  {:.2}s\n  lufs {}  peak {:.1} dB\n  peaks {} buckets x{} ch ({} B)\n  features {} dims\n  key {}\n  analysis {} ms",
                    d.sample_rate,
                    d.channels,
                    d.duration_ms() as f64 / 1000.0,
                    lufs.map(|l| format!("{l:.1}")).unwrap_or_else(|| "n/a".into()),
                    analysis::peak_db(&d),
                    pk.buckets(),
                    pk.channels,
                    cache::encode(&pk).len(),
                    feats.len(),
                    &key[..18],
                    t.elapsed().as_millis()
                );
            }
            Err(e) => println!("{name}\n  FAILED: {e}"),
        }
    }
    Ok(())
}

fn cmd_stats() -> Result<()> {
    let base = data_dir();
    let db = Db::open(&base.join("library.db"))?;
    println!("roots:");
    for (id, path, label) in db.roots()? {
        println!("  [{id}] {label}  {path}");
    }
    println!("files: {} ok, {} error", db.count_status("ok")?, db.count_status("error")?);
    println!("total: {} files, {} unique sounds", db.count()?, db.count_unique_content()?);

    let peaks_dir = base.join("peaks");
    if peaks_dir.exists() {
        let (mut n, mut bytes) = (0u64, 0u64);
        for e in walkdir::WalkDir::new(&peaks_dir).into_iter().filter_map(|e| e.ok()) {
            if e.file_type().is_file() {
                n += 1;
                bytes += e.metadata().map(|m| m.len()).unwrap_or(0);
            }
        }
        println!("peaks cache: {n} blobs, {:.1} MB", bytes as f64 / 1e6);
    }
    Ok(())
}

fn cmd_play(args: &[String]) -> Result<()> {
    use soundbox::player::{normalise_gain, Cmd, Player};
    use std::sync::Arc;

    let path = Path::new(args.first().map(|s| s.as_str()).unwrap_or_default());
    let d = Arc::new(audio::decode_file(path)?);
    let lufs = analysis::loudness(&d);
    let gain = normalise_gain(lufs, -18.0, analysis::peak_db(&d));

    let p = Player::spawn()?;
    let (rate, ch) = p.output_info().unwrap_or((0, 0));
    println!("device {rate} Hz {ch} ch | file {} Hz {} ch | gain {gain:.3}", d.sample_rate, d.channels);

    let frames = d.frames();
    p.send(Cmd::Load { audio: d.clone(), gain });

    let (s, e) = (frames / 4, frames / 2);
    println!("play region {s}..{e}");
    p.send(Cmd::Play { start: s, end: e, looping: false });
    for _ in 0..10 {
        std::thread::sleep(std::time::Duration::from_millis(60));
        println!("  pos={} playing={}", p.position(), p.is_playing());
        if !p.is_playing() { break; }
    }

    println!("toggle pause/resume");
    p.send(Cmd::Play { start: 0, end: frames, looping: true });
    std::thread::sleep(std::time::Duration::from_millis(120));
    let before = p.position();
    p.send(Cmd::Toggle);
    std::thread::sleep(std::time::Duration::from_millis(120));
    let paused = p.position();
    println!("  paused at {paused} (was {before}) playing={}", p.is_playing());
    p.send(Cmd::Toggle);
    std::thread::sleep(std::time::Duration::from_millis(120));
    println!("  resumed pos={} playing={}", p.position(), p.is_playing());
    Ok(())
}

fn cmd_search(args: &[String]) -> Result<()> {
    let db = Db::open(&data_dir().join("library.db"))?;
    let t = std::time::Instant::now();
    let items = db.all_items()?;
    let n = items.len();
    let load = t.elapsed();
    let mut ix = soundbox::search::Index::new(items);
    println!("loaded {n} items in {:?}, index built in {:?}", load, t.elapsed());

    for q in args {
        let t = std::time::Instant::now();
        let hits = ix.search(q, 20, soundbox::search::Sort::Relevance, false, &Default::default());
        let el = t.elapsed();
        println!("\n  \"{q}\" -> {} hits in {:?}", hits.len(), el);
        for h in hits.iter().take(5) {
            let it = ix.get(h.id).unwrap();
            println!("     {:>6}  {}{}  [{}]", h.score, it.filename,
                if h.via_folder { " (via folder)" } else { "" }, it.folder);
        }
    }
    Ok(())
}

fn cmd_similar(args: &[String]) -> Result<()> {
    use soundbox::search::{Index, Sort};
    use soundbox::similar::{SimilarIndex, DEFAULT_DURATION_RATIO};

    let db = Db::open(&data_dir().join("library.db"))?;
    let mut ix = Index::new(db.all_items()?);

    let t = std::time::Instant::now();
    let sim = SimilarIndex::build(db.all_features()?);
    println!("similarity index: {} entries in {:?}", sim.len(), t.elapsed());

    let query = args.first().cloned().unwrap_or_default();
    let hits = ix.search(&query, 1, Sort::Relevance, false, &Default::default());
    let Some(seed) = hits.first() else {
        println!("no match for {query:?}");
        return Ok(());
    };
    let seed_item = ix.get(seed.id).unwrap().clone();
    println!("\nseed: {} [{}] {:.2}s",
        seed_item.filename, seed_item.folder, seed_item.duration_ms as f64 / 1000.0);

    let t = std::time::Instant::now();
    let ns = sim.query(seed.id, 8, DEFAULT_DURATION_RATIO);
    println!("query in {:?}\n", t.elapsed());
    for n in ns {
        if let Some(i) = ix.get(n.id) {
            println!("  {:.4}  {:<52} [{}] {:.2}s",
                n.score, i.filename, i.folder, i.duration_ms as f64 / 1000.0);
        }
    }
    Ok(())
}
