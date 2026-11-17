//! Basalt-standard persistence through `quasar-storage` and `quasar-config`.
//!
//! | File | Category |
//! |---|---|
//! | `data/save/slot-1.json` … `slot-5.json` | game saves: synced and backed up |
//! | `data/config/shared/settings.toml` | audio and gameplay settings: synced |
//! | `data/config/shared/controls.toml` | rebound controls: synced |
//! | `data/config/display.toml` | window and graphics settings: this machine only |
//!
//! Settings are typed TOML read with `quasar_config::load_file_typed` and
//! written atomically with `quasar_config::save_file_typed`; saves are JSON
//! written atomically with `quasar_storage::write_json`. Missing files start
//! from defaults. Unreadable or newer files are never overwritten: a corrupt
//! save is copied aside first, and a newer one is left alone.
//!
//! Files from earlier versions (`profile.json`, `preferences.json`,
//! `display.json`) are migrated once into the new layout and kept beside it
//! with a `.migrated` suffix.

use planetfall_sim::profile::LegacyProfile;
use planetfall_sim::save::{SLOTS, SaveGame, SlotMeta};
use quasar_storage::{GamePaths, read_json, write_json};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const SETTINGS_VERSION: u32 = 1;
const DISPLAY_VERSION: u32 = 1;

/// Settings that follow the player between machines.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Preferences {
    pub version: u32,
    pub master_volume: f32,
    pub music_volume: f32,
    pub effects_volume: f32,
    pub screen_shake: bool,
    /// The slot Continue loads (1-based in the file, like the menu).
    pub last_slot: Option<usize>,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            version: SETTINGS_VERSION,
            master_volume: 0.8,
            music_volume: 0.55,
            effects_volume: 0.9,
            screen_shake: true,
            last_slot: None,
        }
    }
}

impl Preferences {
    fn sanitize(mut self) -> Self {
        self.version = SETTINGS_VERSION;
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
        self.last_slot = self.last_slot.filter(|s| (1..=SLOTS).contains(s));
        self
    }
}

/// Settings for this machine only.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Display {
    pub version: u32,
    pub fullscreen: bool,
    pub width: u32,
    pub height: u32,
    pub shadows: bool,
    pub bloom: bool,
}

impl Default for Display {
    fn default() -> Self {
        Self {
            version: DISPLAY_VERSION,
            fullscreen: false,
            width: 1600,
            height: 900,
            shadows: true,
            bloom: true,
        }
    }
}

impl Display {
    fn sanitize(mut self) -> Self {
        self.version = DISPLAY_VERSION;
        self.width = self.width.clamp(640, 7680);
        self.height = self.height.clamp(360, 4320);
        self
    }
}

/// What a save slot holds, for the slot picker.
#[derive(Clone, Debug, PartialEq)]
pub enum SlotState {
    Empty,
    Saved(SlotSummary),
    /// The file exists but cannot be used (corrupt, or from a newer build).
    Unusable(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct SlotSummary {
    pub meta: SlotMeta,
    pub location: String,
    pub drops: u32,
    pub extractions: u32,
    pub modules: usize,
    pub tutorial: bool,
    pub mid_drop: bool,
}

pub struct Storage {
    pub paths: GamePaths,
    saving: bool,
}

impl Storage {
    pub fn discover(no_save: bool) -> std::io::Result<Self> {
        // A checkout keeps its data/ beside Cargo.toml instead of target/.
        let development_root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let paths = GamePaths::discover(Some(development_root))?;
        let storage = Self {
            paths,
            saving: !no_save,
        };
        if storage.saving {
            for dir in [
                &storage.paths.save,
                &storage.paths.config,
                &storage.paths.shared_config,
                &storage.paths.cache,
            ] {
                std::fs::create_dir_all(dir)?;
            }
        }
        Ok(storage)
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

    // -- save slots -----------------------------------------------------------

    pub fn slot_path(&self, slot: usize) -> PathBuf {
        self.paths.save.join(format!("slot-{slot}.json"))
    }

    /// Every slot, 1-based, for the picker.
    pub fn slots(&self) -> Vec<(usize, SlotState)> {
        (1..=SLOTS).map(|s| (s, self.slot_state(s))).collect()
    }

    fn slot_state(&self, slot: usize) -> SlotState {
        match self.read_slot(slot) {
            Ok(None) => SlotState::Empty,
            Ok(Some(save)) => {
                let p = &save.profile;
                let (location, mid_drop) = match save.session.as_ref().and_then(|s| s.run_planet())
                {
                    Some(planet) => (format!("On {}", planet.name()), true),
                    None => (
                        p.orbiting
                            .map_or("Deep space".into(), |pl| format!("Orbiting {}", pl.name())),
                        false,
                    ),
                };
                SlotState::Saved(SlotSummary {
                    meta: save.meta,
                    location,
                    drops: p.stats.drops,
                    extractions: p.stats.extractions,
                    modules: p.upgrades.len(),
                    tutorial: !p.tutorial_complete,
                    mid_drop,
                })
            }
            Err(e) => SlotState::Unusable(e),
        }
    }

    fn read_slot(&self, slot: usize) -> Result<Option<SaveGame>, String> {
        let path = self.slot_path(slot);
        // Read as a tree first: an older save's drop in progress may be in a
        // format this build cannot resume, and is dropped before parsing.
        let raw =
            read_json::<serde_json::Value>(&path).map_err(|e| format!("unreadable save ({e})"))?;
        let save = raw
            .map(|mut value| {
                let version = value
                    .get("version")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(0);
                if SaveGame::strip_stale_run(version as u32)
                    && let Some(session) = value.get_mut("session").and_then(|s| s.as_object_mut())
                {
                    session.insert("run".into(), serde_json::Value::Null);
                }
                serde_json::from_value::<SaveGame>(value)
            })
            .transpose()
            .map_err(|e| format!("unreadable save ({e})"))?;
        save.map(|s| s.migrate().map_err(|e| e.to_string()))
            .transpose()
    }

    /// Load a slot. A corrupt file is copied aside so a new game can reuse
    /// the slot without losing it.
    pub fn load_slot(&self, slot: usize) -> Result<SaveGame, String> {
        match self.read_slot(slot) {
            Ok(Some(save)) => Ok(save),
            Ok(None) => Err(format!("slot {slot} is empty")),
            Err(e) => {
                if e.starts_with("unreadable") && self.saving {
                    let aside = self
                        .paths
                        .save
                        .join(format!("slot-{slot}.corrupt-{}.json", unix_time()));
                    let _ = std::fs::copy(self.slot_path(slot), &aside);
                }
                Err(e)
            }
        }
    }

    /// Write a slot. Refuses to replace a save from a newer build.
    pub fn save_slot(&self, slot: usize, save: &SaveGame) -> std::io::Result<()> {
        if !self.saving {
            return Ok(());
        }
        if let SlotState::Unusable(reason) = self.slot_state(slot)
            && reason.contains("newer")
        {
            return Err(std::io::Error::other(reason));
        }
        write_json(&self.slot_path(slot), save)
    }

    /// Delete a slot (kept as `slot-N.deleted.json` until the next delete).
    pub fn delete_slot(&self, slot: usize) -> std::io::Result<()> {
        if !self.saving {
            return Ok(());
        }
        let path = self.slot_path(slot);
        if path.exists() {
            std::fs::rename(
                &path,
                self.paths.save.join(format!("slot-{slot}.deleted.json")),
            )?;
        }
        Ok(())
    }

    // -- settings -----------------------------------------------------------

    fn settings_path(&self) -> PathBuf {
        self.paths.shared_config.join("settings.toml")
    }

    fn display_path(&self) -> PathBuf {
        self.paths.config.join("display.toml")
    }

    pub fn load_preferences(&self) -> Preferences {
        read_toml::<Preferences>(&self.settings_path())
            .unwrap_or_default()
            .sanitize()
    }

    pub fn save_preferences(&self, preferences: &Preferences) -> Result<(), String> {
        if !self.saving {
            return Ok(());
        }
        quasar_config::save_file_typed(self.settings_path(), preferences).map_err(|e| e.to_string())
    }

    pub fn load_display(&self) -> Display {
        read_toml::<Display>(&self.display_path())
            .unwrap_or_default()
            .sanitize()
    }

    pub fn save_display(&self, display: &Display) -> Result<(), String> {
        if !self.saving {
            return Ok(());
        }
        quasar_config::save_file_typed(self.display_path(), display).map_err(|e| e.to_string())
    }

    // -- controls -----------------------------------------------------------

    #[cfg(feature = "desktop")]
    fn controls_path(&self) -> PathBuf {
        self.paths.shared_config.join("controls.toml")
    }

    /// The player's bindings merged over `defaults` (so actions added by an
    /// update keep their default). An unreadable file is left untouched and
    /// the defaults are used.
    #[cfg(feature = "desktop")]
    pub fn load_controls(&self, defaults: &quasar_input::Bindings) -> quasar_input::Bindings {
        quasar_ui::load_bindings(self.controls_path(), defaults).unwrap_or_else(|e| {
            eprintln!("planetfall: ignoring controls file ({e}); using defaults");
            defaults.clone()
        })
    }

    #[cfg(feature = "desktop")]
    pub fn save_controls(&self, bindings: &quasar_input::Bindings) -> Result<(), String> {
        if !self.saving {
            return Ok(());
        }
        quasar_ui::save_bindings(self.controls_path(), bindings).map_err(|e| e.to_string())
    }

    // -- migration from earlier versions -------------------------------------

    /// One-time moves: `profile.json` into slot 1, JSON settings into TOML.
    /// Originals are renamed `*.migrated` rather than deleted. Call after
    /// content is installed (the old save names modules and planets).
    pub fn migrate_legacy(&self) {
        if !self.saving {
            return;
        }
        let old_prefs = self.paths.shared_config.join("preferences.json");
        if old_prefs.exists() && !self.settings_path().exists() {
            #[derive(Deserialize, Default)]
            #[serde(default)]
            struct Old {
                master_volume: Option<f32>,
                music_volume: Option<f32>,
                effects_volume: Option<f32>,
                screen_shake: Option<bool>,
            }
            if let Ok(Some(old)) = read_json::<Old>(&old_prefs) {
                let d = Preferences::default();
                let prefs = Preferences {
                    master_volume: old.master_volume.unwrap_or(d.master_volume),
                    music_volume: old.music_volume.unwrap_or(d.music_volume),
                    effects_volume: old.effects_volume.unwrap_or(d.effects_volume),
                    screen_shake: old.screen_shake.unwrap_or(d.screen_shake),
                    ..self.load_preferences()
                }
                .sanitize();
                if self.save_preferences(&prefs).is_ok() {
                    let _ = std::fs::rename(&old_prefs, old_prefs.with_extension("json.migrated"));
                }
            }
        }
        let old_display = self.paths.config.join("display.json");
        if old_display.exists() && !self.display_path().exists() {
            #[derive(Deserialize, Default)]
            #[serde(default)]
            struct Old {
                fullscreen: Option<bool>,
                width: Option<u32>,
                height: Option<u32>,
            }
            if let Ok(Some(old)) = read_json::<Old>(&old_display) {
                let d = Display::default();
                let display = Display {
                    fullscreen: old.fullscreen.unwrap_or(d.fullscreen),
                    width: old.width.unwrap_or(d.width),
                    height: old.height.unwrap_or(d.height),
                    ..d
                }
                .sanitize();
                if self.save_display(&display).is_ok() {
                    let _ =
                        std::fs::rename(&old_display, old_display.with_extension("json.migrated"));
                }
            }
        }
        // The profile last: it records slot 1 in the (now migrated) settings.
        let profile = self.paths.save.join("profile.json");
        if profile.exists() && !self.slot_path(1).exists() {
            let migrated = read_json::<LegacyProfile>(&profile)
                .ok()
                .flatten()
                .and_then(|legacy| legacy.into_profile().ok());
            if let Some(profile_v3) = migrated {
                let save = SaveGame {
                    version: planetfall_sim::save::SAVE_VERSION,
                    meta: SlotMeta {
                        created: unix_time(),
                        saved: unix_time(),
                        play_seconds: 0,
                    },
                    profile: profile_v3,
                    session: None,
                };
                if write_json(&self.slot_path(1), &save).is_ok() {
                    let _ = std::fs::rename(&profile, profile.with_extension("json.migrated"));
                    let mut prefs = self.load_preferences();
                    prefs.last_slot = prefs.last_slot.or(Some(1));
                    let _ = self.save_preferences(&prefs);
                }
            }
        }
    }
}

/// A TOML file, or None when missing or unreadable (defaults are used and
/// the bad file is left for the player to inspect).
fn read_toml<T: serde::de::DeserializeOwned>(path: &Path) -> Option<T> {
    if !path.exists() {
        return None;
    }
    quasar_config::load_file_typed(path).ok()
}

pub fn unix_time() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// A new save for a fresh game in a slot.
pub fn new_meta() -> SlotMeta {
    let now = unix_time();
    SlotMeta {
        created: now,
        saved: now,
        play_seconds: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use planetfall_sim::{Game, Profile};

    fn storage_in(dir: &Path) -> Storage {
        let root = dir.to_path_buf();
        let storage = Storage {
            paths: GamePaths {
                app: root.join("app"),
                save: root.join("data/save"),
                config: root.join("data/config"),
                shared_config: root.join("data/config/shared"),
                cache: root.join("data/cache"),
                root,
            },
            saving: true,
        };
        for dir in [
            &storage.paths.save,
            &storage.paths.config,
            &storage.paths.shared_config,
        ] {
            std::fs::create_dir_all(dir).unwrap();
        }
        storage
    }

    #[test]
    fn slots_save_load_list_and_delete() {
        let dir = tempfile::tempdir().unwrap();
        let storage = storage_in(dir.path());
        assert!(storage.slots().iter().all(|(_, s)| *s == SlotState::Empty));
        let mut game = Game::new(Profile::default(), 1);
        game.profile.materials.ferrite = 12;
        let save = SaveGame::capture(&game, new_meta());
        storage.save_slot(3, &save).unwrap();
        let loaded = storage.load_slot(3).unwrap();
        assert_eq!(loaded.profile.materials.ferrite, 12);
        let slots = storage.slots();
        assert!(matches!(&slots[2].1, SlotState::Saved(s) if s.tutorial && s.drops == 0));
        assert_eq!(slots[0].1, SlotState::Empty);
        storage.delete_slot(3).unwrap();
        assert_eq!(storage.slots()[2].1, SlotState::Empty);
        assert!(storage.paths.save.join("slot-3.deleted.json").exists());
    }

    /// A version 1 save with a drop in progress (old run format) still
    /// loads: the drop is dropped and the slot resumes on the ship.
    #[test]
    fn old_drops_in_progress_are_dropped_not_fatal() {
        let dir = tempfile::tempdir().unwrap();
        let storage = storage_in(dir.path());
        let game = Game::new(Profile::default(), 1);
        let mut value = serde_json::to_value(SaveGame::capture(&game, new_meta())).unwrap();
        value["version"] = 1.into();
        value["session"]["run"] = serde_json::json!({ "player": { "pos": [1, 2] }, "weapons": [] });
        std::fs::write(storage.slot_path(2), value.to_string()).unwrap();
        let loaded = storage.load_slot(2).unwrap();
        assert!(
            loaded
                .session
                .as_ref()
                .is_some_and(|s| s.run_planet().is_none())
        );
        assert_eq!(loaded.version, planetfall_sim::save::SAVE_VERSION);
    }

    #[test]
    fn corrupt_saves_are_kept_and_newer_saves_never_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let storage = storage_in(dir.path());
        std::fs::write(storage.slot_path(1), b"{ not json").unwrap();
        assert!(matches!(storage.slots()[0].1, SlotState::Unusable(_)));
        assert!(storage.load_slot(1).is_err());
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

        let newer = r#"{"version": 99, "meta": {}, "profile": {}, "session": null}"#;
        std::fs::write(storage.slot_path(2), newer).unwrap();
        let save = SaveGame::capture(&Game::new(Profile::default(), 1), new_meta());
        assert!(storage.save_slot(2, &save).is_err());
        assert_eq!(
            std::fs::read_to_string(storage.slot_path(2)).unwrap(),
            newer
        );
    }

    #[test]
    fn settings_round_trip_as_toml_and_are_sanitized() {
        let dir = tempfile::tempdir().unwrap();
        let storage = storage_in(dir.path());
        let prefs = Preferences {
            music_volume: 0.25,
            last_slot: Some(4),
            ..Preferences::default()
        };
        storage.save_preferences(&prefs).unwrap();
        let text = std::fs::read_to_string(storage.settings_path()).unwrap();
        assert!(text.contains("music_volume = 0.25"), "{text}");
        assert_eq!(storage.load_preferences(), prefs);
        std::fs::write(
            storage.settings_path(),
            "master_volume = 7.0\nlast_slot = 9\n",
        )
        .unwrap();
        let p = storage.load_preferences();
        assert_eq!(p.master_volume, 1.0);
        assert_eq!(p.last_slot, None);
        let display = Display {
            shadows: false,
            ..Display::default()
        };
        storage.save_display(&display).unwrap();
        assert_eq!(storage.load_display(), display);
    }

    /// Earlier layouts move into slot 1 and TOML settings, keeping originals.
    #[test]
    fn legacy_files_migrate_once() {
        let dir = tempfile::tempdir().unwrap();
        let storage = storage_in(dir.path());
        std::fs::write(
            storage.paths.save.join("profile.json"),
            r#"{"version":2,"materials":{"ferrite":30},"upgrades":{"drill_rig":1},"tutorial_complete":true}"#,
        )
        .unwrap();
        std::fs::write(
            storage.paths.shared_config.join("preferences.json"),
            r#"{"version":1,"music_volume":0.1}"#,
        )
        .unwrap();
        std::fs::write(
            storage.paths.config.join("display.json"),
            r#"{"version":1,"fullscreen":true,"width":1280,"height":720}"#,
        )
        .unwrap();
        storage.migrate_legacy();
        let save = storage.load_slot(1).unwrap();
        assert_eq!(save.profile.materials.ferrite, 30);
        assert!(save.profile.tutorial_complete);
        assert_eq!(storage.load_preferences().music_volume, 0.1);
        assert_eq!(storage.load_preferences().last_slot, Some(1));
        assert!(storage.load_display().fullscreen);
        assert!(storage.paths.save.join("profile.json.migrated").exists());
        assert!(!storage.paths.save.join("profile.json").exists());
    }
}
