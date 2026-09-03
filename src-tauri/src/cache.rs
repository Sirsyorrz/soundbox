use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Result};

const MAGIC: &[u8; 4] = b"SBPK";
/// Bump to invalidate every cached blob without a schema migration.
pub const VERSION: u16 = 2;
/// Coarse level: ~10 KB per 3-minute stereo file.
pub const BUCKET_SAMPLES: usize = 2048;
/// Floor on bucket count so short sounds still have a drawable sparkline.
/// At 2048 samples/bucket a 0.3 s clip is only 6 buckets, which renders as a
/// handful of blocks. Costs nothing: short files are small either way.
pub const MIN_BUCKETS: usize = 128;

pub struct Peaks {
    pub sample_rate: u32,
    pub channels: usize,
    pub bucket_samples: usize,
    /// Per channel, min/max pairs quantised to i16.
    pub data: Vec<Vec<(i16, i16)>>,
}

impl Peaks {
    pub fn buckets(&self) -> usize {
        self.data.first().map(|c| c.len()).unwrap_or(0)
    }

    pub fn to_f32(&self) -> Vec<Vec<(f32, f32)>> {
        self.data
            .iter()
            .map(|ch| {
                ch.iter()
                    .map(|(lo, hi)| (*lo as f32 / i16::MAX as f32, *hi as f32 / i16::MAX as f32))
                    .collect()
            })
            .collect()
    }
}

impl Peaks {
    /// Channel-summed min/max at an arbitrary width, for row sparklines.
    ///
    /// Summing rather than taking channel 0 avoids a hard-panned sound looking
    /// silent in the list.
    pub fn mono_downsample(&self, width: usize) -> Vec<(f32, f32)> {
        let src = self.buckets();
        if src == 0 || width == 0 {
            return Vec::new();
        }
        let width = width.min(src);
        let per = src as f64 / width as f64;
        let scale = 1.0 / i16::MAX as f32;

        (0..width)
            .map(|b| {
                let start = (b as f64 * per) as usize;
                let end = (((b + 1) as f64 * per) as usize).clamp(start + 1, src);
                let mut lo = 0i16;
                let mut hi = 0i16;
                for ch in &self.data {
                    for s in &ch[start..end] {
                        if s.0 < lo {
                            lo = s.0;
                        }
                        if s.1 > hi {
                            hi = s.1;
                        }
                    }
                }
                (lo as f32 * scale, hi as f32 * scale)
            })
            .collect()
    }
}

pub fn blob_path(cache_dir: &Path, content_key: &str) -> PathBuf {
    // Strip the "b3:" prefix and shard by the first two hex chars; a flat
    // directory of 10k+ entries is slow to list on Windows.
    let hex = content_key.strip_prefix("b3:").unwrap_or(content_key);
    let (shard, rest) = hex.split_at(2.min(hex.len()));
    cache_dir.join("peaks").join(shard).join(format!("{rest}.pk"))
}

pub fn encode(p: &Peaks) -> Vec<u8> {
    let mut out = Vec::with_capacity(20 + p.channels * p.buckets() * 4);
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&VERSION.to_le_bytes());
    out.extend_from_slice(&p.sample_rate.to_le_bytes());
    out.extend_from_slice(&(p.channels as u16).to_le_bytes());
    out.extend_from_slice(&(p.bucket_samples as u32).to_le_bytes());
    out.extend_from_slice(&(p.buckets() as u32).to_le_bytes());
    for ch in &p.data {
        for (lo, hi) in ch {
            out.extend_from_slice(&lo.to_le_bytes());
            out.extend_from_slice(&hi.to_le_bytes());
        }
    }
    out
}

pub fn decode(bytes: &[u8]) -> Result<Peaks> {
    if bytes.len() < 16 || &bytes[0..4] != MAGIC {
        return Err(anyhow!("not a peaks blob"));
    }
    let u16at = |o: usize| u16::from_le_bytes([bytes[o], bytes[o + 1]]);
    let u32at = |o: usize| u32::from_le_bytes([bytes[o], bytes[o + 1], bytes[o + 2], bytes[o + 3]]);

    let version = u16at(4);
    if version != VERSION {
        return Err(anyhow!("peaks version {version}, expected {VERSION}"));
    }
    let sample_rate = u32at(6);
    let channels = u16at(10) as usize;
    let bucket_samples = u32at(12) as usize;
    let buckets = u32at(16) as usize;

    let need = 20 + channels * buckets * 4;
    if bytes.len() < need {
        return Err(anyhow!("peaks blob truncated: {} < {}", bytes.len(), need));
    }

    let mut data = Vec::with_capacity(channels);
    let mut o = 20;
    for _ in 0..channels {
        let mut ch = Vec::with_capacity(buckets);
        for _ in 0..buckets {
            let lo = i16::from_le_bytes([bytes[o], bytes[o + 1]]);
            let hi = i16::from_le_bytes([bytes[o + 2], bytes[o + 3]]);
            ch.push((lo, hi));
            o += 4;
        }
        data.push(ch);
    }
    Ok(Peaks { sample_rate, channels, bucket_samples, data })
}

pub fn write(cache_dir: &Path, content_key: &str, p: &Peaks) -> Result<()> {
    let path = blob_path(cache_dir, content_key);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    // Duplicate files share a content_key, so parallel scans can target the same
    // blob concurrently. The temp name must be unique per writer or they race on
    // rename and one loses its file.
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let tmp = path.with_extension(format!("{}.{n}.tmp", std::process::id()));

    std::fs::File::create(&tmp)?.write_all(&encode(p))?;
    std::fs::rename(&tmp, &path)?;
    Ok(())
}

pub fn read(cache_dir: &Path, content_key: &str) -> Result<Peaks> {
    let mut buf = Vec::new();
    std::fs::File::open(blob_path(cache_dir, content_key))?.read_to_end(&mut buf)?;
    decode(&buf)
}

pub fn build(d: &crate::audio::Decoded) -> Peaks {
    let frames = d.frames();
    build_n(d, frames.div_ceil(BUCKET_SAMPLES).max(1).max(MIN_BUCKETS.min(frames)))
}

/// Peaks at an explicit resolution, for the detail view.
///
/// The cached level is far too coarse to draw a short sound: a 1-second clip is
/// only ~21 buckets, which renders as visible blocks. Detail views ask for a
/// bucket count matched to the canvas instead.
pub fn build_n(d: &crate::audio::Decoded, buckets: usize) -> Peaks {
    let frames = d.frames();
    let buckets = buckets.clamp(1, frames.max(1));
    let per = frames as f64 / buckets as f64;
    let mut data = vec![Vec::with_capacity(buckets); d.channels];

    for b in 0..buckets {
        let start = (b as f64 * per) as usize;
        let end = (((b + 1) as f64 * per) as usize).clamp(start + 1, frames);
        for (ch, out) in data.iter_mut().enumerate() {
            let mut lo = 0.0f32;
            let mut hi = 0.0f32;
            for f in start..end {
                let v = d.samples[f * d.channels + ch];
                if v < lo {
                    lo = v;
                }
                if v > hi {
                    hi = v;
                }
            }
            let q = |v: f32| (v.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
            out.push((q(lo), q(hi)));
        }
    }

    Peaks {
        sample_rate: d.sample_rate,
        channels: d.channels,
        bucket_samples: per.round().max(1.0) as usize,
        data,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::Decoded;

    fn synth(channels: usize, frames: usize) -> Decoded {
        let samples = (0..frames * channels)
            .map(|i| {
                ((i / channels) as f32 * 0.01).sin() * if i % channels == 0 { 1.0 } else { 0.5 }
            })
            .collect();
        Decoded { samples, channels, sample_rate: 48000 }
    }

    #[test]
    fn roundtrip_preserves_shape_and_values() {
        for ch in [1usize, 2, 6] {
            let p = build(&synth(ch, 10_000));
            let back = decode(&encode(&p)).unwrap();
            assert_eq!(back.channels, ch);
            assert_eq!(back.sample_rate, 48000);
            assert_eq!(back.bucket_samples, p.bucket_samples);
            assert_eq!(back.buckets(), p.buckets());
            assert_eq!(back.data, p.data);
        }
    }

    #[test]
    fn detail_resolution_is_honoured() {
        let d = synth(2, 44_100);
        assert_eq!(build_n(&d, 4000).buckets(), 4000);
        // A short file cannot have more buckets than it has frames.
        assert_eq!(build_n(&synth(1, 300), 4000).buckets(), 300);
    }

    #[test]
    fn peaks_bracket_the_signal() {
        let d = synth(2, 10_000);
        let p = build(&d);
        let per = d.frames() as f64 / p.buckets() as f64;
        let f = p.to_f32();
        for (c, ch) in f.iter().enumerate() {
            for (b, (lo, hi)) in ch.iter().enumerate() {
                let start = (b as f64 * per) as usize;
                let end = (((b + 1) as f64 * per) as usize).clamp(start + 1, d.frames());
                for i in start..end {
                    let v = d.samples[i * d.channels + c];
                    assert!(v >= lo - 1e-3 && v <= hi + 1e-3, "sample {v} outside [{lo},{hi}]");
                }
            }
        }
    }

    #[test]
    fn rejects_garbage_and_wrong_version() {
        assert!(decode(b"nope").is_err());
        let mut b = encode(&build(&synth(2, 4096)));
        b[4] = 99;
        assert!(decode(&b).is_err());
    }

    #[test]
    fn mono_downsample_sums_channels_and_hits_width() {
        // Silent left, loud right: taking channel 0 would look like silence.
        let frames = 8192;
        let samples = (0..frames * 2).map(|i| if i % 2 == 0 { 0.0 } else { 0.8 }).collect();
        let d = Decoded { samples, channels: 2, sample_rate: 48000 };
        let spark = build(&d).mono_downsample(50);
        assert_eq!(spark.len(), 50);
        assert!(spark.iter().any(|(_, hi)| *hi > 0.5), "right channel should show, got {spark:?}");
    }

    #[test]
    fn short_files_still_get_a_drawable_sparkline() {
        // 0.3 s at 44.1k is ~6 buckets without the floor.
        let d = synth(1, 13_230);
        assert!(build(&d).buckets() >= MIN_BUCKETS);
        // Never more buckets than frames.
        assert_eq!(build(&synth(1, 40)).buckets(), 40);
    }

    #[test]
    fn mono_downsample_handles_degenerate_widths() {
        let d = synth(2, 5000);
        let p = build(&d);
        assert!(p.mono_downsample(0).is_empty());
        // Cannot invent more detail than the cache holds.
        assert!(p.mono_downsample(10_000).len() <= p.buckets());
    }

    #[test]
    fn blob_path_shards() {
        let p = blob_path(std::path::Path::new("/c"), "b3:abcdef");
        assert!(p.ends_with("peaks/ab/cdef.pk"), "{p:?}");
    }
}
