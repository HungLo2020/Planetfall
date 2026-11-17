//! Sound through `quasar-audio`: WAV clips on music/effects buses, spatial
//! panning for world events, and looping music/ambience per game phase.
//! A missing audio device leaves the game silent but playable.

use crate::storage::Preferences;
use planetfall_sim::Phase;
use planetfall_sim::content::{AbilityKind, Rarity};
use planetfall_sim::events::GameEvent;
use planetfall_sim::math::V2;
use quasar_audio::{
    AudioEngine, BusId, Clip, EngineConfig, Listener, LoopMode, PlaybackConfig, SpatialConfig,
    SpatialModelKind, VoiceHandle,
};
use quasar_core::Rng;
use std::collections::BTreeMap;
use std::path::Path;

pub struct Audio {
    engine: AudioEngine,
    music_bus: BusId,
    effects_bus: BusId,
    clips: BTreeMap<String, Clip>,
    music: Option<(&'static str, VoiceHandle)>,
    ambience: Option<(&'static str, VoiceHandle)>,
    last_played: BTreeMap<String, u64>,
    frame: u64,
    /// Per-play pitch and gain spread (`quasar_audio` Variation).
    variation: Rng,
}

impl Audio {
    pub fn open(device: Option<&str>, asset_root: &Path, prefs: &Preferences) -> Option<Self> {
        let engine = match AudioEngine::open(device, EngineConfig::default()) {
            Ok(engine) => engine,
            Err(e) => {
                eprintln!("planetfall: audio unavailable ({e}); continuing silently");
                return None;
            }
        };
        let music_bus = engine.create_bus("music", prefs.music_volume, 4).ok()?;
        let effects_bus = engine
            .create_bus("effects", prefs.effects_volume, 48)
            .ok()?;
        // Every WAV in assets/sounds, by file stem; content data names them.
        let mut clips = BTreeMap::new();
        for path in sound_files(asset_root) {
            let Some(name) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            match Clip::load(&path) {
                Ok(clip) => {
                    clips.insert(name.to_owned(), clip);
                }
                Err(e) => eprintln!("planetfall: sound {}: {e}", path.display()),
            }
        }
        let audio = Self {
            engine,
            music_bus,
            effects_bus,
            clips,
            music: None,
            ambience: None,
            last_played: BTreeMap::new(),
            frame: 0,
            variation: Rng::seed_from_u64(0x1234_5678),
        };
        audio.apply(prefs);
        Some(audio)
    }

    pub fn apply(&self, prefs: &Preferences) {
        let _ = self.engine.set_master_volume(prefs.master_volume);
        let _ = self
            .engine
            .set_bus_volume(self.music_bus, prefs.music_volume);
        let _ = self
            .engine
            .set_bus_volume(self.effects_bus, prefs.effects_volume);
    }

    pub fn set_listener(&self, at: [f32; 3]) {
        let _ = self.engine.set_listener(Listener {
            position: at,
            forward: [0.0, 0.0, -1.0],
            up: [0.0, 1.0, 0.0],
        });
    }

    /// Play a one-shot effect; `min_gap` frames throttles rapid repeats.
    pub fn play(&mut self, name: &str, volume: f32, at: Option<V2>, min_gap: u64) {
        if let Some(&last) = self.last_played.get(name)
            && self.frame.saturating_sub(last) < min_gap
        {
            return;
        }
        let Some(clip) = self.clips.get(name).cloned() else {
            return;
        };
        self.last_played.insert(name.to_owned(), self.frame);
        let spatial = at.map(|p| SpatialConfig {
            position: [p[0], 0.0, p[1]],
            model: SpatialModelKind::Inverse {
                min_distance: 8.0,
                max_distance: 60.0,
                rolloff: 0.8,
            },
            pan_strength: 0.6,
        });
        let _ = self.engine.play(
            clip,
            PlaybackConfig {
                bus: self.effects_bus,
                volume,
                spatial,
                ..PlaybackConfig::default()
            }
            .with_variation(0.06, 0.08, &mut self.variation),
        );
    }

    pub fn ui(&mut self, name: &'static str) {
        self.play(name, 0.6, None, 2);
    }

    fn loop_on(&mut self, slot: Slot, name: Option<&'static str>, volume: f32) {
        let current = match slot {
            Slot::Music => &mut self.music,
            Slot::Ambience => &mut self.ambience,
        };
        if current.as_ref().map(|(n, _)| *n) == name {
            return;
        }
        if let Some((_, handle)) = current.take() {
            let _ = self.engine.stop(&handle);
        }
        let Some(name) = name else { return };
        let Some(clip) = self.clips.get(name).cloned() else {
            return;
        };
        let bus = match slot {
            Slot::Music => self.music_bus,
            Slot::Ambience => self.effects_bus,
        };
        if let Ok(handle) = self.engine.play(
            clip,
            PlaybackConfig {
                bus,
                volume,
                looping: LoopMode::Whole,
                priority: 100,
                ..PlaybackConfig::default()
            },
        ) {
            let current = match slot {
                Slot::Music => &mut self.music,
                Slot::Ambience => &mut self.ambience,
            };
            *current = Some((name, handle));
        }
    }

    /// Choose music and ambience for the current phase.
    pub fn set_scene(&mut self, phase: Phase, title: bool) {
        self.frame += 1;
        let (music, ambience) = match phase {
            _ if title => (Some("music_ship"), Some("ship_hum")),
            Phase::Ship | Phase::Ftl { .. } => (Some("music_ship"), Some("ship_hum")),
            Phase::Descent { .. } => (None, None),
            Phase::Surface | Phase::Ascent { .. } => (Some("music_surface"), Some("planet_wind")),
            Phase::Lost { .. } => (None, Some("planet_wind")),
        };
        self.loop_on(Slot::Music, music, 0.7);
        self.loop_on(Slot::Ambience, ambience, 0.5);
    }

    pub fn handle(&mut self, events: &[GameEvent]) {
        for event in events {
            match *event {
                GameEvent::Fired { weapon, at } => {
                    let info = weapon.info();
                    self.play(&info.sound, info.volume, Some(at), 3);
                }
                GameEvent::EnemyHit { at } => self.play("enemy_hit", 0.3, Some(at), 3),
                GameEvent::EnemyKilled { kind, at } => {
                    self.play(&kind.info().death_sound, 0.45, Some(at), 2);
                }
                GameEvent::EnemyAlerted { kind, at } => {
                    if let Some(sound) = &kind.info().alert_sound {
                        self.play(sound, 0.5, Some(at), 45);
                    }
                }
                GameEvent::Explosion { at, .. } => self.play("explosion", 0.7, Some(at), 4),
                GameEvent::PlayerHurt { .. } => self.play("player_hurt", 0.7, None, 8),
                GameEvent::PlayerDied => self.play("player_die", 0.9, None, 0),
                GameEvent::Dashed => self.play("dash", 0.5, None, 0),
                GameEvent::MineHit { at, .. } => self.play("mine_hit", 0.4, Some(at), 4),
                GameEvent::BlockBroken { at, .. } => self.play("block_break", 0.6, Some(at), 2),
                GameEvent::PickedUpXp => self.play("pickup_xp", 0.25, None, 3),
                GameEvent::PickedUpMaterial { .. } => self.play("pickup_material", 0.5, None, 3),
                GameEvent::LevelUp { .. } => self.play("level_up", 0.7, None, 0),
                GameEvent::WeaponFound { .. } => self.play("weapon_found", 0.8, None, 0),
                GameEvent::ObjectiveComplete => self.play("console", 0.8, None, 0),
                GameEvent::ExtractionCalled => self.play("alarm", 0.7, None, 0),
                GameEvent::ExtractionDenied | GameEvent::Denied => {
                    self.play("ui_deny", 0.6, None, 6)
                }
                GameEvent::PodLanded { at } => self.play("pod_impact", 0.9, Some(at), 0),
                // Wall hits are visual only (sparks); the shot already sounded.
                GameEvent::Impact { .. } => {}
                GameEvent::AbilityUsed { kind, from, .. } => {
                    let clip = match kind {
                        AbilityKind::Bore => "ability_bore",
                        AbilityKind::Blink => "ability_blink",
                        AbilityKind::Barrier => "ability_barrier",
                        AbilityKind::Turret => "ability_turret",
                    };
                    self.play(clip, 0.8, Some(from), 0);
                }
                GameEvent::AbilityNotReady => self.play("ui_deny", 0.35, None, 10),
                GameEvent::PlayerDown { .. } => self.play("player_die", 0.7, None, 0),
                GameEvent::ClassChanged { .. } => self.play("class_select", 0.6, None, 0),
                GameEvent::ClassLevelUp { .. } => self.play("level_up", 0.8, None, 0),
                GameEvent::CacheOpened { at } => self.play("weapon_found", 0.7, Some(at), 0),
                GameEvent::RareUpgrade { rarity } => {
                    let clip = if rarity == Rarity::Legendary {
                        "weapon_found"
                    } else {
                        "ui_confirm"
                    };
                    self.play(clip, 0.7, None, 0);
                }
                // Statuses are visual (particles and tints).
                GameEvent::Status { .. } => {}
                GameEvent::Liftoff => self.play("liftoff", 0.9, None, 0),
                GameEvent::TerminalOpened => self.play("console", 0.5, None, 4),
                GameEvent::TerminalClosed => self.play("ui_move", 0.5, None, 2),
                GameEvent::UpgradeBuilt { .. } => self.play("construct", 0.9, None, 0),
                GameEvent::FtlStarted { .. } => self.play("ftl", 0.9, None, 0),
                GameEvent::DescentStarted => {
                    self.play("door", 0.6, None, 0);
                    self.play("pod_descent", 0.8, None, 0);
                }
                GameEvent::Landed => self.play("pod_impact", 1.0, None, 0),
                GameEvent::TutorialStep | GameEvent::TutorialComplete => {
                    self.play("tutorial", 0.6, None, 10)
                }
                GameEvent::BuildMode { .. } => self.play("console", 0.4, None, 2),
                GameEvent::BlockPlaced { .. } => self.play("construct", 0.45, None, 2),
                GameEvent::BlockRemoved => self.play("block_break", 0.45, None, 2),
                GameEvent::FtlArrived { .. }
                | GameEvent::RunEnded { .. }
                | GameEvent::ProfileChanged => {}
            }
        }
    }
}

enum Slot {
    Music,
    Ambience,
}

/// The game's sound files, sorted.
fn sound_files(asset_root: &Path) -> Vec<std::path::PathBuf> {
    let mut files: Vec<_> = std::fs::read_dir(asset_root.join("sounds"))
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "wav"))
        .collect();
    files.sort();
    files
}

#[cfg(test)]
mod tests {
    use super::sound_files;
    use planetfall_sim::content::{EnemyId, WeaponId};

    /// Every sound decodes, and every sound the content data names exists
    /// (no audio device needed).
    #[test]
    fn all_clips_decode_and_content_sounds_exist() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets");
        let files = sound_files(&root);
        assert!(!files.is_empty());
        for path in &files {
            let clip = quasar_audio::Clip::load(path)
                .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            assert!(clip.frames() > 0, "{} is empty", path.display());
        }
        let has = |name: &str| {
            files
                .iter()
                .any(|p| p.file_stem().is_some_and(|s| s == name))
        };
        for w in WeaponId::all() {
            assert!(has(&w.info().sound), "weapon {} sound", w.key());
        }
        for e in EnemyId::all() {
            let info = e.info();
            assert!(has(&info.death_sound), "enemy {} death sound", e.key());
            if let Some(alert) = &info.alert_sound {
                assert!(has(alert), "enemy {} alert sound", e.key());
            }
        }
    }
}
