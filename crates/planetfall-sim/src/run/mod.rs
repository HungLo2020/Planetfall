//! One drop onto a planet: movement, class abilities, auto-firing weapons,
//! the horde, mining, loot, level-up perks and extraction. Pure game state;
//! no rendering. Players are a list (index 0 is local) so co-op can follow.
//!
//! The run is split by system; each file adds `impl Run` methods:
//!
//! | Module | System |
//! |---|---|
//! | `player` | movement, dash, damage taken, mining |
//! | `abilities` | class abilities and what they deploy (barriers, turrets) |
//! | `weapons` | targeting, firing by weapon behaviour, projectiles, drones |
//! | `enemies` | horde steering, attacks, deaths, drops, explosions |
//! | `spawner` | spawn pressure and data-driven spawn tables |
//! | `loot` | pickups, weapon caches, level-ups and perks |
//! | `extraction` | the extraction pod, hold zone and visual effects |
//!
//! Every type serializes, so a drop in progress can be saved and resumed:
//! the terrain saves as its seed plus the blocks mined, and derived state
//! (spatial index, flow field) is rebuilt on load.

mod abilities;
mod arsenal;
mod enemies;
mod extraction;
mod hazards;
mod loot;
pub mod mission;
mod player;
mod spawner;
mod weapons;

use crate::content::{
    ClassId, Element, EnemyId, Material, Materials, ModId, ModStat, MutatorEffect, Offer,
    PerkEffect, PlanetId, Rarity, Special, WeaponId, WeaponStats, content, seconds, xp_to_next,
};
use crate::events::GameEvent;
use crate::input::{Command, GameInput};
use crate::math::V2;
use crate::mission::Contract;
use crate::terrain::{FlowField, Terrain};
use quasar_core::Rng;
use quasar_sim::{StateHash, StateHasher};
use quasar_spatial::{GridConfig, SpatialGrid};
use serde::{Deserialize, Serialize};

pub use extraction::{extraction_hold, extraction_inbound, extraction_radius};
pub use hazards::{Hazard, HazardKind, Quake};
pub use loot::Find;
pub use mission::{MissionState, Progress, Relay, Rig, Survivor};

/// What one player brings to a drop: their class, its tiers and the weapon
/// they start with.
#[derive(Clone, Debug, PartialEq)]
pub struct Loadout {
    pub class: ClassId,
    pub class_level: u32,
    pub start_weapon: WeaponId,
    /// Stats from equipped gear.
    pub gear: Vec<(PerkEffect, f32)>,
    /// Familiar weapons the class has not found yet (blueprint finds).
    pub blueprints: Vec<WeaponId>,
}

impl Loadout {
    /// A class at level 1 with its signature weapon.
    pub fn basic(class: ClassId) -> Self {
        Self {
            class,
            class_level: 1,
            start_weapon: class.signature(),
            gear: Vec::new(),
            blueprints: class.familiar().collect(),
        }
    }
}

/// Profile-derived numbers a run starts with.
#[derive(Clone, Debug, PartialEq)]
pub struct RunConfig {
    pub planet: PlanetId,
    pub seed: u64,
    pub tutorial: bool,
    /// The contract taken (its planet should match `planet`).
    pub contract: Contract,
    /// Hazard level 1 to 5.
    pub hazard: u8,
    /// One per player (co-op ready; the game has one today).
    pub players: Vec<Loadout>,
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

/// A class's active ability during a drop.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Ability {
    pub tier: u8,
    /// Ticks until the next charge is ready.
    pub cooldown: u32,
    /// The full cooldown last started (for the HUD).
    pub cooldown_total: u32,
    /// Uses ready now, and the most it can hold (Echo Step gives two).
    pub charges: u8,
    pub max_charges: u8,
    /// Ability upgrades taken this drop, with their rarity.
    pub mods: Vec<(ModId, Rarity)>,
}

impl Ability {
    fn sum(&self, stat: ModStat) -> f32 {
        self.mods
            .iter()
            .filter(|(m, _)| m.info().stat == stat)
            .map(|(m, r)| m.amount(*r) / 100.0)
            .sum()
    }

    /// Damage multiplier from upgrades.
    pub fn power(&self) -> f32 {
        1.0 + self.sum(ModStat::Power)
    }

    /// Range, duration and post-count multiplier from upgrades.
    pub fn reach(&self) -> f32 {
        1.0 + self.sum(ModStat::Reach)
    }

    pub fn has(&self, special: Special) -> bool {
        self.mods
            .iter()
            .any(|(m, _)| m.info().special == Some(special))
    }

    pub fn stacks(&self, id: ModId) -> u32 {
        self.mods.iter().filter(|(m, _)| *m == id).count() as u32
    }
}

/// Element potencies carried by a hit (index by [`Element::index`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Elements(pub [f32; 4]);

impl Elements {
    pub fn get(&self, e: Element) -> f32 {
        self.0[e.index()]
    }

    pub fn add(&mut self, e: Element, potency: f32) {
        self.0[e.index()] += potency;
    }

    pub fn any(&self) -> bool {
        self.0.iter().any(|p| *p > 0.0)
    }

    pub fn single(e: Element, potency: f32) -> Self {
        let mut out = Self::default();
        out.add(e, potency);
        out
    }
}

/// One instance of damage to an enemy.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Hit {
    pub damage: f32,
    /// Bonus against heavy enemies (from the attacker).
    pub heavy: f32,
    pub elements: Elements,
    /// Shock arcs and burns do not set off further shock arcs.
    pub secondary: bool,
}

impl Hit {
    pub fn plain(damage: f32) -> Self {
        Self {
            damage,
            ..Self::default()
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Player {
    pub class: ClassId,
    pub class_level: u32,
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
    /// Out of the fight (health reached zero).
    pub down: bool,
    pub level: u32,
    pub xp: u32,
    /// Every XP point collected this drop (class XP is earned from it).
    pub xp_total: u32,
    pending_levels: u32,
    pub move_speed: f32,
    pub damage_multiplier: f32,
    pub cooldown_multiplier: f32,
    pub pickup_range: f32,
    pub mining_multiplier: f32,
    pub regeneration: f32,
    pub ore_bonus: f32,
    pub extraction_speed: f32,
    pub dash_cooldown_multiplier: f32,
    pub heavy_damage: f32,
    pub ability_cooldown_multiplier: f32,
    pub luck: f32,
    /// Fraction of incoming damage ignored (gear; capped in `hurt_player`).
    #[serde(default)]
    pub armor: f32,
    /// Extra XP per shard (gear).
    #[serde(default)]
    pub xp_gain: f32,
    /// Fractional XP carried between shards.
    #[serde(default)]
    pub xp_fraction: f32,
    /// Familiar weapon blueprints this player can still find.
    #[serde(default)]
    pub blueprints: Vec<WeaponId>,
    /// Ticks jammed by a disruptor (slower weapons, no ability).
    #[serde(default)]
    pub jammed: u32,
    /// Ticks chilled by cold (slower).
    #[serde(default)]
    pub chilled: u32,
    /// Ticks soaked by water.
    #[serde(default)]
    pub wet: u32,
    /// Oxygen left on thin-air drops.
    #[serde(default)]
    pub oxygen: f32,
    /// Ticks until standing hazards (lava, spores, thin air) hurt again.
    #[serde(default)]
    pub hazard_ticks: u32,
    pub mining: Option<Mining>,
    mine_sound: u32,
    pub weapons: Vec<WeaponSlot>,
    pub drone_angle: f32,
    pub ability: Ability,
    /// Opened caches waiting to offer their upgrade.
    pending_caches: u32,
}

impl Player {
    fn new(loadout: Loadout, config: &RunConfig, pos: V2) -> Self {
        let tuning = &content().tuning.player;
        let class = loadout.class;
        let mut player = Self {
            class,
            class_level: loadout.class_level,
            pos,
            yaw: 0.0,
            health: config.max_health,
            max_health: config.max_health,
            walk: 0.0,
            moving: false,
            dash_ticks: 0,
            dash_cooldown: 0,
            dash_dir: V2::new(0.0, 1.0),
            invulnerable_ticks: seconds(tuning.spawn_invulnerability),
            hurt_ticks: 0,
            down: false,
            level: 1,
            xp: 0,
            xp_total: 0,
            pending_levels: 0,
            move_speed: tuning.speed,
            damage_multiplier: config.damage_multiplier,
            cooldown_multiplier: 1.0,
            pickup_range: tuning.pickup_range + config.pickup_bonus,
            mining_multiplier: config.mining_multiplier,
            regeneration: 0.0,
            ore_bonus: 0.0,
            extraction_speed: 0.0,
            dash_cooldown_multiplier: 1.0,
            heavy_damage: 0.0,
            ability_cooldown_multiplier: 1.0,
            luck: 0.0,
            armor: 0.0,
            xp_gain: 0.0,
            xp_fraction: 0.0,
            blueprints: loadout.blueprints.clone(),
            jammed: 0,
            chilled: 0,
            wet: 0,
            oxygen: content().tuning.hazards.oxygen_max,
            hazard_ticks: 0,
            mining: None,
            mine_sound: 0,
            weapons: vec![WeaponSlot::new(loadout.start_weapon, 0.5)],
            drone_angle: 0.0,
            ability: Ability {
                tier: class.ability_tier(loadout.class_level),
                cooldown: 0,
                cooldown_total: 1,
                charges: 1,
                max_charges: 1,
                mods: Vec::new(),
            },
            pending_caches: 0,
        };
        let tier = class.passive_tier(loadout.class_level);
        for effect in &class.info().passive.effects {
            let amount = effect.amount + effect.per_tier * f32::from(tier - 1);
            player.apply(effect.effect, amount);
        }
        for &(effect, amount) in &loadout.gear {
            player.apply(effect, amount);
        }
        // Mutators that change the player.
        let contract = &config.contract;
        if let Some(k) = contract.has_mutator(MutatorEffect::LowGravity) {
            player.move_speed *= k;
        }
        if let Some(k) = contract.has_mutator(MutatorEffect::GlassCannon) {
            player.damage_multiplier *= k;
        }
        player.max_health = player.max_health.max(1.0);
        player.health = player.max_health;
        player
    }

    pub fn xp_needed(&self) -> u32 {
        xp_to_next(self.level)
    }

    pub fn alive(&self) -> bool {
        !self.down
    }

    /// Apply a perk or passive effect (see [`PerkEffect`]).
    pub fn apply(&mut self, effect: PerkEffect, a: f32) {
        match effect {
            PerkEffect::MaxHealth => {
                self.max_health += a;
                self.health = (self.health + a.max(0.0)).min(self.max_health);
            }
            PerkEffect::MoveSpeed => self.move_speed *= a,
            PerkEffect::Damage => self.damage_multiplier += a,
            PerkEffect::FireRate => self.cooldown_multiplier *= a,
            PerkEffect::PickupRange => self.pickup_range += a,
            PerkEffect::Mining => self.mining_multiplier += a,
            PerkEffect::Regeneration => self.regeneration += a,
            PerkEffect::OreBonus => self.ore_bonus += a,
            PerkEffect::ExtractionSpeed => self.extraction_speed += a,
            PerkEffect::DashCooldown => self.dash_cooldown_multiplier *= a,
            PerkEffect::HeavyDamage => self.heavy_damage += a,
            PerkEffect::AbilityCooldown => self.ability_cooldown_multiplier *= a,
            PerkEffect::Luck => self.luck += a,
            PerkEffect::Armor => self.armor += a,
            PerkEffect::XpGain => self.xp_gain += a,
        }
    }

    /// The current value of an effect (for perk caps).
    pub fn effect_total(&self, effect: PerkEffect) -> f32 {
        match effect {
            PerkEffect::MaxHealth => self.max_health,
            PerkEffect::MoveSpeed => self.move_speed,
            PerkEffect::Damage => self.damage_multiplier,
            PerkEffect::FireRate => self.cooldown_multiplier,
            PerkEffect::PickupRange => self.pickup_range,
            PerkEffect::Mining => self.mining_multiplier,
            PerkEffect::Regeneration => self.regeneration,
            PerkEffect::OreBonus => self.ore_bonus,
            PerkEffect::ExtractionSpeed => self.extraction_speed,
            PerkEffect::DashCooldown => self.dash_cooldown_multiplier,
            PerkEffect::HeavyDamage => self.heavy_damage,
            PerkEffect::AbilityCooldown => self.ability_cooldown_multiplier,
            PerkEffect::Luck => self.luck,
            PerkEffect::Armor => self.armor,
            PerkEffect::XpGain => self.xp_gain,
        }
    }

    pub fn carries(&self, kind: WeaponId) -> bool {
        self.weapons.iter().any(|w| w.kind == kind)
    }

    /// Re-derive ability charges after an ability upgrade.
    fn refresh_ability(&mut self) {
        let max = if self.ability.has(Special::Echo) {
            2
        } else {
            1
        };
        if max > self.ability.max_charges {
            self.ability.charges += max - self.ability.max_charges;
            self.ability.max_charges = max;
        }
    }

    /// Ability cooldown multiplier including Quick Charge upgrades.
    pub fn ability_cooldown(&self) -> f32 {
        let faster: f32 = self
            .ability
            .mods
            .iter()
            .filter(|(m, _)| m.info().stat == ModStat::Cooldown)
            .map(|(m, r)| m.amount(*r) / 100.0)
            .sum();
        self.ability_cooldown_multiplier / (1.0 + faster)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WeaponSlot {
    pub kind: WeaponId,
    /// Upgrades taken this drop, with their rarity.
    pub mods: Vec<(ModId, Rarity)>,
    cooldown: f32,
    /// Ticks since this weapon last fired (for muzzle flashes).
    pub since_fired: u32,
    /// Direction of the last shot.
    pub aim: f32,
    /// Flamer heat (0..1) and whether it is locked cooling down.
    #[serde(default)]
    pub heat: f32,
    #[serde(default)]
    pub overheated: bool,
}

impl WeaponSlot {
    fn new(kind: WeaponId, cooldown: f32) -> Self {
        Self {
            kind,
            mods: Vec::new(),
            cooldown,
            since_fired: 999,
            aim: 0.0,
            heat: 0.0,
            overheated: false,
        }
    }

    /// 1 plus the upgrades taken (for the HUD).
    pub fn level(&self) -> usize {
        1 + self.mods.len()
    }

    fn sum(&self, stat: ModStat) -> f32 {
        self.mods
            .iter()
            .filter(|(m, _)| m.info().stat == stat)
            .map(|(m, r)| m.amount(*r))
            .sum()
    }

    /// Base stats with every upgrade applied.
    pub fn stats(&self) -> WeaponStats {
        let mut s = self.kind.base();
        let growth = content().tuning.weapons.growth * self.mods.len() as f32;
        s.damage *= 1.0 + growth + self.sum(ModStat::Damage) / 100.0;
        s.cooldown /= 1.0 + self.sum(ModStat::FireRate) / 100.0;
        s.count += self.sum(ModStat::Count).round() as u32;
        s.range *= 1.0 + self.sum(ModStat::Range) / 100.0;
        s.radius *= 1.0 + self.sum(ModStat::Area) / 100.0;
        s.pierce += self.sum(ModStat::Pierce).round() as u32;
        s
    }

    /// The weapon's base element plus element upgrades.
    pub fn elements(&self) -> Elements {
        let mut e = Elements::default();
        if let Some(base) = self.kind.info().element {
            e.add(base.kind, base.potency);
        }
        for (m, r) in &self.mods {
            if let Some(element) = m.info().element {
                e.add(element, m.amount(*r));
            }
        }
        e
    }

    pub fn has(&self, special: Special) -> bool {
        self.mods
            .iter()
            .any(|(m, _)| m.info().special == Some(special))
    }

    pub fn stacks(&self, id: ModId) -> u32 {
        self.mods.iter().filter(|(m, _)| *m == id).count() as u32
    }
}

/// Something a class ability left in the world.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum DeployableKind {
    /// A shock post the horde cannot pass.
    Barrier,
    /// Fires `weapon` at the nearest enemy in range; a mobile one follows
    /// its owner.
    Turret { weapon: WeaponId, mobile: bool },
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Deployable {
    pub kind: DeployableKind,
    pub owner: usize,
    pub pos: V2,
    pub yaw: f32,
    /// Ticks left, and the lifetime it started with.
    pub ttl: u32,
    pub total: u32,
    damage: f32,
    /// Turret reach in metres (barriers: zap reach).
    pub range: f32,
    heavy: f32,
    elements: Elements,
    cooldown: f32,
    /// Ticks since it last fired or zapped something.
    pub since_fired: u32,
}

/// Radius of a barrier post (enemies are pushed out of it).
pub const BARRIER_RADIUS: f32 = 0.55;

/// What makes an elite enemy dangerous.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Affix {
    /// Much more health.
    Armored,
    /// Faster.
    Swift,
    /// Hits much harder.
    Frenzied,
    /// Breaks into smaller copies when killed.
    Splitting,
    /// Heals over time.
    Regenerating,
}

impl Affix {
    pub const ALL: [Self; 5] = [
        Self::Armored,
        Self::Swift,
        Self::Frenzied,
        Self::Splitting,
        Self::Regenerating,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Armored => "Armored",
            Self::Swift => "Swift",
            Self::Frenzied => "Frenzied",
            Self::Splitting => "Splitting",
            Self::Regenerating => "Regenerating",
        }
    }
}

/// Surge waves: warned, then a burst and a spell of heavy pressure.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Surge {
    /// The threat level that triggers the next surge.
    pub next: u32,
    /// Ticks until a warned surge arrives (0: none pending).
    pub warning: u32,
    /// Ticks of surge pressure left.
    pub active: u32,
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
    /// Element statuses: ticks left (and damage per second).
    pub burning: u32,
    burn_dps: f32,
    pub corroded: u32,
    acid_dps: f32,
    /// Builds up from cold hits (0..1); at 1 the enemy freezes.
    pub chill: f32,
    pub frozen: u32,
    /// Elites carry an affix, glow and are worth more.
    pub elite: Option<Affix>,
    /// Burrowers travelling underground (untargetable).
    #[serde(default)]
    pub burrowed: bool,
    /// Behaviour timer (surfaced time, ranged and pulse cooldowns).
    #[serde(default)]
    pub state_ticks: u32,
    /// Boss ability timer.
    #[serde(default)]
    pub special_ticks: u32,
    /// Soaked by water (shock arcs through them).
    #[serde(default)]
    pub wet: u32,
    /// Split copies do not split again.
    #[serde(default)]
    pub split: bool,
}

impl Enemy {
    /// A new enemy at full health with no statuses.
    pub fn fresh(id: u32, kind: EnemyId, pos: V2, health: f32) -> Self {
        Self {
            id,
            kind,
            pos,
            yaw: 0.0,
            health,
            max_health: health,
            hit_ticks: 0,
            attack_ticks: 0,
            walk: 0.0,
            spawn_ticks: 24,
            attack_cooldown: 30,
            drone_cooldown: 0,
            speed_scale: 1.0,
            alerted: false,
            burning: 0,
            burn_dps: 0.0,
            corroded: 0,
            acid_dps: 0.0,
            chill: 0.0,
            frozen: 0,
            elite: None,
            burrowed: kind.info().behavior == crate::content::EnemyBehavior::Burrower,
            state_ticks: 0,
            special_ticks: 0,
            wet: 0,
            split: false,
        }
    }

    /// Flyers ride over rock; burrowers underground cannot be hit.
    pub fn flying(&self) -> bool {
        self.kind.info().behavior == crate::content::EnemyBehavior::Flyer
    }

    /// Heavy enemies take the Striker's bonus: brutes and every elite.
    pub fn heavy(&self) -> bool {
        self.elite.is_some() || self.kind.info().health >= weapons::HEAVY_HEALTH
    }
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
    /// Bounces between enemies and off walls.
    Ricochet {
        bounces: u32,
    },
    /// A proximity mine (still).
    Mine,
    /// Steers toward an enemy id.
    Homing {
        target: u32,
    },
    /// Flies to `to`, then pulls and grinds for `active` ticks.
    Well {
        to: V2,
        active: u32,
    },
    Mortar {
        from: V2,
        to: V2,
        total: u32,
    },
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
    /// Bonus against heavy enemies (from the firing player).
    heavy: f32,
    elements: Elements,
    /// Mortar shells that split on impact.
    cluster: bool,
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

    /// Blast or vortex radius.
    pub fn radius_world(&self) -> f32 {
        self.radius
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Beam {
    /// Linear colour (the firing weapon's, or the shock colour).
    pub color: [f32; 3],
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

/// Why an upgrade choice is on screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum OfferSource {
    LevelUp,
    /// A dive level that grants a new weapon.
    WeaponPick,
    /// A supply cache (rare or better).
    Cache,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum RunState {
    Active,
    /// A player chooses one of `choices`; the drop pauses meanwhile.
    LevelUp {
        player: usize,
        choices: Vec<Offer>,
        source: OfferSource,
    },
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
    /// Everyone on the drop; index 0 is the local player.
    pub players: Vec<Player>,
    pub deployables: Vec<Deployable>,
    pub enemies: Vec<Enemy>,
    pub corpses: Vec<Corpse>,
    pub projectiles: Vec<Projectile>,
    pub beams: Vec<Beam>,
    pub blasts: Vec<Blast>,
    pub pickups: Vec<Pickup>,
    pub caches: Vec<Cache>,
    pub carried: Materials,
    /// The contract and its objectives.
    pub mission: MissionState,
    /// Far chambers from the layout (mission set pieces).
    pub sites: Vec<V2>,
    pub extraction: Extraction,
    pub ticks: u32,
    pub kills: u32,
    pub state: RunState,
    /// The tutorial holds the horde back until the player has their bearings.
    pub spawning_enabled: bool,
    /// Test/autoplay switch: ignore incoming damage.
    pub invulnerable: bool,
    pub caches_opened: u32,
    /// Equipment and weapon blueprints found this drop (kept even on death).
    #[serde(default)]
    pub finds: Vec<Find>,
    /// Ticks left showing the latest find on the HUD.
    #[serde(default)]
    pub find_banner: u32,
    pub blocks_mined: u32,
    hazard: f32,
    /// Rises all drop long; see [`Run::threat_level`].
    pub threat: f32,
    pub surge: Surge,
    spawn_timer: f32,
    /// Fractional extraction progress from extraction-speed bonuses.
    hold_carry: f32,
    next_id: u32,
    rng: Rng,
    #[serde(skip, default = "spatial_index")]
    index: SpatialGrid<2, u32>,
    #[serde(skip)]
    scratch: Vec<u32>,
    /// Spore clouds, vents and oxygen vents.
    #[serde(default)]
    pub hazards: Vec<Hazard>,
    #[serde(default)]
    pub quake: hazards::Quake,
    /// Spit in flight from ranged enemies.
    #[serde(default)]
    pub enemy_shots: Vec<EnemyShot>,
}

/// A ranged enemy's spit.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct EnemyShot {
    pub pos: V2,
    pub vel: V2,
    pub damage: f32,
    pub ttl: u32,
    /// The spitter's kind (for its colour).
    pub kind: EnemyId,
}

impl Run {
    pub fn new(config: RunConfig) -> Self {
        assert!(!config.players.is_empty(), "a drop needs a player");
        let (terrain, layout) = Terrain::generate(config.planet, config.seed, config.tutorial);
        let flow = FlowField::new(&terrain, layout.landing);
        let info = config.planet.info();
        let mut rng = Rng::seed_from_u64(config.seed ^ 0xA5A5_5A5A);
        let mut mission =
            MissionState::new(config.contract, config.hazard, &layout, &terrain, &mut rng);
        if config.tutorial {
            mission.primary.need = content().tuning.tutorial.objective;
        }
        let mut run = Self {
            hazards: Vec::new(),
            quake: hazards::Quake::default(),
            enemy_shots: Vec::new(),
            planet: config.planet,
            tutorial: config.tutorial,
            flow: Some(flow),
            landing: layout.landing,
            players: config
                .players
                .iter()
                .enumerate()
                .map(|(n, loadout)| {
                    let offset = V2::new(
                        n as f32 * 1.2 - 0.6 * (config.players.len() - 1) as f32,
                        2.2,
                    );
                    Player::new(loadout.clone(), &config, layout.landing + offset)
                })
                .collect(),
            deployables: Vec::new(),
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
            carried: Materials::default(),
            mission,
            sites: layout.sites.clone(),
            extraction: Extraction::Idle,
            ticks: 0,
            kills: 0,
            state: RunState::Active,
            spawning_enabled: !config.tutorial,
            invulnerable: false,
            caches_opened: 0,
            finds: Vec::new(),
            find_banner: 0,
            blocks_mined: 0,
            hazard: info.hazard,
            threat: 0.0,
            surge: Surge {
                next: content().tuning.threat.surge_every.max(1),
                ..Surge::default()
            },
            spawn_timer: 0.0,
            hold_carry: 0.0,
            next_id: 1,
            rng,
            index: spatial_index(),
            scratch: Vec::new(),
            terrain,
        };
        let (placed, quake) = hazards::place(&run, &layout.spots);
        run.hazards = placed;
        run.quake = quake;
        run
    }

    pub fn minutes(&self) -> f32 {
        self.ticks as f32 / 3600.0
    }

    /// The whole threat level (0, 1, 2, ...).
    pub fn threat_level(&self) -> u32 {
        self.threat.max(0.0) as u32
    }

    /// Threat shaped by `tuning.threat.curve` (for enemy health and damage).
    pub(super) fn threat_curve(&self) -> f32 {
        self.threat.max(0.0).powf(content().tuning.threat.curve)
    }

    /// The local player (index 0).
    pub fn player(&self) -> &Player {
        &self.players[0]
    }

    pub fn player_mut(&mut self) -> &mut Player {
        &mut self.players[0]
    }

    pub fn level_up_choices(&self) -> Option<&[Offer]> {
        match &self.state {
            RunState::LevelUp { choices, .. } => Some(choices),
            _ => None,
        }
    }

    /// Why the current choice is offered.
    pub fn offer_source(&self) -> Option<OfferSource> {
        match &self.state {
            RunState::LevelUp { source, .. } => Some(*source),
            _ => None,
        }
    }

    /// The living player nearest `at`.
    pub fn nearest_player(&self, at: V2) -> Option<usize> {
        self.players
            .iter()
            .enumerate()
            .filter(|(_, p)| p.alive())
            .min_by(|(_, a), (_, b)| {
                a.pos
                    .distance_squared(at)
                    .total_cmp(&b.pos.distance_squared(at))
            })
            .map(|(i, _)| i)
    }

    pub fn outcome(&self) -> Option<RunOutcome> {
        match self.state {
            RunState::Ended(outcome) => Some(outcome),
            _ => None,
        }
    }

    /// Advance one fixed tick with the local player's input.
    pub fn update(&mut self, input: &GameInput, events: &mut Vec<GameEvent>) {
        self.update_players(std::slice::from_ref(input), events);
    }

    /// Advance one fixed tick; `inputs[i]` drives player `i` (missing inputs
    /// stand still).
    pub fn update_players(&mut self, inputs: &[GameInput], events: &mut Vec<GameEvent>) {
        let idle = GameInput::default();
        let input = |i: usize| inputs.get(i).unwrap_or(&idle);
        if inputs.iter().any(|i| i.command == Some(Command::Abandon))
            && !matches!(self.state, RunState::Ended(_))
        {
            self.state = RunState::Ended(RunOutcome::Abandoned);
            events.push(GameEvent::RunEnded { extracted: false });
            return;
        }
        match &self.state {
            RunState::Ended(_) => return,
            RunState::LevelUp {
                player, choices, ..
            } => {
                let player = *player;
                if let Some(Command::ChoosePerk(i)) = input(player).command
                    && let Some(&offer) = choices.get(i)
                {
                    self.apply_offer(player, offer, events);
                    self.state = RunState::Active;
                    self.offer_level_up(events);
                }
                return;
            }
            RunState::Active => {}
        }
        self.ticks += 1;
        self.find_banner = self.find_banner.saturating_sub(1);
        for i in 0..self.players.len() {
            self.update_player(i, input(i), events);
        }
        let focus = self
            .nearest_player(self.landing)
            .map_or(self.landing, |i| self.players[i].pos);
        let flow = self
            .flow
            .get_or_insert_with(|| FlowField::new(&self.terrain, focus));
        flow.update(&mut self.terrain, focus);
        self.rebuild_index();
        for i in 0..self.players.len() {
            if input(i).ability {
                self.use_ability(i, events);
            }
        }
        self.update_deployables(events);
        self.fire_weapons(events);
        self.update_projectiles(events);
        self.update_drones(events);
        self.update_statuses(events);
        self.update_hazards(events);
        self.update_enemies(events);
        self.update_enemy_shots(events);
        self.update_blasts();
        self.remove_dead(events);
        self.update_pickups(events);
        self.update_caches(events);
        self.update_threat(events);
        self.update_spawner(events);
        self.update_mission(events);
        let call = inputs.iter().any(|i| i.call_extraction);
        self.update_extraction(call, events);
        self.update_effects();
        for (i, p) in self.players.iter_mut().enumerate() {
            if !p.down && p.health <= 0.0 {
                p.health = 0.0;
                p.down = true;
                p.mining = None;
                events.push(GameEvent::PlayerDown { player: i });
            }
        }
        if self.players.iter().all(|p| p.down) {
            self.state = RunState::Ended(RunOutcome::Died);
            events.push(GameEvent::PlayerDied);
            events.push(GameEvent::RunEnded { extracted: false });
        } else if matches!(self.state, RunState::Active) {
            self.offer_level_up(events);
        }
    }

    /// Feed everything that steers the run into `h` (positions as raw bits).
    pub fn hash_into(&self, h: &mut StateHasher) {
        for p in &self.players {
            for v in [p.pos.x, p.pos.y, p.health] {
                h.write_f32_bits(v);
            }
            h.write_u32(p.ability.cooldown);
        }
        h.write_u64(self.deployables.len() as u64);
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

#[cfg(test)]
mod tests;
