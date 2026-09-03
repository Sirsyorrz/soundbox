use std::path::{Path, PathBuf};

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Profile {
    pub id: String,
    pub name: String,
    pub color: String,
    pub last_opened: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Registry {
    pub profiles: Vec<Profile>,
    pub active: String,
}

const COLORS: &[&str] = &["#4ea1ff", "#4ade80", "#f59e0b", "#f472b6", "#a78bfa", "#f87171"];

fn registry_path(base: &Path) -> PathBuf {
    base.join("profiles.json")
}

pub fn profile_dir(base: &Path, id: &str) -> PathBuf {
    base.join("profiles").join(id)
}

pub fn db_path(base: &Path, id: &str) -> PathBuf {
    profile_dir(base, id).join("library.db")
}

/// Ids only need to be unique and path-safe; they are never shown.
fn new_id() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{nanos:x}")
}

impl Registry {
    pub fn load(base: &Path) -> Result<Self> {
        let path = registry_path(base);
        if !path.exists() {
            return Ok(Registry::default());
        }
        let text = std::fs::read_to_string(&path)?;
        // A corrupt registry must not orphan every profile on disk.
        Ok(serde_json::from_str(&text).unwrap_or_default())
    }

    pub fn save(&self, base: &Path) -> Result<()> {
        std::fs::create_dir_all(base)?;
        let tmp = registry_path(base).with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(self)?)?;
        std::fs::rename(tmp, registry_path(base))?;
        Ok(())
    }

    pub fn get(&self, id: &str) -> Option<&Profile> {
        self.profiles.iter().find(|p| p.id == id)
    }

    pub fn active_profile(&self) -> Option<&Profile> {
        self.get(&self.active)
    }

    pub fn create(&mut self, base: &Path, name: &str) -> Result<Profile> {
        let name = name.trim();
        if name.is_empty() {
            return Err(anyhow!("profile name cannot be empty"));
        }
        if self.profiles.iter().any(|p| p.name.eq_ignore_ascii_case(name)) {
            return Err(anyhow!("a profile called {name} already exists"));
        }
        let p = Profile {
            id: new_id(),
            name: name.to_string(),
            color: COLORS[self.profiles.len() % COLORS.len()].to_string(),
            last_opened: crate::db::now(),
        };
        std::fs::create_dir_all(profile_dir(base, &p.id))?;
        self.profiles.push(p.clone());
        self.save(base)?;
        Ok(p)
    }

    pub fn rename(&mut self, base: &Path, id: &str, name: &str) -> Result<()> {
        let name = name.trim();
        if name.is_empty() {
            return Err(anyhow!("profile name cannot be empty"));
        }
        if self.profiles.iter().any(|p| p.id != id && p.name.eq_ignore_ascii_case(name)) {
            return Err(anyhow!("a profile called {name} already exists"));
        }
        let p = self
            .profiles
            .iter_mut()
            .find(|p| p.id == id)
            .ok_or_else(|| anyhow!("no such profile"))?;
        p.name = name.to_string();
        self.save(base)
    }

    /// Deletes the profile and its database. The audio files are untouched.
    pub fn delete(&mut self, base: &Path, id: &str) -> Result<String> {
        if self.profiles.len() <= 1 {
            return Err(anyhow!("cannot delete the only profile"));
        }
        let idx = self
            .profiles
            .iter()
            .position(|p| p.id == id)
            .ok_or_else(|| anyhow!("no such profile"))?;
        self.profiles.remove(idx);
        let _ = std::fs::remove_dir_all(profile_dir(base, id));
        if self.active == id {
            self.active = self.profiles[0].id.clone();
        }
        self.save(base)?;
        Ok(self.active.clone())
    }

    pub fn switch(&mut self, base: &Path, id: &str) -> Result<()> {
        if self.get(id).is_none() {
            return Err(anyhow!("no such profile"));
        }
        self.active = id.to_string();
        if let Some(p) = self.profiles.iter_mut().find(|p| p.id == id) {
            p.last_opened = crate::db::now();
        }
        self.save(base)
    }
}

/// Ensures a usable registry exists, adopting any pre-profile database.
///
/// Early builds kept a single `library.db` at the top level. Moving it into the
/// first profile preserves an existing library instead of silently starting
/// empty next to it.
pub fn init(base: &Path) -> Result<Registry> {
    let mut reg = Registry::load(base)?;
    reg.profiles.retain(|p| !p.id.is_empty());

    if reg.profiles.is_empty() {
        let p = reg.create(base, "Default")?;
        reg.active = p.id.clone();

        let legacy = base.join("library.db");
        if legacy.is_file() {
            std::fs::rename(&legacy, db_path(base, &p.id))?;
            // sqlite sidecar files, if the db was not closed cleanly
            for ext in ["library.db-wal", "library.db-shm"] {
                let from = base.join(ext);
                if from.is_file() {
                    let _ = std::fs::rename(&from, profile_dir(base, &p.id).join(ext));
                }
            }
        }
        reg.save(base)?;
    }

    if reg.get(&reg.active.clone()).is_none() {
        reg.active = reg.profiles[0].id.clone();
        reg.save(base)?;
    }
    std::fs::create_dir_all(profile_dir(base, &reg.active))?;
    Ok(reg)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("sb_prof_{}_{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn init_creates_a_default_profile() {
        let base = tmp("init");
        let reg = init(&base).unwrap();
        assert_eq!(reg.profiles.len(), 1);
        assert_eq!(reg.active, reg.profiles[0].id);
        assert!(db_path(&base, &reg.active).parent().unwrap().is_dir());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn init_adopts_a_pre_profile_database() {
        let base = tmp("migrate");
        std::fs::write(base.join("library.db"), b"pretend sqlite").unwrap();

        let reg = init(&base).unwrap();
        assert!(!base.join("library.db").exists(), "the old file should have moved");
        assert_eq!(
            std::fs::read(db_path(&base, &reg.active)).unwrap(),
            b"pretend sqlite",
            "an existing library must survive the upgrade"
        );
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn registry_round_trips() {
        let base = tmp("roundtrip");
        let mut reg = init(&base).unwrap();
        reg.create(&base, "Ben's SFX").unwrap();

        let loaded = Registry::load(&base).unwrap();
        assert_eq!(loaded.profiles.len(), 2);
        assert_eq!(loaded.profiles[1].name, "Ben's SFX");
        assert_eq!(loaded.active, reg.active);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn duplicate_names_are_refused() {
        let base = tmp("dupe");
        let mut reg = init(&base).unwrap();
        reg.create(&base, "Music").unwrap();
        assert!(reg.create(&base, "music").is_err(), "names compare case-insensitively");
        assert!(reg.create(&base, "  ").is_err());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn deleting_the_active_profile_falls_back_to_another() {
        let base = tmp("delete");
        let mut reg = init(&base).unwrap();
        let other = reg.create(&base, "Other").unwrap();
        reg.switch(&base, &other.id).unwrap();

        let now_active = reg.delete(&base, &other.id).unwrap();
        assert_ne!(now_active, other.id);
        assert_eq!(reg.active, now_active);
        assert!(!profile_dir(&base, &other.id).exists(), "its database should be gone");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn the_last_profile_cannot_be_deleted() {
        let base = tmp("last");
        let mut reg = init(&base).unwrap();
        assert!(reg.delete(&base, &reg.active.clone()).is_err());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn a_corrupt_registry_does_not_lose_the_app() {
        let base = tmp("corrupt");
        std::fs::write(registry_path(&base), b"{ not json").unwrap();
        let reg = init(&base).unwrap();
        assert_eq!(reg.profiles.len(), 1, "should recover by starting fresh");
        let _ = std::fs::remove_dir_all(&base);
    }
}
