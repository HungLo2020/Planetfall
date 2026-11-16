//! Typed content definitions, deserialized from `assets/data/*.toml`.
//!
//! Code owns behaviour *archetypes* (how a bolt or an exploder works); every
//! number, name, drop table, spawn rule, palette, model and sound name is
//! data. See the comments at the top of each data file.

use super::{Material, Materials};
use serde::Deserialize;
use std::collections::BTreeMap;

/// Visual palette for a planet surface (linear RGB).
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
pub struct Palette {
    pub floor: [f32; 3],
    pub floor_alt: [f32; 3],
    pub rock: [f32; 3],
    pub rock_top: [f32; 3],
    pub bedrock: [f32; 3],
    pub ambient: [f32; 3],
    pub sun: [f32; 3],
    pub sky: [f32; 3],
    pub planet: [f32; 3],
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct OreChances {
    pub ferrite: f32,
    pub lumite: f32,
    pub voidstone: f32,
}

impl OreChances {
    pub fn get(&self, material: Material) -> f32 {
        match material {
            Material::Ferrite => self.ferrite,
            Material::Lumite => self.lumite,
            Material::Voidstone => self.voidstone,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanetDef {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub description: String,
    /// Reactor & Nav Core level needed to plot a course here.
    pub nav_level: u8,
    /// Multiplier on enemy health, damage and spawn pressure.
    pub hazard: f32,
    /// Lumite needed to complete the drop objective.
    pub objective: u32,
    pub ore: OreChances,
    /// Enemy spawn weight multipliers by enemy id.
    #[serde(default)]
    pub spawn_weights: BTreeMap<String, f32>,
    pub palette: Palette,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WeaponBehavior {
    /// Fast bolts at the nearest target, fanned when `count` > 1.
    Bolt,
    /// A spread of short-lived pellets.
    Scatter,
    /// Chain lightning jumping `count` times within `radius`.
    Arc,
    /// Lobbed shells that burst in `radius`; `speed` is flight seconds.
    Mortar,
    /// `count` blades circling at `range`, `speed` radians per second.
    Orbit,
}

/// Per-level weapon numbers.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WeaponStats {
    pub cooldown: f32,
    pub damage: f32,
    pub range: f32,
    /// Bolts, pellets, chain jumps, shells or drones depending on behaviour.
    pub count: u32,
    pub speed: f32,
    pub radius: f32,
    pub pierce: u32,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WeaponDef {
    pub id: String,
    pub name: String,
    pub description: String,
    pub behavior: WeaponBehavior,
    /// Every drop starts with the starter weapon.
    #[serde(default)]
    pub starter: bool,
    /// Fires without line of sight (lobbed).
    #[serde(default)]
    pub ignores_walls: bool,
    pub model: String,
    #[serde(default = "one")]
    pub model_scale: f32,
    /// Extra turn of the held model, in degrees.
    #[serde(default)]
    pub model_yaw: f32,
    pub sound: String,
    #[serde(default = "half")]
    pub volume: f32,
    pub color: [f32; 3],
    pub levels: Vec<WeaponStats>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EnemyBehavior {
    /// Walks the flow field and strikes in melee.
    Walker,
    /// Like a walker, but speeds up inside `lunge.range`.
    Lunger,
    /// Detonates on contact and on death (`explosion`).
    Exploder,
}

#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Lunge {
    pub range: f32,
    pub speed: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Explosion {
    pub radius: f32,
    /// Damage dealt to other enemies caught in the blast.
    pub enemy_damage: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spawn {
    pub weight: f32,
    pub count: [u32; 2],
    #[serde(default)]
    pub after_minutes: f32,
    /// Waits for the late game (time or extraction).
    #[serde(default)]
    pub late: bool,
    /// Whether it appears in the tutorial drop.
    #[serde(default = "yes")]
    pub tutorial: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Drop {
    pub material: Material,
    pub amount: u32,
    pub chance: f32,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnemyDef {
    pub id: String,
    pub name: String,
    pub behavior: EnemyBehavior,
    pub health: f32,
    pub speed: f32,
    pub damage: f32,
    pub radius: f32,
    pub xp: u32,
    pub model: String,
    pub blood: [f32; 3],
    pub death_sound: String,
    #[serde(default)]
    pub alert_sound: Option<String>,
    #[serde(default)]
    pub alert_chance: f32,
    #[serde(default)]
    pub lunge: Option<Lunge>,
    #[serde(default)]
    pub explosion: Option<Explosion>,
    pub spawn: Spawn,
    #[serde(default)]
    pub drops: Vec<Drop>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PerkEffect {
    MaxHealth,
    MoveSpeed,
    Damage,
    FireRate,
    PickupRange,
    Mining,
    Regeneration,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PerkDef {
    pub id: String,
    pub title: String,
    pub description: String,
    pub effect: PerkEffect,
    pub amount: f32,
    pub weight: u32,
    /// Cap on the stacked effect (only meaningful for additive effects).
    #[serde(default)]
    pub max: Option<f32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UpgradeEffect {
    MaxHealth,
    Damage,
    Mining,
    PickupRange,
    /// Unlocks planets with a higher `nav_level`.
    NavLevel,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spin {
    pub node: String,
    pub axis: Axis,
    pub speed: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Axis {
    X,
    Y,
    Z,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpgradeDef {
    pub id: String,
    pub name: String,
    pub description: String,
    pub effect: UpgradeEffect,
    pub per_level: f32,
    /// `costs[n]` buys level `n + 1`; its length is the max level.
    pub costs: Vec<Materials>,
    /// Inclusive floor rect `(x0, y0, x1, y1)` built with the first level.
    pub room: [i32; 4],
    /// Model shown in the room.
    pub station: String,
    #[serde(default = "one")]
    pub station_scale: f32,
    #[serde(default)]
    pub station_offset: [f32; 3],
    #[serde(default)]
    pub spin: Option<Spin>,
    pub light: [f32; 3],
    pub light_intensity: f32,
}

impl UpgradeDef {
    pub fn max_level(&self) -> u8 {
        self.costs.len().min(u8::MAX as usize) as u8
    }
}

/// `assets/data/tuning.toml`.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Tuning {
    pub player: PlayerTuning,
    pub weapons: WeaponTuning,
    pub xp: XpTuning,
    pub horde: HordeTuning,
    pub extraction: ExtractionTuning,
    pub tutorial: TutorialTuning,
    pub blocks: BlockCosts,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlayerTuning {
    pub speed: f32,
    pub radius: f32,
    pub health: f32,
    pub pickup_range: f32,
    pub mining_rate: f32,
    pub rock_ferrite_chance: f32,
    pub dash_speed: f32,
    pub dash_ticks: u32,
    pub dash_cooldown_ticks: u32,
    pub spawn_invulnerability: f32,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WeaponTuning {
    pub max_carried: usize,
    pub max_level: u8,
    pub cache_overflow_damage: f32,
    pub new_weapon_weight: u32,
    pub level_up_weight: u32,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct XpTuning {
    pub base: u32,
    pub per_level: u32,
    pub cache_xp: u32,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HordeTuning {
    pub spawn_period: f32,
    pub cap: usize,
    pub tutorial_cap: usize,
    pub tutorial_intensity: f32,
    pub extraction_intensity: f32,
    pub minutes_ramp: f32,
    pub late_minutes: f32,
    pub health_ramp_per_minute: f32,
    pub damage_ramp_per_minute: f32,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExtractionTuning {
    pub inbound: f32,
    pub hold: f32,
    pub radius: f32,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TutorialTuning {
    pub objective: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlockCosts {
    pub deck: Materials,
    pub hull: Materials,
    pub window: Materials,
    pub lamp: Materials,
}

/// Top-level tables of the list files.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PlanetFile {
    pub planet: Vec<PlanetDef>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct WeaponFile {
    pub weapon: Vec<WeaponDef>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct EnemyFile {
    pub enemy: Vec<EnemyDef>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PerkFile {
    pub perk: Vec<PerkDef>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct UpgradeFile {
    pub upgrade: Vec<UpgradeDef>,
}

fn one() -> f32 {
    1.0
}

fn half() -> f32 {
    0.5
}

fn yes() -> bool {
    true
}
