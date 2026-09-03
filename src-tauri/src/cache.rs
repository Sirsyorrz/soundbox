use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Result};

const MAGIC: &[u8; 4] = b"SBPK";
/// Bump to invalidate every cached blob without a schema migration.
pub const VERSION: u16 = 1;
/// Coarse level: ~10 KB per 3-minute stereo file.
pub const BUCKET_SAMPLES: usize = 2048;

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
    let buckets = frames.div_ceil(BUCKET_SAMPLES).max(1);
    let mut data = vec![Vec::with_capacity(buckets); d.channels];

    for b in 0..buckets {
        let start = b * BUCKET_SAMPLES;
        let end = ((b + 1) * BUCKET_SAMPLES).min(frames);
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
        bucket_samples: BUCKET_SAMPLES,
        data,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::Decoded;

    fn synth(channels: usize, frames: usize) -> Decoded {
        let samples = (0..frames * channels)
            .map(|i| ((i / channels) as f32 * 0.01).sin() * if i % channels == 0 { 1.0 } else { 0.5 })
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
            assert_eq!(back.bucket_samples, BUCKET_SAMPLES);
            assert_eq!(back.buckets(), p.buckets());
            assert_eq!(back.data, p.data);
        }
    }

    #[test]
    fn peaks_bracket_the_signal() {
        let d = synth(2, 10_000);
        let f = build(&d).to_f32();
        for (c, ch) in f.iter().enumerate() {
            for (b, (lo, hi)) in ch.iter().enumerate() {
                let start = b * BUCKET_SAMPLES;
                let end = ((b + 1) * BUCKET_SAMPLES).min(d.frames());
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
    fn blob_path_shards() {
        let p = blob_path(std::path::Path::new("/c"), "b3:abcdef");
        assert!(p.ends_with("peaks/ab/cdef.pk"), "{p:?}");
    }
}
