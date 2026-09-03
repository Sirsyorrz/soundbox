use std::sync::Arc;

use rustfft::{num_complex::Complex32, Fft, FftPlanner};

use crate::audio::Decoded;

const FRAME: usize = 1024;
const HOP: usize = 512;
const BANDS: usize = 16;
pub const FEATURE_DIM: usize = 40;

/// EBU R128 integrated loudness (LUFS). `None` for signals too short or too
/// quiet for the standard to produce a meaningful gated measurement.
pub fn loudness(d: &Decoded) -> Option<f64> {
    let mut m = ebur128::EbuR128::new(d.channels as u32, d.sample_rate, ebur128::Mode::I).ok()?;
    m.add_frames_f32(&d.samples).ok()?;
    match m.loudness_global() {
        Ok(l) if l.is_finite() => Some(l),
        _ => None,
    }
}

pub fn peak_db(d: &Decoded) -> f32 {
    let peak = d.samples.iter().fold(0.0f32, |a, s| a.max(s.abs()));
    if peak <= 0.0 {
        -120.0
    } else {
        20.0 * peak.log10()
    }
}

struct Welford {
    n: f64,
    mean: f64,
    m2: f64,
}

impl Welford {
    fn new() -> Self {
        Welford { n: 0.0, mean: 0.0, m2: 0.0 }
    }
    fn push(&mut self, x: f64) {
        if !x.is_finite() {
            return;
        }
        self.n += 1.0;
        let d = x - self.mean;
        self.mean += d / self.n;
        self.m2 += d * (x - self.mean);
    }
    fn mean_var(&self) -> (f64, f64) {
        if self.n < 2.0 {
            (self.mean, 0.0)
        } else {
            (self.mean, self.m2 / self.n)
        }
    }
}

/// Fixed-length timbral descriptor used for "sounds like this".
///
/// Log-spaced band energies rather than true MFCCs: no DCT stage, but for
/// nearest-neighbour lookup over a sound library the two behave similarly and
/// this avoids a mel filterbank dependency.
///
/// Layout (40 dims):
///   0..32   16 log-band energies, mean and variance
///   32..38  centroid, rolloff85, flatness, bandwidth, ZCR, RMS (means)
///   38      crest factor
///   39      log duration
pub fn features(d: &Decoded) -> Vec<f32> {
    let mut out = vec![0.0f32; FEATURE_DIM];
    let frames = d.frames();
    if frames == 0 || d.sample_rate == 0 {
        return out;
    }

    // Mono sum; similarity should not depend on channel layout.
    let mono: Vec<f32> = (0..frames)
        .map(|i| {
            let mut s = 0.0;
            for c in 0..d.channels {
                s += d.samples[i * d.channels + c];
            }
            s / d.channels as f32
        })
        .collect();

    let mut planner = FftPlanner::<f32>::new();
    let fft: Arc<dyn Fft<f32>> = planner.plan_fft_forward(FRAME);
    let window: Vec<f32> = (0..FRAME)
        .map(|i| {
            let x = std::f32::consts::PI * 2.0 * i as f32 / FRAME as f32;
            0.5 - 0.5 * x.cos()
        })
        .collect();

    // 20 Hz floor: below that is inaudible rumble that would dominate a log scale.
    let nyq = d.sample_rate as f32 / 2.0;
    let lo = 20.0f32.min(nyq * 0.5);
    let edges: Vec<usize> = (0..=BANDS)
        .map(|b| {
            let f = lo * (nyq / lo).powf(b as f32 / BANDS as f32);
            ((f / nyq) * (FRAME / 2) as f32).round().clamp(0.0, (FRAME / 2 - 1) as f32) as usize
        })
        .collect();

    let mut band_stats: Vec<Welford> = (0..BANDS).map(|_| Welford::new()).collect();
    let (mut cen, mut rol, mut fla, mut bwd) =
        (Welford::new(), Welford::new(), Welford::new(), Welford::new());
    let mut rms_stat = Welford::new();

    let mut buf = vec![Complex32::new(0.0, 0.0); FRAME];
    let mut mag = vec![0.0f32; FRAME / 2];

    let mut pos = 0;
    while pos + FRAME <= mono.len() {
        for i in 0..FRAME {
            buf[i] = Complex32::new(mono[pos + i] * window[i], 0.0);
        }
        fft.process(&mut buf);
        for i in 0..FRAME / 2 {
            mag[i] = buf[i].norm();
        }

        let total: f32 = mag.iter().sum();
        if total > 1e-9 {
            for b in 0..BANDS {
                let (s, e) = (edges[b], edges[b + 1].max(edges[b] + 1));
                let energy: f32 = mag[s..e.min(mag.len())].iter().map(|m| m * m).sum();
                band_stats[b].push((energy + 1e-10).ln() as f64);
            }

            let centroid: f32 =
                mag.iter().enumerate().map(|(i, m)| i as f32 * m).sum::<f32>() / total;
            cen.push((centroid / (FRAME / 2) as f32) as f64);

            let mut acc = 0.0;
            let mut roll = 0usize;
            for (i, m) in mag.iter().enumerate() {
                acc += m;
                if acc >= total * 0.85 {
                    roll = i;
                    break;
                }
            }
            rol.push((roll as f32 / (FRAME / 2) as f32) as f64);

            let logsum: f32 =
                mag.iter().map(|m| (m + 1e-10).ln()).sum::<f32>() / (FRAME / 2) as f32;
            let geo = logsum.exp();
            let arith = total / (FRAME / 2) as f32;
            fla.push((geo / (arith + 1e-10)) as f64);

            let var: f32 = mag
                .iter()
                .enumerate()
                .map(|(i, m)| {
                    let dv = i as f32 - centroid;
                    dv * dv * m
                })
                .sum::<f32>()
                / total;
            bwd.push((var.sqrt() / (FRAME / 2) as f32) as f64);
        }

        let rms = (mono[pos..pos + FRAME].iter().map(|s| s * s).sum::<f32>() / FRAME as f32).sqrt();
        rms_stat.push(rms as f64);

        pos += HOP;
    }

    for b in 0..BANDS {
        let (m, v) = band_stats[b].mean_var();
        out[b * 2] = m as f32;
        out[b * 2 + 1] = v as f32;
    }
    out[32] = cen.mean_var().0 as f32;
    out[33] = rol.mean_var().0 as f32;
    out[34] = fla.mean_var().0 as f32;
    out[35] = bwd.mean_var().0 as f32;

    let zc = mono.windows(2).filter(|w| (w[0] < 0.0) != (w[1] < 0.0)).count();
    out[36] = zc as f32 / mono.len().max(1) as f32;

    let (rms_mean, _) = rms_stat.mean_var();
    out[37] = rms_mean as f32;

    let peak = mono.iter().fold(0.0f32, |a, s| a.max(s.abs()));
    out[38] = if rms_mean > 1e-9 { (peak as f64 / rms_mean) as f32 } else { 0.0 };
    out[39] = ((frames as f32 / d.sample_rate as f32).max(1e-3)).ln();

    for v in out.iter_mut() {
        if !v.is_finite() {
            *v = 0.0;
        }
    }
    out
}

pub fn features_to_blob(f: &[f32]) -> Vec<u8> {
    f.iter().flat_map(|v| v.to_le_bytes()).collect()
}

pub fn blob_to_features(b: &[u8]) -> Vec<f32> {
    b.as_chunks::<4>().0.iter().map(|c| f32::from_le_bytes(*c)).collect()
}
