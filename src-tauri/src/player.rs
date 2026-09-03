use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};

use anyhow::{anyhow, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use crate::audio::Decoded;

/// ~5 ms at 48k. Ramps region edges so seeking mid-waveform does not click.
const FADE_FRAMES: f64 = 240.0;

pub enum Cmd {
    Load { audio: Arc<Decoded>, gain: f32 },
    Play { start: usize, end: usize, looping: bool },
    Toggle,
    Stop,
    SetLooping(bool),
    SetGain(f32),
    SetVolume(f32),
    Quit,
}

struct Shared {
    audio: Option<Arc<Decoded>>,
    step: f64,
    pos: f64,
    region: (usize, usize),
    looping: bool,
    /// Per-file normalisation.
    gain: f32,
    /// Master volume, set by the user.
    volume: f32,
}

/// Handle to the audio thread.
///
/// `cpal::Stream` is `!Send` on some backends, so it never leaves the thread that
/// created it; all interaction is via the command channel.
pub struct Player {
    tx: Sender<Cmd>,
    pos: Arc<AtomicU64>,
    playing: Arc<AtomicBool>,
    output: Arc<Mutex<Option<(u32, usize)>>>,
}

impl Player {
    pub fn spawn() -> Result<Self> {
        let (tx, rx) = channel::<Cmd>();
        let pos = Arc::new(AtomicU64::new(0));
        let playing = Arc::new(AtomicBool::new(false));
        let output: Arc<Mutex<Option<(u32, usize)>>> = Arc::new(Mutex::new(None));
        let (ready_tx, ready_rx) = channel::<Result<(), String>>();

        {
            let pos = pos.clone();
            let playing = playing.clone();
            let output = output.clone();
            std::thread::Builder::new().name("soundbox-audio".into()).spawn(move || {
                match build_stream(pos, playing.clone()) {
                    Ok((stream, shared, rate, channels)) => {
                        *output.lock().unwrap() = Some((rate, channels));
                        let _ = ready_tx.send(Ok(()));
                        run(rx, shared, playing, rate);
                        drop(stream);
                    }
                    Err(e) => {
                        let _ = ready_tx.send(Err(e.to_string()));
                    }
                }
            })?;
        }

        match ready_rx.recv() {
            Ok(Ok(())) => Ok(Player { tx, pos, playing, output }),
            Ok(Err(e)) => Err(anyhow!(e)),
            Err(e) => Err(anyhow!(e)),
        }
    }

    pub fn send(&self, c: Cmd) {
        let _ = self.tx.send(c);
    }

    /// Playhead in source frames.
    pub fn position(&self) -> u64 {
        self.pos.load(Ordering::Relaxed)
    }

    pub fn is_playing(&self) -> bool {
        self.playing.load(Ordering::Relaxed)
    }

    pub fn output_info(&self) -> Option<(u32, usize)> {
        *self.output.lock().unwrap()
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        let _ = self.tx.send(Cmd::Quit);
    }
}

type Built = (cpal::Stream, Arc<Mutex<Shared>>, u32, usize);

fn build_stream(pos: Arc<AtomicU64>, playing: Arc<AtomicBool>) -> Result<Built> {
    let device = cpal::default_host()
        .default_output_device()
        .ok_or_else(|| anyhow!("no default output device"))?;
    let config = device.default_output_config()?;
    let rate = config.sample_rate().0;
    let channels = config.channels() as usize;

    let shared = Arc::new(Mutex::new(Shared {
        audio: None,
        step: 1.0,
        pos: 0.0,
        region: (0, 0),
        looping: false,
        gain: 1.0,
        volume: 1.0,
    }));

    let cb = shared.clone();
    let stream = device.build_output_stream(
        &config.config(),
        move |out: &mut [f32], _| {
            if !playing.load(Ordering::Relaxed) {
                out.fill(0.0);
                return;
            }
            // Never block the audio thread; emit silence on contention instead.
            let Ok(mut s) = cb.try_lock() else {
                out.fill(0.0);
                return;
            };
            let Some(audio) = s.audio.clone() else {
                out.fill(0.0);
                return;
            };
            let (rs, re) = s.region;
            if re <= rs {
                out.fill(0.0);
                playing.store(false, Ordering::Relaxed);
                return;
            }

            let src_ch = audio.channels;
            for f in 0..out.len() / channels {
                if s.pos >= re as f64 {
                    if s.looping {
                        s.pos = rs as f64;
                    } else {
                        for c in 0..channels {
                            out[f * channels + c] = 0.0;
                        }
                        playing.store(false, Ordering::Relaxed);
                        continue;
                    }
                }
                let i = s.pos as usize;
                let fade = ((s.pos - rs as f64) / FADE_FRAMES)
                    .min((re as f64 - s.pos) / FADE_FRAMES)
                    .clamp(0.0, 1.0) as f32;
                let g = s.gain * s.volume * fade;
                for c in 0..channels {
                    let v = audio.samples.get(i * src_ch + c.min(src_ch - 1)).copied().unwrap_or(0.0);
                    out[f * channels + c] = v * g;
                }
                s.pos += s.step;
            }
            pos.store(s.pos as u64, Ordering::Relaxed);
        },
        |e| eprintln!("audio stream error: {e}"),
        None,
    )?;
    stream.play()?;
    Ok((stream, shared, rate, channels))
}

fn run(
    rx: std::sync::mpsc::Receiver<Cmd>,
    shared: Arc<Mutex<Shared>>,
    playing: Arc<AtomicBool>,
    out_rate: u32,
) {
    while let Ok(cmd) = rx.recv() {
        let mut s = shared.lock().unwrap();
        match cmd {
            Cmd::Load { audio, gain } => {
                playing.store(false, Ordering::Relaxed);
                s.step = audio.sample_rate as f64 / out_rate as f64;
                s.region = (0, audio.frames());
                s.pos = 0.0;
                s.gain = gain;
                s.audio = Some(audio);
            }
            Cmd::Play { start, end, looping } => {
                let total = s.audio.as_ref().map(|a| a.frames()).unwrap_or(0);
                let start = start.min(total);
                s.region = (start, end.min(total).max(start));
                s.pos = start as f64;
                s.looping = looping;
                playing.store(total > 0, Ordering::Relaxed);
            }
            Cmd::Toggle => {
                if playing.load(Ordering::Relaxed) {
                    playing.store(false, Ordering::Relaxed);
                } else {
                    let (rs, re) = s.region;
                    if re > rs {
                        if s.pos >= re as f64 || s.pos < rs as f64 {
                            s.pos = rs as f64;
                        }
                        playing.store(true, Ordering::Relaxed);
                    }
                }
            }
            Cmd::Stop => {
                playing.store(false, Ordering::Relaxed);
                s.pos = s.region.0 as f64;
            }
            Cmd::SetLooping(l) => s.looping = l,
            Cmd::SetGain(g) => s.gain = g,
            Cmd::SetVolume(v) => s.volume = v.clamp(0.0, 2.0),
            Cmd::Quit => return,
        }
    }
}

/// Preview gain that brings a file to `target` LUFS, clamped so a very quiet
/// file is not amplified into clipping.
pub fn normalise_gain(lufs: Option<f64>, target: f64, peak_db: f32) -> f32 {
    let Some(lufs) = lufs else { return 1.0 };
    let desired_db = target - lufs;
    let headroom_db = (-0.5 - peak_db as f64).max(0.0);
    let db = desired_db.min(headroom_db).clamp(-24.0, 24.0);
    10f64.powf(db / 20.0) as f32
}
