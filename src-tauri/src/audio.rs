use anyhow::{anyhow, Result};
use std::path::Path;
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
        self.samples.len().checked_div(self.channels).unwrap_or(0)
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
                let b = buf
                    .get_or_insert_with(|| SampleBuffer::<f32>::new(audio.capacity() as u64, spec));
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
