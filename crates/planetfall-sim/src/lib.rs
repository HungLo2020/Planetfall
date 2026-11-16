//! Planetfall game rules.
//!
//! This crate owns the whole game as plain Rust state advanced by fixed 60 Hz
//! ticks of device-independent [`input::GameInput`]. It has no window, GPU,
//! audio or file I/O dependencies, so it runs headless, in tests and under the
//! [`autoplay::Autopilot`]. Quasar supplies navigation (`quasar-nav`) and
//! spatial queries (`quasar-spatial`).

pub mod autoplay;
pub mod content;
pub mod events;
pub mod game;
pub mod input;
pub mod math;
pub mod profile;
pub mod rng;
pub mod run;
pub mod ship;
pub mod terrain;
pub mod tutorial;

pub use game::{Game, Phase, Terminal};
pub use input::{Command, GameInput};
pub use profile::Profile;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::autoplay::Autopilot;
    use crate::events::GameEvent;
    use crate::tutorial::TutorialStep;

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
            }
            if game.profile.tutorial_complete && game.profile.stats.extractions >= 2 {
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
        assert!(game.ship.layout.rooms.len() > 3);
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
        game.ship.player.pos = [pod[0], pod[1] - 2.6];
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
            orbiting: Some(content::PlanetId::Verdant),
            ..Profile::default()
        };
        let mut game = Game::new(profile, 5);
        let pod = game
            .ship
            .layout
            .station(ship::StationKind::DropPod)
            .unwrap()
            .pos;
        game.ship.player.pos = [pod[0], pod[1] - 2.6];
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
