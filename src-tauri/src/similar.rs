/// Nearest-neighbour lookup over the scan's timbral feature vectors.
///
/// Brute force: 31k x 40 dims is ~5 MB and a full sweep costs well under a
/// millisecond, so an ANN index would add dependencies and staleness for no
/// measurable gain.
use crate::analysis::blob_to_features;

/// A 2 s impact and a 4 min ambience can be spectrally close but are never
/// interchangeable, so candidates are gated on duration ratio.
pub const DEFAULT_DURATION_RATIO: f64 = 3.0;

pub struct Entry {
    pub id: i64,
    pub content_key: String,
    pub duration_ms: u64,
}

pub struct SimilarIndex {
    entries: Vec<Entry>,
    /// Row-major, z-scored then L2-normalised so cosine similarity is a dot product.
    vecs: Vec<f32>,
    dim: usize,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Neighbour {
    pub id: i64,
    /// Cosine similarity in [-1, 1]; 1 is identical.
    pub score: f32,
}

impl SimilarIndex {
    pub fn build(rows: Vec<(i64, String, u64, Vec<u8>)>) -> Self {
        let dim = rows.iter().map(|(_, _, _, b)| b.len() / 4).find(|d| *d > 0).unwrap_or(0);

        let mut entries = Vec::with_capacity(rows.len());
        let mut vecs: Vec<f32> = Vec::with_capacity(rows.len() * dim);

        for (id, content_key, duration_ms, blob) in rows {
            let mut f = blob_to_features(&blob);
            if dim == 0 {
                continue;
            }
            f.resize(dim, 0.0);
            entries.push(Entry { id, content_key, duration_ms });
            vecs.extend_from_slice(&f);
        }

        let n = entries.len();
        if n > 0 && dim > 0 {
            // Dimensions have wildly different natural ranges (log band energy
            // vs zero-crossing rate). Without standardising, a couple of dims
            // would dominate the distance entirely.
            for d in 0..dim {
                let mut mean = 0.0f64;
                for i in 0..n {
                    mean += vecs[i * dim + d] as f64;
                }
                mean /= n as f64;

                let mut var = 0.0f64;
                for i in 0..n {
                    let x = vecs[i * dim + d] as f64 - mean;
                    var += x * x;
                }
                let sd = (var / n as f64).sqrt();
                let inv = if sd > 1e-9 { 1.0 / sd } else { 0.0 };

                for i in 0..n {
                    vecs[i * dim + d] = ((vecs[i * dim + d] as f64 - mean) * inv) as f32;
                }
            }

            for i in 0..n {
                let row = &mut vecs[i * dim..(i + 1) * dim];
                let norm = row.iter().map(|v| v * v).sum::<f32>().sqrt();
                if norm > 1e-9 {
                    for v in row.iter_mut() {
                        *v /= norm;
                    }
                }
            }
        }

        SimilarIndex { entries, vecs, dim }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn position(&self, id: i64) -> Option<usize> {
        self.entries.iter().position(|e| e.id == id)
    }

    pub fn query(&self, id: i64, limit: usize, duration_ratio: f64) -> Vec<Neighbour> {
        let Some(target) = self.position(id) else { return Vec::new() };
        if self.dim == 0 {
            return Vec::new();
        }

        let tv = &self.vecs[target * self.dim..(target + 1) * self.dim];
        let t_dur = self.entries[target].duration_ms.max(1) as f64;
        let t_key = &self.entries[target].content_key;

        let mut out: Vec<Neighbour> = Vec::new();
        for (i, e) in self.entries.iter().enumerate() {
            if i == target {
                continue;
            }
            // Aliases of the same recording are not useful suggestions.
            if &e.content_key == t_key {
                continue;
            }
            if duration_ratio > 0.0 {
                let ratio =
                    (e.duration_ms.max(1) as f64 / t_dur).max(t_dur / e.duration_ms.max(1) as f64);
                if ratio > duration_ratio {
                    continue;
                }
            }
            let v = &self.vecs[i * self.dim..(i + 1) * self.dim];
            let score: f32 = tv.iter().zip(v).map(|(a, b)| a * b).sum();
            out.push(Neighbour { id: e.id, score });
        }

        out.sort_unstable_by(|a, b| {
            b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal)
        });
        out.truncate(limit);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blob(v: &[f32]) -> Vec<u8> {
        v.iter().flat_map(|x| x.to_le_bytes()).collect()
    }

    /// id 2 is a near-duplicate of id 1; the rest are spread out.
    ///
    /// Enough entries that per-dimension variance is meaningful. With only two
    /// or three rows, standardisation amplifies a low-variance dimension so far
    /// that genuinely close vectors stop looking close - an artefact of the
    /// sample size, not of the metric.
    fn index() -> SimilarIndex {
        SimilarIndex::build(vec![
            (1, "a".into(), 1000, blob(&[1.0, 1.0, 1.0, 1.0])),
            (2, "b".into(), 1100, blob(&[1.01, 1.02, 0.99, 1.0])),
            (3, "c".into(), 1000, blob(&[-1.0, -1.0, -1.0, -1.0])),
            (4, "d".into(), 1000, blob(&[0.0, -2.0, 3.0, 1.0])),
            (5, "e".into(), 1000, blob(&[2.0, 0.0, -3.0, -1.0])),
            (6, "f".into(), 1000, blob(&[-3.0, 2.0, 1.0, -2.0])),
        ])
    }

    #[test]
    fn nearest_is_the_similar_one() {
        let hits = index().query(1, 10, 0.0);
        assert_eq!(hits[0].id, 2, "closest vector should rank first, got {hits:?}");
    }

    #[test]
    fn excludes_self() {
        assert!(index().query(1, 10, 0.0).iter().all(|n| n.id != 1));
    }

    #[test]
    fn excludes_aliases_of_the_same_recording() {
        let ix = SimilarIndex::build(vec![
            (1, "same".into(), 1000, blob(&[1.0, 1.0, 0.0])),
            (2, "same".into(), 1000, blob(&[1.0, 1.0, 0.0])),
            (3, "other".into(), 1000, blob(&[0.9, 1.1, 0.0])),
        ]);
        let hits = ix.query(1, 10, 0.0);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id, 3);
    }

    #[test]
    fn duration_gate_excludes_wildly_different_lengths() {
        let ix = SimilarIndex::build(vec![
            (1, "a".into(), 1_000, blob(&[1.0, 1.0, 0.0])),
            (2, "b".into(), 100_000, blob(&[1.0, 1.0, 0.0])),
        ]);
        assert!(ix.query(1, 10, 3.0).is_empty(), "100x longer should be gated out");
        assert_eq!(ix.query(1, 10, 0.0).len(), 1, "gate disabled should include it");
    }

    #[test]
    fn unknown_id_is_empty_not_a_panic() {
        assert!(index().query(999, 10, 3.0).is_empty());
    }

    #[test]
    fn constant_dimension_does_not_produce_nan() {
        // Every dim constant: a naive z-score divides by zero throughout.
        let ix = SimilarIndex::build(vec![
            (1, "a".into(), 1000, blob(&[2.0, 2.0])),
            (2, "b".into(), 1000, blob(&[2.0, 2.0])),
            (3, "c".into(), 1000, blob(&[2.0, 2.0])),
        ]);
        assert!(ix.query(1, 10, 0.0).iter().all(|n| n.score.is_finite()));
    }
}
