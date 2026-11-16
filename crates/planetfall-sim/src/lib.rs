//! Planetfall game rules.
//!
//! This crate owns the whole game as plain Rust state advanced by fixed 60 Hz
//! ticks of device-independent [`input::GameInput`]. It has no window, GPU,
//! audio or file I/O dependencies, so it runs headless, in tests and under the
//! [`autoplay::Autopilot`]. Quasar supplies the seeded RNG (`quasar-core`),
//! vector math (`quasar-math`), replay verification (`quasar-sim`),
//! navigation (`quasar-nav`) and spatial queries (`quasar-spatial`).

pub mod autoplay;
pub mod content;
pub mod events;
pub mod game;
pub mod input;
pub mod math;
pub mod profile;
pub mod run;
pub mod save;
pub mod ship;
pub mod terrain;
pub mod tutorial;
pub mod voxel;

pub use game::{Game, Phase, Terminal};
pub use input::{Command, GameInput};
pub use profile::Profile;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::autoplay::Autopilot;
    use crate::events::GameEvent;
    use crate::math::V2;
    use crate::tutorial::TutorialStep;

    /// The tutorial finishes on its first extraction whatever the seed, even
    /// when the drop ends before a level-up or cache has been taught.
    #[test]
    fn tutorial_finishes_for_every_seed() {
        for seed in 0..12 {
            let mut game = Game::new(Profile::default(), seed);
            let mut pilot = Autopilot::new();
            for _ in 0..60 * 60 * 6 {
                if let Some(run) = &mut game.run {
                    run.invulnerable = true;
                }
                let input = pilot.input(&game);
                game.update(&input);
                if game.profile.tutorial_complete {
                    break;
                }
            }
            assert!(
                game.profile.tutorial_complete,
                "seed {seed}: tutorial stuck at {:?} after {} extractions",
                game.tutorial_step(),
                game.profile.stats.extractions
            );
            assert_eq!(game.profile.stats.extractions, 1, "seed {seed}");
        }
    }

    /// Play the full loop from a fresh profile: tutorial, drops and upgrades.
    #[test]
    fn autopilot_completes_tutorial_and_upgrades_ship() {
        let mut game = Game::new(Profile::default(), 1234);
        let mut pilot = Autopilot::new();
        let mut saw = std::collections::BTreeSet::new();
        for _ in 0..60 * 60 * 25 {
            if let Some(run) = &mut game.run {
                run.invulnerable = true;
            }
            let input = pilot.input(&game);
            game.update(&input);
            for e in &game.events {
                saw.insert(format!("{:?}", std::mem::discriminant(e)));
                if let GameEvent::UpgradeBuilt { .. } = e {
                    saw.insert("upgrade".into());
                }
                if let GameEvent::BlockPlaced { .. } = e {
                    saw.insert("block".into());
                }
            }
            if game.profile.tutorial_complete
                && game.profile.stats.extractions >= 2
                && saw.contains("block")
                && game.ship.build.is_none()
            {
                break;
            }
        }
        assert!(
            game.profile.tutorial_complete,
            "tutorial stuck at {:?}",
            game.tutorial_step()
        );
        assert!(
            game.profile.stats.extractions >= 2,
            "stats: {:?}",
            game.profile.stats
        );
        assert!(saw.contains("upgrade"), "never built a module");
        assert!(saw.contains("block"), "never placed a ship block");
        assert!(!game.profile.ship_edits.is_empty());
        assert!(game.ship.build.is_none(), "left build mode");
        assert!(game.ship.layout.rooms.len() > 3);
    }

    /// Record an autopilot session with `quasar-sim`, then replay the inputs
    /// on a fresh game: every tick's state hash must match, and a snapshot
    /// restored mid-run must continue identically.
    #[test]
    fn recorded_sessions_replay_without_divergence() {
        use quasar_sim::{Recorder, Replay, SimulationConfig};
        const SEED: u64 = 77;
        let config = SimulationConfig::new(1, SEED, "planetfall");
        let mut recorder = Recorder::new(Game::new(Profile::default(), SEED), config);
        let mut pilot = Autopilot::new();
        let mut midpoint = None;
        for tick in 0..6_000 {
            let input = pilot.input(recorder.simulation());
            recorder.tick(input).unwrap();
            if tick == 3_000 {
                midpoint = Some(recorder.snapshot());
            }
        }
        assert!(recorder.simulation().profile.stats.drops > 0);
        let expected = recorder.current_hash();
        let journal = recorder.journal().clone();
        let mut replay = Replay::new(Game::new(Profile::default(), SEED), journal.clone()).unwrap();
        let summary = replay.run_to_end().unwrap();
        assert_eq!(summary.last_hash, expected);

        // Rewind to the mid-run snapshot and replay only the tail.
        let snapshot = midpoint.unwrap();
        let tail: Vec<GameInput> = journal
            .records()
            .iter()
            .filter(|r| r.tick > snapshot.tick)
            .map(|r| r.input)
            .collect();
        recorder.restore(&snapshot).unwrap();
        for input in tail {
            recorder.tick(input).unwrap();
        }
        assert_eq!(recorder.current_hash(), expected);
    }

    #[test]
    fn deploying_without_orbit_is_refused() {
        let profile = Profile {
            tutorial_complete: true,
            ..Profile::default()
        };
        let mut game = Game::new(profile, 1);
        let pod = game
            .ship
            .layout
            .station(ship::StationKind::DropPod)
            .unwrap()
            .pos;
        game.ship.player.pos = V2::new(pod[0], pod[1] - 2.6);
        game.update(&GameInput {
            interact: true,
            ..GameInput::default()
        });
        assert!(game.events.contains(&GameEvent::Denied));
        assert_eq!(game.phase, Phase::Ship);
        assert!(game.notice.is_some());
    }

    #[test]
    fn death_loses_carried_materials() {
        let profile = Profile {
            tutorial_complete: true,
            orbiting: Some(content::PlanetId::first()),
            ..Profile::default()
        };
        let mut game = Game::new(profile, 5);
        let pod = game
            .ship
            .layout
            .station(ship::StationKind::DropPod)
            .unwrap()
            .pos;
        game.ship.player.pos = V2::new(pod[0], pod[1] - 2.6);
        game.update(&GameInput {
            interact: true,
            ..GameInput::default()
        });
        while game.phase != Phase::Surface {
            game.update(&GameInput::default());
        }
        let run = game.run.as_mut().unwrap();
        run.carried.ferrite = 50;
        run.player.health = 0.1;
        run.player.invulnerable_ticks = 0;
        // Let the horde finish the job.
        for _ in 0..60 * 120 {
            if let Some(run) = &mut game.run {
                run.player.health = run.player.health.min(0.1);
            }
            game.update(&GameInput::default());
            if matches!(game.terminal, Some(Terminal::Debrief(_))) {
                break;
            }
        }
        let Some(Terminal::Debrief(summary)) = &game.terminal else {
            panic!("no debrief, phase {:?}", game.phase);
        };
        assert_eq!(summary.lost.ferrite, 50);
        assert_eq!(game.profile.materials.ferrite, 0);
        assert_eq!(game.profile.stats.deaths, 1);
        assert_eq!(game.tutorial_step(), None::<TutorialStep>);
    }
}
