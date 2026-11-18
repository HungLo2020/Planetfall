//! Game content: planets, weapons, enemies, perks, ship modules, crew
//! classes and tuning,
//! loaded from `assets/data/*.toml` (see [`defs`]).
//!
//! The simulation never reads files. The app reads the data directory and
//! calls [`install`] once at startup; anything that runs earlier (tests,
//! headless tools) gets [`Content::builtin`], the same files compiled in.
//! Content is immutable once installed, so determinism is unaffected.
//!
//! Content is referenced by small `Copy` handles ([`PlanetId`],
//! [`WeaponId`], [`EnemyId`], [`PerkId`], [`UpgradeId`], [`ClassId`]) that index the
//! tables and serialize as their string `id`, so saves survive reordering.

pub mod defs;
mod records;

pub use defs::{
    AbilityDef, AbilityKind, AbilityTier, AffixDef, ArsenalTuning, Axis, BlockCosts, BossSpecial,
    Burrow, ClassDef, ClassUnlock, ComboTuning, DecorKind, Drop, Element, ElementDef,
    EnemyBehavior, EnemyDef, EquipmentTuning, Explosion, GearDef, GearSlot, GearStat,
    HazardEffects, HazardStyle, Lunge, ModDef, ModStat, ModTarget, MutatorDef, MutatorEffect,
    ObjectiveDef, ObjectiveKind, ObjectiveRole, OreChances, Palette, PassiveDef, PassiveEffect,
    PerkDef, PerkEffect, PlanetDef, Pulse, Ranged, Rarity, SiteDef, Spawn, Special, Spin,
    TerrainStyle, TradeTuning, Tuning, UpgradeDef, UpgradeEffect, VentKind, WeaponBehavior,
    WeaponDef, WeaponStats, WeaponUpgrades, listed_stat,
};
use quasar_config::content::{ContentFormat, ContentRecord, ContentRegistry, ContentSnapshot};
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

pub const TICK_RATE: f32 = 60.0;
pub const DT: f32 = 1.0 / TICK_RATE;

pub fn seconds(s: f32) -> u32 {
    (s * TICK_RATE).round() as u32
}

// ---------------------------------------------------------------------------
// Materials (a fixed set: the economy, HUD and voxels depend on all three)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Material {
    Ferrite,
    Lumite,
    Voidstone,
}

impl Material {
    pub const ALL: [Self; 3] = [Self::Ferrite, Self::Lumite, Self::Voidstone];

    pub fn name(self) -> &'static str {
        match self {
            Self::Ferrite => "Ferrite",
            Self::Lumite => "Lumite",
            Self::Voidstone => "Voidstone",
        }
    }
}

/// A bundle of persistent materials (banked on the ship or carried on a run).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Materials {
    pub ferrite: u32,
    pub lumite: u32,
    pub voidstone: u32,
}

impl Materials {
    pub const fn new(ferrite: u32, lumite: u32, voidstone: u32) -> Self {
        Self {
            ferrite,
            lumite,
            voidstone,
        }
    }

    pub fn get(&self, m: Material) -> u32 {
        match m {
            Material::Ferrite => self.ferrite,
            Material::Lumite => self.lumite,
            Material::Voidstone => self.voidstone,
        }
    }

    pub fn add(&mut self, m: Material, amount: u32) {
        let slot = match m {
            Material::Ferrite => &mut self.ferrite,
            Material::Lumite => &mut self.lumite,
            Material::Voidstone => &mut self.voidstone,
        };
        *slot = slot.saturating_add(amount);
    }

    pub fn add_all(&mut self, other: Materials) {
        for m in Material::ALL {
            self.add(m, other.get(m));
        }
    }

    pub fn covers(&self, cost: Materials) -> bool {
        Material::ALL.iter().all(|&m| self.get(m) >= cost.get(m))
    }

    /// Spend `cost` if affordable. Returns whether it was spent.
    pub fn spend(&mut self, cost: Materials) -> bool {
        if !self.covers(cost) {
            return false;
        }
        self.ferrite -= cost.ferrite;
        self.lumite -= cost.lumite;
        self.voidstone -= cost.voidstone;
        true
    }

    pub fn total(&self) -> u32 {
        self.ferrite + self.lumite + self.voidstone
    }
}

// ---------------------------------------------------------------------------
// Ship blocks (tied to voxel block IDs; costs are data)
// ---------------------------------------------------------------------------

/// Blocks the player can place on the ship in build mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ShipBlock {
    /// Floor plating: extends the ship outward.
    Deck,
    /// Hull wall.
    Hull,
    /// Glass panel.
    Window,
    /// Light fixture.
    Lamp,
}

impl ShipBlock {
    pub const ALL: [ShipBlock; 4] = [Self::Deck, Self::Hull, Self::Window, Self::Lamp];

    pub fn name(self) -> &'static str {
        match self {
            Self::Deck => "Deck plating",
            Self::Hull => "Hull wall",
            Self::Window => "Window",
            Self::Lamp => "Lamp",
        }
    }

    /// Cost to place; removing a block you placed refunds it.
    pub fn cost(self) -> Materials {
        let costs = &content().tuning.blocks;
        match self {
            Self::Deck => costs.deck,
            Self::Hull => costs.hull,
            Self::Window => costs.window,
            Self::Lamp => costs.lamp,
        }
    }

    /// The deck layer (`-1`) or the wall layer (`0`).
    pub fn layer(self) -> i32 {
        if self == Self::Deck { -1 } else { 0 }
    }

    pub fn next(self) -> Self {
        let i = Self::ALL.iter().position(|&b| b == self).unwrap_or(0);
        Self::ALL[(i + 1) % Self::ALL.len()]
    }
}

// ---------------------------------------------------------------------------
// Registry
// ---------------------------------------------------------------------------

/// Every content table, validated.
#[derive(Clone, Debug, PartialEq)]
pub struct Content {
    pub planets: Vec<PlanetDef>,
    pub weapons: Vec<WeaponDef>,
    pub enemies: Vec<EnemyDef>,
    pub perks: Vec<PerkDef>,
    pub upgrades: Vec<UpgradeDef>,
    pub classes: Vec<ClassDef>,
    pub mods: Vec<ModDef>,
    pub objectives: Vec<ObjectiveDef>,
    pub sites: Vec<SiteDef>,
    pub mutators: Vec<MutatorDef>,
    pub gear: Vec<GearDef>,
    pub affixes: Vec<AffixDef>,
    pub tuning: Tuning,
}

/// The text of each data file.
#[derive(Clone, Copy, Debug)]
pub struct ContentFiles<'a> {
    pub planets: &'a str,
    pub weapons: &'a str,
    pub enemies: &'a str,
    pub perks: &'a str,
    pub upgrades: &'a str,
    pub tuning: &'a str,
    pub classes: &'a str,
    pub mods: &'a str,
    pub objectives: &'a str,
    pub sites: &'a str,
    pub mutators: &'a str,
    pub gear: &'a str,
    pub affixes: &'a str,
}

impl ContentFiles<'static> {
    /// The shipped data files, compiled in.
    pub const BUILTIN: Self = Self {
        planets: include_str!("../../../../assets/data/planets.toml"),
        weapons: include_str!("../../../../assets/data/weapons.toml"),
        enemies: include_str!("../../../../assets/data/enemies.toml"),
        perks: include_str!("../../../../assets/data/perks.toml"),
        upgrades: include_str!("../../../../assets/data/upgrades.toml"),
        tuning: include_str!("../../../../assets/data/tuning.toml"),
        classes: include_str!("../../../../assets/data/classes.toml"),
        mods: include_str!("../../../../assets/data/mods.toml"),
        objectives: include_str!("../../../../assets/data/objectives.toml"),
        sites: include_str!("../../../../assets/data/sites.toml"),
        mutators: include_str!("../../../../assets/data/mutators.toml"),
        gear: include_str!("../../../../assets/data/gear.toml"),
        affixes: include_str!("../../../../assets/data/affixes.toml"),
    };
}

/// File names in the data directory, in [`ContentFiles`] order.
pub const FILE_NAMES: [&str; 13] = [
    "planets.toml",
    "weapons.toml",
    "enemies.toml",
    "perks.toml",
    "upgrades.toml",
    "tuning.toml",
    "classes.toml",
    "mods.toml",
    "objectives.toml",
    "sites.toml",
    "mutators.toml",
    "gear.toml",
    "affixes.toml",
];

/// Table names: the top-level key of each record file (`[[weapon]]`).
const TABLES: [&str; 12] = [
    "planet",
    "weapon",
    "enemy",
    "perk",
    "upgrade",
    "class",
    "mod",
    "objective",
    "site",
    "mutator",
    "gear",
    "affix",
];

impl Content {
    /// Parse and validate the data files. Record tables go through Quasar's
    /// content registry, which checks every record's fields and the
    /// references between tables and reports every problem at once as
    /// `table[record].field: message`; checks spanning a whole table or the
    /// tuning follow.
    pub fn from_toml(files: ContentFiles<'_>) -> Result<Self, String> {
        let mut registry = ContentRegistry::new();
        registry
            .register::<PlanetDef>("planet")
            .register::<WeaponDef>("weapon")
            .register::<EnemyDef>("enemy")
            .register::<PerkDef>("perk")
            .register::<UpgradeDef>("upgrade")
            .register::<ClassDef>("class")
            .register::<ModDef>("mod")
            .register::<ObjectiveDef>("objective")
            .register::<SiteDef>("site")
            .register::<MutatorDef>("mutator")
            .register::<GearDef>("gear")
            .register::<AffixDef>("affix");
        let sources = [
            files.planets,
            files.weapons,
            files.enemies,
            files.perks,
            files.upgrades,
            files.classes,
            files.mods,
            files.objectives,
            files.sites,
            files.mutators,
            files.gear,
            files.affixes,
        ];
        let tables: Vec<(&str, &str, ContentFormat)> = TABLES
            .iter()
            .zip(sources)
            .map(|(name, source)| (*name, source, ContentFormat::Toml))
            .collect();
        registry.load_all(&tables).map_err(|e| e.to_string())?;
        let tuning: Tuning = quasar_config::load_toml_typed(files.tuning)
            .map_err(|e| format!("tuning.toml: {e}"))?;
        let snapshot = registry.snapshot();
        fn records<T: ContentRecord + Clone>(snapshot: &ContentSnapshot, name: &str) -> Vec<T> {
            snapshot
                .table::<T>(name)
                .map(|t| t.records().to_vec())
                .unwrap_or_default()
        }
        let content = Self {
            planets: records(&snapshot, "planet"),
            weapons: records(&snapshot, "weapon"),
            enemies: records(&snapshot, "enemy"),
            perks: records(&snapshot, "perk"),
            upgrades: records(&snapshot, "upgrade"),
            classes: records(&snapshot, "class"),
            mods: records(&snapshot, "mod"),
            objectives: records(&snapshot, "objective"),
            sites: records(&snapshot, "site"),
            mutators: records(&snapshot, "mutator"),
            gear: records(&snapshot, "gear"),
            affixes: records(&snapshot, "affix"),
            tuning,
        };
        content.validate()?;
        Ok(content)
    }

    /// The compiled-in data files.
    pub fn builtin() -> Self {
        Self::from_toml(ContentFiles::BUILTIN).expect("built-in content is valid")
    }

    /// Checks across a whole table or against the tuning (per-record checks
    /// run in each record's [`ContentRecord::validate`]).
    pub fn validate(&self) -> Result<(), String> {
        let mut errors = Vec::new();
        for (table, empty) in TABLES.iter().zip([
            self.planets.is_empty(),
            self.weapons.is_empty(),
            self.enemies.is_empty(),
            self.perks.is_empty(),
            self.upgrades.is_empty(),
            self.classes.is_empty(),
            self.mods.is_empty(),
            self.objectives.is_empty(),
            self.sites.is_empty(),
            self.mutators.is_empty(),
            self.gear.is_empty(),
            self.affixes.is_empty(),
        ]) {
            if empty {
                errors.push(format!("{table}: no records"));
            }
        }
        if self.classes.iter().filter(|c| c.starter).count() != 1 {
            errors.push("class: exactly one class must be marked starter".into());
        }
        for planet in &self.planets {
            let here = |planets: &Vec<String>| planets.is_empty() || planets.contains(&planet.id);
            let primary = self
                .objectives
                .iter()
                .any(|o| o.role == ObjectiveRole::Primary && here(&o.planets));
            if !primary {
                errors.push(format!("objective: no primary objective for {}", planet.id));
            }
            if !self.sites.iter().any(|s| here(&s.planets)) {
                errors.push(format!("site: no site condition for {}", planet.id));
            }
        }
        if !self
            .objectives
            .iter()
            .any(|o| o.kind == ObjectiveKind::OreQuota)
        {
            errors.push("objective: an ore_quota objective is needed for the tutorial".into());
        }
        for slot in GearSlot::ALL {
            if !self
                .gear
                .iter()
                .any(|g| g.slot == slot && g.min_hazard <= 1)
            {
                errors.push(format!("gear: no {} base drops at hazard 1", slot.name()));
            }
            let affixes = self
                .affixes
                .iter()
                .filter(|a| a.slots.is_empty() || a.slots.contains(&slot))
                .count();
            if affixes < self.tuning.equipment.affixes[3] {
                errors.push(format!(
                    "affix: {} has {affixes} affixes, fewer than a legendary rolls",
                    slot.name()
                ));
            }
        }
        if !self.planets.iter().any(|p| p.nav_level == 0) {
            errors.push("planet: none is reachable at nav level 0".into());
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("\n"))
        }
    }
}

static CONTENT: OnceLock<Content> = OnceLock::new();

/// The installed content (the built-in tables until [`install`] runs).
pub fn content() -> &'static Content {
    CONTENT.get_or_init(Content::builtin)
}

/// Install loaded content. Must run before anything reads content; returns
/// the content back if something already did.
pub fn install(content: Content) -> Result<(), Box<Content>> {
    CONTENT.set(content).map_err(Box::new)
}

/// Content handles: a table index that serializes as the entry's `id`.
macro_rules! content_id {
    ($(#[$doc:meta])* $name:ident, $table:ident, $def:ty) => {
        $(#[$doc])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(pub u16);

        impl $name {
            /// Every entry, in data-file order.
            pub fn all() -> impl Iterator<Item = Self> {
                (0..content().$table.len() as u16).map(Self)
            }

            pub fn info(self) -> &'static $def {
                &content().$table[usize::from(self.0)]
            }

            /// The stable string id used in data files and saves.
            pub fn key(self) -> &'static str {
                &self.info().id
            }

            pub fn from_key(key: &str) -> Option<Self> {
                content()
                    .$table
                    .iter()
                    .position(|d| d.id == key)
                    .map(|i| Self(i as u16))
            }
        }

        impl Serialize for $name {
            fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                s.serialize_str(self.key())
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                let key = String::deserialize(d)?;
                Self::from_key(&key).ok_or_else(|| {
                    serde::de::Error::custom(format!(
                        concat!("unknown ", stringify!($table), " id {:?}"),
                        key
                    ))
                })
            }
        }
    };
}

content_id!(
    /// A planet in `planets.toml`.
    PlanetId,
    planets,
    PlanetDef
);
content_id!(
    /// A weapon in `weapons.toml`.
    WeaponId,
    weapons,
    WeaponDef
);
content_id!(
    /// An enemy in `enemies.toml`.
    EnemyId,
    enemies,
    EnemyDef
);
content_id!(
    /// A field perk in `perks.toml`.
    PerkId,
    perks,
    PerkDef
);
content_id!(
    /// A ship module in `upgrades.toml`.
    UpgradeId,
    upgrades,
    UpgradeDef
);
content_id!(
    /// A crew class in `classes.toml`.
    ClassId,
    classes,
    ClassDef
);
content_id!(
    /// A weapon or ability upgrade in `mods.toml`.
    ModId,
    mods,
    ModDef
);
content_id!(
    /// A mission objective in `objectives.toml`.
    ObjectiveId,
    objectives,
    ObjectiveDef
);
content_id!(
    /// A site condition in `sites.toml`.
    SiteId,
    sites,
    SiteDef
);
content_id!(
    /// A mutator in `mutators.toml`.
    MutatorId,
    mutators,
    MutatorDef
);
content_id!(
    /// An equipment base in `gear.toml`.
    GearId,
    gear,
    GearDef
);
content_id!(
    /// An equipment affix in `affixes.toml`.
    AffixId,
    affixes,
    AffixDef
);

impl PlanetId {
    pub fn name(self) -> &'static str {
        &self.info().name
    }
}

impl EnemyId {
    pub fn name(self) -> &'static str {
        &self.info().name
    }
}

impl WeaponId {
    pub fn name(self) -> &'static str {
        &self.info().name
    }

    /// Base stats (before upgrades).
    pub fn base(self) -> WeaponStats {
        self.info().stats
    }
}

impl ClassId {
    pub fn name(self) -> &'static str {
        &self.info().name
    }

    /// The class new commanders start as.
    pub fn starter() -> Self {
        Self::all()
            .find(|c| c.info().starter)
            .expect("validated: a starter class exists")
    }

    pub fn signature(self) -> WeaponId {
        WeaponId::from_key(&self.info().signature).expect("validated reference")
    }

    pub fn familiar(self) -> impl Iterator<Item = WeaponId> {
        self.info()
            .familiar
            .iter()
            .filter_map(|k| WeaponId::from_key(k))
    }

    pub fn is_familiar(self, weapon: WeaponId) -> bool {
        weapon == self.signature() || self.familiar().any(|w| w == weapon)
    }

    /// Ability tier (1-based) at a class level.
    pub fn ability_tier(self, level: u32) -> u8 {
        tier_at(&self.info().ability_levels, level)
    }

    /// Passive tier (1-based) at a class level.
    pub fn passive_tier(self, level: u32) -> u8 {
        tier_at(&self.info().passive_levels, level)
    }

    /// Ability numbers at a tier (clamped to the defined tiers).
    pub fn ability(self, tier: u8) -> &'static AbilityTier {
        let tiers = &self.info().ability.tiers;
        &tiers[usize::from(tier.max(1) - 1).min(tiers.len() - 1)]
    }

    /// Familiar weapons unlocked as starting weapons by class level.
    pub fn level_unlocks(self, level: u32) -> impl Iterator<Item = WeaponId> {
        self.info()
            .unlocks
            .iter()
            .filter(move |u| u.level <= level)
            .filter_map(|u| WeaponId::from_key(&u.weapon))
    }
}

fn tier_at(levels: &[u32], level: u32) -> u8 {
    levels.iter().filter(|&&l| l <= level).count().max(1) as u8
}

/// Class XP needed to go from class `level` to `level + 1`.
pub fn class_xp_to_next(level: u32) -> u32 {
    let t = &content().tuning.class;
    (t.xp_base * t.xp_growth.powi(level.saturating_sub(1) as i32)).round() as u32
}

/// Class level reached with `xp` in total, and the XP into that level.
pub fn class_level(xp: u32) -> (u32, u32) {
    let max = content().tuning.class.max_level;
    let mut level = 1;
    let mut rest = xp;
    while level < max && rest >= class_xp_to_next(level) {
        rest -= class_xp_to_next(level);
        level += 1;
    }
    (level, rest)
}

impl UpgradeId {
    pub fn name(self) -> &'static str {
        &self.info().name
    }

    pub fn max_level(self) -> u8 {
        self.info().max_level()
    }

    /// Cost of going from `level` to `level + 1`.
    pub fn cost(self, level: u8) -> Option<Materials> {
        self.info().costs.get(usize::from(level)).copied()
    }
}

impl PlanetId {
    /// The first planet (nav level 0), where new commanders start.
    pub fn first() -> Self {
        Self::all()
            .min_by_key(|p| p.info().nav_level)
            .expect("validated: planets exist")
    }
}

/// What a level-up (or cache) can give.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Upgrade {
    /// A new weapon in a free slot (dive levels in `pick_levels`).
    NewWeapon(WeaponId),
    /// A mod on one carried weapon.
    WeaponMod(WeaponId, ModId),
    /// A mod on the class ability.
    AbilityMod(ModId),
    /// A field or class perk.
    Field(PerkId),
}

/// One offered choice: an upgrade at a rarity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Offer {
    pub upgrade: Upgrade,
    pub rarity: Rarity,
}

impl ModId {
    /// Value at a rarity.
    pub fn amount(self, rarity: Rarity) -> f32 {
        self.info().amounts[rarity.index()]
    }
}

impl PerkId {
    /// Effect amount at a rarity (multiplicative effects scale their change
    /// from 1).
    pub fn amount(self, rarity: Rarity) -> f32 {
        let info = self.info();
        let k = content().tuning.rarity.perk_scale[rarity.index()];
        if info.effect.multiplicative() {
            1.0 + (info.amount - 1.0) * k
        } else {
            info.amount * k
        }
    }
}

/// A number without needless decimals ("12", "1.5").
fn number(v: f32) -> String {
    if (v - v.round()).abs() < 0.01 {
        format!("{}", v.round() as i64)
    } else {
        format!("{v:.1}")
    }
}

impl Offer {
    pub fn title(self) -> String {
        match self.upgrade {
            Upgrade::NewWeapon(w) => format!("NEW WEAPON: {}", w.info().name),
            Upgrade::WeaponMod(w, m) => format!("{}: {}", w.info().name, m.info().title),
            Upgrade::AbilityMod(m) => format!("Ability: {}", m.info().title),
            Upgrade::Field(p) => p.info().title.clone(),
        }
    }

    pub fn description(self) -> String {
        match self.upgrade {
            Upgrade::NewWeapon(w) => w.info().description.clone(),
            Upgrade::WeaponMod(w, m) => {
                // Weapons word their own upgrades ("drones spin 20% faster").
                let info = m.info();
                w.info()
                    .upgrades
                    .get(info.stat)
                    .unwrap_or(&info.description)
                    .replace("{amount}", &number(m.amount(self.rarity)))
            }
            Upgrade::AbilityMod(m) => m
                .info()
                .description
                .replace("{amount}", &number(m.amount(self.rarity))),
            Upgrade::Field(p) => {
                let a = p.amount(self.rarity);
                let percent = if p.info().effect.multiplicative() {
                    (a - 1.0).abs() * 100.0
                } else {
                    a * 100.0
                };
                p.info()
                    .description
                    .replace("{amount}", &number(a))
                    .replace("{percent}", &number(percent))
            }
        }
    }
}

/// XP needed to advance from `level` to `level + 1`.
pub fn xp_to_next(level: u32) -> u32 {
    let xp = &content().tuning.xp;
    xp.base + level * xp.per_level + (xp.square * (level * level) as f32).round() as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_content_is_valid_and_complete() {
        let c = Content::builtin();
        assert_eq!(c.planets.len(), 5);
        assert_eq!(c.weapons.len(), 13);
        assert_eq!(c.enemies.iter().filter(|e| !e.boss).count(), 8);
        // Every planet has a boss pool of real bosses.
        for p in &c.planets {
            assert!(!p.bosses.is_empty(), "{} has no bosses", p.id);
            for b in &p.bosses {
                assert!(c.enemies.iter().any(|e| &e.id == b && e.boss), "{b}");
            }
        }
        assert!(!c.perks.is_empty());
        assert_eq!(c.upgrades.len(), 5);
        assert_eq!(c.classes.len(), 4);
        assert!(c.objectives.len() >= 9);
        assert!(!c.sites.is_empty());
        assert!(!c.mutators.is_empty());
    }

    #[test]
    fn classes_level_up_and_unlock_tiers() {
        let striker = ClassId::from_key("striker").unwrap();
        assert_eq!(ClassId::starter(), striker);
        assert_eq!(class_level(0), (1, 0));
        let first = class_xp_to_next(1);
        assert_eq!(class_level(first), (2, 0));
        assert!(class_xp_to_next(5) > class_xp_to_next(1));
        assert_eq!(striker.ability_tier(1), 1);
        assert_eq!(striker.ability_tier(4), 2);
        assert_eq!(striker.ability_tier(20), 3);
        assert!(striker.ability(3).damage > striker.ability(1).damage);
        assert!(striker.is_familiar(striker.signature()));
        assert_eq!(striker.level_unlocks(1).count(), 0);
        assert!(striker.level_unlocks(5).count() >= 2);
        // Huge XP stops at the level cap.
        assert_eq!(class_level(u32::MAX).0, content().tuning.class.max_level);
    }

    #[test]
    fn ids_round_trip_through_their_keys() {
        for p in PlanetId::all() {
            assert_eq!(PlanetId::from_key(p.key()), Some(p));
            let json = serde_json::to_string(&p).unwrap();
            assert_eq!(serde_json::from_str::<PlanetId>(&json).unwrap(), p);
        }
        assert!(serde_json::from_str::<WeaponId>("\"no-such-gun\"").is_err());
    }

    #[test]
    fn bad_data_is_reported() {
        let mut files = ContentFiles::BUILTIN;
        let broken = files.classes.replace("starter = true", "");
        files.classes = &broken;
        let error = Content::from_toml(files).unwrap_err();
        assert!(error.contains("starter"), "{error}");
        // Problems name the record and field; every record is checked.
        let broken = ContentFiles::BUILTIN
            .planets
            .replace("hazard = 1.45", "hazard = -1.0")
            .replace("hazard = 2.0", "hazard = -1.0");
        files = ContentFiles::BUILTIN;
        files.planets = &broken;
        let error = Content::from_toml(files).unwrap_err();
        assert!(error.contains("planet[cinder].hazard"), "{error}");
        assert!(error.contains("planet[hollow].hazard"), "{error}");
        // References between tables are checked once the records parse.
        let dangling = ContentFiles::BUILTIN.planets.replace("husk = ", "hsk = ");
        files.planets = &dangling;
        let error = Content::from_toml(files).unwrap_err();
        assert!(error.contains("unknown enemy id `hsk`"), "{error}");
        // Classes must name real, familiar weapons.
        let bad = ContentFiles::BUILTIN.classes.replace(
            "weapon = \"plasma-mortar\" },\n  { level = 5, weapon = \"scattergun\" }",
            "weapon = \"laser-sword\" },\n  { level = 5, weapon = \"scattergun\" }",
        );
        files = ContentFiles::BUILTIN;
        files.classes = &bad;
        let error = Content::from_toml(files).unwrap_err();
        assert!(
            error.contains("class[prospector].unlocks[0].weapon"),
            "{error}"
        );
        let typo = ContentFiles::BUILTIN
            .enemies
            .replace("health = 26.0", "helth = 26.0");
        files = ContentFiles::BUILTIN;
        files.enemies = &typo;
        assert!(Content::from_toml(files).is_err());
    }

    #[test]
    fn materials_spend_is_all_or_nothing() {
        let mut bank = Materials::new(10, 2, 0);
        assert!(!bank.spend(Materials::new(5, 3, 0)));
        assert_eq!(bank, Materials::new(10, 2, 0));
        assert!(bank.spend(Materials::new(5, 2, 0)));
        assert_eq!(bank, Materials::new(5, 0, 0));
    }

    /// Rarer offers are worth more, and descriptions show the real amount.
    #[test]
    fn rarity_scales_amounts_and_text() {
        let hot = ModId::from_key("hotter-rounds").unwrap();
        assert!(hot.amount(Rarity::Legendary) > hot.amount(Rarity::Common));
        let blaster = WeaponId::from_key("pulse-blaster").unwrap();
        let offer = Offer {
            upgrade: Upgrade::WeaponMod(blaster, hot),
            rarity: Rarity::Rare,
        };
        assert!(
            offer.description().contains("48"),
            "{}",
            offer.description()
        );
        let boots = PerkId::from_key("servo-boots").unwrap();
        assert!(boots.amount(Rarity::Legendary) > boots.amount(Rarity::Common));
        assert!(boots.amount(Rarity::Common) > 1.0);
        let text = Offer {
            upgrade: Upgrade::Field(boots),
            rarity: Rarity::Common,
        }
        .description();
        assert_eq!(text, "+10% movement speed.");
        // Elements need rare, behaviour changes legendary.
        for m in ModId::all() {
            let info = m.info();
            if info.stat == ModStat::Element {
                assert!(info.min_rarity >= Rarity::Rare, "{}", m.key());
            }
            if info.stat == ModStat::Special {
                assert_eq!(info.min_rarity, Rarity::Legendary, "{}", m.key());
            }
        }
    }
}
