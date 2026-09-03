use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};

/// Filename hits outrank folder hits; a folder name is weaker evidence.
const W_NAME: u32 = 3;
const W_FOLDER: u32 = 1;
/// A folder must match this well before its whole contents are pulled in.
const FOLDER_EXPAND_MIN: u32 = 40;

#[derive(Clone)]
pub struct Item {
    pub id: i64,
    pub filename: String,
    pub folder: String,
    pub duration_ms: u64,
    pub channels: usize,
    pub sample_rate: u32,
    pub ext: String,
    pub content_key: String,
    pub added_at: i64,
    pub mtime: i64,
    pub last_played: i64,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Hit {
    pub id: i64,
    pub score: u32,
    /// Character offsets in the filename, for highlighting.
    pub indices: Vec<u32>,
    /// True when pulled in by a folder match rather than its own name.
    pub via_folder: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Sort {
    Relevance,
    Name,
    Added,
    Modified,
    Recent,
    Duration,
    Folder,
}

pub struct Index {
    items: Vec<Item>,
    matcher: Matcher,
}

impl Index {
    pub fn new(items: Vec<Item>) -> Self {
        Index { items, matcher: Matcher::new(Config::DEFAULT.match_paths()) }
    }

    pub fn items(&self) -> &[Item] {
        &self.items
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn get(&self, id: i64) -> Option<&Item> {
        self.items.iter().find(|i| i.id == id)
    }

    pub fn search(&mut self, query: &str, limit: usize, sort: Sort, desc: bool) -> Vec<Hit> {
        if query.trim().is_empty() {
            let mut hits: Vec<Hit> = self
                .items
                .iter()
                .map(|i| Hit { id: i.id, score: 0, indices: Vec::new(), via_folder: false })
                .collect();
            self.apply_sort(&mut hits, sort, Sort::Name, desc);
            hits.truncate(limit);
            return hits;
        }

        let pat = Pattern::parse(query, CaseMatching::Ignore, Normalization::Smart);
        let mut buf = Vec::new();
        let mut idx_buf = Vec::new();

        // Folders scored once each, not once per file, so a 500-file folder does
        // not cost 500 redundant matches.
        let mut folders: Vec<&str> = self.items.iter().map(|i| i.folder.as_str()).collect();
        folders.sort_unstable();
        folders.dedup();

        let matcher = &mut self.matcher;
        let folder_scores: std::collections::HashMap<&str, u32> = folders
            .into_iter()
            .filter_map(|f| {
                buf.clear();
                pat.score(Utf32Str::new(f, &mut buf), matcher).map(|s| (f, s))
            })
            .collect();

        let mut hits: Vec<Hit> = Vec::new();
        for item in &self.items {
            idx_buf.clear();
            buf.clear();
            let name_score =
                pat.indices(Utf32Str::new(&item.filename, &mut buf), matcher, &mut idx_buf);
            let folder_score = folder_scores.get(item.folder.as_str()).copied();

            let (score, via_folder) = match (name_score, folder_score) {
                (Some(n), Some(f)) => (n * W_NAME + f * W_FOLDER, false),
                (Some(n), None) => (n * W_NAME, false),
                (None, Some(f)) if f >= FOLDER_EXPAND_MIN => (f * W_FOLDER, true),
                _ => continue,
            };

            let mut indices = idx_buf.clone();
            indices.sort_unstable();
            indices.dedup();
            hits.push(Hit { id: item.id, score, indices, via_folder });
        }

        self.apply_sort(&mut hits, sort, Sort::Relevance, desc);
        hits.truncate(limit);
        hits
    }

    /// `fallback` applies when the caller asks for relevance but there are no
    /// scores to rank by, i.e. an empty query.
    ///
    /// `desc` reverses after sorting rather than negating keys, so it behaves
    /// identically for text and numeric columns.
    fn apply_sort(&self, hits: &mut [Hit], sort: Sort, fallback: Sort, desc: bool) {
        let sort = if sort == Sort::Relevance { fallback } else { sort };
        let by = |id: i64| self.items.iter().find(|i| i.id == id);
        match sort {
            Sort::Relevance => {
                hits.sort_unstable_by(|a, b| b.score.cmp(&a.score).then(a.id.cmp(&b.id)))
            }
            Sort::Name => hits.sort_by_cached_key(|h| {
                by(h.id).map(|i| i.filename.to_lowercase()).unwrap_or_default()
            }),
            Sort::Folder => hits.sort_by_cached_key(|h| {
                by(h.id).map(|i| (i.folder.to_lowercase(), i.filename.to_lowercase())).unwrap_or_default()
            }),
            Sort::Added => hits.sort_by_cached_key(|h| by(h.id).map(|i| i.added_at).unwrap_or(0)),
            Sort::Modified => hits.sort_by_cached_key(|h| by(h.id).map(|i| i.mtime).unwrap_or(0)),
            Sort::Recent => {
                // Leading bool pushes never-played files past every played one;
                // without it a timestamp of 0 would sort to the very top.
                hits.sort_by_cached_key(|h| {
                    let t = by(h.id).map(|i| i.last_played).unwrap_or(0);
                    (t == 0, std::cmp::Reverse(t))
                });
            }
            Sort::Duration => {
                hits.sort_by_cached_key(|h| by(h.id).map(|i| i.duration_ms).unwrap_or(0))
            }
        }
        if desc && sort != Sort::Relevance {
            hits.reverse();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: i64, folder: &str, filename: &str) -> Item {
        Item {
            id,
            filename: filename.into(),
            folder: folder.into(),
            duration_ms: 1000,
            channels: 2,
            sample_rate: 48000,
            ext: "wav".into(),
            content_key: format!("b3:{id}"),
            added_at: id,
            mtime: id,
            last_played: 0,
        }
    }

    fn index() -> Index {
        Index::new(vec![
            item(1, "SFX/Farts", "wet_one.wav"),
            item(2, "SFX/Farts", "dry_two.wav"),
            item(3, "SFX/Impacts/Metal", "hit_03.wav"),
            item(4, "Music", "fatboy_slim.mp3"),
        ])
    }

    #[test]
    fn empty_query_returns_everything() {
        assert_eq!(index().search("", 100, Sort::Relevance, false).len(), 4);
    }

    #[test]
    fn typo_tolerant_on_filenames() {
        let hits = index().search("fatby", 10, Sort::Relevance, false);
        assert_eq!(hits[0].id, 4, "fuzzy match should survive a dropped letter");
    }

    #[test]
    fn folder_match_pulls_in_contents() {
        let hits = index().search("fart", 10, Sort::Relevance, false);
        let ids: Vec<i64> = hits.iter().map(|h| h.id).collect();
        assert!(ids.contains(&1) && ids.contains(&2), "both files in SFX/Farts, got {ids:?}");
        assert!(hits.iter().filter(|h| h.id == 1 || h.id == 2).all(|h| h.via_folder));
    }

    #[test]
    fn filename_outranks_folder() {
        let mut ix = Index::new(vec![
            item(1, "Music", "impact.wav"),
            item(2, "SFX/Impacts/Metal", "clang.wav"),
        ]);
        let hits = ix.search("impact", 10, Sort::Relevance, false);
        assert_eq!(hits[0].id, 1, "direct filename hit should win");
    }

    #[test]
    fn highlight_indices_point_into_the_filename() {
        let hits = index().search("hit", 10, Sort::Relevance, false);
        let h = hits.iter().find(|h| h.id == 3).expect("hit_03.wav should match");
        assert_eq!(h.indices, vec![0, 1, 2]);
    }
}
