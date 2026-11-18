//! Sound through `quasar-audio`: WAV clips on music/effects buses, spatial
//! panning for world events, and looping music/ambience per game phase.
//! A missing audio device leaves the game silent but playable.

use crate::storage::Preferences;
use planetfall_sim::content::{AbilityKind, Rarity};
use planetfall_sim::events::GameEvent;
use planetfall_sim::math::V2;
use planetfall_sim::run::Extraction;
use planetfall_sim::{Game, Phase};
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
    /// Clips by sound name, with their variants (`name.v2`, ...).
    clips: BTreeMap<String, Vec<Clip>>,
    music: Option<Track>,
    ambience: Option<Track>,
    /// Loops fading out after a change.
    fading: Vec<Track>,
    last_played: BTreeMap<String, (u64, usize)>,
    /// The XP pickup chain: notes climbed so far and the frame of the last.
    xp_step: u32,
    xp_frame: u64,
    frame: u64,
    /// Per-play pitch and gain spread (`quasar_audio` Variation).
    variation: Rng,
    /// Footstep phase already sounded (stride index).
    stride: i64,
    /// Last extraction countdown second beeped.
    countdown: u32,
}

/// A looping music or ambience voice and its fade.
struct Track {
    name: String,
    handle: VoiceHandle,
    volume: f32,
    gain: f32,
}

/// Walk-animation units per footstep (half the astronaut's walk cycle).
const STRIDE: f32 = std::f32::consts::TAU / 2.2 / 2.0;
/// Seconds to crossfade music and ambience.
const CROSSFADE: f32 = 1.5;

impl Audio {
    pub fn open(device: Option<&str>, asset_root: &Path, prefs: &Preferences) -> Option<Self> {
        let engine = match AudioEngine::open(device, EngineConfig::default()) {
            Ok(engine) => engine,
            Err(e) => {
                eprintln!("planetfall: audio unavailable ({e}); continuing silently");
                return None;
            }
        };
        let music_bus = engine.create_bus("music", prefs.music_volume, 6).ok()?;
        let effects_bus = engine
            .create_bus("effects", prefs.effects_volume, 64)
            .ok()?;
        // Every WAV and Ogg in assets/sounds, by name; content data names them.
        let mut clips: BTreeMap<String, Vec<Clip>> = BTreeMap::new();
        for path in sound_files(asset_root) {
            let Some(name) = sound_name(&path) else {
                continue;
            };
            match Clip::load(&path) {
                Ok(clip) => clips.entry(name).or_default().push(clip),
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
            fading: Vec::new(),
            last_played: BTreeMap::new(),
            xp_step: 0,
            xp_frame: 0,
            frame: 0,
            variation: Rng::seed_from_u64(0x1234_5678),
            stride: 0,
            countdown: 0,
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

    /// Play a one-shot effect (a random variant, never the same one twice
    /// running); `min_gap` frames throttles rapid repeats.
    pub fn play(&mut self, name: &str, volume: f32, at: Option<V2>, min_gap: u64) {
        self.play_pitched(name, volume, at, min_gap, 1.0);
    }

    /// [`Self::play`] at a pitch multiplier (on top of the usual variation).
    pub fn play_pitched(
        &mut self,
        name: &str,
        volume: f32,
        at: Option<V2>,
        min_gap: u64,
        pitch: f32,
    ) {
        let last = self.last_played.get(name).copied();
        if let Some((frame, _)) = last
            && self.frame.saturating_sub(frame) < min_gap
        {
            return;
        }
        let Some(variants) = self.clips.get(name) else {
            return;
        };
        let mut pick = self.variation.range_usize(0..variants.len());
        if variants.len() > 1 && last.is_some_and(|(_, v)| v == pick) {
            pick = (pick + 1) % variants.len();
        }
        let clip = variants[pick].clone();
        self.last_played.insert(name.to_owned(), (self.frame, pick));
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
                pitch,
                spatial,
                ..PlaybackConfig::default()
            }
            // A tuned note keeps its pitch; everything else varies a little.
            .with_variation(
                if pitch == 1.0 { 0.05 } else { 0.0 },
                0.06,
                &mut self.variation,
            ),
        );
    }

    pub fn ui(&mut self, name: &'static str) {
        self.play(name, 0.55, None, 2);
    }

    /// Switch a loop slot, fading the old loop out and the new one in.
    fn loop_on(&mut self, slot: Slot, name: Option<&str>, volume: f32) {
        let current = match slot {
            Slot::Music => &mut self.music,
            Slot::Ambience => &mut self.ambience,
        };
        if current.as_ref().map(|t| t.name.as_str()) == name {
            return;
        }
        if let Some(old) = current.take() {
            self.fading.push(old);
        }
        let Some(name) = name else { return };
        let Some(clip) = self.clips.get(name).and_then(|v| v.first()).cloned() else {
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
                volume: 0.0,
                looping: LoopMode::Whole,
                priority: 100,
                ..PlaybackConfig::default()
            },
        ) {
            let track = Track {
                name: name.to_owned(),
                handle,
                volume,
                gain: 0.0,
            };
            match slot {
                Slot::Music => self.music = Some(track),
                Slot::Ambience => self.ambience = Some(track),
            }
        }
    }

    /// Ramp loop volumes toward their targets; stop faded-out loops.
    fn fade_loops(&mut self, dt: f32) {
        let step = dt / CROSSFADE;
        for track in self.music.iter_mut().chain(self.ambience.iter_mut()) {
            track.gain = (track.gain + step).min(1.0);
            let _ = self
                .engine
                .set_voice_volume(&track.handle, track.volume * track.gain);
        }
        self.fading.retain_mut(|track| {
            track.gain -= step;
            if track.gain <= 0.0 {
                let _ = self.engine.stop(&track.handle);
                return false;
            }
            let _ = self
                .engine
                .set_voice_volume(&track.handle, track.volume * track.gain);
            true
        });
    }

    /// Music and ambience for the moment, footsteps and countdowns. Call
    /// once per frame.
    pub fn update(&mut self, game: &Game, title: bool, dt: f32) {
        self.frame += 1;
        let run = game.run.as_ref();
        let boss = run.is_some_and(|r| {
            r.mission.target.is_some_and(|id| {
                r.enemies
                    .iter()
                    .any(|e| e.id == id && e.health > 0.0 && e.kind.info().boss)
            })
        });
        let extracting = run.is_some_and(|r| {
            matches!(
                r.extraction,
                Extraction::Inbound { .. } | Extraction::Waiting { .. }
            )
        });
        let planet_amb = run.map(|r| format!("amb_{}", r.planet.key()));
        let (music, ambience): (Option<&str>, Option<String>) = match game.phase {
            _ if title => (Some("music_ship"), Some("ship_hum".into())),
            Phase::Ship | Phase::Ftl { .. } => (Some("music_ship"), Some("ship_hum".into())),
            Phase::Descent { .. } => (None, None),
            Phase::Surface if boss => (Some("music_boss"), planet_amb),
            Phase::Surface if extracting => (Some("music_extraction"), planet_amb),
            Phase::Surface | Phase::Ascent { .. } => (Some("music_surface"), planet_amb),
            Phase::Lost { .. } => (None, planet_amb),
        };
        self.loop_on(Slot::Music, music, 0.55);
        let ambience = ambience.filter(|a| self.clips.contains_key(a));
        self.loop_on(Slot::Ambience, ambience.as_deref(), 0.45);
        self.fade_loops(dt);
        if title {
            return;
        }
        // Footsteps in time with the walk animation.
        let (walk, moving, metal) = match (run, game.phase) {
            (Some(r), Phase::Surface) => (r.player().walk, r.player().moving, false),
            (_, Phase::Ship) => (game.ship.player.walk, game.ship.player.moving, true),
            _ => (0.0, false, true),
        };
        let stride = ((walk / STRIDE) + 0.5).floor() as i64;
        if stride != self.stride {
            self.stride = stride;
            if moving {
                let (name, volume) = if metal {
                    ("step_metal", 0.22)
                } else {
                    ("step_rock", 0.28)
                };
                self.play(name, volume, None, 4);
            }
        }
        // Countdown beeps for the last seconds before the pod lands.
        if let Some(Extraction::Inbound { ticks, .. }) = run.map(|r| r.extraction) {
            let left = ticks.div_ceil(60);
            if left <= 5 && left != self.countdown && left > 0 {
                self.countdown = left;
                self.play("beep", if left == 1 { 0.7 } else { 0.5 }, None, 0);
            }
        } else {
            self.countdown = 0;
        }
    }

    pub fn handle(&mut self, events: &[GameEvent]) {
        use planetfall_sim::content::{Material, VentKind};
        use planetfall_sim::events::ComboKind;
        for event in events {
            match *event {
                GameEvent::Fired { weapon, at } => {
                    let info = weapon.info();
                    self.play(&info.sound, info.volume, Some(at), 3);
                }
                GameEvent::EnemyHit { at } => self.play("enemy_hit", 0.32, Some(at), 3),
                GameEvent::EnemyKilled { kind, at } => {
                    let volume = if kind.info().boss { 1.0 } else { 0.42 };
                    self.play(&kind.info().death_sound, volume, Some(at), 2);
                }
                GameEvent::EnemyAlerted { kind, at } => {
                    if let Some(sound) = &kind.info().alert_sound {
                        self.play(sound, 0.45, Some(at), 45);
                    }
                }
                // The crouch hisses so a lunge can be heard coming.
                GameEvent::EnemyLunged { kind, at } => {
                    if let Some(sound) = &kind.info().alert_sound {
                        self.play(sound, 0.35, Some(at), 10);
                    }
                }
                GameEvent::Explosion { at, .. } => self.play("explosion", 0.7, Some(at), 4),
                GameEvent::PlayerHurt { .. } => self.play("player_hurt", 0.65, None, 8),
                GameEvent::PlayerDied => self.play("player_die", 0.9, None, 0),
                GameEvent::Dashed => self.play("dash", 0.5, None, 0),
                GameEvent::MineHit { at, material } => {
                    let name = if material.is_some() {
                        "mine_hit_ore"
                    } else {
                        "mine_hit"
                    };
                    self.play(name, 0.4, Some(at), 4);
                }
                GameEvent::BlockCracked { at, .. } => self.play("block_crack", 0.4, Some(at), 3),
                GameEvent::BlockBroken { at, material } => {
                    let name = if material.is_some() {
                        "ore_break"
                    } else {
                        "block_break"
                    };
                    self.play(name, 0.5, Some(at), 3);
                }
                // Pickups in quick succession climb a pentatonic scale, so
                // a vacuumed stream of XP rings out a run of notes.
                GameEvent::PickedUpXp => {
                    let chained = self.frame.saturating_sub(self.xp_frame) < 30;
                    self.xp_step = if chained {
                        (self.xp_step + 1).min(9)
                    } else {
                        0
                    };
                    self.xp_frame = self.frame;
                    const SCALE: [f32; 10] =
                        [0.0, 2.0, 4.0, 7.0, 9.0, 12.0, 14.0, 16.0, 19.0, 21.0];
                    let pitch = 2f32.powf(SCALE[self.xp_step as usize] / 12.0) * 0.8;
                    self.play_pitched("pickup_xp", 0.22, None, 3, pitch);
                }
                GameEvent::PickedUpMaterial { material, .. } => {
                    let name = match material {
                        Material::Ferrite => "pickup_ferrite",
                        Material::Lumite => "pickup_lumite",
                        Material::Voidstone => "pickup_voidstone",
                    };
                    self.play(name, 0.45, None, 3);
                }
                GameEvent::LevelUp { .. } => self.play("level_up", 0.7, None, 0),
                GameEvent::WeaponFound { .. } => self.play("loot_rare", 0.75, None, 0),
                GameEvent::GearFound { rarity, .. } => {
                    let name = match rarity {
                        Rarity::Common => "loot_common",
                        Rarity::Uncommon => "loot_uncommon",
                        Rarity::Rare => "loot_rare",
                        Rarity::Legendary => "loot_legendary",
                    };
                    self.play(name, 0.75, None, 0);
                }
                GameEvent::SporeBlast { at } => self.play("spore_blast", 0.8, Some(at), 0),
                GameEvent::VentPulse { at, kind } => {
                    let name = match kind {
                        VentKind::Heat => "vent_heat",
                        VentKind::Cold => "vent_cold",
                    };
                    self.play(name, 0.5, Some(at), 6);
                }
                GameEvent::OxygenLow => self.play("oxygen_low", 0.7, None, 0),
                GameEvent::QuakeWarning => self.play("quake_warning", 0.7, None, 0),
                GameEvent::Quake { .. } => self.play("quake", 1.0, None, 0),
                GameEvent::Rubble { at } => self.play("rubble", 0.55, Some(at), 4),
                GameEvent::Combo { kind, at } => {
                    let name = match kind {
                        ComboKind::Shatter => "shatter",
                        ComboKind::Thermal => "thermal",
                        ComboKind::Conductive => "conductive",
                    };
                    self.play(name, 0.55, Some(at), 4);
                }
                GameEvent::Spat { at } => self.play("spit", 0.45, Some(at), 4),
                GameEvent::Jammed { at } => self.play("jam", 0.6, Some(at), 2),
                GameEvent::Burrow { at, .. } => self.play("burrow", 0.55, Some(at), 3),
                GameEvent::BossAbility { at } => self.play("boss_slam", 0.8, Some(at), 0),
                GameEvent::Split { at } => self.play("split", 0.55, Some(at), 2),
                GameEvent::BlueprintFound { .. } => self.play("loot_legendary", 0.8, None, 0),
                GameEvent::Equipped => self.play("equip", 0.55, None, 2),
                GameEvent::Traded => self.play("trade", 0.6, None, 2),
                GameEvent::ObjectiveComplete => self.play("objective", 0.8, None, 0),
                GameEvent::ExtractionCalled => self.play("alarm", 0.6, None, 0),
                GameEvent::ExtractionDenied | GameEvent::Denied => {
                    self.play("ui_deny", 0.55, None, 6)
                }
                GameEvent::PodLanded { at } => self.play("pod_impact", 0.9, Some(at), 0),
                // Wall hits are visual only (sparks); the shot already sounded.
                GameEvent::Impact { .. } => {}
                GameEvent::AbilityUsed { kind, from, .. } => {
                    let clip = match kind {
                        AbilityKind::Bore => "ability_bore",
                        AbilityKind::Blink => "ability_blink",
                        AbilityKind::Barrier => "ability_barrier",
                        AbilityKind::Dome => "ability_dome",
                    };
                    self.play(clip, 0.75, Some(from), 0);
                }
                GameEvent::AbilityNotReady => self.play("ui_deny", 0.3, None, 10),
                GameEvent::PlayerDown { .. } => self.play("player_die", 0.7, None, 0),
                GameEvent::ClassChanged { .. } => self.play("class_select", 0.6, None, 0),
                GameEvent::ClassLevelUp { .. } => self.play("class_level_up", 0.8, None, 0),
                GameEvent::CacheOpened { at } => self.play("cache_open", 0.7, Some(at), 0),
                GameEvent::RareUpgrade { rarity } => {
                    let clip = if rarity == Rarity::Legendary {
                        "loot_legendary"
                    } else {
                        "loot_rare"
                    };
                    self.play(clip, 0.6, None, 0);
                }
                GameEvent::SurgeWarning { .. } => self.play("surge_warning", 0.7, None, 0),
                GameEvent::ObjectiveProgress => self.play("progress", 0.6, None, 2),
                GameEvent::SecondaryComplete => self.play("objective", 0.7, None, 0),
                GameEvent::BossAwakened { at } => self.play("boss_awaken", 1.0, Some(at), 0),
                GameEvent::SurvivorFound => self.play("survivor", 0.75, None, 0),
                GameEvent::SurvivorDown | GameEvent::RigBroken => self.play("alarm", 0.6, None, 0),
                GameEvent::RigStarted { at } => self.play("rig_start", 0.8, Some(at), 0),
                GameEvent::SurgeStarted { .. } => self.play("surge_start", 0.8, None, 0),
                GameEvent::ThreatRose { .. } => self.play("threat_rose", 0.5, None, 0),
                // Statuses are visual (particles and tints).
                GameEvent::Status { .. } => {}
                GameEvent::Liftoff => self.play("liftoff", 0.9, None, 0),
                GameEvent::TerminalOpened => self.play("console", 0.5, None, 4),
                GameEvent::TerminalClosed => self.play("console_close", 0.5, None, 2),
                GameEvent::UpgradeBuilt { .. } => self.play("construct", 0.9, None, 0),
                GameEvent::FtlStarted { .. } => self.play("ftl", 0.9, None, 0),
                GameEvent::DescentStarted => {
                    self.play("door", 0.6, None, 0);
                    self.play("pod_descent", 0.8, None, 0);
                }
                GameEvent::Landed => self.play("pod_impact", 1.0, None, 0),
                GameEvent::TutorialStep | GameEvent::TutorialComplete => {
                    self.play("tutorial", 0.55, None, 10)
                }
                GameEvent::BuildMode { .. } => self.play("console", 0.4, None, 2),
                GameEvent::BlockPlaced { .. } => self.play("build_place", 0.5, None, 2),
                GameEvent::BlockRemoved => self.play("build_remove", 0.5, None, 2),
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

/// The game's sound files (WAV and Ogg Vorbis), sorted.
fn sound_files(asset_root: &Path) -> Vec<std::path::PathBuf> {
    let mut files: Vec<_> = std::fs::read_dir(asset_root.join("sounds"))
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "wav" || e == "ogg"))
        .collect();
    files.sort();
    files
}

/// The sound a file belongs to: `blaster.v3.ogg` is a variant of `blaster`.
fn sound_name(path: &Path) -> Option<String> {
    let stem = path.file_stem()?.to_str()?;
    let base = match stem.rsplit_once(".v") {
        Some((base, n)) if n.parse::<u32>().is_ok() => base,
        _ => stem,
    };
    Some(base.to_owned())
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
                .any(|p| super::sound_name(p).is_some_and(|s| s == name))
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
