use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};

/// Filename hits outrank folder hits; a folder name is weaker evidence.
const W_NAME: u32 = 3;
const W_FOLDER: u32 = 1;
/// Tags are user-authored, so a tag hit is stronger evidence than a folder name.
const W_TAG: u32 = 2;
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
    pub favorite: bool,
    pub tags: Vec<String>,
    /// The subset of `tags` that came from the path rather than the user. They
    /// are not stored, so they cannot be removed and never travel in a pack.
    pub folder_tags: Vec<String>,
}

#[derive(Default, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Filter {
    #[serde(default)]
    pub favorites_only: bool,
    #[serde(default)]
    pub tag: Option<String>,
}

impl Filter {
    pub fn is_active(&self) -> bool {
        self.favorites_only || self.tag.is_some()
    }

    pub fn keeps(&self, i: &Item) -> bool {
        if self.favorites_only && !i.favorite {
            return false;
        }
        match &self.tag {
            Some(t) => i.tags.iter().any(|x| x == t),
            None => true,
        }
    }
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

    pub fn search(
        &mut self,
        query: &str,
        limit: usize,
        sort: Sort,
        desc: bool,
        filter: &Filter,
    ) -> Vec<Hit> {
        if query.trim().is_empty() {
            let mut hits: Vec<Hit> = self
                .items
                .iter()
                .filter(|i| filter.keeps(i))
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
            if !filter.keeps(item) {
                continue;
            }
            idx_buf.clear();
            buf.clear();
            let name_score =
                pat.indices(Utf32Str::new(&item.filename, &mut buf), matcher, &mut idx_buf);
            let folder_score = folder_scores.get(item.folder.as_str()).copied();
            // Folder-derived tags are excluded here: the folder path is already
            // its own haystack, and counting them twice would both inflate the
            // score and bypass folder expansion.
            let tag_score = item
                .tags
                .iter()
                .filter(|t| !item.folder_tags.contains(t))
                .filter_map(|t| {
                    buf.clear();
                    pat.score(Utf32Str::new(t, &mut buf), matcher)
                })
                .max();

            let (score, via_folder) = match (name_score, folder_score, tag_score) {
                (Some(n), f, t) => {
                    (n * W_NAME + f.unwrap_or(0) * W_FOLDER + t.unwrap_or(0) * W_TAG, false)
                }
                (None, _, Some(t)) => (t * W_TAG, false),
                (None, Some(f), None) if f >= FOLDER_EXPAND_MIN => (f * W_FOLDER, true),
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
                by(h.id)
                    .map(|i| (i.folder.to_lowercase(), i.filename.to_lowercase()))
                    .unwrap_or_default()
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
            favorite: false,
            // Mirrors the database, which merges folder names into the tag list.
            tags: folder.split('/').filter(|s| !s.is_empty()).map(str::to_string).collect(),
            folder_tags: folder.split('/').filter(|s| !s.is_empty()).map(str::to_string).collect(),
        }
    }

    #[test]
    fn a_folder_name_filters_like_a_tag() {
        let mut ix = index();
        let mut f = |name: &str| {
            let filter = Filter { favorites_only: false, tag: Some(name.to_string()) };
            ix.search("", 999, Sort::Relevance, false, &filter).len()
        };
        // Nested folders each filter independently.
        assert_eq!(f("Farts"), 2);
        assert_eq!(f("SFX"), 3, "the parent covers everything beneath it");
        assert_eq!(f("nope"), 0);
    }

    fn index() -> Index {
        Index::new(vec![
            item(1, "SFX/Farts", "wet_one.wav"),
            item(2, "SFX/Farts", "dry_two.wav"),
            item(3, "SFX/Impacts/Metal", "hit_03.wav"),
            item(4, "Music", "fatboy_slim.mp3"),
        ])
    }

    fn tagged(id: i64, folder: &str, filename: &str, tags: &[&str], fav: bool) -> Item {
        let mut i = item(id, folder, filename);
        i.tags = tags.iter().map(|t| t.to_string()).collect();
        i.favorite = fav;
        i
    }

    #[test]
    fn tag_match_finds_a_file_whose_name_does_not_match() {
        let mut ix = Index::new(vec![tagged(1, "Music", "GB_004.wav", &["glass", "break"], false)]);
        let hits = ix.search("glass", 10, Sort::Relevance, false, &Filter::default());
        assert_eq!(hits.len(), 1, "tag should be searchable");
    }

    #[test]
    fn favorites_filter_excludes_the_rest() {
        let mut ix = Index::new(vec![
            tagged(1, "a", "one.wav", &[], true),
            tagged(2, "a", "two.wav", &[], false),
        ]);
        let f = Filter { favorites_only: true, tag: None };
        let hits = ix.search("", 10, Sort::Relevance, false, &f);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id, 1);
    }

    #[test]
    fn tag_filter_applies_to_a_query_too() {
        let mut ix = Index::new(vec![
            tagged(1, "a", "hit.wav", &["metal"], false),
            tagged(2, "a", "hit_two.wav", &["wood"], false),
        ]);
        let f = Filter { favorites_only: false, tag: Some("metal".into()) };
        let hits = ix.search("hit", 10, Sort::Relevance, false, &f);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id, 1);
    }

    #[test]
    fn empty_query_returns_everything() {
        assert_eq!(index().search("", 100, Sort::Relevance, false, &Filter::default()).len(), 4);
    }

    #[test]
    fn typo_tolerant_on_filenames() {
        let hits = index().search("fatby", 10, Sort::Relevance, false, &Filter::default());
        assert_eq!(hits[0].id, 4, "fuzzy match should survive a dropped letter");
    }

    #[test]
    fn folder_match_pulls_in_contents() {
        let hits = index().search("fart", 10, Sort::Relevance, false, &Filter::default());
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
        let hits = ix.search("impact", 10, Sort::Relevance, false, &Filter::default());
        assert_eq!(hits[0].id, 1, "direct filename hit should win");
    }

    #[test]
    fn highlight_indices_point_into_the_filename() {
        let hits = index().search("hit", 10, Sort::Relevance, false, &Filter::default());
        let h = hits.iter().find(|h| h.id == 3).expect("hit_03.wav should match");
        assert_eq!(h.indices, vec![0, 1, 2]);
    }
}
