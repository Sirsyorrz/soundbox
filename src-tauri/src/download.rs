//! Running yt-dlp for one import: building its arguments, reading its progress
//! and stopping it.

use std::ffi::OsString;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};

use anyhow::{anyhow, bail, Result};
use serde::Deserialize;

use crate::tools::{self, Resolved};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    /// Whatever the site offers that SoundBox can decode, untouched.
    Original,
    Wav,
    Flac,
    Mp3,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Request {
    pub url: String,
    pub dest: String,
    pub format: Format,
    pub playlist: bool,
    /// (start, end) as yt-dlp section times: seconds, `m:ss`, or `inf`.
    pub clip: Option<(String, String)>,
}

/// Formats symphonia decodes, best first. `best[ext=mp4]` covers sites with no
/// audio-only stream, since the audio track of an mp4 plays fine.
const PLAYABLE: &str = "bestaudio[ext=m4a]/bestaudio[ext=mp3]/bestaudio[acodec=vorbis]/\
bestaudio[ext=flac]/bestaudio[ext=wav]/best[ext=mp4]/best[ext=mp3]";

const PROGRESS: &str = "download:SBDL\t%(progress.downloaded_bytes)s\t%(progress.total_bytes)s\t\
%(progress.total_bytes_estimate)s\t%(info.playlist_index)s\t%(info.playlist_count)s\t%(info.title)s";
const POSTPROCESS: &str = "postprocess:SBPP\t%(info.title)s";

impl Request {
    pub fn needs_ffmpeg(&self) -> bool {
        self.format != Format::Original || self.clip.is_some()
    }

    pub fn validate(&self) -> Result<()> {
        let url = self.url.trim();
        if !(url.starts_with("http://") || url.starts_with("https://")) {
            bail!("not a web link: {url}");
        }
        if let Some((a, b)) = &self.clip {
            for t in [a, b] {
                let ok = t == "inf"
                    || (!t.is_empty()
                        && t.chars().all(|c| c.is_ascii_digit() || c == ':' || c == '.'));
                if !ok {
                    bail!("clip times look like 90, 1:30 or 1:02:03 (got \"{t}\")");
                }
            }
        }
        Ok(())
    }
}

pub fn args(req: &Request, tools: &Resolved, temp: &Path, files_log: &Path) -> Vec<OsString> {
    let mut a: Vec<OsString> = Vec::new();
    let mut push = |s: &str| a.push(s.into());

    // A user's own yt-dlp config could change output names or formats under us.
    push("--ignore-config");
    push("--newline");
    push("--progress");
    push("--encoding");
    push("utf-8");
    // The file's date should be when it arrived, so "added" and "modified"
    // sorting put new imports on top.
    push("--no-mtime");
    // Libraries get shared between Windows and Linux machines.
    push("--windows-filenames");

    let name = match (&req.clip, req.playlist) {
        (Some(_), _) => "%(title).160B [%(section_start)d-%(section_end)d].%(ext)s",
        // A single video still imported with playlist on has no playlist_title,
        // and "." keeps it in the folder itself. Slashes inside a replacement
        // get sanitised, so `&{}/` cannot do this.
        (None, true) => "%(playlist_title|.)s/%(title).180B.%(ext)s",
        (None, false) => "%(title).180B.%(ext)s",
    };
    push("-o");
    push(name);

    if req.playlist {
        push("--yes-playlist");
        // One private video should not sink the other 49.
        push("--ignore-errors");
    } else {
        push("--no-playlist");
    }

    match req.format {
        Format::Original => {
            push("-f");
            push(PLAYABLE);
        }
        f => {
            push("-f");
            push("bestaudio/best");
            push("-x");
            push("--audio-format");
            push(match f {
                Format::Wav => "wav",
                Format::Flac => "flac",
                _ => "mp3",
            });
            push("--audio-quality");
            push("0");
        }
    }

    if let Some((start, end)) = &req.clip {
        push("--download-sections");
        push(&format!("*{start}-{end}"));
        // Without re-encoding at the cuts, a 5 s clip from YouTube came out 8 s.
        push("--force-keyframes-at-cuts");
    }

    push("--progress-template");
    push(PROGRESS);
    push("--progress-template");
    push(POSTPROCESS);

    a.push("-P".into());
    a.push(req.dest.clone().into());
    // Partial files live outside the library, so a cancelled job leaves nothing
    // half-written for the scanner to trip over.
    let mut t = OsString::from("temp:");
    t.push(temp);
    a.push("-P".into());
    a.push(t);

    if let Some(dir) = tools.ffmpeg.as_ref().and_then(|p| p.parent()) {
        a.push("--ffmpeg-location".into());
        a.push(dir.into());
    }

    // Written by yt-dlp as UTF-8 regardless of the console code page, unlike
    // stdout, so non-ASCII titles survive on Windows.
    a.push("--print-to-file".into());
    a.push("after_move:filepath".into());
    a.push(files_log.into());

    a.push("--".into());
    a.push(req.url.trim().into());
    a
}

#[derive(Clone, Debug, PartialEq)]
pub enum Line {
    Progress {
        done: Option<u64>,
        total: Option<u64>,
        index: Option<u32>,
        count: Option<u32>,
        title: String,
    },
    Converting {
        title: String,
    },
    Error(String),
    Other,
}

fn num<T: std::str::FromStr>(s: &str) -> Option<T> {
    s.parse().ok()
}

pub fn parse_line(line: &str) -> Line {
    let line = line.trim_end_matches(['\r', '\n']);
    if let Some(rest) = line.strip_prefix("SBDL\t") {
        let f: Vec<&str> = rest.splitn(6, '\t').collect();
        if f.len() == 6 {
            let est = f[2].parse::<f64>().ok().map(|v| v as u64);
            return Line::Progress {
                done: num(f[0]),
                total: num(f[1]).or(est),
                index: num(f[3]),
                count: num(f[4]),
                title: f[5].to_string(),
            };
        }
    }
    if let Some(title) = line.strip_prefix("SBPP\t") {
        return Line::Converting { title: title.to_string() };
    }
    if let Some(e) = line.trim().strip_prefix("ERROR: ") {
        return Line::Error(e.to_string());
    }
    Line::Other
}

/// Managed tools go first on PATH, which is how yt-dlp finds Deno on every
/// version without needing the newer `--js-runtimes` flag.
pub fn spawn(ytdlp: &Path, tools_dir: &Path, args: &[OsString]) -> Result<Child> {
    let mut path: Vec<PathBuf> = vec![tools_dir.to_path_buf(), tools_dir.join("ffmpeg")];
    if let Some(existing) = std::env::var_os("PATH") {
        path.extend(std::env::split_paths(&existing));
    }
    let mut cmd = tools::command(ytdlp);
    cmd.args(args)
        .env("PATH", std::env::join_paths(path)?)
        .env("PYTHONIOENCODING", "utf-8")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // Its own group, so cancelling also stops the ffmpeg it spawns.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    cmd.spawn().map_err(|e| anyhow!("could not start yt-dlp: {e}"))
}

/// The PyInstaller build is a launcher plus the real process, and ffmpeg may be
/// running under that, so the whole tree has to go.
pub fn kill_tree(pid: u32) {
    #[cfg(windows)]
    {
        let _ = tools::command(Path::new("taskkill"))
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .output();
    }
    #[cfg(unix)]
    {
        let _ = std::process::Command::new("kill").args(["-TERM", &format!("-{pid}")]).output();
    }
}

/// Feeds every line from both streams to `on_line` until yt-dlp exits. Returns
/// the last error yt-dlp printed, if it failed.
pub fn drive(mut child: Child, mut on_line: impl FnMut(Line)) -> Result<(), String> {
    let (tx, rx) = std::sync::mpsc::channel::<String>();
    let readers: Vec<_> = [
        child.stdout.take().map(|s| Box::new(s) as Box<dyn std::io::Read + Send>),
        child.stderr.take().map(|s| Box::new(s) as Box<dyn std::io::Read + Send>),
    ]
    .into_iter()
    .flatten()
    .map(|stream| {
        let tx = tx.clone();
        std::thread::spawn(move || {
            for line in BufReader::new(stream).split(b'\n').map_while(Result::ok) {
                if tx.send(String::from_utf8_lossy(&line).into_owned()).is_err() {
                    break;
                }
            }
        })
    })
    .collect();
    drop(tx);

    let mut last_error = None;
    for line in rx {
        match parse_line(&line) {
            Line::Error(e) => last_error = Some(e),
            Line::Other => {}
            l => on_line(l),
        }
    }
    for r in readers {
        let _ = r.join();
    }
    match child.wait() {
        Ok(s) if s.success() => Ok(()),
        Ok(s) => Err(last_error.unwrap_or_else(|| format!("yt-dlp stopped ({s})"))),
        Err(e) => Err(e.to_string()),
    }
}

pub fn read_files_log(path: &Path) -> Vec<PathBuf> {
    let Ok(text) = std::fs::read_to_string(path) else { return Vec::new() };
    let mut seen = std::collections::HashSet::new();
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(PathBuf::from)
        .filter(|p| p.is_file() && seen.insert(p.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req() -> Request {
        Request {
            url: "https://example.com/v".into(),
            dest: "/lib/in".into(),
            format: Format::Original,
            playlist: false,
            clip: None,
        }
    }

    fn strs(a: &[OsString]) -> Vec<String> {
        a.iter().map(|s| s.to_string_lossy().to_string()).collect()
    }

    #[test]
    fn progress_line_parses() {
        let l = parse_line("SBDL\t1024\tNA\t4096.7\t2\t10\tA\ttitle with tab\r");
        assert_eq!(
            l,
            Line::Progress {
                done: Some(1024),
                total: Some(4096),
                index: Some(2),
                count: Some(10),
                title: "A\ttitle with tab".into()
            }
        );
        let l = parse_line("SBDL\tNA\tNA\tNA\tNA\tNA\tx");
        assert!(matches!(l, Line::Progress { done: None, total: None, index: None, .. }));
        assert_eq!(parse_line("ERROR: [youtube] gone"), Line::Error("[youtube] gone".into()));
        assert_eq!(parse_line("[download] 5%"), Line::Other);
    }

    #[test]
    fn original_needs_no_ffmpeg_but_clips_do() {
        let mut r = req();
        assert!(!r.needs_ffmpeg());
        r.clip = Some(("1:00".into(), "1:05".into()));
        assert!(r.needs_ffmpeg());
        r.clip = None;
        r.format = Format::Flac;
        assert!(r.needs_ffmpeg());
    }

    #[test]
    fn url_goes_last_after_separator() {
        let a = strs(&args(&req(), &Resolved::default(), Path::new("/t"), Path::new("/t/f")));
        assert_eq!(&a[a.len() - 2..], ["--", "https://example.com/v"]);
        assert!(a.contains(&"--no-playlist".to_string()));
        assert!(!a.contains(&"-x".to_string()));
        assert!(!a.contains(&"--ffmpeg-location".to_string()));
    }

    #[test]
    fn conversion_and_clip_args() {
        let mut r = req();
        r.format = Format::Wav;
        r.clip = Some(("5".into(), "inf".into()));
        let tools = Resolved { ffmpeg: Some(PathBuf::from("/x/ff/ffmpeg")), ..Default::default() };
        let a = strs(&args(&r, &tools, Path::new("/t"), Path::new("/t/f")));
        let after = |flag: &str| a[a.iter().position(|s| s == flag).unwrap() + 1].clone();
        assert_eq!(after("--audio-format"), "wav");
        assert_eq!(after("--download-sections"), "*5-inf");
        assert_eq!(after("--ffmpeg-location"), "/x/ff");
    }

    #[test]
    fn validation() {
        let mut r = req();
        assert!(r.validate().is_ok());
        r.url = "file:///etc/passwd".into();
        assert!(r.validate().is_err());
        r = req();
        r.clip = Some(("1:00".into(), "two".into()));
        assert!(r.validate().is_err());
        r.clip = Some(("".into(), "inf".into()));
        assert!(r.validate().is_err());
    }
}
