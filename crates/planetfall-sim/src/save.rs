//! Whole-game saves: progress ([`Profile`]) plus the session in flight
//! (tutorial step, where the player stands, a drop in progress), so loading
//! a slot continues exactly where the player left off. Pure data: the app
//! decides where slots live and when to write them.

use crate::game::{Game, Phase};
use crate::math::V2;
use crate::profile::{Profile, ProfileError};
use crate::run::Run;
use crate::tutorial::Tutorial;
use quasar_core::Rng;
use serde::{Deserialize, Serialize};

/// Save-file schema. The profile inside has its own version.
///
/// - 1: first slot format.
/// - 2: drops hold a list of players with classes and abilities.
/// - 3: weapon upgrades with rarities, elements and statuses.
/// - 4: the threat level, surges and elites.
///
/// A drop in progress from an older version cannot be resumed; the app
/// strips it before parsing ([`SaveGame::strip_stale_run`]) and the slot
/// resumes on the ship.
pub const SAVE_VERSION: u32 = 4;
/// Selectable save slots.
pub const SLOTS: usize = 5;

/// Facts for the slot picker, kept beside the game state.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SlotMeta {
    /// Unix seconds.
    pub created: u64,
    pub saved: u64,
    pub play_seconds: u64,
}

/// The session in flight: everything [`Game`] needs beyond the profile.
#[derive(Clone, Serialize, Deserialize)]
pub struct Session {
    pub tick: u64,
    rng: Rng,
    tutorial: Option<Tutorial>,
    ship_pos: V2,
    ship_yaw: f32,
    /// A drop in progress, resumed on the surface.
    run: Option<Run>,
}

impl Session {
    /// The planet of the drop in progress, if any.
    pub fn run_planet(&self) -> Option<crate::content::PlanetId> {
        self.run.as_ref().map(|r| r.planet)
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct SaveGame {
    pub version: u32,
    pub meta: SlotMeta,
    pub profile: Profile,
    pub session: Option<Session>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SaveError {
    Newer(u32),
    Profile(ProfileError),
}

impl std::fmt::Display for SaveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Newer(v) => write!(
                f,
                "save format {v} is newer than this build supports ({SAVE_VERSION})"
            ),
            Self::Profile(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for SaveError {}

impl SaveGame {
    /// Capture `game` for writing. Transitional phases are settled on a
    /// copy (an FTL jump arrives, a descent lands, an ascent banks its cargo,
    /// a lost drop ends) so the live game is untouched.
    pub fn capture(game: &Game, meta: SlotMeta) -> Self {
        let mut settled = game.clone();
        settled.settle();
        let session = Session {
            tick: settled.tick,
            rng: settled.rng().clone(),
            tutorial: settled.tutorial.clone(),
            ship_pos: settled.ship.player.pos,
            ship_yaw: settled.ship.player.yaw,
            run: settled.run.take(),
        };
        Self {
            version: SAVE_VERSION,
            meta,
            profile: settled.profile,
            session: Some(session),
        }
    }

    /// Whether a save of `version` can resume its drop in progress (older
    /// drops are stored in a format this build cannot read).
    pub fn strip_stale_run(version: u32) -> bool {
        version < SAVE_VERSION
    }

    /// Check versions and migrate the profile.
    pub fn migrate(mut self) -> Result<Self, SaveError> {
        if self.version > SAVE_VERSION {
            return Err(SaveError::Newer(self.version));
        }
        self.version = SAVE_VERSION;
        self.profile = self.profile.migrate().map_err(SaveError::Profile)?;
        Ok(self)
    }

    /// Rebuild the game. `seed` only matters for slots without a session.
    pub fn restore(self, seed: u64) -> Game {
        let mut game = Game::new(self.profile, seed);
        let Some(session) = self.session else {
            return game;
        };
        game.tick = session.tick;
        game.set_rng(session.rng);
        game.tutorial = session.tutorial;
        game.ship.player.pos = session.ship_pos;
        game.ship.player.yaw = session.ship_yaw;
        if let Some(run) = session.run {
            game.run = Some(run);
            game.phase = Phase::Surface;
        }
        game
    }
}

impl Game {
    /// Finish any transition so the game rests in the ship or on the surface.
    fn settle(&mut self) {
        let mut guard = 0;
        while !matches!(self.phase, Phase::Ship | Phase::Surface) && guard < 10_000 {
            self.update(&crate::input::GameInput::default());
            guard += 1;
        }
        self.terminal = None;
        self.notice = None;
        self.ship.build = None;
        self.events.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::autoplay::Autopilot;

    fn play(game: &mut Game, pilot: &mut Autopilot, ticks: u32) {
        for _ in 0..ticks {
            let input = pilot.input(game);
            game.update(&input);
        }
    }

    /// Saving mid-drop and loading continues the same drop, tutorial step
    /// and stats, then plays on identically.
    #[test]
    fn slots_resume_mid_drop() {
        let mut game = Game::new(Profile::default(), 9);
        let mut pilot = Autopilot::new();
        play(&mut game, &mut pilot, 2000);
        while game.phase != Phase::Surface {
            play(&mut game, &mut pilot, 1);
        }
        play(&mut game, &mut pilot, 300);
        let save = SaveGame::capture(&game, SlotMeta::default());
        let json = serde_json::to_string(&save).unwrap();
        let loaded: SaveGame = serde_json::from_str(&json).unwrap();
        let mut resumed = loaded.migrate().unwrap().restore(0);
        assert_eq!(resumed.phase, Phase::Surface);
        assert_eq!(resumed.tutorial_step(), game.tutorial_step());
        assert_eq!(resumed.state_hash(), game.state_hash());
        let mut pilot_b = Autopilot::new();
        play(&mut game, &mut pilot, 600);
        play(&mut resumed, &mut pilot_b, 600);
        assert_eq!(resumed.state_hash(), game.state_hash());
    }

    /// Mid-transition saves settle: a descent lands, an ascent banks.
    #[test]
    fn transitions_settle_when_saved() {
        let mut game = Game::new(Profile::default(), 3);
        let mut pilot = Autopilot::new();
        while !matches!(game.phase, Phase::Ftl { .. }) {
            play(&mut game, &mut pilot, 1);
        }
        let save = SaveGame::capture(&game, SlotMeta::default());
        assert!(save.profile.orbiting.is_some(), "FTL arrives on save");
        assert!(
            matches!(game.phase, Phase::Ftl { .. }),
            "live game untouched"
        );
        let restored = save.restore(0);
        assert_eq!(restored.phase, Phase::Ship);
    }

    #[test]
    fn newer_saves_are_refused() {
        let save = SaveGame {
            version: SAVE_VERSION + 1,
            meta: SlotMeta::default(),
            profile: Profile::default(),
            session: None,
        };
        assert!(matches!(save.migrate(), Err(SaveError::Newer(_))));
    }
}
