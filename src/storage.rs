//! Basalt-standard persistence through `quasar-storage`.
//!
//! | File | Category |
//! |---|---|
//! | `data/save/profile.json` | progress: synced and backed up |
//! | `data/config/shared/preferences.json` | volumes and gameplay options: synced |
//! | `data/config/display.json` | window mode and size: this machine only |
//!
//! Missing files start from defaults. Unreadable files are kept untouched
//! (copied aside for a corrupt save) and never silently overwritten.

use planetfall_sim::Profile;
use quasar_storage::{GamePaths, read_json, write_json};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const PREFERENCES_VERSION: u32 = 1;
const DISPLAY_VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Preferences {
    pub version: u32,
    pub master_volume: f32,
    pub music_volume: f32,
    pub effects_volume: f32,
    pub screen_shake: bool,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            version: PREFERENCES_VERSION,
            master_volume: 0.8,
            music_volume: 0.55,
            effects_volume: 0.9,
            screen_shake: true,
        }
    }
}

impl Preferences {
    fn sanitize(mut self) -> Self {
        self.version = PREFERENCES_VERSION;
        for v in [
            &mut self.master_volume,
            &mut self.music_volume,
            &mut self.effects_volume,
        ] {
            *v = if v.is_finite() {
                v.clamp(0.0, 1.0)
            } else {
                0.8
            };
        }
        self
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Display {
    pub version: u32,
    pub fullscreen: bool,
    pub width: u32,
    pub height: u32,
}

impl Default for Display {
    fn default() -> Self {
        Self {
            version: DISPLAY_VERSION,
            fullscreen: false,
            width: 1600,
            height: 900,
        }
    }
}

pub struct Storage {
    pub paths: GamePaths,
    saving: bool,
    /// False when the save on disk is from a newer build; never clobber it.
    profile_writable: bool,
}

impl Storage {
    pub fn discover(no_save: bool) -> std::io::Result<Self> {
        // A checkout keeps its data/ beside Cargo.toml instead of target/.
        let development_root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let paths = GamePaths::discover(Some(development_root))?;
        if !no_save {
            for dir in [&paths.save, &paths.shared_config, &paths.cache] {
                std::fs::create_dir_all(dir)?;
            }
        }
        Ok(Self {
            paths,
            saving: !no_save,
            profile_writable: true,
        })
    }

    /// Packaged builds ship assets in `app/assets`; checkouts use the repo copy.
    pub fn asset_root(&self, explicit: Option<&Path>) -> PathBuf {
        if let Some(path) = explicit {
            return path.to_path_buf();
        }
        let packaged = self.paths.app.join("assets");
        if packaged.join("models").is_dir() {
            return packaged;
        }
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets")
    }

    fn profile_path(&self) -> PathBuf {
        self.paths.save.join("profile.json")
    }

    /// Load progress, returning a warning to show when something was wrong.
    pub fn load_profile(&mut self) -> (Profile, Option<String>) {
        let path = self.profile_path();
        match read_json::<Profile>(&path) {
            Ok(None) => (Profile::default(), None),
            Ok(Some(profile)) => match profile.migrate() {
                Ok(profile) => (profile, None),
                Err(e) => {
                    self.profile_writable = false;
                    (
                        Profile::default(),
                        Some(format!("{e}; progress will not be saved this session")),
                    )
                }
            },
            Err(e) => {
                let aside = self
                    .paths
                    .save
                    .join(format!("profile.corrupt-{}.json", unix_time()));
                let kept = self.saving && std::fs::copy(&path, &aside).is_ok();
                if !kept {
                    self.profile_writable = false;
                }
                (
                    Profile::default(),
                    Some(if kept {
                        format!(
                            "save was unreadable ({e}); kept a copy at {} and started fresh",
                            aside.display()
                        )
                    } else {
                        format!(
                            "save was unreadable ({e}); progress will not be saved this session"
                        )
                    }),
                )
            }
        }
    }

    pub fn save_profile(&self, profile: &Profile) -> std::io::Result<()> {
        if !self.saving || !self.profile_writable {
            return Ok(());
        }
        write_json(&self.profile_path(), profile)
    }

    pub fn load_preferences(&self) -> Preferences {
        read_json::<Preferences>(&self.paths.shared_config.join("preferences.json"))
            .ok()
            .flatten()
            .unwrap_or_default()
            .sanitize()
    }

    pub fn save_preferences(&self, preferences: &Preferences) -> std::io::Result<()> {
        if !self.saving {
            return Ok(());
        }
        write_json(
            &self.paths.shared_config.join("preferences.json"),
            preferences,
        )
    }

    pub fn load_display(&self) -> Display {
        let mut display: Display = read_json(&self.paths.config.join("display.json"))
            .ok()
            .flatten()
            .unwrap_or_default();
        display.version = DISPLAY_VERSION;
        display.width = display.width.clamp(640, 7680);
        display.height = display.height.clamp(360, 4320);
        display
    }

    pub fn save_display(&self, display: &Display) -> std::io::Result<()> {
        if !self.saving {
            return Ok(());
        }
        write_json(&self.paths.config.join("display.json"), display)
    }
}

fn unix_time() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn storage_in(dir: &Path) -> Storage {
        let root = dir.to_path_buf();
        Storage {
            paths: GamePaths {
                app: root.join("app"),
                save: root.join("data/save"),
                config: root.join("data/config"),
                shared_config: root.join("data/config/shared"),
                cache: root.join("data/cache"),
                root,
            },
            saving: true,
            profile_writable: true,
        }
    }

    #[test]
    fn profile_round_trip_and_corrupt_files_are_kept() {
        let dir = tempfile::tempdir().unwrap();
        let mut storage = storage_in(dir.path());
        let (fresh, warning) = storage.load_profile();
        assert!(warning.is_none());
        let mut profile = fresh;
        profile.materials.ferrite = 12;
        storage.save_profile(&profile).unwrap();
        assert_eq!(storage.load_profile().0, profile);

        std::fs::write(storage.profile_path(), b"{ not json").unwrap();
        let (loaded, warning) = storage.load_profile();
        assert_eq!(loaded, Profile::default());
        assert!(warning.unwrap().contains("kept a copy"));
        let kept = std::fs::read_dir(&storage.paths.save)
            .unwrap()
            .filter(|e| {
                e.as_ref()
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .contains("corrupt")
            })
            .count();
        assert_eq!(kept, 1);
    }

    #[test]
    fn newer_saves_are_never_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let mut storage = storage_in(dir.path());
        std::fs::create_dir_all(&storage.paths.save).unwrap();
        let newer = r#"{"version": 99, "materials": {"ferrite": 5}}"#;
        std::fs::write(storage.profile_path(), newer).unwrap();
        let (_, warning) = storage.load_profile();
        assert!(warning.is_some());
        storage.save_profile(&Profile::default()).unwrap();
        assert_eq!(
            std::fs::read_to_string(storage.profile_path()).unwrap(),
            newer
        );
    }

    #[test]
    fn preferences_are_sanitized() {
        let dir = tempfile::tempdir().unwrap();
        let storage = storage_in(dir.path());
        std::fs::create_dir_all(&storage.paths.shared_config).unwrap();
        std::fs::write(
            storage.paths.shared_config.join("preferences.json"),
            r#"{"master_volume": 7.0}"#,
        )
        .unwrap();
        let p = storage.load_preferences();
        assert_eq!(p.master_volume, 1.0);
        assert_eq!(p.music_volume, Preferences::default().music_volume);
    }
}
