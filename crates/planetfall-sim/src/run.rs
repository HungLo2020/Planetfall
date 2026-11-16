//! One drop onto a planet: movement, auto-firing weapons, the horde, mining,
//! loot, level-up perks and extraction. Pure game state; no rendering.

use crate::content::{
    BLOATER_BLAST, DT, EnemyKind, MAX_WEAPON_LEVEL, MAX_WEAPONS, Material, Materials, Perk,
    PlanetId, WeaponKind, seconds, xp_to_next,
};
use crate::events::GameEvent;
use crate::input::{Command, GameInput};
use crate::math::{self, FloatExt, V2};
use crate::terrain::{FlowField, TILE, Terrain, Tile};
use quasar_core::Rng;
use quasar_sim::{StateHash, StateHasher};
use quasar_spatial::{Aabb, GridConfig, SpatialGrid};

pub const PLAYER_RADIUS: f32 = 0.45;
pub const PLAYER_SPEED: f32 = 6.0;
pub const EXTRACTION_RADIUS: f32 = 3.6;
pub const EXTRACTION_INBOUND: u32 = 12 * 60;
pub const EXTRACTION_HOLD: u32 = 6 * 60;
const DASH_TICKS: u32 = 11;
const DASH_COOLDOWN: u32 = 70;
const DASH_SPEED: f32 = 17.0;
const MINING_RATE: f32 = 3.0;
const MAX_SPARKS: usize = 400;

/// Profile-derived numbers a run starts with.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RunConfig {
    pub planet: PlanetId,
    pub seed: u64,
    pub tutorial: bool,
    pub max_health: f32,
    pub damage_multiplier: f32,
    pub mining_multiplier: f32,
    pub pickup_bonus: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mining {
    pub tile: (i32, i32),
    /// 0..1 progress through the block's toughness.
    pub progress: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Player {
    pub pos: V2,
    pub yaw: f32,
    pub health: f32,
    pub max_health: f32,
    pub walk: f32,
    pub moving: bool,
    pub dash_ticks: u32,
    pub dash_cooldown: u32,
    dash_dir: V2,
    pub invulnerable_ticks: u32,
    pub hurt_ticks: u32,
    pub level: u32,
    pub xp: u32,
    pending_levels: u32,
    pub move_speed: f32,
    pub damage_multiplier: f32,
    pub cooldown_multiplier: f32,
    pub pickup_range: f32,
    pub mining_multiplier: f32,
    pub regeneration: f32,
    pub mining: Option<Mining>,
    mine_sound: u32,
}

impl Player {
    pub fn xp_needed(&self) -> u32 {
        xp_to_next(self.level)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WeaponSlot {
    pub kind: WeaponKind,
    pub level: u8,
    cooldown: f32,
    /// Ticks since this weapon last fired (for muzzle flashes).
    pub since_fired: u32,
    /// Direction of the last shot.
    pub aim: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Enemy {
    pub id: u32,
    pub kind: EnemyKind,
    pub pos: V2,
    pub yaw: f32,
    pub health: f32,
    pub max_health: f32,
    pub hit_ticks: u32,
    pub attack_ticks: u32,
    pub walk: f32,
    pub spawn_ticks: u32,
    attack_cooldown: u32,
    drone_cooldown: u32,
    speed_scale: f32,
    alerted: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Corpse {
    pub kind: EnemyKind,
    pub pos: V2,
    pub yaw: f32,
    pub ticks: u32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ProjectileKind {
    Bolt,
    Pellet,
    Mortar { from: V2, to: V2, total: u32 },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Projectile {
    pub kind: ProjectileKind,
    pub pos: V2,
    pub height: f32,
    vel: V2,
    damage: f32,
    radius: f32,
    pub age: u32,
    ttl: u32,
    pierce: u32,
    hit: Vec<u32>,
}

impl Projectile {
    pub fn direction(&self) -> V2 {
        self.vel.normalize_or_zero()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Beam {
    pub points: Vec<V2>,
    pub ttl: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlastKind {
    Plasma,
    Bloater,
    PodLanding,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Blast {
    pub kind: BlastKind,
    pub pos: V2,
    pub radius: f32,
    pub ttl: u32,
    pub total: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PickupKind {
    Xp(u32),
    Material(Material, u32),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pickup {
    pub kind: PickupKind,
    pub pos: V2,
    pub height: f32,
    vel: V2,
    vy: f32,
    magnet: bool,
    pub age: u32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cache {
    pub pos: V2,
    pub opened: bool,
    pub open_ticks: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SparkKind {
    Rock,
    Ore(Material),
    Blood,
    Energy,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spark {
    pub kind: SparkKind,
    pub pos: [f32; 3],
    vel: [f32; 3],
    pub ttl: u32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Extraction {
    Idle,
    /// Pod inbound; it lands at `pos` when `ticks` reaches zero.
    Inbound {
        ticks: u32,
        pos: V2,
    },
    /// Pod down; the player must stand inside the zone for `EXTRACTION_HOLD` ticks.
    Waiting {
        pos: V2,
        hold: u32,
    },
    Departed {
        pos: V2,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunOutcome {
    Extracted,
    Died,
    Abandoned,
}

#[derive(Clone, Debug, PartialEq)]
pub enum RunState {
    Active,
    LevelUp(Vec<Perk>),
    Ended(RunOutcome),
}

#[derive(Clone)]
pub struct Run {
    pub planet: PlanetId,
    pub tutorial: bool,
    pub terrain: Terrain,
    flow: FlowField,
    pub landing: V2,
    pub player: Player,
    pub weapons: Vec<WeaponSlot>,
    pub enemies: Vec<Enemy>,
    pub corpses: Vec<Corpse>,
    pub projectiles: Vec<Projectile>,
    pub beams: Vec<Beam>,
    pub blasts: Vec<Blast>,
    pub pickups: Vec<Pickup>,
    pub caches: Vec<Cache>,
    pub sparks: Vec<Spark>,
    pub carried: Materials,
    pub objective: u32,
    pub objective_done: bool,
    pub extraction: Extraction,
    pub ticks: u32,
    pub kills: u32,
    pub state: RunState,
    /// The tutorial holds the horde back until the player has their bearings.
    pub spawning_enabled: bool,
    /// Test/autoplay switch: ignore incoming damage.
    pub invulnerable: bool,
    pub drone_angle: f32,
    pub caches_opened: u32,
    pub blocks_mined: u32,
    hazard: f32,
    spawn_timer: f32,
    next_id: u32,
    rng: Rng,
    index: SpatialGrid<2, u32>,
    scratch: Vec<u32>,
}

impl Run {
    pub fn new(config: RunConfig) -> Self {
        let (terrain, layout) = Terrain::generate(config.planet, config.seed, config.tutorial);
        let flow = FlowField::new(&terrain, layout.landing);
        let info = config.planet.info();
        let objective = if config.tutorial { 4 } else { info.objective };
        Self {
            planet: config.planet,
            tutorial: config.tutorial,
            flow,
            landing: layout.landing,
            player: Player {
                pos: (layout.landing + V2::new(0.0, 2.2)),
                yaw: 0.0,
                health: config.max_health,
                max_health: config.max_health,
                walk: 0.0,
                moving: false,
                dash_ticks: 0,
                dash_cooldown: 0,
                dash_dir: V2::new(0.0, 1.0),
                invulnerable_ticks: seconds(2.0),
                hurt_ticks: 0,
                level: 1,
                xp: 0,
                pending_levels: 0,
                move_speed: PLAYER_SPEED,
                damage_multiplier: config.damage_multiplier,
                cooldown_multiplier: 1.0,
                pickup_range: 3.0 + config.pickup_bonus,
                mining_multiplier: config.mining_multiplier,
                regeneration: 0.0,
                mining: None,
                mine_sound: 0,
            },
            weapons: vec![WeaponSlot {
                kind: WeaponKind::PulseBlaster,
                level: 1,
                cooldown: 0.5,
                since_fired: 999,
                aim: 0.0,
            }],
            enemies: Vec::new(),
            corpses: Vec::new(),
            projectiles: Vec::new(),
            beams: Vec::new(),
            blasts: Vec::new(),
            pickups: Vec::new(),
            caches: layout
                .caches
                .iter()
                .map(|&pos| Cache {
                    pos,
                    opened: false,
                    open_ticks: 0,
                })
                .collect(),
            sparks: Vec::new(),
            carried: Materials::default(),
            objective,
            objective_done: false,
            extraction: Extraction::Idle,
            ticks: 0,
            kills: 0,
            state: RunState::Active,
            spawning_enabled: !config.tutorial,
            invulnerable: false,
            drone_angle: 0.0,
            caches_opened: 0,
            blocks_mined: 0,
            hazard: info.hazard,
            spawn_timer: 0.0,
            next_id: 1,
            rng: Rng::seed_from_u64(config.seed ^ 0xA5A5_5A5A),
            index: SpatialGrid::new(GridConfig::new(4.0, 64, 4096).expect("valid grid config"))
                .expect("valid spatial grid"),
            scratch: Vec::new(),
            terrain,
        }
    }

    pub fn minutes(&self) -> f32 {
        self.ticks as f32 / 3600.0
    }

    pub fn level_up_choices(&self) -> Option<&[Perk]> {
        match &self.state {
            RunState::LevelUp(choices) => Some(choices),
            _ => None,
        }
    }

    pub fn outcome(&self) -> Option<RunOutcome> {
        match self.state {
            RunState::Ended(outcome) => Some(outcome),
            _ => None,
        }
    }

    pub fn weapon_level(&self, kind: WeaponKind) -> Option<u8> {
        self.weapons
            .iter()
            .find(|w| w.kind == kind)
            .map(|w| w.level)
    }

    /// World positions of the orbit drones, if equipped.
    pub fn drone_positions(&self) -> Vec<V2> {
        let Some(level) = self.weapon_level(WeaponKind::OrbitDrones) else {
            return Vec::new();
        };
        let stats = WeaponKind::OrbitDrones.stats(level);
        (0..stats.count)
            .map(|i| {
                let a = self.drone_angle + i as f32 * std::f32::consts::TAU / stats.count as f32;
                self.player.pos + V2::new(a.sin() * stats.range, a.cos() * stats.range)
            })
            .collect()
    }

    /// Advance one fixed tick.
    pub fn update(&mut self, input: &GameInput, events: &mut Vec<GameEvent>) {
        if input.command == Some(Command::Abandon) && !matches!(self.state, RunState::Ended(_)) {
            self.state = RunState::Ended(RunOutcome::Abandoned);
            events.push(GameEvent::RunEnded { extracted: false });
            return;
        }
        match &self.state {
            RunState::Ended(_) => return,
            RunState::LevelUp(choices) => {
                if let Some(Command::ChoosePerk(i)) = input.command
                    && let Some(&perk) = choices.get(i)
                {
                    self.apply_perk(perk, events);
                    self.state = RunState::Active;
                    self.offer_level_up(events);
                }
                return;
            }
            RunState::Active => {}
        }
        self.ticks += 1;
        self.update_player(input, events);
        self.flow.update(&mut self.terrain, self.player.pos);
        self.rebuild_index();
        self.fire_weapons(events);
        self.update_projectiles(events);
        self.update_drones(events);
        self.update_enemies(events);
        self.update_blasts(events);
        self.remove_dead(events);
        self.update_pickups(events);
        self.update_caches(events);
        self.update_spawner();
        self.update_extraction(input, events);
        self.update_effects();
        if !self.objective_done && self.carried.lumite >= self.objective {
            self.objective_done = true;
            events.push(GameEvent::ObjectiveComplete);
        }
        if self.player.health <= 0.0 {
            self.player.health = 0.0;
            self.state = RunState::Ended(RunOutcome::Died);
            events.push(GameEvent::PlayerDied);
            events.push(GameEvent::RunEnded { extracted: false });
        } else if matches!(self.state, RunState::Active) {
            self.offer_level_up(events);
        }
    }

    // -- player -------------------------------------------------------------

    fn update_player(&mut self, input: &GameInput, events: &mut Vec<GameEvent>) {
        let p = &mut self.player;
        let movement = input.movement.clamp_length_max(1.0);
        let moving = movement.length() > 0.15;
        p.dash_cooldown = p.dash_cooldown.saturating_sub(1);
        p.invulnerable_ticks = p.invulnerable_ticks.saturating_sub(1);
        p.hurt_ticks = p.hurt_ticks.saturating_sub(1);
        if input.dash && p.dash_cooldown == 0 && p.dash_ticks == 0 {
            p.dash_ticks = DASH_TICKS;
            p.dash_cooldown = DASH_COOLDOWN;
            p.dash_dir = if moving {
                movement.normalize_or_zero()
            } else {
                math::from_yaw(p.yaw)
            };
            p.invulnerable_ticks = p.invulnerable_ticks.max(DASH_TICKS + 4);
            events.push(GameEvent::Dashed);
        }
        let velocity = if p.dash_ticks > 0 {
            p.dash_ticks -= 1;
            p.dash_dir * DASH_SPEED
        } else {
            movement * p.move_speed
        };
        let target = p.pos + (velocity * DT);
        let (resolved, hit) = self.terrain.resolve_circle(target, PLAYER_RADIUS);
        p.pos = resolved;
        p.moving = moving;
        if moving {
            p.yaw = math::turn_toward(p.yaw, math::yaw_of(movement), 0.35);
            p.walk += velocity.length() * DT * 1.6;
        }
        if p.regeneration > 0.0 {
            p.health = (p.health + p.regeneration * DT).min(p.max_health);
        }
        // Auto-mine the block the player is pushing into.
        let mut mining = None;
        if moving
            && let Some((x, y)) = hit
            && self.terrain.tile(x, y).minable()
        {
            let to_block = (Terrain::center_of(x, y) - p.pos).normalize_or_zero();
            if to_block.dot(movement.normalize_or_zero()) > 0.35 {
                mining = Some((x, y));
            }
        }
        let Some((x, y)) = mining else {
            p.mining = None;
            return;
        };
        let tile = self.terrain.tile(x, y);
        let amount = MINING_RATE * p.mining_multiplier * DT;
        let at = Terrain::center_of(x, y);
        p.mine_sound += 1;
        if p.mine_sound % 14 == 1 {
            events.push(GameEvent::MineHit { at });
            let kind = match tile {
                Tile::Ore { material, .. } => SparkKind::Ore(material),
                _ => SparkKind::Rock,
            };
            let face = p.pos + (((at - p.pos).normalize_or_zero()) * 0.8);
            spawn_sparks(&mut self.sparks, &mut self.rng, kind, face, 1.0, 3);
        }
        match self.terrain.mine(x, y, amount) {
            Some(broken) => {
                self.player.mining = None;
                self.blocks_mined += 1;
                let material = match broken.was {
                    Tile::Ore { material, .. } => Some(material),
                    _ => None,
                };
                events.push(GameEvent::BlockBroken { at, material });
                let kind = material.map_or(SparkKind::Rock, SparkKind::Ore);
                spawn_sparks(&mut self.sparks, &mut self.rng, kind, at, 1.4, 12);
                let drops: &[(Material, u32)] = match material {
                    Some(Material::Ferrite) => &[(Material::Ferrite, 2), (Material::Ferrite, 2)],
                    Some(Material::Lumite) => &[(Material::Lumite, 1), (Material::Lumite, 1)],
                    Some(Material::Voidstone) => &[(Material::Voidstone, 1)],
                    None => &[],
                };
                for &(m, amount) in drops {
                    self.drop_pickup(PickupKind::Material(m, amount), at);
                }
                if material.is_none() && self.rng.chance(0.12) {
                    self.drop_pickup(PickupKind::Material(Material::Ferrite, 1), at);
                }
            }
            None => {
                let max = tile.max_hp();
                let remaining = match self.terrain.tile(x, y) {
                    Tile::Rock { hp } | Tile::Ore { hp, .. } => hp,
                    _ => 0.0,
                };
                self.player.mining = Some(Mining {
                    tile: (x, y),
                    progress: if max > 0.0 {
                        1.0 - remaining / max
                    } else {
                        0.0
                    },
                });
            }
        }
    }

    fn hurt_player(&mut self, amount: f32, events: &mut Vec<GameEvent>) {
        if self.invulnerable || self.player.invulnerable_ticks > 0 || amount <= 0.0 {
            return;
        }
        self.player.health -= amount;
        self.player.hurt_ticks = 18;
        self.player.invulnerable_ticks = 30;
        events.push(GameEvent::PlayerHurt { amount });
    }

    // -- weapons ------------------------------------------------------------

    fn rebuild_index(&mut self) {
        self.index.clear();
        for (i, e) in self.enemies.iter().enumerate() {
            if e.health <= 0.0 {
                continue;
            }
            let r = f64::from(e.kind.stats().radius);
            let (x, z) = (f64::from(e.pos[0]), f64::from(e.pos[1]));
            if let Ok(bounds) = Aabb::new([x - r, z - r], [x + r, z + r]) {
                self.index.insert(i as u32, bounds);
            }
        }
    }

    fn nearest_enemy(&self, from: V2, range: f32) -> Option<usize> {
        self.index
            .nearest([f64::from(from[0]), f64::from(from[1])], f64::from(range))
            .map(|n| n.id as usize)
            .filter(|&i| self.enemies[i].health > 0.0)
    }

    fn enemies_near(&mut self, at: V2, radius: f32) -> Vec<usize> {
        self.scratch.clear();
        self.index.query_radius_into(
            [f64::from(at[0]), f64::from(at[1])],
            f64::from(radius),
            &mut self.scratch,
        );
        self.scratch
            .iter()
            .map(|&i| i as usize)
            .filter(|&i| self.enemies[i].health > 0.0)
            .collect()
    }

    fn fire_weapons(&mut self, events: &mut Vec<GameEvent>) {
        for slot in 0..self.weapons.len() {
            let w = self.weapons[slot];
            self.weapons[slot].since_fired = w.since_fired.saturating_add(1);
            if w.kind == WeaponKind::OrbitDrones {
                continue;
            }
            let stats = w.kind.stats(w.level);
            let cooldown = (w.cooldown - DT).max(0.0);
            self.weapons[slot].cooldown = cooldown;
            if cooldown > 0.0 {
                continue;
            }
            let origin = self.player.pos + V2::new(0.0, 0.0);
            let Some(target) = self.nearest_enemy(origin, stats.range) else {
                continue;
            };
            let target_pos = self.enemies[target].pos;
            if !self.terrain.line_clear(origin, target_pos) && w.kind != WeaponKind::PlasmaMortar {
                continue;
            }
            let dir = (target_pos - origin).normalize_or_zero();
            let damage = stats.damage * self.player.damage_multiplier;
            match w.kind {
                WeaponKind::PulseBlaster => {
                    for i in 0..stats.count {
                        let spread = (i as f32 - (stats.count as f32 - 1.0) * 0.5) * 0.08;
                        self.spawn_projectile(
                            ProjectileKind::Bolt,
                            origin,
                            math::rotate(dir, spread),
                            &stats,
                            damage,
                        );
                    }
                }
                WeaponKind::Scattergun => {
                    for i in 0..stats.count {
                        let t = i as f32 / (stats.count - 1).max(1) as f32 - 0.5;
                        let spread = t * 0.75 + self.rng.range_f32(-0.04..0.04);
                        self.spawn_projectile(
                            ProjectileKind::Pellet,
                            origin,
                            math::rotate(dir, spread),
                            &stats,
                            damage,
                        );
                    }
                }
                WeaponKind::ArcCaster => {
                    let mut points = vec![origin];
                    let mut hit = vec![target];
                    let mut last = target;
                    self.damage_enemy(target, damage, events);
                    points.push(self.enemies[target].pos);
                    for _ in 1..stats.count {
                        let from = self.enemies[last].pos;
                        let next = self
                            .enemies_near(from, stats.radius)
                            .into_iter()
                            .filter(|i| !hit.contains(i))
                            .min_by(|&a, &b| {
                                self.enemies[a]
                                    .pos
                                    .distance_squared(from)
                                    .total_cmp(&self.enemies[b].pos.distance_squared(from))
                            });
                        let Some(next) = next else { break };
                        self.damage_enemy(next, damage * 0.85, events);
                        points.push(self.enemies[next].pos);
                        hit.push(next);
                        last = next;
                    }
                    self.beams.push(Beam { points, ttl: 9 });
                }
                WeaponKind::PlasmaMortar => {
                    for _ in 0..stats.count {
                        let candidates = self.enemies_near(origin, stats.range);
                        let pick = if candidates.is_empty() {
                            target
                        } else {
                            candidates[self.rng.range_usize(0..candidates.len())]
                        };
                        let to = self.enemies[pick].pos;
                        let total = seconds(stats.speed).max(1);
                        self.projectiles.push(Projectile {
                            kind: ProjectileKind::Mortar {
                                from: origin,
                                to,
                                total,
                            },
                            pos: origin,
                            height: 1.2,
                            vel: ((to - origin) * 1.0 / stats.speed),
                            damage,
                            radius: stats.radius,
                            age: 0,
                            ttl: total,
                            pierce: 0,
                            hit: Vec::new(),
                        });
                    }
                }
                WeaponKind::OrbitDrones => unreachable!(),
            }
            let slot_ref = &mut self.weapons[slot];
            slot_ref.cooldown = stats.cooldown * self.player.cooldown_multiplier;
            slot_ref.since_fired = 0;
            slot_ref.aim = math::yaw_of(dir);
            events.push(GameEvent::Fired {
                weapon: w.kind,
                at: origin,
            });
        }
    }

    fn spawn_projectile(
        &mut self,
        kind: ProjectileKind,
        origin: V2,
        dir: V2,
        stats: &crate::content::WeaponStats,
        damage: f32,
    ) {
        let start = origin + (dir * 0.6);
        self.projectiles.push(Projectile {
            kind,
            pos: start,
            height: 1.1,
            vel: (dir * stats.speed),
            damage,
            radius: stats.radius,
            age: 0,
            ttl: (stats.range / stats.speed * 60.0) as u32 + 2,
            pierce: stats.pierce,
            hit: Vec::new(),
        });
    }

    fn update_projectiles(&mut self, events: &mut Vec<GameEvent>) {
        let mut projectiles = std::mem::take(&mut self.projectiles);
        projectiles.retain_mut(|p| {
            p.age += 1;
            match p.kind {
                ProjectileKind::Mortar { from, to, total } => {
                    let t = (p.age as f32 / total as f32).min(1.0);
                    p.pos = V2::new(from[0].lerp(to[0], t), from[1].lerp(to[1], t));
                    p.height = 1.2 + 6.0 * t * (1.0 - t) - 1.2 * t;
                    if p.age >= total {
                        self.blasts.push(Blast {
                            kind: BlastKind::Plasma,
                            pos: to,
                            radius: p.radius,
                            ttl: 22,
                            total: 22,
                        });
                        for i in self.enemies_near(to, p.radius) {
                            self.damage_enemy(i, p.damage, events);
                        }
                        events.push(GameEvent::Explosion {
                            at: to,
                            radius: p.radius,
                        });
                        return false;
                    }
                    true
                }
                ProjectileKind::Bolt | ProjectileKind::Pellet => {
                    p.pos += p.vel * DT;
                    if p.age > p.ttl {
                        return false;
                    }
                    if self.terrain.solid_at(p.pos) {
                        spawn_sparks(
                            &mut self.sparks,
                            &mut self.rng,
                            SparkKind::Energy,
                            p.pos,
                            0.5,
                            2,
                        );
                        return false;
                    }
                    for i in self.enemies_near(p.pos, p.radius + 1.0) {
                        let e = self.enemies[i];
                        if p.hit.contains(&e.id) {
                            continue;
                        }
                        if e.pos.distance(p.pos) > p.radius + e.kind.stats().radius {
                            continue;
                        }
                        p.hit.push(e.id);
                        self.damage_enemy(i, p.damage, events);
                        if p.pierce == 0 {
                            return false;
                        }
                        p.pierce -= 1;
                    }
                    true
                }
            }
        });
        projectiles.append(&mut self.projectiles);
        self.projectiles = projectiles;
    }

    fn update_drones(&mut self, events: &mut Vec<GameEvent>) {
        let Some(level) = self.weapon_level(WeaponKind::OrbitDrones) else {
            return;
        };
        let stats = WeaponKind::OrbitDrones.stats(level);
        self.drone_angle = (self.drone_angle + stats.speed * DT) % std::f32::consts::TAU;
        let damage = stats.damage * self.player.damage_multiplier;
        let mut struck = false;
        for drone in self.drone_positions() {
            for i in self.enemies_near(drone, stats.radius + 1.0) {
                let e = self.enemies[i];
                if e.drone_cooldown > 0
                    || e.pos.distance(drone) > stats.radius + e.kind.stats().radius
                {
                    continue;
                }
                self.enemies[i].drone_cooldown =
                    seconds(stats.cooldown * self.player.cooldown_multiplier);
                self.damage_enemy(i, damage, events);
                struck = true;
            }
        }
        if struck
            && let Some(w) = self
                .weapons
                .iter_mut()
                .find(|w| w.kind == WeaponKind::OrbitDrones)
        {
            if w.since_fired > 20 {
                events.push(GameEvent::Fired {
                    weapon: WeaponKind::OrbitDrones,
                    at: self.player.pos,
                });
            }
            w.since_fired = 0;
        }
    }

    fn damage_enemy(&mut self, i: usize, amount: f32, events: &mut Vec<GameEvent>) {
        let e = &mut self.enemies[i];
        if e.health <= 0.0 {
            return;
        }
        e.health -= amount;
        e.hit_ticks = 6;
        let at = e.pos;
        if e.health <= 0.0 {
            // Deaths are resolved in `remove_dead`.
            return;
        }
        events.push(GameEvent::EnemyHit { at });
    }

    // -- enemies ------------------------------------------------------------

    fn update_enemies(&mut self, events: &mut Vec<GameEvent>) {
        let player = self.player.pos;
        let mut melee = 0.0f32;
        let mut detonate = Vec::new();
        for i in 0..self.enemies.len() {
            let e = self.enemies[i];
            if e.health <= 0.0 {
                continue;
            }
            let stats = e.kind.stats();
            let to_player = player - e.pos;
            let dist = to_player.length();
            let direct = dist < TILE * 2.5 && self.terrain.line_clear(e.pos, player);
            let goal = if direct {
                player
            } else {
                self.flow
                    .next_waypoint(&self.terrain, e.pos)
                    .unwrap_or(player)
            };
            let dir = (goal - e.pos).normalize_or_zero();
            // Separation from neighbours so hordes spread around the player.
            let mut push = V2::ZERO;
            for j in self.enemies_near(e.pos, stats.radius + 1.0) {
                if j == i {
                    continue;
                }
                let o = self.enemies[j];
                let min = stats.radius + o.kind.stats().radius;
                let d = e.pos - o.pos;
                let l = d.length();
                if l < min && l > 1e-4 {
                    push += d * (min - l) / l;
                } else if l <= 1e-4 {
                    push += V2::new(((i as f32) * 1.7).sin(), ((i as f32) * 1.3).cos());
                }
            }
            let reach = stats.radius + PLAYER_RADIUS + 0.2;
            let mut speed = stats.speed * e.speed_scale;
            if dist < reach {
                speed = 0.0;
            }
            if e.spawn_ticks > 0 {
                speed = 0.0;
            }
            if e.kind == EnemyKind::Skitter && dist < 5.0 && dist > reach {
                speed *= 1.35;
            }
            let step = (dir * speed * DT) + (push * 0.35);
            let (pos, _) = self
                .terrain
                .resolve_circle(e.pos + step, stats.radius * 0.9);
            let en = &mut self.enemies[i];
            en.pos = pos;
            en.spawn_ticks = en.spawn_ticks.saturating_sub(1);
            en.hit_ticks = en.hit_ticks.saturating_sub(1);
            en.attack_ticks = en.attack_ticks.saturating_sub(1);
            en.attack_cooldown = en.attack_cooldown.saturating_sub(1);
            en.drone_cooldown = en.drone_cooldown.saturating_sub(1);
            if to_player.length() > 1e-3 {
                en.yaw = math::turn_toward(en.yaw, math::yaw_of(to_player), 0.18);
            }
            en.walk += speed * DT * 1.7;
            if !en.alerted && dist < 12.0 {
                en.alerted = true;
                if matches!(e.kind, EnemyKind::Brute | EnemyKind::Husk) && self.rng.chance(0.35) {
                    events.push(GameEvent::EnemyAlerted {
                        kind: e.kind,
                        at: e.pos,
                    });
                }
            }
            if dist < reach && en.spawn_ticks == 0 {
                if e.kind == EnemyKind::Bloater {
                    detonate.push(i);
                } else if en.attack_cooldown == 0 {
                    en.attack_cooldown = 60;
                    en.attack_ticks = 14;
                    melee += stats.damage * self.damage_scale();
                }
            }
        }
        if melee > 0.0 {
            self.hurt_player(melee, events);
        }
        for i in detonate {
            self.enemies[i].health = 0.0;
        }
    }

    fn damage_scale(&self) -> f32 {
        (0.75 + 0.25 * self.hazard) * (1.0 + self.minutes() * 0.08)
    }

    fn update_blasts(&mut self, _events: &mut Vec<GameEvent>) {
        for b in &mut self.blasts {
            b.ttl = b.ttl.saturating_sub(1);
        }
        self.blasts.retain(|b| b.ttl > 0);
    }

    fn remove_dead(&mut self, events: &mut Vec<GameEvent>) {
        // Bloater chains can kill more enemies, so loop until stable.
        for _ in 0..4 {
            let dead: Vec<usize> = (0..self.enemies.len())
                .filter(|&i| self.enemies[i].health <= 0.0)
                .collect();
            if dead.is_empty() {
                break;
            }
            let mut blasts = Vec::new();
            for &i in &dead {
                let e = self.enemies[i];
                self.kills += 1;
                events.push(GameEvent::EnemyKilled {
                    kind: e.kind,
                    at: e.pos,
                });
                self.corpses.push(Corpse {
                    kind: e.kind,
                    pos: e.pos,
                    yaw: e.yaw,
                    ticks: 0,
                });
                spawn_sparks(
                    &mut self.sparks,
                    &mut self.rng,
                    SparkKind::Blood,
                    e.pos,
                    1.0,
                    6,
                );
                let xp = e.kind.stats().xp;
                self.drop_pickup(PickupKind::Xp(xp), e.pos);
                match e.kind {
                    EnemyKind::Brute => {
                        self.drop_pickup(PickupKind::Material(Material::Voidstone, 1), e.pos);
                        self.drop_pickup(PickupKind::Material(Material::Ferrite, 3), e.pos);
                    }
                    EnemyKind::Bloater => blasts.push(e.pos),
                    _ => {
                        if self.rng.chance(0.04) {
                            self.drop_pickup(PickupKind::Material(Material::Ferrite, 1), e.pos);
                        }
                    }
                }
            }
            let mut i = 0;
            self.enemies.retain(|e| {
                i += 1;
                e.health > 0.0
            });
            self.rebuild_index();
            for at in blasts {
                self.blasts.push(Blast {
                    kind: BlastKind::Bloater,
                    pos: at,
                    radius: BLOATER_BLAST,
                    ttl: 24,
                    total: 24,
                });
                events.push(GameEvent::Explosion {
                    at,
                    radius: BLOATER_BLAST,
                });
                if at.distance(self.player.pos) < BLOATER_BLAST + PLAYER_RADIUS {
                    self.hurt_player(
                        EnemyKind::Bloater.stats().damage * self.damage_scale(),
                        events,
                    );
                }
                for j in self.enemies_near(at, BLOATER_BLAST) {
                    self.damage_enemy(j, 30.0, events);
                }
            }
        }
        for c in &mut self.corpses {
            c.ticks += 1;
        }
        self.corpses.retain(|c| c.ticks < 50);
    }

    // -- loot ---------------------------------------------------------------

    fn drop_pickup(&mut self, kind: PickupKind, at: V2) {
        let angle = self.rng.range_f32(0.0..std::f32::consts::TAU);
        let speed = self.rng.range_f32(1.0..3.0);
        self.pickups.push(Pickup {
            kind,
            pos: at,
            height: 0.6,
            vel: V2::new(angle.sin() * speed, angle.cos() * speed),
            vy: self.rng.range_f32(3.0..5.0),
            magnet: false,
            age: 0,
        });
    }

    fn update_pickups(&mut self, events: &mut Vec<GameEvent>) {
        let player = self.player.pos;
        let range = self.player.pickup_range;
        let mut collected = Vec::new();
        for (i, p) in self.pickups.iter_mut().enumerate() {
            p.age += 1;
            let to = player - p.pos;
            let dist = to.length();
            if p.age > 20 && dist < range {
                p.magnet = true;
            }
            if p.magnet {
                let speed = 8.0 + p.age.min(240) as f32 * 0.05;
                let step = (to.normalize_or_zero()) * (speed * DT).min(dist);
                p.pos += step;
                p.height = p.height.lerp(0.9, 0.2);
            } else {
                p.vy -= 18.0 * DT;
                p.height = (p.height + p.vy * DT).max(0.35);
                if p.height <= 0.35 {
                    p.vy = 0.0;
                    p.vel *= 0.8;
                }
                let next = p.pos + (p.vel * DT);
                if !self.terrain.solid_at(next) {
                    p.pos = next;
                }
            }
            if dist < 0.75 && p.age > 10 {
                collected.push(i);
            }
        }
        for &i in collected.iter().rev() {
            let p = self.pickups.swap_remove(i);
            match p.kind {
                PickupKind::Xp(amount) => {
                    self.player.xp += amount;
                    while self.player.xp >= self.player.xp_needed() {
                        self.player.xp -= self.player.xp_needed();
                        self.player.level += 1;
                        self.player.pending_levels += 1;
                    }
                    events.push(GameEvent::PickedUpXp);
                }
                PickupKind::Material(material, amount) => {
                    self.carried.add(material, amount);
                    events.push(GameEvent::PickedUpMaterial { material, amount });
                }
            }
        }
    }

    fn update_caches(&mut self, events: &mut Vec<GameEvent>) {
        for i in 0..self.caches.len() {
            let c = self.caches[i];
            if c.opened {
                self.caches[i].open_ticks += 1;
                continue;
            }
            if c.pos.distance(self.player.pos) > 1.9 {
                continue;
            }
            self.caches[i].opened = true;
            self.caches_opened += 1;
            let unowned: Vec<WeaponKind> = WeaponKind::ALL
                .into_iter()
                .filter(|k| self.weapon_level(*k).is_none())
                .collect();
            let weapon = if self.weapons.len() < MAX_WEAPONS && !unowned.is_empty() {
                let kind = unowned[self.rng.range_usize(0..unowned.len())];
                self.weapons.push(WeaponSlot {
                    kind,
                    level: 1,
                    cooldown: 0.3,
                    since_fired: 999,
                    aim: 0.0,
                });
                kind
            } else {
                let slot = self
                    .weapons
                    .iter_mut()
                    .filter(|w| w.level < MAX_WEAPON_LEVEL)
                    .min_by_key(|w| w.level);
                match slot {
                    Some(w) => {
                        w.level += 1;
                        w.kind
                    }
                    None => {
                        self.player.damage_multiplier += 0.1;
                        self.weapons[0].kind
                    }
                }
            };
            self.drop_pickup(PickupKind::Xp(5), c.pos);
            events.push(GameEvent::WeaponFound { weapon });
        }
    }

    // -- level ups ----------------------------------------------------------

    fn offer_level_up(&mut self, events: &mut Vec<GameEvent>) {
        if self.player.pending_levels == 0 || !matches!(self.state, RunState::Active) {
            return;
        }
        self.player.pending_levels -= 1;
        let mut pool: Vec<(Perk, u32)> = Vec::new();
        if self.weapons.len() < MAX_WEAPONS {
            for k in WeaponKind::ALL {
                if self.weapon_level(k).is_none() {
                    pool.push((Perk::NewWeapon(k), 3));
                }
            }
        }
        for w in &self.weapons {
            if w.level < MAX_WEAPON_LEVEL {
                pool.push((Perk::WeaponLevel(w.kind), 4));
            }
        }
        for perk in Perk::STATS {
            let weight = match perk {
                Perk::Regeneration if self.player.regeneration >= 3.0 => 0,
                Perk::MiningSpeed => 1,
                _ => 2,
            };
            if weight > 0 {
                pool.push((perk, weight));
            }
        }
        let mut choices = Vec::new();
        while choices.len() < 3 && !pool.is_empty() {
            let total: u32 = pool.iter().map(|(_, w)| w).sum();
            let mut roll = self.rng.range_usize(0..total as usize) as u32;
            let index = pool
                .iter()
                .position(|(_, w)| {
                    if roll < *w {
                        true
                    } else {
                        roll -= w;
                        false
                    }
                })
                .unwrap_or(0);
            choices.push(pool.swap_remove(index).0);
        }
        events.push(GameEvent::LevelUp {
            level: self.player.level,
        });
        self.state = RunState::LevelUp(choices);
    }

    fn apply_perk(&mut self, perk: Perk, events: &mut Vec<GameEvent>) {
        let p = &mut self.player;
        match perk {
            Perk::NewWeapon(kind) => {
                if self.weapon_level(kind).is_none() && self.weapons.len() < MAX_WEAPONS {
                    self.weapons.push(WeaponSlot {
                        kind,
                        level: 1,
                        cooldown: 0.2,
                        since_fired: 999,
                        aim: 0.0,
                    });
                    events.push(GameEvent::WeaponFound { weapon: kind });
                }
            }
            Perk::WeaponLevel(kind) => {
                if let Some(w) = self.weapons.iter_mut().find(|w| w.kind == kind) {
                    w.level = (w.level + 1).min(MAX_WEAPON_LEVEL);
                }
            }
            Perk::MaxHealth => {
                p.max_health += 20.0;
                p.health = (p.health + 20.0).min(p.max_health);
            }
            Perk::MoveSpeed => p.move_speed *= 1.1,
            Perk::Damage => p.damage_multiplier += 0.15,
            Perk::FireRate => p.cooldown_multiplier *= 0.9,
            Perk::PickupRange => p.pickup_range += 1.5,
            Perk::MiningSpeed => p.mining_multiplier += 0.4,
            Perk::Regeneration => p.regeneration += 1.0,
        }
    }

    // -- horde --------------------------------------------------------------

    fn intensity(&self) -> f32 {
        let extraction = !matches!(self.extraction, Extraction::Idle);
        self.hazard.sqrt()
            * (1.0 + self.minutes() * 0.5)
            * if self.tutorial { 0.5 } else { 1.0 }
            * if extraction { 1.8 } else { 1.0 }
    }

    fn update_spawner(&mut self) {
        if !self.spawning_enabled || matches!(self.extraction, Extraction::Departed { .. }) {
            return;
        }
        let cap = if self.tutorial { 45 } else { 160 };
        if self.enemies.len() >= cap {
            return;
        }
        self.spawn_timer += DT * self.intensity();
        // One group every SPAWN_PERIOD seconds at intensity 1; pressure ramps with time.
        const SPAWN_PERIOD: f32 = 2.6;
        if self.spawn_timer < SPAWN_PERIOD {
            return;
        }
        self.spawn_timer -= SPAWN_PERIOD;
        let minutes = self.minutes();
        let late = minutes > 2.5 || !matches!(self.extraction, Extraction::Idle);
        let roll = self.rng.next_f32();
        let (kind, count) = if !self.tutorial && late && roll < 0.06 {
            (EnemyKind::Brute, 1)
        } else if minutes > 1.0 && roll < 0.2 {
            (EnemyKind::Bloater, 1 + usize::from(self.rng.chance(0.3)))
        } else if roll < 0.5 {
            (EnemyKind::Skitter, 3 + self.rng.range_usize(0..3))
        } else {
            (EnemyKind::Husk, 2 + self.rng.range_usize(0..3))
        };
        let Some(center) = self.find_spawn_point() else {
            return;
        };
        let health_scale = (1.0 + minutes * 0.22) * (0.6 + 0.4 * self.hazard);
        for n in 0..count {
            let jitter = V2::new(self.rng.range_f32(-1.2..1.2), self.rng.range_f32(-1.2..1.2));
            let pos = center + jitter;
            let pos = if self.terrain.solid_at(pos) {
                center
            } else {
                pos
            };
            let stats = kind.stats();
            let id = self.next_id;
            self.next_id += 1;
            self.enemies.push(Enemy {
                id,
                kind,
                pos,
                yaw: self.rng.range_f32(0.0..std::f32::consts::TAU),
                health: stats.health * health_scale,
                max_health: stats.health * health_scale,
                hit_ticks: 0,
                attack_ticks: 0,
                walk: n as f32,
                spawn_ticks: 24,
                attack_cooldown: 30,
                drone_cooldown: 0,
                speed_scale: self.rng.range_f32(0.85..1.15),
                alerted: false,
            });
        }
    }

    fn find_spawn_point(&mut self) -> Option<V2> {
        for _ in 0..40 {
            let angle = self.rng.range_f32(0.0..std::f32::consts::TAU);
            let dist = self.rng.range_f32(17.0..26.0);
            let p = self.player.pos + V2::new(angle.sin() * dist, angle.cos() * dist);
            let (x, y) = Terrain::tile_of(p);
            if self.terrain.tile(x, y).solid() {
                continue;
            }
            let d = self.flow.distance_at(x, y);
            if d.is_finite() && d < 40.0 {
                return Some(Terrain::center_of(x, y));
            }
        }
        None
    }

    // -- extraction -----------------------------------------------------------

    fn update_extraction(&mut self, input: &GameInput, events: &mut Vec<GameEvent>) {
        if input.call_extraction {
            if matches!(self.extraction, Extraction::Idle) && self.objective_done {
                self.extraction = Extraction::Inbound {
                    ticks: EXTRACTION_INBOUND,
                    pos: self.landing,
                };
                self.spawning_enabled = true;
                events.push(GameEvent::ExtractionCalled);
            } else if matches!(self.extraction, Extraction::Idle) {
                events.push(GameEvent::ExtractionDenied);
            }
        }
        match self.extraction {
            Extraction::Inbound { ticks, pos } => {
                if ticks <= 1 {
                    self.extraction = Extraction::Waiting { pos, hold: 0 };
                    self.blasts.push(Blast {
                        kind: BlastKind::PodLanding,
                        pos,
                        radius: 3.0,
                        ttl: 30,
                        total: 30,
                    });
                    for i in self.enemies_near(pos, 3.0) {
                        self.enemies[i].health = 0.0;
                    }
                    events.push(GameEvent::PodLanded { at: pos });
                } else {
                    self.extraction = Extraction::Inbound {
                        ticks: ticks - 1,
                        pos,
                    };
                }
            }
            Extraction::Waiting { pos, hold } => {
                let inside = self.player.pos.distance(pos) < EXTRACTION_RADIUS;
                let hold = if inside {
                    hold + 1
                } else {
                    hold.saturating_sub(2)
                };
                if hold >= EXTRACTION_HOLD {
                    self.extraction = Extraction::Departed { pos };
                    self.state = RunState::Ended(RunOutcome::Extracted);
                    events.push(GameEvent::Liftoff);
                    events.push(GameEvent::RunEnded { extracted: true });
                } else {
                    self.extraction = Extraction::Waiting { pos, hold };
                }
            }
            Extraction::Idle | Extraction::Departed { .. } => {}
        }
    }

    fn update_effects(&mut self) {
        for b in &mut self.beams {
            b.ttl = b.ttl.saturating_sub(1);
        }
        self.beams.retain(|b| b.ttl > 0);
        for s in &mut self.sparks {
            s.vel[1] -= 14.0 * DT;
            for axis in 0..3 {
                s.pos[axis] += s.vel[axis] * DT;
            }
            if s.pos[1] < 0.05 {
                s.pos[1] = 0.05;
                s.vel = [s.vel[0] * 0.5, -s.vel[1] * 0.3, s.vel[2] * 0.5];
            }
            s.ttl = s.ttl.saturating_sub(1);
        }
        self.sparks.retain(|s| s.ttl > 0);
    }

    /// Order-sensitive digest for determinism checks.
    /// Feed everything that steers the run into `h` (positions as raw bits).
    pub fn hash_into(&self, h: &mut StateHasher) {
        for v in [self.player.pos.x, self.player.pos.y, self.player.health] {
            h.write_f32_bits(v);
        }
        h.write_u32(self.ticks);
        h.write_u32(self.kills);
        h.write_u32(self.carried.total());
        h.write_u64(self.terrain.revision);
        h.write_rng(&self.rng);
        h.write_u64(self.enemies.len() as u64);
        for e in &self.enemies {
            h.write_u32(e.id);
            for v in [e.pos.x, e.pos.y, e.health] {
                h.write_f32_bits(v);
            }
        }
        h.write_u64(self.projectiles.len() as u64);
        h.write_u64(self.pickups.len() as u64);
    }

    pub fn state_hash(&self) -> StateHash {
        let mut h = StateHasher::new();
        self.hash_into(&mut h);
        h.finish()
    }
}

fn spawn_sparks(
    sparks: &mut Vec<Spark>,
    rng: &mut Rng,
    kind: SparkKind,
    at: V2,
    height: f32,
    count: usize,
) {
    for _ in 0..count {
        if sparks.len() >= MAX_SPARKS {
            sparks.remove(0);
        }
        let a = rng.range_f32(0.0..std::f32::consts::TAU);
        let s = rng.range_f32(1.5..4.5);
        sparks.push(Spark {
            kind,
            pos: [at[0], height, at[1]],
            vel: [a.sin() * s, rng.range_f32(2.0..6.0), a.cos() * s],
            ttl: 20 + rng.range_usize(0..25) as u32,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(seed: u64) -> RunConfig {
        RunConfig {
            planet: PlanetId::Verdant,
            seed,
            tutorial: false,
            max_health: 100.0,
            damage_multiplier: 1.0,
            mining_multiplier: 1.0,
            pickup_bonus: 0.0,
        }
    }

    fn step(run: &mut Run, input: GameInput, ticks: u32) -> Vec<GameEvent> {
        let mut events = Vec::new();
        for _ in 0..ticks {
            if let Some(choices) = run.level_up_choices() {
                assert!(!choices.is_empty());
                run.update(
                    &GameInput {
                        command: Some(Command::ChoosePerk(0)),
                        ..GameInput::default()
                    },
                    &mut events,
                );
            }
            run.update(&input, &mut events);
        }
        events
    }

    #[test]
    fn deterministic_for_seed() {
        let input = GameInput {
            movement: V2::new(0.3, -0.7),
            ..GameInput::default()
        };
        let mut a = Run::new(config(5));
        let mut b = Run::new(config(5));
        step(&mut a, input, 1500);
        step(&mut b, input, 1500);
        assert_eq!(a.state_hash(), b.state_hash());
    }

    #[test]
    fn horde_spawns_and_weapons_kill() {
        let mut run = Run::new(config(11));
        run.invulnerable = true;
        let events = step(&mut run, GameInput::default(), 60 * 60);
        assert!(events.iter().any(|e| matches!(e, GameEvent::Fired { .. })));
        assert!(run.kills > 0, "no kills after a minute");
        assert!(
            run.player.level > 1 || run.level_up_choices().is_some() || !run.pickups.is_empty()
        );
    }

    #[test]
    fn walking_into_walls_mines_them() {
        let mut run = Run::new(config(2));
        run.spawning_enabled = false;
        let mut events = Vec::new();
        for dir in [V2::X, V2::NEG_X, V2::Y, V2::NEG_Y] {
            events.extend(step(
                &mut run,
                GameInput {
                    movement: dir,
                    ..GameInput::default()
                },
                600,
            ));
        }
        assert!(run.blocks_mined > 0);
        assert!(
            events
                .iter()
                .any(|e| matches!(e, GameEvent::BlockBroken { .. }))
        );
    }

    #[test]
    fn extraction_requires_objective_then_ends_run() {
        let mut run = Run::new(config(4));
        run.spawning_enabled = false;
        let call = GameInput {
            call_extraction: true,
            ..GameInput::default()
        };
        let events = step(&mut run, call, 1);
        assert!(events.contains(&GameEvent::ExtractionDenied));
        run.carried.lumite = run.objective;
        step(&mut run, GameInput::default(), 1);
        assert!(run.objective_done);
        step(&mut run, call, 1);
        assert!(matches!(run.extraction, Extraction::Inbound { .. }));
        run.spawning_enabled = false;
        run.invulnerable = true;
        // Walk back to the landing site and wait for liftoff.
        for _ in 0..(EXTRACTION_INBOUND + EXTRACTION_HOLD + 600) {
            let to = run.landing - run.player.pos;
            let input = GameInput {
                movement: if to.length() > 0.5 {
                    to.normalize_or_zero()
                } else {
                    V2::new(0.0, 0.0)
                },
                ..GameInput::default()
            };
            step(&mut run, input, 1);
            if run.outcome().is_some() {
                break;
            }
        }
        assert_eq!(run.outcome(), Some(RunOutcome::Extracted));
    }

    #[test]
    fn caches_grant_weapons() {
        let mut run = Run::new(config(8));
        run.spawning_enabled = false;
        let cache = run.caches[0].pos;
        run.player.pos = cache;
        let events = step(&mut run, GameInput::default(), 2);
        assert!(
            events
                .iter()
                .any(|e| matches!(e, GameEvent::WeaponFound { .. }))
        );
        assert_eq!(run.weapons.len(), 2);
    }
}
