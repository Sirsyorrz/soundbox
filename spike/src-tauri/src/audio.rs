use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use anyhow::{anyhow, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

pub struct Decoded {
    /// Interleaved.
    pub samples: Vec<f32>,
    pub channels: usize,
    pub sample_rate: u32,
}

impl Decoded {
    pub fn frames(&self) -> usize {
        if self.channels == 0 {
            0
        } else {
            self.samples.len() / self.channels
        }
    }
    pub fn duration_ms(&self) -> u64 {
        if self.sample_rate == 0 {
            0
        } else {
            self.frames() as u64 * 1000 / self.sample_rate as u64
        }
    }
}

pub fn decode_file(path: &Path) -> Result<Decoded> {
    let file = std::fs::File::open(path)?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());

    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }

    let probed = symphonia::default::get_probe().format(
        &hint,
        mss,
        &FormatOptions { enable_gapless: true, ..Default::default() },
        &MetadataOptions::default(),
    )?;

    let mut format = probed.format;
    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != symphonia::core::codecs::CODEC_TYPE_NULL)
        .ok_or_else(|| anyhow!("no decodable audio track"))?;

    let track_id = track.id;
    let mut decoder =
        symphonia::default::get_codecs().make(&track.codec_params, &DecoderOptions::default())?;

    let mut sample_rate = track.codec_params.sample_rate.unwrap_or(0);
    let mut channels = track.codec_params.channels.map(|c| c.count()).unwrap_or(0);
    let mut samples: Vec<f32> = Vec::new();
    let mut buf: Option<SampleBuffer<f32>> = None;

    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            // 0.5 signals clean EOF as an UnexpectedEof io error.
            Err(symphonia::core::errors::Error::IoError(e))
                if e.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                break
            }
            Err(symphonia::core::errors::Error::ResetRequired) => break,
            Err(e) => return Err(e.into()),
        };
        if packet.track_id() != track_id {
            continue;
        }
        match decoder.decode(&packet) {
            Ok(audio) => {
                let spec = *audio.spec();
                if sample_rate == 0 {
                    sample_rate = spec.rate;
                }
                if channels == 0 {
                    channels = spec.channels.count();
                }
                let b = buf.get_or_insert_with(|| {
                    SampleBuffer::<f32>::new(audio.capacity() as u64, spec)
                });
                b.copy_interleaved_ref(audio);
                samples.extend_from_slice(b.samples());
            }
            // Malformed packets mid-file are recoverable; skip them.
            Err(symphonia::core::errors::Error::DecodeError(_)) => continue,
            Err(e) => return Err(e.into()),
        }
    }

    if samples.is_empty() {
        return Err(anyhow!("decoded zero samples"));
    }
    Ok(Decoded { samples, channels: channels.max(1), sample_rate })
}

/// Downsample to min/max pairs per bucket, per channel.
pub fn build_peaks(d: &Decoded, buckets: usize) -> Vec<Vec<(f32, f32)>> {
    let frames = d.frames();
    let buckets = buckets.max(1).min(frames.max(1));
    let per = (frames as f64 / buckets as f64).max(1.0);

    (0..d.channels)
        .map(|ch| {
            (0..buckets)
                .map(|b| {
                    let start = (b as f64 * per) as usize;
                    let end = (((b + 1) as f64 * per) as usize).min(frames);
                    let mut lo = 0.0f32;
                    let mut hi = 0.0f32;
                    for f in start..end.max(start + 1).min(frames) {
                        let v = d.samples[f * d.channels + ch];
                        if v < lo {
                            lo = v;
                        }
                        if v > hi {
                            hi = v;
                        }
                    }
                    (lo, hi)
                })
                .collect()
        })
        .collect()
}

struct Shared {
    samples: Arc<Vec<f32>>,
    channels: usize,
    /// Resample ratio: source frames advanced per output frame.
    step: f64,
    pos: f64,
    region: (usize, usize),
    looping: bool,
    gain: f32,
}

impl Shared {
    fn empty() -> Self {
        Shared {
            samples: Arc::new(Vec::new()),
            channels: 1,
            step: 1.0,
            pos: 0.0,
            region: (0, 0),
            looping: false,
            gain: 1.0,
        }
    }
}

pub struct Engine {
    shared: Arc<Mutex<Shared>>,
    playing: Arc<AtomicBool>,
    /// Playhead in source frames, for the UI to poll.
    pos_frames: Arc<AtomicU64>,
    _stream: cpal::Stream,
    out_channels: usize,
    out_rate: u32,
}

// cpal::Stream is !Send on some backends. Access is serialised by a Mutex, but
// that does not make the stream handle itself safe to move across threads, so
// this is an assertion rather than a proof. Holds on ALSA/WASAPI; the real app
// should own the stream on a dedicated thread and drive it over a channel.
unsafe impl Send for Engine {}
unsafe impl Sync for Engine {}

const FADE_FRAMES: f64 = 240.0; // ~5 ms at 48k, declicks region boundaries

impl Engine {
    pub fn new() -> Result<Self> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| anyhow!("no default output device"))?;
        let config = device.default_output_config()?;
        let out_rate = config.sample_rate().0;
        let out_channels = config.channels() as usize;

        let shared = Arc::new(Mutex::new(Shared::empty()));
        let playing = Arc::new(AtomicBool::new(false));
        let pos_frames = Arc::new(AtomicU64::new(0));

        let cb_shared = shared.clone();
        let cb_playing = playing.clone();
        let cb_pos = pos_frames.clone();

        let stream = device.build_output_stream(
            &config.config(),
            move |out: &mut [f32], _| {
                let frames = out.len() / out_channels;
                if !cb_playing.load(Ordering::Relaxed) {
                    out.fill(0.0);
                    return;
                }
                // Never block the audio thread; drop a buffer instead.
                let Ok(mut s) = cb_shared.try_lock() else {
                    out.fill(0.0);
                    return;
                };
                let (rs, re) = s.region;
                if s.samples.is_empty() || re <= rs {
                    out.fill(0.0);
                    cb_playing.store(false, Ordering::Relaxed);
                    return;
                }

                for f in 0..frames {
                    if s.pos >= re as f64 {
                        if s.looping {
                            s.pos = rs as f64;
                        } else {
                            for c in 0..out_channels {
                                out[f * out_channels + c] = 0.0;
                            }
                            cb_playing.store(false, Ordering::Relaxed);
                            continue;
                        }
                    }
                    let i = s.pos as usize;
                    let from_start = s.pos - rs as f64;
                    let to_end = re as f64 - s.pos;
                    let fade = (from_start / FADE_FRAMES).min(to_end / FADE_FRAMES).clamp(0.0, 1.0)
                        as f32;
                    let g = s.gain * fade;

                    for c in 0..out_channels {
                        let src_c = c.min(s.channels - 1);
                        let idx = i * s.channels + src_c;
                        let v = s.samples.get(idx).copied().unwrap_or(0.0);
                        out[f * out_channels + c] = v * g;
                    }
                    s.pos += s.step;
                }
                cb_pos.store(s.pos as u64, Ordering::Relaxed);
            },
            |err| eprintln!("audio stream error: {err}"),
            None,
        )?;
        stream.play()?;

        Ok(Engine { shared, playing, pos_frames, _stream: stream, out_channels, out_rate })
    }

    pub fn load(&self, d: &Decoded, gain: f32) {
        let mut s = self.shared.lock().unwrap();
        s.samples = Arc::new(d.samples.clone());
        s.channels = d.channels;
        s.step = d.sample_rate as f64 / self.out_rate as f64;
        s.region = (0, d.frames());
        s.pos = 0.0;
        s.gain = gain;
        self.playing.store(false, Ordering::Relaxed);
    }

    pub fn play_region(&self, start: usize, end: usize, looping: bool) {
        {
            let mut s = self.shared.lock().unwrap();
            let total = if s.channels == 0 { 0 } else { s.samples.len() / s.channels };
            let start = start.min(total);
            let end = end.min(total).max(start);
            s.region = (start, end);
            s.pos = start as f64;
            s.looping = looping;
        }
        self.playing.store(true, Ordering::Relaxed);
    }

    pub fn stop(&self) {
        self.playing.store(false, Ordering::Relaxed);
    }

    /// Play/pause toggle. Resumes from the playhead; restarts only if the
    /// playhead has run past the region end.
    pub fn toggle(&self) -> bool {
        if self.playing.load(Ordering::Relaxed) {
            self.playing.store(false, Ordering::Relaxed);
            return false;
        }
        {
            let mut s = self.shared.lock().unwrap();
            let (rs, re) = s.region;
            if re <= rs || s.samples.is_empty() {
                return false;
            }
            if s.pos >= re as f64 || s.pos < rs as f64 {
                s.pos = rs as f64;
            }
        }
        self.playing.store(true, Ordering::Relaxed);
        true
    }

    pub fn set_looping(&self, looping: bool) {
        self.shared.lock().unwrap().looping = looping;
    }

    pub fn position(&self) -> u64 {
        self.pos_frames.load(Ordering::Relaxed)
    }

    pub fn is_playing(&self) -> bool {
        self.playing.load(Ordering::Relaxed)
    }

    pub fn output_info(&self) -> (u32, usize) {
        (self.out_rate, self.out_channels)
    }
}
