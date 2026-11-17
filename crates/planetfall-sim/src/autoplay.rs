//! A simple autopilot that plays the whole loop. Used for headless runs,
//! soak tests and CI, so every feature is drivable without a window.

use crate::content::{
    ClassId, Material, ModStat, Offer, PerkEffect, PlanetId, ShipBlock, Upgrade, UpgradeId,
};
use crate::game::{Game, Phase, Terminal, nearest_ore};
use crate::input::{Command, GameInput};
use crate::math::V2;
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
                    let target = PlanetId::all()
                        .collect::<Vec<_>>()
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
                Terminal::Locker => {
                    let want = wanted_class(game);
                    let profile = &game.profile;
                    let options = profile.start_options(want);
                    if profile.class != want {
                        Command::SelectClass(want)
                    } else if options.len() > 1 && profile.start_weapon(want) == want.signature() {
                        // Exercise the starting-weapon choice once unlocked.
                        Command::SetStartWeapon(options[1])
                    } else {
                        Command::Close
                    }
                }
                Terminal::Debrief(_) | Terminal::Module(_) => Command::Close,
            };
            return GameInput {
                command: pulse.then_some(command),
                ..GameInput::default()
            };
        }
        if let Some(input) = self.build(game, pulse) {
            return input;
        }
        let goal = if game.profile.orbiting.is_none()
            || game.profile.orbiting.is_some_and(|o| {
                PlanetId::all()
                    .any(|p| game.profile.unlocked(p) && p.info().nav_level > o.info().nav_level)
            }) {
            StationKind::NavConsole
        } else if affordable(game).is_some() {
            StationKind::Fabricator
        } else if game.profile.class != wanted_class(game) {
            StationKind::Locker
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
            V2::new(spine, pos[1])
        } else if (pos[1] - station.pos[1]).abs() > 1.0 {
            V2::new(spine, station.pos[1])
        } else {
            station.pos
        };
        GameInput {
            movement: (waypoint - pos).normalize_or_zero(),
            ..GameInput::default()
        }
    }

    /// Exercise free-form building once: after the first extraction, place a
    /// lamp on the bridge, then leave build mode.
    fn build(&self, game: &Game, pulse: bool) -> Option<GameInput> {
        let profile = &game.profile;
        let ship = &game.ship;
        let wanted = profile.tutorial_complete
            && profile.ship_edits.is_empty()
            && profile.stats.extractions >= 1
            && profile.materials.covers(ShipBlock::Lamp.cost());
        let press = |input: GameInput| Some(if pulse { input } else { GameInput::default() });
        match ship.build {
            None if wanted => press(GameInput {
                build: true,
                ..GameInput::default()
            }),
            None => None,
            Some(_) if !wanted => press(GameInput {
                build: true,
                ..GameInput::default()
            }),
            Some(block) if block != ShipBlock::Lamp => press(GameInput {
                call_extraction: true,
                ..GameInput::default()
            }),
            // A bridge corner, clear of the walking routes.
            Some(_) if ship.build_cursor() == (5, 3) && ship.can_place(profile).is_ok() => {
                press(GameInput {
                    interact: true,
                    ..GameInput::default()
                })
            }
            Some(_) => {
                // Up the spine to the bridge, then west along row 3.
                let pos = ship.player.pos;
                let spine = 7.5 * TILE;
                let row = 3.5 * TILE;
                let waypoint = if (pos[1] - row).abs() > 0.3 {
                    V2::new(spine, row)
                } else {
                    V2::new(5.5 * TILE, row)
                };
                Some(GameInput {
                    movement: (waypoint - pos).normalize_or_zero(),
                    ..GameInput::default()
                })
            }
        }
    }

    fn surface(&self, game: &Game, pulse: bool) -> GameInput {
        let Some(run) = &game.run else {
            return GameInput::default();
        };
        if let Some(choices) = run.level_up_choices() {
            return GameInput {
                command: pulse.then_some(Command::ChoosePerk(best_offer(choices))),
                ..GameInput::default()
            };
        }
        let player = run.player().pos;
        // Build up before calling the pod, as a player would: a couple of
        // minutes or a few levels, whichever comes first.
        let ready =
            run.objective_done && (run.tutorial || run.minutes() >= 2.0 || run.player().level >= 6);
        let goal: Option<V2> = if ready {
            Some(run.landing)
        } else if let Some(cache) = run
            .caches
            .iter()
            .find(|c| !c.opened && c.pos.distance(player) < 14.0)
        {
            Some(cache.pos)
        } else {
            nearest_ore(run, Material::Lumite)
        };
        let mut movement = goal.map_or(V2::new(0.0, 0.0), |g| (g - player).normalize_or_zero());
        if ready && run.landing.distance(player) < 1.5 {
            movement = V2::new(0.0, 0.0);
        }
        // Sidestep nearby hostiles.
        for e in &run.enemies {
            let away = player - e.pos;
            let d = away.length();
            if d < 3.0 && d > 1e-3 {
                movement += away * 0.6 / d;
            }
        }
        // Use the class ability when the horde closes in.
        let crowd = run
            .enemies
            .iter()
            .filter(|e| e.pos.distance(player) < 6.0)
            .count();
        let ability = run.player().ability.cooldown == 0 && crowd >= 3 && pulse;
        GameInput {
            movement: movement.clamp_length_max(1.0),
            call_extraction: ready && matches!(run.extraction, Extraction::Idle) && pulse,
            dash: run.player().hurt_ticks > 0 && pulse,
            ability,
            ..GameInput::default()
        }
    }
}

/// A sensible pick: rarer first, then new weapons and offence over
/// utility.
fn best_offer(choices: &[Offer]) -> usize {
    let score = |o: &Offer| {
        let kind = match o.upgrade {
            Upgrade::NewWeapon(_) => 50,
            Upgrade::WeaponMod(_, m) => match m.info().stat {
                ModStat::Special => 40,
                ModStat::Damage | ModStat::FireRate | ModStat::Count => 30,
                ModStat::Element => 25,
                _ => 15,
            },
            Upgrade::AbilityMod(_) => 18,
            Upgrade::Field(p) => match p.info().effect {
                PerkEffect::Damage | PerkEffect::FireRate | PerkEffect::MaxHealth => 22,
                PerkEffect::Regeneration => 20,
                _ => 10,
            },
        };
        kind + o.rarity.index() as i32 * 12
    };
    choices
        .iter()
        .enumerate()
        .max_by_key(|(i, o)| (score(o), -(*i as i32)))
        .map_or(0, |(i, _)| i)
}

/// The class the autopilot plays: the starter through the tutorial, then a
/// different class after each extraction so every class gets exercised.
fn wanted_class(game: &Game) -> ClassId {
    if !game.profile.tutorial_complete {
        return ClassId::starter();
    }
    let classes: Vec<ClassId> = ClassId::all().collect();
    classes[game.profile.stats.extractions as usize % classes.len()]
}

fn affordable(game: &Game) -> Option<UpgradeId> {
    UpgradeId::all().find(|&u| {
        game.profile
            .next_cost(u)
            .is_some_and(|cost| game.profile.materials.covers(cost))
    })
}
