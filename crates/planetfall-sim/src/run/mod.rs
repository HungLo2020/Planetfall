//! One drop onto a planet: movement, auto-firing weapons, the horde, mining,
//! loot, level-up perks and extraction. Pure game state; no rendering.
//!
//! The run is split by system; each file adds `impl Run` methods:
//!
//! | Module | System |
//! |---|---|
//! | `player` | movement, dash, damage taken, mining |
//! | `weapons` | targeting, firing by weapon behaviour, projectiles, drones |
//! | `enemies` | horde steering, attacks, deaths, drops, explosions |
//! | `spawner` | spawn pressure and data-driven spawn tables |
//! | `loot` | pickups, weapon caches, level-ups and perks |
//! | `extraction` | the extraction pod, hold zone and visual effects |
//!
//! Every type serializes, so a drop in progress can be saved and resumed:
//! the terrain saves as its seed plus the blocks mined, and derived state
//! (spatial index, flow field) is rebuilt on load.

mod enemies;
mod extraction;
mod loot;
mod player;
mod spawner;
mod weapons;

use crate::content::{
    EnemyId, Material, Materials, Perk, PlanetId, WeaponId, content, seconds, xp_to_next,
};
use crate::events::GameEvent;
use crate::input::{Command, GameInput};
use crate::math::V2;
use crate::terrain::{FlowField, Terrain};
use quasar_core::Rng;
use quasar_sim::{StateHash, StateHasher};
use quasar_spatial::{GridConfig, SpatialGrid};
use serde::{Deserialize, Serialize};

pub use extraction::{extraction_hold, extraction_inbound, extraction_radius};

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

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Mining {
    pub tile: (i32, i32),
    /// 0..1 progress through the block's toughness.
    pub progress: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
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

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct WeaponSlot {
    pub kind: WeaponId,
    pub level: u8,
    cooldown: f32,
    /// Ticks since this weapon last fired (for muzzle flashes).
    pub since_fired: u32,
    /// Direction of the last shot.
    pub aim: f32,
}

impl WeaponSlot {
    fn new(kind: WeaponId, cooldown: f32) -> Self {
        Self {
            kind,
            level: 1,
            cooldown,
            since_fired: 999,
            aim: 0.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Enemy {
    pub id: u32,
    pub kind: EnemyId,
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

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Corpse {
    pub kind: EnemyId,
    pub pos: V2,
    pub yaw: f32,
    pub ticks: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum ProjectileKind {
    Bolt,
    Pellet,
    Mortar { from: V2, to: V2, total: u32 },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Projectile {
    pub kind: ProjectileKind,
    /// The weapon that fired it (for its colour).
    pub weapon: WeaponId,
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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Beam {
    pub weapon: WeaponId,
    pub points: Vec<V2>,
    pub ttl: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum BlastKind {
    Weapon(WeaponId),
    Enemy(EnemyId),
    PodLanding,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Blast {
    pub kind: BlastKind,
    pub pos: V2,
    pub radius: f32,
    pub ttl: u32,
    pub total: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PickupKind {
    Xp(u32),
    Material(Material, u32),
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Pickup {
    pub kind: PickupKind,
    pub pos: V2,
    pub height: f32,
    vel: V2,
    vy: f32,
    magnet: bool,
    pub age: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Cache {
    pub pos: V2,
    pub opened: bool,
    pub open_ticks: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum SparkKind {
    Rock,
    Ore(Material),
    Blood(EnemyId),
    Energy,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Spark {
    pub kind: SparkKind,
    pub pos: [f32; 3],
    vel: [f32; 3],
    pub ttl: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Extraction {
    Idle,
    /// Pod inbound; it lands at `pos` when `ticks` reaches zero.
    Inbound {
        ticks: u32,
        pos: V2,
    },
    /// Pod down; the player must stand inside the zone for `extraction_hold()` ticks.
    Waiting {
        pos: V2,
        hold: u32,
    },
    Departed {
        pos: V2,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RunOutcome {
    Extracted,
    Died,
    Abandoned,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum RunState {
    Active,
    LevelUp(Vec<Perk>),
    Ended(RunOutcome),
}

fn spatial_index() -> SpatialGrid<2, u32> {
    SpatialGrid::new(GridConfig::new(4.0, 64, 4096).expect("valid grid config"))
        .expect("valid spatial grid")
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Run {
    pub planet: PlanetId,
    pub tutorial: bool,
    pub terrain: Terrain,
    /// Rebuilt on the next tick after loading.
    #[serde(skip)]
    flow: Option<FlowField>,
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
    #[serde(skip, default = "spatial_index")]
    index: SpatialGrid<2, u32>,
    #[serde(skip)]
    scratch: Vec<u32>,
}

impl Run {
    pub fn new(config: RunConfig) -> Self {
        let (terrain, layout) = Terrain::generate(config.planet, config.seed, config.tutorial);
        let flow = FlowField::new(&terrain, layout.landing);
        let info = config.planet.info();
        let tuning = &content().tuning;
        let objective = if config.tutorial {
            tuning.tutorial.objective
        } else {
            info.objective
        };
        Self {
            planet: config.planet,
            tutorial: config.tutorial,
            flow: Some(flow),
            landing: layout.landing,
            player: Player {
                pos: layout.landing + V2::new(0.0, 2.2),
                yaw: 0.0,
                health: config.max_health,
                max_health: config.max_health,
                walk: 0.0,
                moving: false,
                dash_ticks: 0,
                dash_cooldown: 0,
                dash_dir: V2::new(0.0, 1.0),
                invulnerable_ticks: seconds(tuning.player.spawn_invulnerability),
                hurt_ticks: 0,
                level: 1,
                xp: 0,
                pending_levels: 0,
                move_speed: tuning.player.speed,
                damage_multiplier: config.damage_multiplier,
                cooldown_multiplier: 1.0,
                pickup_range: tuning.player.pickup_range + config.pickup_bonus,
                mining_multiplier: config.mining_multiplier,
                regeneration: 0.0,
                mining: None,
                mine_sound: 0,
            },
            weapons: vec![WeaponSlot::new(WeaponId::starter(), 0.5)],
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
            index: spatial_index(),
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

    pub fn weapon_level(&self, kind: WeaponId) -> Option<u8> {
        self.weapons
            .iter()
            .find(|w| w.kind == kind)
            .map(|w| w.level)
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
        let flow = self
            .flow
            .get_or_insert_with(|| FlowField::new(&self.terrain, self.player.pos));
        flow.update(&mut self.terrain, self.player.pos);
        self.rebuild_index();
        self.fire_weapons(events);
        self.update_projectiles(events);
        self.update_drones(events);
        self.update_enemies(events);
        self.update_blasts();
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
mod tests;
