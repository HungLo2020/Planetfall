//! A simple autopilot that plays the whole loop. Used for headless runs,
//! soak tests and CI, so every feature is drivable without a window.

use crate::content::{
    AbilityKind, ClassId, GearSlot, Material, ModStat, ObjectiveKind, Offer, PerkEffect, PlanetId,
    Rarity, ShipBlock, Upgrade, UpgradeId, WeaponBehavior,
};
use crate::game::{Game, Phase, Terminal, nearest_ore};
use crate::input::{Command, GameInput};
use crate::math::V2;
use crate::run::Extraction;
use crate::ship::StationKind;
use crate::ship::TILE;

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
                Terminal::Navigation if wants_trade(game) => Command::Dock,
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
                    } else if let Some(id) = gear_upgrade(game) {
                        Command::Equip(id)
                    } else if let Some(alt) = options
                        .iter()
                        .copied()
                        .filter(|w| *w != want.signature())
                        .find(|w| {
                            // Direct fire holds the first wave; lobbed and
                            // orbiting weapons do not on their own.
                            matches!(
                                w.info().behavior,
                                WeaponBehavior::Bolt
                                    | WeaponBehavior::Scatter
                                    | WeaponBehavior::Arc
                            )
                        })
                        .filter(|_| profile.start_weapon(want) == want.signature())
                    {
                        // Exercise the starting-weapon choice once unlocked.
                        Command::SetStartWeapon(alt)
                    } else {
                        Command::Close
                    }
                }
                // Rotate through the board; push the hazard up as it
                // unlocks, and back off to Easy while deaths outpace half
                // the extractions.
                Terminal::MissionBoard => {
                    let stats = &game.profile.stats;
                    let struggling = stats.deaths * 2 > stats.extractions;
                    // Geared up and rarely dying: push one level harder.
                    let comfortable = stats.deaths * 5 < stats.extractions
                        && game.profile.equipped(game.profile.class).len() >= 3;
                    let cap = if comfortable { 3 } else { 2 };
                    Command::Deploy {
                        contract: stats.drops as usize % game.profile.board.len().max(1),
                        hazard: if struggling {
                            1
                        } else {
                            game.profile.hazard_unlocked.min(cap)
                        },
                    }
                }
                Terminal::Trade => trade_action(game).unwrap_or(Command::Close),
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
        let goal = if game.profile.docked && trade_action(game).is_some() {
            StationKind::DropPod
        } else if wants_trade(game)
            || game.profile.orbiting.is_none()
            || game.profile.orbiting.is_some_and(|o| {
                PlanetId::all()
                    .any(|p| game.profile.unlocked(p) && p.info().nav_level > o.info().nav_level)
            })
        {
            StationKind::NavConsole
        } else if affordable(game).is_some() {
            StationKind::Fabricator
        } else if game.profile.class != wanted_class(game) || gear_upgrade(game).is_some() {
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
        // Greed, as a player would: keep fighting and mining after the
        // objective until hurt, the threat is high or time is long.
        let p = run.player();
        // Nothing left worth mining: go home too.
        let mined_out = Material::ALL.iter().all(|m| nearest_ore(run, *m).is_none());
        let ready = run.objective_done()
            && (run.tutorial
                || mined_out
                || p.health < p.max_health * 0.6
                || run.threat_level() >= 8
                || run.minutes() >= 14.0);
        let mission = &run.mission;
        let nearest = |points: &mut dyn Iterator<Item = V2>| {
            points.min_by(|a, b| a.distance(player).total_cmp(&b.distance(player)))
        };
        let mut keep_away = false;
        let o2 = &crate::content::content().tuning.hazards;
        let breathless = run.thin_air() && p.oxygen < o2.oxygen_max * 0.35;
        let goal: Option<V2> = if ready {
            Some(run.landing)
        } else if breathless {
            // Refill at the nearest oxygen vent (or the pod).
            nearest(
                &mut run
                    .hazards
                    .iter()
                    .filter(|h| h.kind == crate::run::HazardKind::Oxygen)
                    .map(|h| h.pos)
                    .chain(std::iter::once(run.landing)),
            )
        } else if let Some(cache) = run
            .caches
            .iter()
            .find(|c| !c.opened && c.pos.distance(player) < 14.0)
        {
            Some(cache.pos)
        } else if !mission.primary.done {
            match mission.primary_kind() {
                ObjectiveKind::OreQuota => nearest_ore(run, Material::Lumite),
                // Build up before waking a boss: levels or a few minutes,
                // mining well clear of its arena.
                ObjectiveKind::Boss
                    if mission.target.is_none()
                        && run.player().level < 8
                        && run.minutes() < 4.0 =>
                {
                    let arena = mission.arena.unwrap_or(run.landing);
                    ore_clear_of(run, arena, 26.0).or_else(|| nearest_ore(run, Material::Lumite))
                }
                // The hunted elite roams from the start: level up and keep
                // away from it first.
                ObjectiveKind::Hunt if run.player().level < 6 && run.minutes() < 3.0 => {
                    match mission
                        .target
                        .and_then(|id| run.enemies.iter().find(|e| e.id == id))
                    {
                        Some(e) if e.pos.distance(player) < 10.0 => Some(player * 2.0 - e.pos),
                        Some(e) => ore_clear_of(run, e.pos, 14.0),
                        None => nearest_ore(run, Material::Lumite),
                    }
                }
                ObjectiveKind::Boss | ObjectiveKind::Hunt => {
                    match mission
                        .target
                        .and_then(|id| run.enemies.iter().find(|e| e.id == id))
                    {
                        // Keep the guns on it from a few metres away.
                        Some(e) if e.pos.distance(player) < 5.0 => {
                            keep_away = true;
                            Some(player * 2.0 - e.pos)
                        }
                        Some(e) => Some(e.pos),
                        None => mission.arena.or_else(|| run.sites.first().copied()),
                    }
                }
                ObjectiveKind::Rescue => mission.survivor.map(|s| {
                    if s.following && !s.down {
                        run.landing
                    } else {
                        s.pos
                    }
                }),
                ObjectiveKind::Defend => mission
                    .rig
                    .map(|r| r.pos)
                    .filter(|p| p.distance(player) > 1.2),
                ObjectiveKind::Salvage => nearest(&mut mission.parts.iter().copied()),
                _ => nearest_ore(run, Material::Lumite),
            }
        } else {
            // Secondary objectives nearby, else more ore for the haul.
            nearest(&mut mission.glowcaps.iter().copied())
                .filter(|p| p.distance(player) < 25.0)
                .or_else(|| {
                    nearest(&mut mission.relays.iter().filter(|r| !r.done).map(|r| r.pos))
                        .filter(|p| p.distance(player) < 30.0)
                })
                .or_else(|| nearest_ore(run, Material::Lumite))
                .or_else(|| nearest_ore(run, Material::Voidstone))
                .or_else(|| nearest_ore(run, Material::Ferrite))
        };
        let _ = keep_away;
        // Wait at a vent until the tank is nearly full.
        let refilling = run.thin_air()
            && p.oxygen < o2.oxygen_max * 0.9
            && run.hazards.iter().any(|h| {
                h.kind == crate::run::HazardKind::Oxygen
                    && h.pos.distance(player) < o2.vent_radius * 0.8
            });
        let mut movement = if refilling {
            V2::new(0.0, 0.0)
        } else {
            goal.map_or(V2::new(0.0, 0.0), |g| (g - player).normalize_or_zero())
        };
        // Crossing lava is fine; standing in it (waiting, drilling) is not.
        let standing = movement.length() < 0.1 || p.mining.is_some();
        if standing && run.terrain.ground_at(player) == crate::terrain::Ground::Lava {
            let (tx, ty) = crate::terrain::Terrain::tile_of(player);
            let out = (-2..=2)
                .flat_map(|dy| (-2..=2).map(move |dx| (tx + dx, ty + dy)))
                .filter(|&(x, y)| {
                    run.terrain.tile(x, y) == crate::terrain::Tile::Floor
                        && run.terrain.ground(x, y) != crate::terrain::Ground::Lava
                })
                .map(|(x, y)| crate::terrain::Terrain::center_of(x, y))
                .min_by(|a, b| a.distance(player).total_cmp(&b.distance(player)));
            if let Some(out) = out {
                movement = (out - player).normalize_or_zero();
            }
        }
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
        // Use the class ability when the horde closes in. A blink is an
        // escape: only when hurt, and only once facing away from the crowd.
        let near: Vec<V2> = run
            .enemies
            .iter()
            .filter(|e| e.pos.distance(player) < 6.0)
            .map(|e| e.pos)
            .collect();
        let me = run.player();
        let mut ability = me.ability.charges > 0 && near.len() >= 3 && pulse;
        if me.class.info().ability.kind == AbilityKind::Blink {
            let centroid = near.iter().fold(V2::ZERO, |a, b| a + *b) / near.len().max(1) as f32;
            let away = (player - centroid).normalize_or_zero();
            let hurt = me.health < me.max_health * 0.6;
            if hurt && near.len() >= 3 {
                movement = away;
            }
            ability &= hurt && crate::math::from_yaw(me.yaw).dot(away) > 0.7;
        }
        GameInput {
            movement: movement.clamp_length_max(1.0),
            call_extraction: ready && matches!(run.extraction, Extraction::Idle) && pulse,
            dash: run.player().hurt_ticks > 0 && pulse,
            ability,
            ..GameInput::default()
        }
    }
}

/// Spare (unworn) items kept per slot when trading.
const SPARES: usize = 2;

/// Worth a trip to the station: more spare gear than a visit leaves behind
/// (so the ship never shuttles back and forth).
fn wants_trade(game: &Game) -> bool {
    let p = &game.profile;
    !p.docked
        && p.tutorial_complete
        && p.stash.iter().filter(|g| p.wearer(g.id).is_none()).count() > SPARES * 4 + 2
}

/// A better unworn item for one of the active class's slots.
fn gear_upgrade(game: &Game) -> Option<u32> {
    let p = &game.profile;
    let worn = p.equipped(p.class);
    GearSlot::ALL.iter().find_map(|&slot| {
        let current = worn
            .iter()
            .find(|g| g.slot() == slot)
            .map_or(0.0, |g| g.score());
        p.stash
            .iter()
            .filter(|g| g.slot() == slot && p.wearer(g.id).is_none() && g.score() > current)
            .max_by(|a, b| a.score().total_cmp(&b.score()))
            .map(|g| g.id)
    })
}

/// One station trade: sell spares (keeping the two best unworn per slot),
/// fuse spare commons, then buy an upgrade for the active class.
fn trade_action(game: &Game) -> Option<Command> {
    let p = &game.profile;
    let t = &crate::content::content().tuning.trade;
    for slot in GearSlot::ALL {
        let mut spare: Vec<&crate::gear::Gear> = p
            .stash
            .iter()
            .filter(|g| g.slot() == slot && p.wearer(g.id).is_none())
            .collect();
        spare.sort_by(|a, b| b.score().total_cmp(&a.score()));
        if let Some(g) = spare.get(SPARES) {
            return Some(Command::Sell(g.id));
        }
    }
    if p.fusible(Rarity::Common).len() >= t.fuse && p.credits >= t.fuse_cost[0] {
        return Some(Command::Fuse(Rarity::Common));
    }
    let worn = p.equipped(p.class);
    p.market
        .iter()
        .position(|g| {
            let current = worn
                .iter()
                .find(|w| w.slot() == g.slot())
                .map_or(0.0, |w| w.score());
            g.price() <= p.credits && g.score() > current
        })
        .map(Command::Buy)
}

/// The nearest lumite at least `clear` metres from `avoid`.
fn ore_clear_of(run: &crate::run::Run, avoid: V2, clear: f32) -> Option<V2> {
    let player = run.player().pos;
    run.terrain
        .ores(Material::Lumite)
        .map(|(x, y)| crate::terrain::Terrain::center_of(x, y))
        .filter(|p| p.distance(avoid) >= clear)
        .min_by(|a, b| a.distance(player).total_cmp(&b.distance(player)))
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
