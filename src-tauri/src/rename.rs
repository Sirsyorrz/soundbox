use std::path::{Path, PathBuf};

use anyhow::{anyhow, Result};

/// Illegal on Windows; `/` and NUL are illegal on POSIX. Sanitising against the
/// union keeps names valid on both machines, since a library is shared between
/// them and a name that is legal here can be unusable there.
const ILLEGAL: &[char] = &['<', '>', ':', '"', '/', '\\', '|', '?', '*'];

/// Reserved even with an extension, and reserved case-insensitively.
const RESERVED: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Leaves headroom under the traditional 260-char Windows path limit.
const MAX_STEM: usize = 120;

#[derive(Debug, PartialEq)]
pub enum Invalid {
    Empty,
    TooLong,
    Reserved,
    IllegalChars(String),
}

impl std::fmt::Display for Invalid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Invalid::Empty => write!(f, "name cannot be empty"),
            Invalid::TooLong => write!(f, "name is longer than {MAX_STEM} characters"),
            Invalid::Reserved => write!(f, "that name is reserved by Windows"),
            Invalid::IllegalChars(c) => write!(f, "name cannot contain {c}"),
        }
    }
}

/// Validates a filename stem, i.e. without its extension.
pub fn validate(stem: &str) -> Result<(), Invalid> {
    let trimmed = stem.trim();
    if trimmed.is_empty() {
        return Err(Invalid::Empty);
    }
    if trimmed.chars().count() > MAX_STEM {
        return Err(Invalid::TooLong);
    }
    let bad: Vec<String> = trimmed
        .chars()
        .filter(|c| ILLEGAL.contains(c) || (*c as u32) < 0x20)
        .map(|c| if (c as u32) < 0x20 { "control characters".into() } else { c.to_string() })
        .collect();
    if !bad.is_empty() {
        let mut uniq: Vec<String> = bad;
        uniq.dedup();
        return Err(Invalid::IllegalChars(uniq.join(" ")));
    }
    // Windows resolves CON.wav to the console device, not a file.
    let base = trimmed.split('.').next().unwrap_or(trimmed);
    if RESERVED.iter().any(|r| r.eq_ignore_ascii_case(base)) {
        return Err(Invalid::Reserved);
    }
    Ok(())
}

/// Best-effort cleanup for a proposed name, used to prefill the rename box.
pub fn sanitise(stem: &str) -> String {
    let cleaned: String = stem
        .chars()
        .map(|c| if ILLEGAL.contains(&c) || (c as u32) < 0x20 { '_' } else { c })
        .collect();
    // Windows silently strips trailing dots and spaces, which would make the
    // name on disk differ from the one shown.
    cleaned.trim().trim_end_matches(['.', ' ']).chars().take(MAX_STEM).collect()
}

pub struct Renamed {
    pub from: PathBuf,
    pub to: PathBuf,
    /// The requested name was taken, so a " (n)" suffix was applied.
    pub suffixed: bool,
}

/// Renames within the same directory, preserving the extension.
///
/// `on_collision_suffix` appends " (2)", " (3)" and so on rather than
/// overwriting; silently replacing an existing sound would be destructive.
pub fn rename_file(path: &Path, new_stem: &str, on_collision_suffix: bool) -> Result<Renamed> {
    validate(new_stem).map_err(|e| anyhow!("{e}"))?;
    if !path.is_file() {
        return Err(anyhow!("{} no longer exists", path.display()));
    }

    let dir = path.parent().ok_or_else(|| anyhow!("no parent directory"))?;
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    let stem = new_stem.trim();

    let build = |n: u32| -> PathBuf {
        let name = match (n, ext.is_empty()) {
            (0, true) => stem.to_string(),
            (0, false) => format!("{stem}.{ext}"),
            (_, true) => format!("{stem} ({n})"),
            (_, false) => format!("{stem} ({n}).{ext}"),
        };
        dir.join(name)
    };

    let mut target = build(0);
    if target == path {
        return Ok(Renamed { from: path.into(), to: target, suffixed: false });
    }
    let mut suffixed = false;
    if target.exists() {
        if !on_collision_suffix {
            return Err(anyhow!("{} already exists", target.display()));
        }
        suffixed = true;
        let mut n = 2;
        loop {
            target = build(n);
            if !target.exists() {
                break;
            }
            n += 1;
            if n > 999 {
                return Err(anyhow!("too many files with that name"));
            }
        }
    }

    std::fs::rename(path, &target)?;
    Ok(Renamed { from: path.into(), to: target, suffixed })
}

pub struct Outcome {
    pub filename: String,
    /// True when the requested name was taken and a suffix was applied.
    pub suffixed: bool,
}

/// Renames on disk and records it, as one operation.
///
/// Lives here rather than in the command layer so the destructive path is
/// testable without a running app.
pub fn perform(db: &crate::db::Db, id: i64, stem: &str, allow_suffix: bool) -> Result<Outcome> {
    let (old_rel, root) = db.location(id)?;
    let path = PathBuf::from(&root).join(&old_rel);

    let done = rename_file(&path, stem, allow_suffix)?;
    let new_rel = done.to.strip_prefix(&root).unwrap_or(&done.to).to_string_lossy().to_string();
    let filename = done.to.file_name().unwrap_or_default().to_string_lossy().to_string();

    db.apply_rename(id, &old_rel, &new_rel)?;
    Ok(Outcome { suffixed: done.suffixed, filename })
}

/// Reverses the most recent rename. `Ok(None)` when there is nothing to undo.
pub fn undo_last(db: &crate::db::Db) -> Result<Option<String>> {
    let Some((log_id, file_id, old_rel, new_rel, root)) = db.last_rename()? else {
        return Ok(None);
    };

    let current = PathBuf::from(&root).join(&new_rel);
    let previous = PathBuf::from(&root).join(&old_rel);
    let old_stem = previous
        .file_stem()
        .ok_or_else(|| anyhow!("cannot determine the previous name"))?
        .to_string_lossy()
        .to_string();

    // Refuse rather than suffix: an undo that lands on yet another name is
    // worse than reporting that it cannot be undone.
    rename_file(&current, &old_stem, false)?;

    db.apply_rename(file_id, &new_rel, &old_rel)?;
    db.drop_rename_log_entry(log_id)?;
    // apply_rename logged the undo itself; drop that too, or a second undo
    // would simply redo the rename.
    if let Some((redo_id, _, _, _, _)) = db.last_rename()? {
        db.drop_rename_log_entry(redo_id)?;
    }
    Ok(Some(old_stem))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_windows_illegal_characters() {
        for bad in ["a/b", "a\\b", "a:b", "a*b", "a?b", "a\"b", "a<b", "a>b", "a|b"] {
            assert!(validate(bad).is_err(), "{bad:?} should be rejected");
        }
    }

    #[test]
    fn rejects_reserved_device_names_regardless_of_case_or_extension() {
        for bad in ["CON", "con", "NUL", "com1", "LPT9", "con.backup"] {
            assert_eq!(validate(bad), Err(Invalid::Reserved), "{bad:?}");
        }
        assert!(validate("console").is_ok(), "only exact device names are reserved");
    }

    #[test]
    fn rejects_empty_and_overlong() {
        assert_eq!(validate("   "), Err(Invalid::Empty));
        assert_eq!(validate(&"a".repeat(MAX_STEM + 1)), Err(Invalid::TooLong));
        assert!(validate(&"a".repeat(MAX_STEM)).is_ok());
    }

    #[test]
    fn accepts_ordinary_names() {
        for ok in ["fart", "big fart 2", "impact_metal_03", "café_ambience", "a.b.c"] {
            assert!(validate(ok).is_ok(), "{ok:?} should be allowed");
        }
    }

    #[test]
    fn sanitise_strips_trailing_dots_and_spaces() {
        // Windows silently drops these, so the file would not match the name.
        assert_eq!(sanitise("name.  "), "name");
        assert_eq!(sanitise("a/b:c"), "a_b_c");
    }

    fn tmp() -> PathBuf {
        let d = std::env::temp_dir().join(format!("sb_rn_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&d);
        d
    }

    #[test]
    fn renames_and_keeps_the_extension() {
        let dir = tmp();
        let src = dir.join("old_name.wav");
        std::fs::write(&src, b"x").unwrap();
        let r = rename_file(&src, "new name", true).unwrap();
        assert_eq!(r.to.file_name().unwrap(), "new name.wav");
        assert!(!r.suffixed, "no collision, so no suffix");
        assert!(r.to.exists() && !src.exists());
        std::fs::remove_file(r.to).unwrap();
    }

    #[test]
    fn collision_suffixes_instead_of_overwriting() {
        let dir = tmp();
        let a = dir.join("keep.wav");
        let b = dir.join("other.wav");
        std::fs::write(&a, b"original").unwrap();
        std::fs::write(&b, b"second").unwrap();

        let r = rename_file(&b, "keep", true).unwrap();
        assert_eq!(r.to.file_name().unwrap(), "keep (2).wav");
        assert!(r.suffixed);
        assert_eq!(std::fs::read(&a).unwrap(), b"original", "must not clobber");

        std::fs::remove_file(a).unwrap();
        std::fs::remove_file(r.to).unwrap();
    }

    #[test]
    fn collision_without_suffix_is_an_error_and_changes_nothing() {
        let dir = tmp();
        let a = dir.join("taken.wav");
        let b = dir.join("mover.wav");
        std::fs::write(&a, b"a").unwrap();
        std::fs::write(&b, b"b").unwrap();

        assert!(rename_file(&b, "taken", false).is_err());
        assert!(b.exists(), "source must survive a refused rename");

        std::fs::remove_file(a).unwrap();
        std::fs::remove_file(b).unwrap();
    }
}
