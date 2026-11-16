//! Game content: planets, weapons, enemies, perks, ship modules and tuning,
//! loaded from `assets/data/*.toml` (see [`defs`]).
//!
//! The simulation never reads files. The app reads the data directory and
//! calls [`install`] once at startup; anything that runs earlier (tests,
//! headless tools) gets [`Content::builtin`], the same files compiled in.
//! Content is immutable once installed, so determinism is unaffected.
//!
//! Content is referenced by small `Copy` handles ([`PlanetId`],
//! [`WeaponId`], [`EnemyId`], [`PerkId`], [`UpgradeId`]) that index the
//! tables and serialize as their string `id`, so saves survive reordering.

pub mod defs;

pub use defs::{
    Axis, BlockCosts, Drop, EnemyBehavior, EnemyDef, Explosion, Lunge, OreChances, Palette,
    PerkDef, PerkEffect, PlanetDef, Spawn, Spin, Tuning, UpgradeDef, UpgradeEffect, WeaponBehavior,
    WeaponDef, WeaponStats,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
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
    };
}

/// File names in the data directory, in [`ContentFiles`] order.
pub const FILE_NAMES: [&str; 6] = [
    "planets.toml",
    "weapons.toml",
    "enemies.toml",
    "perks.toml",
    "upgrades.toml",
    "tuning.toml",
];

fn parse<T: serde::de::DeserializeOwned>(name: &str, text: &str) -> Result<T, String> {
    quasar_config::load_toml_typed(text).map_err(|e| format!("{name}: {e}"))
}

impl Content {
    /// Parse and validate the data files.
    pub fn from_toml(files: ContentFiles<'_>) -> Result<Self, String> {
        let planets: defs::PlanetFile = parse("planets.toml", files.planets)?;
        let weapons: defs::WeaponFile = parse("weapons.toml", files.weapons)?;
        let enemies: defs::EnemyFile = parse("enemies.toml", files.enemies)?;
        let perks: defs::PerkFile = parse("perks.toml", files.perks)?;
        let upgrades: defs::UpgradeFile = parse("upgrades.toml", files.upgrades)?;
        let tuning: Tuning = parse("tuning.toml", files.tuning)?;
        let content = Self {
            planets: planets.planet,
            weapons: weapons.weapon,
            enemies: enemies.enemy,
            perks: perks.perk,
            upgrades: upgrades.upgrade,
            tuning,
        };
        content.validate()?;
        Ok(content)
    }

    /// The compiled-in data files.
    pub fn builtin() -> Self {
        Self::from_toml(ContentFiles::BUILTIN).expect("built-in content is valid")
    }

    /// Cross-checks the parser cannot express. Lists every problem found.
    pub fn validate(&self) -> Result<(), String> {
        let mut errors = Vec::new();
        let mut unique = |kind: &str, ids: Vec<&str>| {
            let mut seen = BTreeSet::new();
            if ids.is_empty() {
                errors.push(format!("no {kind}s defined"));
            }
            for id in ids {
                if id.is_empty() || !seen.insert(id) {
                    errors.push(format!("{kind} id {id:?} is empty or duplicated"));
                }
            }
        };
        unique(
            "planet",
            self.planets.iter().map(|d| d.id.as_str()).collect(),
        );
        unique(
            "weapon",
            self.weapons.iter().map(|d| d.id.as_str()).collect(),
        );
        unique(
            "enemy",
            self.enemies.iter().map(|d| d.id.as_str()).collect(),
        );
        unique("perk", self.perks.iter().map(|d| d.id.as_str()).collect());
        unique(
            "upgrade",
            self.upgrades.iter().map(|d| d.id.as_str()).collect(),
        );
        let levels = usize::from(self.tuning.weapons.max_level);
        if !self.weapons.iter().any(|w| w.starter) {
            errors.push("no weapon is marked starter".into());
        }
        for w in &self.weapons {
            if w.levels.len() != levels {
                errors.push(format!(
                    "weapon {}: {} levels, tuning.weapons.max_level is {levels}",
                    w.id,
                    w.levels.len()
                ));
            }
            if w.levels.iter().any(|l| l.cooldown <= 0.0 || l.range <= 0.0) {
                errors.push(format!(
                    "weapon {}: cooldown and range must be positive",
                    w.id
                ));
            }
        }
        for e in &self.enemies {
            if e.health <= 0.0 || e.radius <= 0.0 {
                errors.push(format!(
                    "enemy {}: health and radius must be positive",
                    e.id
                ));
            }
            if e.behavior == EnemyBehavior::Exploder && e.explosion.is_none() {
                errors.push(format!("enemy {}: exploders need an `explosion`", e.id));
            }
            if e.behavior == EnemyBehavior::Lunger && e.lunge.is_none() {
                errors.push(format!("enemy {}: lungers need a `lunge`", e.id));
            }
            if e.spawn.count[0] == 0 || e.spawn.count[0] > e.spawn.count[1] {
                errors.push(format!("enemy {}: spawn count must be 1..=max", e.id));
            }
        }
        if !self.planets.iter().any(|p| p.nav_level == 0) {
            errors.push("no planet is reachable at nav level 0".into());
        }
        for p in &self.planets {
            for id in p.spawn_weights.keys() {
                if !self.enemies.iter().any(|e| &e.id == id) {
                    errors.push(format!(
                        "planet {}: unknown enemy {id:?} in spawn_weights",
                        p.id
                    ));
                }
            }
        }
        for u in &self.upgrades {
            if u.costs.is_empty() {
                errors.push(format!("upgrade {}: needs at least one cost", u.id));
            }
            let [x0, y0, x1, y1] = u.room;
            if x0 > x1 || y0 > y1 {
                errors.push(format!("upgrade {}: room rect is inverted", u.id));
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("; "))
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

    /// Stats at `level` (1-based, clamped to the defined levels).
    pub fn stats(self, level: u8) -> WeaponStats {
        let levels = &self.info().levels;
        levels[usize::from(level.max(1) - 1).min(levels.len() - 1)]
    }

    pub fn starter() -> Self {
        Self::all()
            .find(|w| w.info().starter)
            .expect("validated: a starter weapon exists")
    }
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

/// A level-up choice.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Perk {
    NewWeapon(WeaponId),
    WeaponLevel(WeaponId),
    Field(PerkId),
}

impl Perk {
    pub fn title(self) -> String {
        match self {
            Self::NewWeapon(w) => format!("NEW: {}", w.info().name),
            Self::WeaponLevel(w) => format!("{} +1", w.info().name),
            Self::Field(p) => p.info().title.clone(),
        }
    }

    pub fn description(self) -> String {
        match self {
            Self::NewWeapon(w) => w.info().description.clone(),
            Self::WeaponLevel(_) => "More damage, speed and projectiles.".into(),
            Self::Field(p) => p.info().description.clone(),
        }
    }
}

/// XP needed to advance from `level` to `level + 1`.
pub fn xp_to_next(level: u32) -> u32 {
    let xp = &content().tuning.xp;
    xp.base + level * xp.per_level
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_content_is_valid_and_complete() {
        let c = Content::builtin();
        assert_eq!(c.planets.len(), 3);
        assert_eq!(c.weapons.len(), 5);
        assert_eq!(c.enemies.len(), 4);
        assert!(!c.perks.is_empty());
        assert_eq!(c.upgrades.len(), 5);
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
        let broken = files.weapons.replace("starter = true", "");
        files.weapons = &broken;
        let error = Content::from_toml(files).unwrap_err();
        assert!(error.contains("starter"), "{error}");
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

    #[test]
    fn weapon_levels_improve() {
        let max = content().tuning.weapons.max_level;
        for w in WeaponId::all() {
            let (a, b) = (w.stats(1), w.stats(max));
            assert!(b.damage > a.damage, "{}", w.key());
            assert!(b.cooldown <= a.cooldown, "{}", w.key());
        }
    }
}
