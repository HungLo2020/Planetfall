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
    /// Floor patches (moss, ash, frost).
    pub patch: [f32; 3],
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
    pub ore: OreChances,
    /// Enemy spawn weight multipliers by enemy id.
    #[serde(default)]
    pub spawn_weights: BTreeMap<String, f32>,
    pub palette: Palette,
    #[serde(default)]
    pub terrain: TerrainStyle,
    #[serde(default)]
    pub hazards: HazardStyle,
    /// Bosses a boss contract here can name (enemy ids).
    #[serde(default)]
    pub bosses: Vec<String>,
}

/// Cave generation knobs per planet (see `cavegen`).
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(default)]
pub struct TerrainStyle {
    /// Extra chambers besides the landing and mission sites (min, max).
    pub chambers: [u32; 2],
    /// Chamber radius range in blocks.
    pub chamber_radius: [f32; 2],
    /// Tunnel half-width range in blocks.
    pub tunnel_width: [f32; 2],
    /// How far chamber walls wobble from round (0 to 1).
    pub roughness: f32,
    /// Chance each chamber gets an extra tunnel that closes a loop.
    pub loops: f32,
    /// Noise alcoves opened along walls (0 to 1).
    pub alcoves: f32,
    /// Floor patch coverage (0 to 1).
    pub patches: f32,
    /// Ore density multiplier on top of the planet's `ore` chances.
    pub seams: f32,
    /// Decoration props growing on the floor near walls.
    pub decor: Option<DecorKind>,
    /// Share of wall-side floor blocks that get a prop.
    pub decor_density: f32,
}

impl Default for TerrainStyle {
    fn default() -> Self {
        Self {
            chambers: [6, 9],
            chamber_radius: [2.5, 5.0],
            tunnel_width: [1.0, 1.5],
            roughness: 0.3,
            loops: 0.35,
            alcoves: 0.5,
            patches: 0.5,
            seams: 1.0,
            decor: None,
            decor_density: 0.15,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecorKind {
    /// Fungal stalks and shelf caps.
    Fungus,
    /// Ember-lit obsidian crystal growths.
    Crystal,
    /// Ice spires.
    Ice,
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
    /// A short cone of fire every `cooldown`, building heat; overheating
    /// locks it until it cools (`[arsenal]`).
    Flamer,
    /// A disc that bounces between enemies and off walls `count` times.
    Ricochet,
    /// Proximity mines dropped at your feet (`count` at most), bursting
    /// in `radius`.
    Mines,
    /// A heavy sweep in front of you within `range`, knocking enemies back.
    Melee,
    /// `count` missiles that steer toward their targets, bursting in
    /// `radius`.
    Homing,
    /// A slow orb that becomes a vortex at the target, pulling enemies in
    /// and grinding them within `radius`.
    Gravity,
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
    /// Keeps its distance and spits (`ranged`).
    Spitter,
    /// Walks in and emits jamming pulses (`pulse`).
    Disruptor,
    /// Moves through rock unseen and surfaces near players (`burrow`).
    Burrower,
    /// Flies straight over rock; fragile.
    Flyer,
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
    /// Threat level a drop must reach before this enemy spawns.
    #[serde(default)]
    pub threat: f32,
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
    /// Placed by objectives only, with a big health bar.
    #[serde(default)]
    pub boss: bool,
    /// Model size.
    #[serde(default = "one")]
    pub scale: f32,
    /// Spitters: ranged attack.
    #[serde(default)]
    pub ranged: Option<Ranged>,
    /// Disruptors: jamming pulse.
    #[serde(default)]
    pub pulse: Option<Pulse>,
    /// Burrowers: when to surface.
    #[serde(default)]
    pub burrow: Option<Burrow>,
    /// Boss ability.
    #[serde(default)]
    pub special: Option<BossSpecial>,
    /// Linear colour multiplied into the model (variants share rigs).
    #[serde(default)]
    pub tint: Option<[f32; 3]>,
}

/// A ranged attacker's spit.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ranged {
    /// Fires within this distance (with line of sight).
    pub range: f32,
    /// Stops closing in at this distance.
    pub keep: f32,
    pub cooldown: f32,
    /// Projectile speed, m/s.
    pub speed: f32,
    pub damage: f32,
    /// Shots per volley (fanned).
    #[serde(default = "one_u32")]
    pub count: u32,
}

fn one_u32() -> u32 {
    1
}

/// A disruptor's jamming pulse: slows weapons and blocks abilities.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pulse {
    pub radius: f32,
    pub cooldown: f32,
    /// Seconds a player stays jammed.
    pub jam: f32,
}

/// A burrower travels through rock unseen and surfaces near its prey.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Burrow {
    /// Surfaces within this distance of a player.
    pub surface_range: f32,
    /// Seconds above ground before it can dive again.
    pub surfaced: f32,
}

/// What a boss does besides hitting hard.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum BossSpecial {
    /// Calls `count` of `enemy` every `every` seconds.
    Brood {
        enemy: String,
        count: u32,
        every: f32,
    },
    /// Leaves burning lava where it walks, every `every` seconds.
    LavaTrail { every: f32 },
    /// A freezing nova within `radius` every `every` seconds.
    FrostNova { every: f32, radius: f32 },
    /// A fan of `count` spit shots every `every` seconds.
    Volley { every: f32, count: u32 },
}

/// Planet hazards (`[planet.hazards]`; sites can add oxygen and quakes).
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct HazardStyle {
    /// Share of the floor covered by lava channels.
    pub lava: f32,
    /// Share of the floor covered by shallow water.
    pub water: f32,
    /// Spore clouds placed on the floor.
    pub spores: u32,
    /// Vents that pulse heat or cold.
    pub vents: Option<VentKind>,
    pub vent_count: u32,
    /// Thin air: an oxygen meter refilled at vents and the pod.
    pub oxygen: bool,
    /// Periodic quakes that bring rubble down.
    pub quakes: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VentKind {
    Heat,
    Cold,
}

/// Whether an objective is the contract's main goal or an optional extra.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObjectiveRole {
    Primary,
    Secondary,
}

/// Objective archetypes (see `objectives.toml`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObjectiveKind {
    OreQuota,
    Boss,
    Rescue,
    Defend,
    Salvage,
    Hunt,
    Glowcaps,
    Cull,
    Scan,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObjectiveDef {
    pub id: String,
    pub role: ObjectiveRole,
    pub kind: ObjectiveKind,
    pub name: String,
    /// `{count}` is replaced with the target at the contract's hazard.
    pub description: String,
    pub count: u32,
    /// Added per hazard level above 1.
    #[serde(default)]
    pub per_hazard: u32,
    /// The boss, hunted elite or culled enemy (id).
    #[serde(default)]
    pub enemy: Option<String>,
    /// Planets it appears on (ids; empty: all).
    #[serde(default)]
    pub planets: Vec<String>,
    pub weight: u32,
    /// Bonus materials when the drop extracts with it done.
    #[serde(default)]
    pub reward: Materials,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SiteDef {
    pub id: String,
    pub name: String,
    pub description: String,
    pub weight: u32,
    #[serde(default)]
    pub planets: Vec<String>,
    #[serde(default = "one")]
    pub ore: f32,
    #[serde(default = "one")]
    pub enemy_health: f32,
    #[serde(default = "one")]
    pub spawn: f32,
    #[serde(default = "one")]
    pub threat: f32,
    #[serde(default)]
    pub spawn_weights: BTreeMap<String, f32>,
    #[serde(default)]
    pub tint: Option<[f32; 3]>,
    /// Thin air: the oxygen meter runs on any planet.
    #[serde(default)]
    pub oxygen: bool,
    /// Unstable rock: quakes bring rubble down.
    #[serde(default)]
    pub quakes: bool,
}

/// Mutator archetypes (see `mutators.toml`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MutatorEffect {
    RichVeins,
    DoubleSwarm,
    NoRegen,
    LowGravity,
    GlassCannon,
}

/// Equipment slots: each class wears one item in each.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GearSlot {
    Helmet,
    Suit,
    Boots,
    /// The gadget holds the most unusual effects.
    Gadget,
}

impl GearSlot {
    pub const ALL: [Self; 4] = [Self::Helmet, Self::Suit, Self::Boots, Self::Gadget];

    pub fn index(self) -> usize {
        self as usize
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Helmet => "Helmet",
            Self::Suit => "Suit",
            Self::Boots => "Boots",
            Self::Gadget => "Gadget",
        }
    }
}

/// A stat change: an effect and its amount (multiplicative effects scale
/// from 1, see [`PerkEffect::multiplicative`]).
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GearStat {
    pub effect: PerkEffect,
    pub amount: f32,
}

/// An equipment base type (`gear.toml`): its slot and built-in stat.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GearDef {
    pub id: String,
    pub name: String,
    pub slot: GearSlot,
    pub description: String,
    /// The base's own stat, scaled by rarity like affixes.
    pub implicit: GearStat,
    pub weight: u32,
    /// Lowest hazard level it can drop at.
    #[serde(default = "first_hazard")]
    pub min_hazard: u8,
}

fn first_hazard() -> u8 {
    1
}

/// A random equipment affix (`affixes.toml`).
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AffixDef {
    pub id: String,
    /// Shown before the base name ("Swift Runner Boots").
    pub name: String,
    pub effect: PerkEffect,
    /// Roll range at common rarity (for multiplicative effects, the
    /// factor; both scale their change from 1).
    pub range: [f32; 2],
    /// Slots it can roll on (empty: any).
    #[serde(default)]
    pub slots: Vec<GearSlot>,
    pub weight: u32,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EquipmentTuning {
    /// Affixes rolled per rarity.
    pub affixes: [usize; 4],
    /// Stat scale per rarity (implicit and affixes).
    pub scale: [f32; 4],
    /// Extra stat scale per hazard level above 1.
    pub hazard_scale: f32,
    /// A cache rolls gear at least this rare.
    pub cache_min: Rarity,
    /// Chance a cache holds a familiar weapon blueprint instead of gear
    /// (while the class has some left to find).
    pub weapon_find: f32,
    /// Chance an elite drops gear.
    pub elite_drop: f32,
    /// Bosses and hunted elites always drop gear at least this rare.
    pub boss_min: Rarity,
    /// Items kept in the stash; finds beyond it are salvaged for credits.
    pub stash_size: usize,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TradeTuning {
    /// Credits for selling an item, by rarity.
    pub sell: [u32; 4],
    /// Extra credits per affix.
    pub sell_per_affix: u32,
    /// Buying costs this many times the sell price.
    pub markup: f32,
    /// Items on sale at each visit.
    pub stock: usize,
    /// Credits to reroll an item's affixes, by rarity.
    pub reroll: [u32; 4],
    /// Items of one rarity fused into one of the next.
    pub fuse: usize,
    /// Credits to fuse, by the rarity fused.
    pub fuse_cost: [u32; 4],
    /// Materials per trade.
    pub lot: u32,
    /// Credits per lot sold, by material (ferrite, lumite, voidstone).
    pub material_sell: [u32; 3],
    /// Credits per lot bought.
    pub material_buy: [u32; 3],
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MutatorDef {
    pub id: String,
    pub name: String,
    pub description: String,
    pub effect: MutatorEffect,
    pub amount: f32,
    pub reward: f32,
    pub weight: u32,
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
    /// Fraction of incoming damage ignored (equipment).
    Armor,
    /// Extra XP from every shard.
    XpGain,
}

impl PerkEffect {
    /// A stat line: "+12 max health", "+4% move speed".
    pub fn describe(self, a: f32) -> String {
        let pct = |v: f32| (v * 100.0).round();
        match self {
            Self::MaxHealth => format!("+{:.0} max health", a),
            Self::MoveSpeed => format!("+{}% move speed", pct(a - 1.0)),
            Self::Damage => format!("+{}% damage", pct(a)),
            Self::FireRate => format!("+{}% fire rate", pct(1.0 / a.max(0.01) - 1.0)),
            Self::PickupRange => format!("+{:.1} m pickup range", a),
            Self::Mining => format!("+{}% mining speed", pct(a)),
            Self::Regeneration => format!("+{:.1} health/s", a),
            Self::OreBonus => format!("+{}% bonus ore", pct(a)),
            Self::ExtractionSpeed => format!("+{}% extraction speed", pct(a)),
            Self::DashCooldown => format!("{}% faster dash", pct(1.0 - a)),
            Self::HeavyDamage => format!("+{}% vs heavies", pct(a)),
            Self::AbilityCooldown => format!("{}% faster ability", pct(1.0 - a)),
            Self::Luck => format!("+{}% luck", pct(a)),
            Self::Armor => format!("{}% armor", pct(a)),
            Self::XpGain => format!("+{}% XP", pct(a)),
        }
    }

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
    pub threat: ThreatTuning,
    pub hazard: HazardTuning,
    pub missions: MissionTuning,
    pub elites: EliteTuning,
    pub class: ClassTuning,
    pub equipment: EquipmentTuning,
    pub arsenal: ArsenalTuning,
    pub combos: ComboTuning,
    pub hazards: HazardEffects,
    pub trade: TradeTuning,
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
    pub square: f32,
    pub cache_xp: u32,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HordeTuning {
    pub spawn_period: f32,
    pub cap: usize,
    pub tutorial_cap: usize,
    pub tutorial_intensity: f32,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ThreatTuning {
    pub per_minute: f32,
    pub extraction_rate: f32,
    pub extraction_intensity: f32,
    pub spawn_per_level: f32,
    pub health_per_level: f32,
    pub damage_per_level: f32,
    pub curve: f32,
    pub surge_every: u32,
    pub surge_warning: f32,
    pub surge_duration: f32,
    pub surge_intensity: f32,
    pub surge_burst: u32,
    pub surge_burst_per_level: u32,
}

/// Per hazard level 1 to 5.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HazardTuning {
    pub names: [String; 5],
    pub enemy_health: [f32; 5],
    pub enemy_damage: [f32; 5],
    pub spawn: [f32; 5],
    pub elites: [f32; 5],
    pub rarity: [f32; 5],
    pub reward: [f32; 5],
    pub class_xp: [f32; 5],
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MissionTuning {
    pub board: usize,
    pub secondary_chance: f32,
    pub mutator_chance: f32,
    pub scan_seconds: f32,
    pub revive_seconds: f32,
    pub rig_start_seconds: f32,
    pub rig_health: f32,
    pub survivor_health: f32,
    pub primary_class_xp: u32,
    pub secondary_class_xp: u32,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EliteTuning {
    pub per_level: f32,
    pub max: f32,
    pub xp: u32,
    pub armored_health: f32,
    pub swift_speed: f32,
    pub swift_health: f32,
    pub frenzied_damage: f32,
    pub frenzied_health: f32,
    /// Splitting elites break into this many smaller copies.
    pub split_count: u32,
    /// Health share of each split copy.
    pub split_health: f32,
    /// Regenerating elites heal this share of their health per second.
    pub regen: f32,
}

/// New weapon archetypes' numbers (`[arsenal]`).
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArsenalTuning {
    /// Flamer: heat per burst, cooling per second, half-angle of the cone.
    pub flamer_heat: f32,
    pub flamer_cooling: f32,
    pub flamer_cone: f32,
    /// Melee: half-angle of the sweep and knockback distance.
    pub melee_arc: f32,
    pub melee_knockback: f32,
    /// Homing: turn rate in radians per second.
    pub homing_turn: f32,
    /// Gravity: seconds the vortex lasts and its pull in m/s.
    pub gravity_seconds: f32,
    pub gravity_pull: f32,
    /// Mines: seconds to arm and seconds they last.
    pub mine_arm: f32,
    pub mine_life: f32,
    /// Ricochet: reach of each bounce.
    pub ricochet_reach: f32,
}

/// Element interactions (`[combos]`).
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ComboTuning {
    /// Shatter: a frozen enemy hit this hard takes extra damage.
    pub shatter_min: f32,
    pub shatter_bonus: f32,
    /// Thermal shock: fire on a frozen or chilled enemy.
    pub thermal_bonus: f32,
    /// Conductive: shock arcs through wet enemies this far.
    pub conductive_reach: f32,
    pub conductive_damage: f32,
    /// Acid fuels burning: burn damage multiplier while corroded.
    pub acid_fuel: f32,
}

/// Hazard numbers (`[hazards]`).
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HazardEffects {
    /// Lava: damage per second standing in it.
    pub lava_damage: f32,
    /// Water: movement multiplier wading, seconds wet after leaving.
    pub water_slow: f32,
    pub wet_seconds: f32,
    /// Spore clouds: radius, damage per second inside, regrowth seconds,
    /// and the blast when fire ignites them.
    pub spore_radius: f32,
    pub spore_damage: f32,
    pub spore_regrow: f32,
    pub spore_blast_radius: f32,
    pub spore_blast_damage: f32,
    /// Vents: radius and seconds between pulses; heat damage and chill.
    pub vent_radius: f32,
    pub vent_every: f32,
    pub heat_damage: f32,
    pub cold_chill: f32,
    /// Chilled players move slower for this many seconds.
    pub chill_seconds: f32,
    pub chill_slow: f32,
    /// Oxygen: meter, drain and refill per second, damage when empty,
    /// refill vents placed, and a cache's top-up.
    pub oxygen_max: f32,
    pub oxygen_drain: f32,
    pub oxygen_refill: f32,
    pub oxygen_damage: f32,
    pub oxygen_vents: u32,
    pub oxygen_cache: f32,
    /// Quakes: seconds between (range), warning, rubble blocks per quake,
    /// and damage to anything caught.
    pub quake_every: [f32; 2],
    pub quake_warning: f32,
    pub quake_tiles: u32,
    pub quake_damage: f32,
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
