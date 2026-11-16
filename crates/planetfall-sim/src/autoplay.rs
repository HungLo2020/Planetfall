//! A simple autopilot that plays the whole loop. Used for headless runs,
//! soak tests and CI, so every feature is drivable without a window.

use crate::content::{Material, PlanetId, ShipUpgrade};
use crate::game::{Game, Phase, Terminal, nearest_ore};
use crate::input::{Command, GameInput};
use crate::math::{self, V2};
use crate::run::Extraction;
use crate::ship::StationKind;
use crate::terrain::TILE;

#[derive(Default)]
pub struct Autopilot {
    tick: u64,
}

impl Autopilot {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn input(&mut self, game: &Game) -> GameInput {
        self.tick += 1;
        let pulse = self.tick.is_multiple_of(2);
        match game.phase {
            Phase::Ship => self.ship(game, pulse),
            Phase::Surface => self.surface(game, pulse),
            _ => GameInput::default(),
        }
    }

    fn ship(&self, game: &Game, pulse: bool) -> GameInput {
        if let Some(terminal) = &game.terminal {
            let command = match terminal {
                Terminal::Navigation => {
                    let target = PlanetId::ALL
                        .into_iter()
                        .rev()
                        .find(|&p| game.profile.unlocked(p) && game.profile.orbiting != Some(p));
                    match target {
                        Some(p)
                            if game
                                .profile
                                .orbiting
                                .is_none_or(|o| o.info().nav_level < p.info().nav_level) =>
                        {
                            Command::TravelTo(p)
                        }
                        _ => Command::Close,
                    }
                }
                Terminal::Fabricator => affordable(game).map_or(Command::Close, Command::Purchase),
                Terminal::Debrief(_) | Terminal::Module(_) => Command::Close,
            };
            return GameInput {
                command: pulse.then_some(command),
                ..GameInput::default()
            };
        }
        let goal = if game.profile.orbiting.is_none()
            || game.profile.orbiting.is_some_and(|o| {
                PlanetId::ALL
                    .iter()
                    .any(|&p| game.profile.unlocked(p) && p.info().nav_level > o.info().nav_level)
            }) {
            StationKind::NavConsole
        } else if affordable(game).is_some() {
            StationKind::Fabricator
        } else {
            StationKind::DropPod
        };
        let Some(station) = game.ship.layout.station(goal) else {
            return GameInput::default();
        };
        if game.ship.focused_station().is_some_and(|s| s.kind == goal) {
            return GameInput {
                interact: pulse,
                ..GameInput::default()
            };
        }
        let pos = game.ship.player.pos;
        let spine = 7.5 * TILE;
        let waypoint = if (pos[1] - station.pos[1]).abs() > 1.0 && (pos[0] - spine).abs() > 0.4 {
            [spine, pos[1]]
        } else if (pos[1] - station.pos[1]).abs() > 1.0 {
            [spine, station.pos[1]]
        } else {
            station.pos
        };
        GameInput {
            movement: math::normalize(math::sub(waypoint, pos)),
            ..GameInput::default()
        }
    }

    fn surface(&self, game: &Game, pulse: bool) -> GameInput {
        let Some(run) = &game.run else {
            return GameInput::default();
        };
        if run.level_up_choices().is_some() {
            return GameInput {
                command: pulse.then_some(Command::ChoosePerk(0)),
                ..GameInput::default()
            };
        }
        let player = run.player.pos;
        let goal: Option<V2> = if run.objective_done {
            Some(run.landing)
        } else if let Some(cache) = run
            .caches
            .iter()
            .find(|c| !c.opened && math::distance(c.pos, player) < 14.0)
        {
            Some(cache.pos)
        } else {
            nearest_ore(run, Material::Lumite)
        };
        let mut movement = goal.map_or([0.0, 0.0], |g| math::normalize(math::sub(g, player)));
        if run.objective_done && math::distance(run.landing, player) < 1.5 {
            movement = [0.0, 0.0];
        }
        // Sidestep nearby hostiles.
        for e in &run.enemies {
            let away = math::sub(player, e.pos);
            let d = math::length(away);
            if d < 3.0 && d > 1e-3 {
                movement = math::add(movement, math::scale(away, 0.6 / d));
            }
        }
        GameInput {
            movement: math::clamp_unit(movement),
            call_extraction: run.objective_done
                && matches!(run.extraction, Extraction::Idle)
                && pulse,
            dash: run.player.hurt_ticks > 0 && pulse,
            ..GameInput::default()
        }
    }
}

fn affordable(game: &Game) -> Option<ShipUpgrade> {
    ShipUpgrade::ALL.into_iter().find(|&u| {
        game.profile
            .next_cost(u)
            .is_some_and(|cost| game.profile.materials.covers(cost))
    })
}
