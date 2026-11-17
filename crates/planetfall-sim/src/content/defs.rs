//! Typed content definitions, deserialized from `assets/data/*.toml`.
//!
//! Code owns behaviour *archetypes* (how a bolt or an exploder works); every
//! number, name, drop table, spawn rule, palette, model and sound name is
//! data. See the comments at the top of each data file.

use super::{Material, Materials};
use serde::{Deserialize, Serialize};
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
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

/// Upgrade rarity. Higher rarities scale amounts and unlock elements
/// (rare) and behaviour changes (legendary).
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum Rarity {
    #[default]
    Common,
    Uncommon,
    Rare,
    Legendary,
}

impl Rarity {
    pub const ALL: [Self; 4] = [Self::Common, Self::Uncommon, Self::Rare, Self::Legendary];

    pub fn index(self) -> usize {
        self as usize
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Common => "Common",
            Self::Uncommon => "Uncommon",
            Self::Rare => "Rare",
            Self::Legendary => "Legendary",
        }
    }
}

/// On-hit elements (pierce is a weapon stat, not a status).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Element {
    /// Burn: damage per second for a few seconds.
    Fire,
    /// Corrode: damage over time and more damage taken.
    Acid,
    /// Chill: slows, and freezes once it builds up.
    Cold,
    /// Chance to arc damage to a nearby enemy.
    Shock,
}

impl Element {
    pub const ALL: [Self; 4] = [Self::Fire, Self::Acid, Self::Cold, Self::Shock];

    pub fn index(self) -> usize {
        self as usize
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Fire => "Fire",
            Self::Acid => "Acid",
            Self::Cold => "Cold",
            Self::Shock => "Shock",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ElementDef {
    pub kind: Element,
    pub potency: f32,
}

/// Base weapon numbers; upgrades stack on top during a drop.
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
    pub stats: WeaponStats,
    #[serde(default)]
    pub element: Option<ElementDef>,
}

/// What an upgrade card applies to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModTarget {
    Weapon,
    Ability,
}

/// The number an upgrade changes (see `mods.toml`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModStat {
    Damage,
    FireRate,
    Count,
    Range,
    Area,
    Pierce,
    Element,
    Cooldown,
    Power,
    Reach,
    Special,
}

/// Legendary behaviour changes (code archetypes).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Special {
    TwinBarrel,
    DoubleBlast,
    Overload,
    Cluster,
    SecondRing,
    MobileSentry,
    Echo,
    MoltenBore,
    ArcFence,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModDef {
    pub id: String,
    pub title: String,
    /// `{amount}` is replaced with the value at the rolled rarity.
    pub description: String,
    pub target: ModTarget,
    pub stat: ModStat,
    /// Value at each rarity (common, uncommon, rare, legendary).
    pub amounts: [f32; 4],
    #[serde(default)]
    pub element: Option<Element>,
    #[serde(default)]
    pub special: Option<Special>,
    /// Weapon behaviours it applies to (empty: all).
    #[serde(default)]
    pub behaviors: Vec<WeaponBehavior>,
    /// Ability kinds it applies to (empty: all).
    #[serde(default)]
    pub abilities: Vec<AbilityKind>,
    #[serde(default)]
    pub min_rarity: Rarity,
    pub weight: u32,
    #[serde(default)]
    pub max_stacks: Option<u32>,
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

/// A stat a perk or class passive changes. Multiplicative effects
/// (`move_speed`, `fire_rate`, `dash_cooldown`, `ability_cooldown`) scale by
/// `amount`; the rest add it.
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
    /// Chance of an extra ore drop per broken vein.
    OreBonus,
    /// Fraction faster the extraction zone fills.
    ExtractionSpeed,
    DashCooldown,
    /// Bonus damage against heavy enemies (100+ base health).
    HeavyDamage,
    AbilityCooldown,
    /// Better upgrade rarities.
    Luck,
}

impl PerkEffect {
    /// Effects that scale by `amount` instead of adding it.
    pub fn multiplicative(self) -> bool {
        matches!(
            self,
            Self::MoveSpeed | Self::FireRate | Self::DashCooldown | Self::AbilityCooldown
        )
    }
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
    /// Only offered to this class (by id).
    #[serde(default)]
    pub class: Option<String>,
    /// Class level needed before a class perk is offered.
    #[serde(default)]
    pub class_level: u32,
}

/// The behaviour archetype of a class's active ability.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AbilityKind {
    /// Tunnel through rock ahead, damaging enemies in the bore.
    Bore,
    /// A wall of shock posts ahead that blocks the horde.
    Barrier,
    /// Teleport ahead, damaging enemies along the path.
    Blink,
    /// A turret that fires a weapon at the nearest enemy.
    Turret,
}

/// Ability numbers at one tier. Which fields matter depends on the kind.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AbilityTier {
    pub cooldown: f32,
    #[serde(default)]
    pub damage: f32,
    #[serde(default)]
    pub range: f32,
    #[serde(default)]
    pub width: f32,
    #[serde(default)]
    pub duration: f32,
    #[serde(default)]
    pub count: u32,
    /// Turrets fire this weapon (id).
    #[serde(default)]
    pub weapon: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AbilityDef {
    pub kind: AbilityKind,
    pub name: String,
    pub description: String,
    pub tiers: Vec<AbilityTier>,
}

#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PassiveEffect {
    pub effect: PerkEffect,
    pub amount: f32,
    /// Added per tier above the first.
    #[serde(default)]
    pub per_tier: f32,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PassiveDef {
    pub name: String,
    pub description: String,
    pub effects: Vec<PassiveEffect>,
}

/// A weapon the class can start with from a class level.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClassUnlock {
    pub level: u32,
    pub weapon: String,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClassDef {
    pub id: String,
    pub name: String,
    pub role: String,
    pub description: String,
    /// New commanders start as this class.
    #[serde(default)]
    pub starter: bool,
    /// Default starting weapon (id).
    pub signature: String,
    /// Weapons offered more often (ids).
    #[serde(default)]
    pub familiar: Vec<String>,
    pub color: [f32; 3],
    pub ability: AbilityDef,
    /// Class level at which each ability tier applies.
    pub ability_levels: Vec<u32>,
    pub passive: PassiveDef,
    /// Class level at which each passive tier applies.
    pub passive_levels: Vec<u32>,
    #[serde(default)]
    pub unlocks: Vec<ClassUnlock>,
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
    pub rarity: RarityTuning,
    pub xp: XpTuning,
    pub horde: HordeTuning,
    pub class: ClassTuning,
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
    /// Dive levels that offer a new weapon while a slot is free.
    pub pick_levels: Vec<u32>,
    pub familiar_weight: f32,
    /// Damage added per upgrade taken on a weapon.
    pub growth: f32,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RarityTuning {
    /// Base odds of common, uncommon, rare and legendary offers.
    pub weights: [f32; 4],
    /// Perk amounts at each rarity (multiplicative effects scale their
    /// change from 1).
    pub perk_scale: [f32; 4],
    /// Each point of planet hazard above 1 multiplies tier n's weight by
    /// (1 + hazard_bonus)^n, as luck does with (1 + luck)^n.
    pub hazard_bonus: f32,
    /// Caches offer this rarity or better.
    pub cache_min: Rarity,
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
pub struct ClassTuning {
    pub failed_share: f32,
    pub extraction_bonus: u32,
    pub xp_base: f32,
    pub xp_growth: f32,
    pub max_level: u32,
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

fn one() -> f32 {
    1.0
}

fn half() -> f32 {
    0.5
}

fn yes() -> bool {
    true
}
