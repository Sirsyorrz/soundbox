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

    pub fn search(&mut self, query: &str, limit: usize) -> Vec<Hit> {
        if query.trim().is_empty() {
            return self
                .items
                .iter()
                .take(limit)
                .map(|i| Hit { id: i.id, score: 0, indices: Vec::new(), via_folder: false })
                .collect();
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

        hits.sort_unstable_by(|a, b| b.score.cmp(&a.score).then(a.id.cmp(&b.id)));
        hits.truncate(limit);
        hits
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
        assert_eq!(index().search("", 100).len(), 4);
    }

    #[test]
    fn typo_tolerant_on_filenames() {
        let hits = index().search("fatby", 10);
        assert_eq!(hits[0].id, 4, "fuzzy match should survive a dropped letter");
    }

    #[test]
    fn folder_match_pulls_in_contents() {
        let hits = index().search("fart", 10);
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
        let hits = ix.search("impact", 10);
        assert_eq!(hits[0].id, 1, "direct filename hit should win");
    }

    #[test]
    fn highlight_indices_point_into_the_filename() {
        let hits = index().search("hit", 10);
        let h = hits.iter().find(|h| h.id == 3).expect("hit_03.wav should match");
        assert_eq!(h.indices, vec![0, 1, 2]);
    }
}
