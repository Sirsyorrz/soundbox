//! External programs used for importing from URLs: yt-dlp, Deno (yt-dlp needs a
//! JS runtime for YouTube) and ffmpeg. SoundBox keeps its own copies under the
//! data dir so yt-dlp can update itself without a SoundBox release.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Channel {
    #[default]
    Stable,
    Nightly,
}

impl Channel {
    fn repo(self) -> &'static str {
        match self {
            Channel::Stable => "yt-dlp/yt-dlp",
            Channel::Nightly => "yt-dlp/yt-dlp-nightly-builds",
        }
    }

    fn name(self) -> &'static str {
        match self {
            Channel::Stable => "stable",
            Channel::Nightly => "nightly",
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub channel: Channel,
    /// Use the yt-dlp on PATH instead of the managed copy. SoundBox never
    /// updates that one, since a package manager owns it.
    pub use_system_ytdlp: bool,
    /// Unix seconds of the last update attempt of the managed yt-dlp.
    pub last_update: i64,
}

pub const UPDATE_INTERVAL_SECS: i64 = 24 * 60 * 60;

pub fn dir(base: &Path) -> PathBuf {
    base.join("tools")
}

fn config_path(dir: &Path) -> PathBuf {
    dir.join("tools.json")
}

pub fn load_config(dir: &Path) -> Config {
    std::fs::read(config_path(dir))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

pub fn save_config(dir: &Path, cfg: &Config) -> Result<()> {
    std::fs::create_dir_all(dir)?;
    std::fs::write(config_path(dir), serde_json::to_vec_pretty(cfg)?)?;
    Ok(())
}

fn exe(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    }
}

pub fn managed_ytdlp(dir: &Path) -> PathBuf {
    dir.join(exe("yt-dlp"))
}

pub fn managed_deno(dir: &Path) -> PathBuf {
    dir.join(exe("deno"))
}

pub fn managed_ffmpeg(dir: &Path) -> PathBuf {
    dir.join("ffmpeg").join(exe("ffmpeg"))
}

pub fn on_path(name: &str) -> Option<PathBuf> {
    let file = exe(name);
    std::env::split_paths(&std::env::var_os("PATH")?).map(|d| d.join(&file)).find(|p| p.is_file())
}

/// A command that never flashes a console window on Windows. The app is a GUI
/// subsystem binary, so every child would otherwise get a fresh console.
pub fn command(program: &Path) -> Command {
    #[allow(unused_mut)]
    let mut cmd = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

#[derive(Clone, Debug, Default)]
pub struct Resolved {
    pub ytdlp: Option<PathBuf>,
    pub ytdlp_managed: bool,
    pub deno: Option<PathBuf>,
    pub ffmpeg: Option<PathBuf>,
}

pub fn resolve(dir: &Path, cfg: &Config) -> Resolved {
    let managed = managed_ytdlp(dir);
    let (ytdlp, ytdlp_managed) = if cfg.use_system_ytdlp {
        (on_path("yt-dlp"), false)
    } else if managed.is_file() {
        (Some(managed), true)
    } else {
        (None, true)
    };
    let deno = Some(managed_deno(dir)).filter(|p| p.is_file()).or_else(|| on_path("deno"));
    let ffmpeg = Some(managed_ffmpeg(dir)).filter(|p| p.is_file()).or_else(|| on_path("ffmpeg"));
    Resolved { ytdlp, ytdlp_managed, deno, ffmpeg }
}

#[derive(Serialize, Clone, Debug, Default)]
pub struct ToolInfo {
    pub path: Option<String>,
    pub version: Option<String>,
    pub managed: bool,
}

#[derive(Serialize, Clone, Debug, Default)]
pub struct Status {
    pub ytdlp: ToolInfo,
    pub deno: ToolInfo,
    pub ffmpeg: ToolInfo,
    pub channel: Channel,
    pub use_system_ytdlp: bool,
    pub system_ytdlp: Option<String>,
    pub last_update: i64,
    /// False where no prebuilt binaries exist, so the UI can say so up front.
    pub supported: bool,
}

fn first_line(program: &Path, arg: &str) -> Option<String> {
    let out = command(program).arg(arg).output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    text.lines().next().map(|l| l.trim().to_string()).filter(|l| !l.is_empty())
}

fn info(path: Option<&PathBuf>, managed_root: &Path, version_arg: &str) -> ToolInfo {
    match path {
        Some(p) => ToolInfo {
            path: Some(p.to_string_lossy().to_string()),
            version: first_line(p, version_arg),
            managed: p.starts_with(managed_root),
        },
        None => ToolInfo::default(),
    }
}

pub fn status(dir: &Path) -> Status {
    let cfg = load_config(dir);
    let r = resolve(dir, &cfg);
    let ffmpeg = {
        let mut i = info(r.ffmpeg.as_ref(), dir, "-version");
        // "ffmpeg version N-12345-gabc Copyright ..." is too long for a label.
        i.version = i.version.map(|v| {
            v.trim_start_matches("ffmpeg version ").split_whitespace().next().unwrap_or("").into()
        });
        i
    };
    let deno = {
        let mut i = info(r.deno.as_ref(), dir, "--version");
        i.version = i.version.map(|v| v.split_whitespace().nth(1).unwrap_or(&v).to_string());
        i
    };
    Status {
        ytdlp: info(r.ytdlp.as_ref(), dir, "--version"),
        deno,
        ffmpeg,
        channel: cfg.channel,
        use_system_ytdlp: cfg.use_system_ytdlp,
        system_ytdlp: on_path("yt-dlp").map(|p| p.to_string_lossy().to_string()),
        last_update: cfg.last_update,
        supported: ytdlp_asset().is_some(),
    }
}

fn ytdlp_asset() -> Option<&'static str> {
    Some(match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "x86_64") => "yt-dlp.exe",
        ("windows", "aarch64") => "yt-dlp_arm64.exe",
        ("linux", "x86_64") => "yt-dlp_linux",
        ("linux", "aarch64") => "yt-dlp_linux_aarch64",
        ("macos", _) => "yt-dlp_macos",
        _ => return None,
    })
}

fn deno_asset() -> Option<&'static str> {
    Some(match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "x86_64") => "deno-x86_64-pc-windows-msvc.zip",
        ("windows", "aarch64") => "deno-aarch64-pc-windows-msvc.zip",
        ("linux", "x86_64") => "deno-x86_64-unknown-linux-gnu.zip",
        ("linux", "aarch64") => "deno-aarch64-unknown-linux-gnu.zip",
        ("macos", "x86_64") => "deno-x86_64-apple-darwin.zip",
        ("macos", "aarch64") => "deno-aarch64-apple-darwin.zip",
        _ => return None,
    })
}

/// yt-dlp's own builds. Every archive holds `<asset stem>/bin/ffmpeg[.exe]`.
fn ffmpeg_asset() -> Option<&'static str> {
    Some(match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "x86_64") => "ffmpeg-master-latest-win64-gpl.zip",
        ("windows", "aarch64") => "ffmpeg-master-latest-winarm64-gpl.zip",
        ("linux", "x86_64") => "ffmpeg-master-latest-linux64-gpl.tar.xz",
        ("linux", "aarch64") => "ffmpeg-master-latest-linuxarm64-gpl.tar.xz",
        _ => return None,
    })
}

/// (stage, bytes done, bytes total if known).
pub type OnProgress<'a> = &'a (dyn Fn(&str, u64, Option<u64>) + Sync);

fn client() -> Result<reqwest::blocking::Client> {
    // reqwest is built without a bundled crypto provider; the updater installs
    // the same one lazily, so whichever runs first wins and both are fine.
    if rustls::crypto::CryptoProvider::get_default().is_none() {
        let _ = rustls::crypto::ring::default_provider().install_default();
    }
    Ok(reqwest::blocking::Client::builder()
        .user_agent(concat!("SoundBox/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(std::time::Duration::from_secs(20))
        .timeout(None)
        .build()?)
}

/// Streams `url` to `dest`, returning the lowercase hex SHA-256 of what arrived.
fn fetch(url: &str, dest: &Path, stage: &str, on_progress: OnProgress) -> Result<String> {
    let mut resp = client()?.get(url).send()?.error_for_status()?;
    let total = resp.content_length();
    let mut file = std::fs::File::create(dest)
        .with_context(|| format!("could not write {}", dest.display()))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 256 * 1024];
    let mut done = 0u64;
    let mut last_report = 0u64;
    loop {
        let n = resp.read(&mut buf)?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])?;
        hasher.update(&buf[..n]);
        done += n as u64;
        if done - last_report > 512 * 1024 {
            on_progress(stage, done, total);
            last_report = done;
        }
    }
    file.flush()?;
    on_progress(stage, done, total);
    Ok(hex(&hasher.finalize()))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn fetch_text(url: &str) -> Result<String> {
    Ok(client()?.get(url).send()?.error_for_status()?.text()?)
}

/// Finds `asset` in a `sha256sum`-style listing. A single-entry file whose name
/// column is missing is taken as the hash of the asset itself.
pub fn sum_for(listing: &str, asset: &str) -> Option<String> {
    let mut lone = None;
    for line in listing.lines() {
        let mut parts = line.split_whitespace();
        let Some(hash) = parts.next() else { continue };
        match parts.next().map(|n| n.trim_start_matches('*')) {
            Some(name) if name == asset => return Some(hash.to_ascii_lowercase()),
            None => lone = Some(hash.to_ascii_lowercase()),
            _ => {}
        }
    }
    lone
}

fn verify(got: &str, sums_url: &str, asset: &str) -> Result<()> {
    let listing = fetch_text(sums_url).context("could not fetch checksums")?;
    let want = sum_for(&listing, asset).ok_or_else(|| anyhow!("{asset} is not in {sums_url}"))?;
    if want != got {
        bail!("checksum mismatch for {asset}; download discarded");
    }
    Ok(())
}

#[cfg(unix)]
fn make_executable(p: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o755))?;
    Ok(())
}

#[cfg(not(unix))]
fn make_executable(_: &Path) -> Result<()> {
    Ok(())
}

/// Windows will not rename over an existing file, so the old one goes first.
fn replace(from: &Path, to: &Path) -> Result<()> {
    if to.exists() {
        std::fs::remove_file(to).with_context(|| format!("{} is in use", to.display()))?;
    }
    std::fs::rename(from, to)?;
    Ok(())
}

pub fn install_ytdlp(dir: &Path, channel: Channel, on_progress: OnProgress) -> Result<()> {
    let asset = ytdlp_asset().ok_or_else(|| anyhow!("no yt-dlp build for this platform"))?;
    std::fs::create_dir_all(dir)?;
    let base = format!("https://github.com/{}/releases/latest/download", channel.repo());
    let part = dir.join(format!("{asset}.part"));
    let got = fetch(&format!("{base}/{asset}"), &part, "yt-dlp", on_progress)?;
    if let Err(e) = verify(&got, &format!("{base}/SHA2-256SUMS"), asset) {
        let _ = std::fs::remove_file(&part);
        return Err(e);
    }
    make_executable(&part)?;
    replace(&part, &managed_ytdlp(dir))
}

fn extract_zip_members(archive: &Path, members: &[(&str, PathBuf)]) -> Result<()> {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(archive)?)?;
    for (name, dest) in members {
        let mut entry =
            zip.by_name(name).with_context(|| format!("{name} missing from archive"))?;
        let part = dest.with_extension("part");
        let mut out = std::fs::File::create(&part)?;
        std::io::copy(&mut entry, &mut out)?;
        drop(out);
        make_executable(&part)?;
        replace(&part, dest)?;
    }
    Ok(())
}

pub fn install_deno(dir: &Path, on_progress: OnProgress) -> Result<()> {
    let asset = deno_asset().ok_or_else(|| anyhow!("no Deno build for this platform"))?;
    std::fs::create_dir_all(dir)?;
    let url = format!("https://github.com/denoland/deno/releases/latest/download/{asset}");
    let archive = dir.join(asset);
    let result = (|| {
        let got = fetch(&url, &archive, "Deno", on_progress)?;
        verify(&got, &format!("{url}.sha256sum"), asset)?;
        extract_zip_members(&archive, &[(exe("deno").as_str(), managed_deno(dir))])
    })();
    let _ = std::fs::remove_file(&archive);
    result
}

pub fn install_ffmpeg(dir: &Path, on_progress: OnProgress) -> Result<()> {
    let asset = ffmpeg_asset().ok_or_else(|| anyhow!("no ffmpeg build for this platform"))?;
    let out_dir = dir.join("ffmpeg");
    std::fs::create_dir_all(&out_dir)?;
    let base = "https://github.com/yt-dlp/FFmpeg-Builds/releases/latest/download";
    let archive = dir.join(asset);
    let top = asset.trim_end_matches(".zip").trim_end_matches(".tar.xz");
    let result = (|| {
        let got = fetch(&format!("{base}/{asset}"), &archive, "ffmpeg", on_progress)?;
        verify(&got, &format!("{base}/checksums.sha256"), asset)?;
        on_progress("ffmpeg (unpacking)", 0, None);
        let names = [exe("ffmpeg"), exe("ffprobe")];
        if asset.ends_with(".zip") {
            let members: Vec<(String, PathBuf)> =
                names.iter().map(|n| (format!("{top}/bin/{n}"), out_dir.join(n))).collect();
            let refs: Vec<(&str, PathBuf)> =
                members.iter().map(|(m, d)| (m.as_str(), d.clone())).collect();
            extract_zip_members(&archive, &refs)
        } else {
            extract_tar_xz(&archive, top, &names, &out_dir)
        }
    })();
    let _ = std::fs::remove_file(&archive);
    result
}

/// Uses the system tar, which every Linux desktop has, rather than linking an
/// xz decoder for one archive.
fn extract_tar_xz(archive: &Path, top: &str, names: &[String], out_dir: &Path) -> Result<()> {
    let tmp = out_dir.join("unpack.tmp");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp)?;
    let mut cmd = Command::new("tar");
    cmd.arg("-xJf").arg(archive).arg("-C").arg(&tmp);
    for n in names {
        cmd.arg(format!("{top}/bin/{n}"));
    }
    let out = cmd.output().context("could not run tar")?;
    let result = if out.status.success() {
        names.iter().try_for_each(|n| {
            let src = tmp.join(top).join("bin").join(n);
            make_executable(&src)?;
            replace(&src, &out_dir.join(n))
        })
    } else {
        Err(anyhow!("tar failed: {}", String::from_utf8_lossy(&out.stderr).trim()))
    };
    let _ = std::fs::remove_dir_all(&tmp);
    result
}

/// Runs yt-dlp's self-updater on the managed copy, which also switches channel.
/// Returns yt-dlp's last line of output for display.
pub fn update_ytdlp(dir: &Path) -> Result<String> {
    let mut cfg = load_config(dir);
    let path = managed_ytdlp(dir);
    if !path.is_file() {
        bail!("yt-dlp is not installed");
    }
    cfg.last_update = crate::db::now();
    save_config(dir, &cfg)?;
    let out = command(&path)
        .args(["--ignore-config", "--update-to", cfg.channel.name()])
        .output()
        .context("could not run yt-dlp")?;
    let text = format!(
        "{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let last = text.lines().map(str::trim).rfind(|l| !l.is_empty()).unwrap_or("done").to_string();
    if out.status.success() {
        Ok(last)
    } else {
        Err(anyhow!(last))
    }
}

pub fn update_due(cfg: &Config, now: i64) -> bool {
    !cfg.use_system_ytdlp && now - cfg.last_update >= UPDATE_INTERVAL_SECS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_named_sum() {
        let l = "aaa  yt-dlp\nBBB *yt-dlp.exe\nccc  yt-dlp_linux\n";
        assert_eq!(sum_for(l, "yt-dlp.exe").as_deref(), Some("bbb"));
        assert_eq!(sum_for(l, "yt-dlp_linux").as_deref(), Some("ccc"));
        assert_eq!(sum_for(l, "missing"), None);
    }

    #[test]
    fn lone_hash_counts_for_the_asset() {
        assert_eq!(sum_for("abc\n", "deno.zip").as_deref(), Some("abc"));
    }

    #[test]
    fn system_ytdlp_is_never_updated() {
        let cfg = Config { use_system_ytdlp: true, ..Default::default() };
        assert!(!update_due(&cfg, i64::MAX));
        assert!(update_due(&Config::default(), UPDATE_INTERVAL_SECS));
        let recent = Config { last_update: 100, ..Default::default() };
        assert!(!update_due(&recent, 100 + UPDATE_INTERVAL_SECS - 1));
    }
}
