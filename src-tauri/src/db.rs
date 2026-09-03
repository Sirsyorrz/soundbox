use std::path::Path;

use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};

pub struct Db {
    pub conn: Connection,
}

#[derive(Debug, Clone)]
pub struct FileRow {
    pub content_key: String,
    pub root_id: i64,
    pub rel_path: String,
    pub filename: String,
    pub ext: String,
    pub size: u64,
    pub mtime: i64,
    pub duration_ms: u64,
    pub sample_rate: u32,
    pub channels: usize,
    pub lufs: Option<f64>,
    pub peak_db: f32,
    pub features: Vec<u8>,
    pub status: String,
}

impl Db {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(p) = path.parent() {
            std::fs::create_dir_all(p)?;
        }
        let conn = Connection::open(path)?;
        // WAL so the scanner can write while the UI reads.
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        let db = Db { conn };
        db.migrate()?;
        Ok(db)
    }

    pub fn open_in_memory() -> Result<Self> {
        let db = Db { conn: Connection::open_in_memory()? };
        db.migrate()?;
        Ok(db)
    }

    fn migrate(&self) -> Result<()> {
        self.conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS roots (
                id       INTEGER PRIMARY KEY,
                path     TEXT NOT NULL UNIQUE,
                label    TEXT NOT NULL,
                enabled  INTEGER NOT NULL DEFAULT 1,
                added_at INTEGER NOT NULL
            );

            -- One row per file on disk. content_key is deliberately NOT unique:
            -- the same audio is routinely aliased under several names, and
            -- collapsing those hides real files and thrashes on every rescan.
            CREATE TABLE IF NOT EXISTS files (
                id          INTEGER PRIMARY KEY,
                content_key TEXT NOT NULL,
                root_id     INTEGER NOT NULL REFERENCES roots(id) ON DELETE CASCADE,
                rel_path    TEXT NOT NULL,
                filename    TEXT NOT NULL,
                ext         TEXT NOT NULL,
                size        INTEGER NOT NULL,
                mtime       INTEGER NOT NULL,
                duration_ms INTEGER NOT NULL DEFAULT 0,
                sample_rate INTEGER NOT NULL DEFAULT 0,
                channels    INTEGER NOT NULL DEFAULT 0,
                lufs        REAL,
                peak_db     REAL,
                features    BLOB,
                peaks_rev   INTEGER NOT NULL DEFAULT 0,
                scanned_at  INTEGER NOT NULL DEFAULT 0,
                status      TEXT NOT NULL DEFAULT 'ok'
            );
            CREATE INDEX IF NOT EXISTS idx_files_root ON files(root_id);
            CREATE INDEX IF NOT EXISTS idx_files_key ON files(content_key);
            CREATE UNIQUE INDEX IF NOT EXISTS idx_files_loc ON files(root_id, rel_path);

            -- User-authored metadata hangs off content, not location, so it
            -- survives renames and moves and is what `.sbpack` exports.
            CREATE TABLE IF NOT EXISTS tags (
                id    INTEGER PRIMARY KEY,
                name  TEXT NOT NULL UNIQUE,
                color TEXT
            );
            CREATE TABLE IF NOT EXISTS file_tags (
                content_key TEXT NOT NULL,
                tag_id      INTEGER NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
                PRIMARY KEY (content_key, tag_id)
            );
            CREATE TABLE IF NOT EXISTS favorites (
                content_key TEXT PRIMARY KEY,
                added_at    INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS plays (
                content_key TEXT NOT NULL,
                played_at   INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS rename_log (
                id            INTEGER PRIMARY KEY,
                file_id       INTEGER NOT NULL,
                old_rel_path  TEXT NOT NULL,
                new_rel_path  TEXT NOT NULL,
                at            INTEGER NOT NULL
            );
            "#,
        )?;
        Ok(())
    }

    pub fn add_root(&self, path: &Path, label: &str) -> Result<i64> {
        let p = path.to_string_lossy().to_string();
        self.conn.execute(
            "INSERT INTO roots (path, label, added_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(path) DO UPDATE SET label = excluded.label",
            params![p, label, now()],
        )?;
        Ok(self.conn.query_row("SELECT id FROM roots WHERE path = ?1", params![p], |r| r.get(0))?)
    }

    pub fn roots(&self) -> Result<Vec<(i64, String, String)>> {
        let mut st = self.conn.prepare("SELECT id, path, label FROM roots ORDER BY id")?;
        let rows = st
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// (size, mtime, peaks_rev) for a known location, used to skip unchanged files.
    pub fn known(&self, root_id: i64, rel_path: &str) -> Result<Option<(u64, i64, u16)>> {
        let r = self
            .conn
            .query_row(
                "SELECT size, mtime, peaks_rev FROM files WHERE root_id = ?1 AND rel_path = ?2",
                params![root_id, rel_path],
                |r| Ok((r.get::<_, i64>(0)? as u64, r.get::<_, i64>(1)?, r.get::<_, i64>(2)? as u16)),
            )
            .optional()?;
        Ok(r)
    }

    pub fn upsert(&self, f: &FileRow) -> Result<()> {
        // Keyed on location: a path holds exactly one file, and its content may
        // change in place. Metadata is keyed on content_key elsewhere.
        self.conn.execute(
            "INSERT INTO files
               (content_key, root_id, rel_path, filename, ext, size, mtime,
                duration_ms, sample_rate, channels, lufs, peak_db, features,
                peaks_rev, scanned_at, status)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16)
             ON CONFLICT(root_id, rel_path) DO UPDATE SET
                content_key=excluded.content_key,
                filename=excluded.filename, ext=excluded.ext,
                size=excluded.size, mtime=excluded.mtime,
                duration_ms=excluded.duration_ms, sample_rate=excluded.sample_rate,
                channels=excluded.channels, lufs=excluded.lufs,
                peak_db=excluded.peak_db, features=excluded.features,
                peaks_rev=excluded.peaks_rev, scanned_at=excluded.scanned_at,
                status=excluded.status",
            params![
                f.content_key,
                f.root_id,
                f.rel_path,
                f.filename,
                f.ext,
                f.size as i64,
                f.mtime,
                f.duration_ms as i64,
                f.sample_rate,
                f.channels as i64,
                f.lufs,
                f.peak_db,
                f.features,
                crate::cache::VERSION,
                now(),
                f.status,
            ],
        )?;
        Ok(())
    }

    pub fn all_items(&self) -> Result<Vec<crate::search::Item>> {
        let mut st = self.conn.prepare(
            "SELECT f.id, f.filename, f.rel_path, f.duration_ms, f.channels,
                    f.sample_rate, f.ext, f.content_key, f.scanned_at, f.mtime,
                    COALESCE((SELECT MAX(played_at) FROM plays p
                              WHERE p.content_key = f.content_key), 0),
                    EXISTS(SELECT 1 FROM favorites fa WHERE fa.content_key = f.content_key),
                    COALESCE((SELECT GROUP_CONCAT(t.name, ' ') FROM file_tags ft
                              JOIN tags t ON t.id = ft.tag_id
                              WHERE ft.content_key = f.content_key), '')
             FROM files f WHERE f.status = 'ok' ORDER BY f.rel_path",
        )?;
        let rows = st
            .query_map([], |r| {
                let rel: String = r.get(2)?;
                let folder = rel.rsplit_once(['/', '\\']).map(|(d, _)| d.to_string()).unwrap_or_default();
                Ok(crate::search::Item {
                    id: r.get(0)?,
                    filename: r.get(1)?,
                    folder,
                    duration_ms: r.get::<_, i64>(3)? as u64,
                    channels: r.get::<_, i64>(4)? as usize,
                    sample_rate: r.get(5)?,
                    ext: r.get(6)?,
                    content_key: r.get(7)?,
                    added_at: r.get(8)?,
                    mtime: r.get(9)?,
                    last_played: r.get(10)?,
                    favorite: r.get(11)?,
                    tags: r
                        .get::<_, String>(12)?
                        .split_whitespace()
                        .map(str::to_string)
                        .collect(),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Absolute path for a file id, rebuilt from its root.
    pub fn path_for(&self, id: i64) -> Result<String> {
        let (root, rel): (String, String) = self.conn.query_row(
            "SELECT r.path, f.rel_path FROM files f JOIN roots r ON r.id = f.root_id WHERE f.id = ?1",
            params![id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        Ok(std::path::Path::new(&root).join(rel).to_string_lossy().to_string())
    }

    /// (absolute path, content_key, lufs, peak_db)
    pub fn load_info(&self, id: i64) -> Result<(String, String, Option<f64>, Option<f32>)> {
        let (root, rel, key, lufs, peak): (String, String, String, Option<f64>, Option<f32>) =
            self.conn.query_row(
                "SELECT r.path, f.rel_path, f.content_key, f.lufs, f.peak_db
                 FROM files f JOIN roots r ON r.id = f.root_id WHERE f.id = ?1",
                params![id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )?;
        let path = std::path::Path::new(&root).join(rel).to_string_lossy().to_string();
        Ok((path, key, lufs, peak))
    }

    /// Removes a root and its file rows. Peaks blobs are content-keyed and may
    /// be shared with another root, so they are deliberately left alone.
    pub fn remove_root(&self, root_id: i64) -> Result<usize> {
        let n = self.conn.execute("DELETE FROM files WHERE root_id = ?1", params![root_id])?;
        self.conn.execute("DELETE FROM roots WHERE id = ?1", params![root_id])?;
        Ok(n)
    }

    pub fn record_play(&self, id: i64) -> Result<()> {
        self.conn.execute(
            "INSERT INTO plays (content_key, played_at)
             SELECT content_key, ?2 FROM files WHERE id = ?1",
            params![id, now()],
        )?;
        Ok(())
    }

    pub fn toggle_favorite(&self, id: i64) -> Result<bool> {
        let key: String =
            self.conn.query_row("SELECT content_key FROM files WHERE id = ?1", params![id], |r| r.get(0))?;
        let is_fav: bool = self
            .conn
            .query_row("SELECT 1 FROM favorites WHERE content_key = ?1", params![key], |_| Ok(true))
            .optional()?
            .unwrap_or(false);
        if is_fav {
            self.conn.execute("DELETE FROM favorites WHERE content_key = ?1", params![key])?;
        } else {
            self.conn.execute(
                "INSERT OR REPLACE INTO favorites (content_key, added_at) VALUES (?1, ?2)",
                params![key, now()],
            )?;
        }
        Ok(!is_fav)
    }

    /// Tags attach to content, so they apply to every alias of a recording and
    /// survive renames.
    pub fn tag_file(&self, id: i64, tag: &str) -> Result<()> {
        let tag = tag.trim().to_lowercase();
        if tag.is_empty() {
            return Ok(());
        }
        let key: String =
            self.conn.query_row("SELECT content_key FROM files WHERE id = ?1", params![id], |r| r.get(0))?;
        self.conn.execute("INSERT OR IGNORE INTO tags (name) VALUES (?1)", params![tag])?;
        let tag_id: i64 =
            self.conn.query_row("SELECT id FROM tags WHERE name = ?1", params![tag], |r| r.get(0))?;
        self.conn.execute(
            "INSERT OR IGNORE INTO file_tags (content_key, tag_id) VALUES (?1, ?2)",
            params![key, tag_id],
        )?;
        Ok(())
    }

    pub fn untag_file(&self, id: i64, tag: &str) -> Result<()> {
        let tag = tag.trim().to_lowercase();
        let key: String =
            self.conn.query_row("SELECT content_key FROM files WHERE id = ?1", params![id], |r| r.get(0))?;
        self.conn.execute(
            "DELETE FROM file_tags WHERE content_key = ?1
             AND tag_id = (SELECT id FROM tags WHERE name = ?2)",
            params![key, tag],
        )?;
        Ok(())
    }

    /// (name, number of files carrying it), commonest first.
    pub fn tag_counts(&self) -> Result<Vec<(String, i64)>> {
        let mut st = self.conn.prepare(
            "SELECT t.name, COUNT(ft.content_key) c FROM tags t
             LEFT JOIN file_tags ft ON ft.tag_id = t.id
             GROUP BY t.id ORDER BY c DESC, t.name",
        )?;
        let rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// (id, content_key, duration_ms, feature blob) for the similarity index.
    pub fn all_features(&self) -> Result<Vec<(i64, String, u64, Vec<u8>)>> {
        let mut st = self.conn.prepare(
            "SELECT id, content_key, duration_ms, features FROM files
             WHERE status = 'ok' AND features IS NOT NULL AND LENGTH(features) > 0",
        )?;
        let rows = st
            .query_map([], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get::<_, i64>(2)? as u64, r.get(3)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn count(&self) -> Result<i64> {
        Ok(self.conn.query_row("SELECT COUNT(*) FROM files", [], |r| r.get(0))?)
    }

    /// Distinct audio content, as opposed to files on disk.
    pub fn count_unique_content(&self) -> Result<i64> {
        Ok(self.conn.query_row("SELECT COUNT(DISTINCT content_key) FROM files", [], |r| r.get(0))?)
    }

    pub fn count_status(&self, status: &str) -> Result<i64> {
        Ok(self.conn.query_row(
            "SELECT COUNT(*) FROM files WHERE status = ?1",
            params![status],
            |r| r.get(0),
        )?)
    }
}

pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
