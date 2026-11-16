//! Static game content and tuning: planets, materials, weapons, enemies, ship
//! upgrades and in-run perks. Everything here is plain data with no state.

use serde::{Deserialize, Serialize};

pub const TICK_RATE: f32 = 60.0;
pub const DT: f32 = 1.0 / TICK_RATE;

pub fn seconds(s: f32) -> u32 {
    (s * TICK_RATE).round() as u32
}

// ---------------------------------------------------------------------------
// Materials
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
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
// Planets
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum PlanetId {
    Verdant,
    Cinder,
    Hollow,
}

/// Visual palette for a planet surface (linear RGB).
#[derive(Clone, Copy, Debug, PartialEq)]
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

#[derive(Clone, Copy, Debug)]
pub struct PlanetInfo {
    pub name: &'static str,
    pub kind: &'static str,
    pub description: &'static str,
    /// Navigation Computer level needed to plot a course here.
    pub nav_level: u8,
    /// Multiplier on enemy health/spawn pressure.
    pub hazard: f32,
    /// Chance an exposed rock wall becomes ferrite / lumite / voidstone ore.
    pub ore: [f32; 3],
    /// Lumite needed to complete the drop objective.
    pub objective: u32,
    pub palette: Palette,
}

impl PlanetId {
    pub const ALL: [Self; 3] = [Self::Verdant, Self::Cinder, Self::Hollow];

    pub fn info(self) -> PlanetInfo {
        match self {
            Self::Verdant => PlanetInfo {
                name: "Verdant-7",
                kind: "Overgrown moon",
                description: "Mossy caverns infested by husks. Rich in ferrite. A good first drop.",
                nav_level: 0,
                hazard: 1.0,
                ore: [0.16, 0.07, 0.0],
                objective: 6,
                palette: Palette {
                    floor: [0.20, 0.26, 0.16],
                    floor_alt: [0.17, 0.23, 0.14],
                    rock: [0.30, 0.34, 0.26],
                    rock_top: [0.30, 0.45, 0.22],
                    bedrock: [0.12, 0.13, 0.12],
                    ambient: [0.10, 0.13, 0.11],
                    sun: [1.0, 0.95, 0.82],
                    sky: [0.05, 0.08, 0.06],
                    planet: [0.35, 0.62, 0.32],
                },
            },
            Self::Cinder => PlanetInfo {
                name: "Cinder Reach",
                kind: "Volcanic world",
                description: "Scorched basalt tunnels. Dense lumite veins, aggressive skitter swarms.",
                nav_level: 1,
                hazard: 1.45,
                ore: [0.12, 0.11, 0.015],
                objective: 9,
                palette: Palette {
                    floor: [0.22, 0.13, 0.10],
                    floor_alt: [0.19, 0.11, 0.09],
                    rock: [0.26, 0.20, 0.18],
                    rock_top: [0.45, 0.20, 0.10],
                    bedrock: [0.10, 0.07, 0.06],
                    ambient: [0.15, 0.08, 0.06],
                    sun: [1.0, 0.72, 0.5],
                    sky: [0.09, 0.03, 0.02],
                    planet: [0.85, 0.38, 0.18],
                },
            },
            Self::Hollow => PlanetInfo {
                name: "Hollow Frost",
                kind: "Frozen rogue planet",
                description: "Ice-choked caves. Voidstone seams and brutes in the dark.",
                nav_level: 2,
                hazard: 2.0,
                ore: [0.10, 0.09, 0.05],
                objective: 12,
                palette: Palette {
                    floor: [0.40, 0.46, 0.52],
                    floor_alt: [0.36, 0.42, 0.48],
                    rock: [0.42, 0.50, 0.58],
                    rock_top: [0.75, 0.85, 0.92],
                    bedrock: [0.14, 0.16, 0.20],
                    ambient: [0.10, 0.13, 0.18],
                    sun: [0.78, 0.86, 1.0],
                    sky: [0.03, 0.05, 0.09],
                    planet: [0.62, 0.78, 0.95],
                },
            },
        }
    }
}

// ---------------------------------------------------------------------------
// Weapons
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum WeaponKind {
    PulseBlaster,
    Scattergun,
    ArcCaster,
    PlasmaMortar,
    OrbitDrones,
}

pub const MAX_WEAPON_LEVEL: u8 = 5;
pub const MAX_WEAPONS: usize = 4;

/// Per-level weapon numbers. `level` is 1-based.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WeaponStats {
    pub cooldown: f32,
    pub damage: f32,
    pub range: f32,
    /// Bolts, pellets, chain jumps or drones depending on the weapon.
    pub count: u32,
    pub speed: f32,
    pub radius: f32,
    pub pierce: u32,
}

impl WeaponKind {
    pub const ALL: [Self; 5] = [
        Self::PulseBlaster,
        Self::Scattergun,
        Self::ArcCaster,
        Self::PlasmaMortar,
        Self::OrbitDrones,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::PulseBlaster => "Pulse Blaster",
            Self::Scattergun => "Scattergun",
            Self::ArcCaster => "Arc Caster",
            Self::PlasmaMortar => "Plasma Mortar",
            Self::OrbitDrones => "Orbit Drones",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::PulseBlaster => "Rapid bolts at the nearest hostile.",
            Self::Scattergun => "Close-range pellet spread.",
            Self::ArcCaster => "Lightning that chains between targets.",
            Self::PlasmaMortar => "Lobbed plasma that bursts on impact.",
            Self::OrbitDrones => "Blades that circle you and shred anything close.",
        }
    }

    pub fn stats(self, level: u8) -> WeaponStats {
        let l = f32::from(level.clamp(1, MAX_WEAPON_LEVEL) - 1);
        match self {
            Self::PulseBlaster => WeaponStats {
                cooldown: 0.42 - l * 0.04,
                damage: 11.0 + l * 3.0,
                range: 13.0,
                count: 1 + u32::from(level >= 4),
                speed: 26.0,
                radius: 0.25,
                pierce: u32::from(level >= 5),
            },
            Self::Scattergun => WeaponStats {
                cooldown: 1.05 - l * 0.08,
                damage: 7.0 + l * 1.2,
                range: 7.5,
                count: 5 + u32::from(level) - 1,
                speed: 22.0,
                radius: 0.22,
                pierce: 0,
            },
            Self::ArcCaster => WeaponStats {
                cooldown: 1.3 - l * 0.12,
                damage: 13.0 + l * 3.5,
                range: 9.0,
                count: 3 + u32::from(level) - 1,
                speed: 0.0,
                radius: 5.0,
                pierce: 0,
            },
            Self::PlasmaMortar => WeaponStats {
                cooldown: 2.2 - l * 0.18,
                damage: 32.0 + l * 8.0,
                range: 14.0,
                count: 1 + u32::from(level >= 4),
                speed: 0.9,
                radius: 2.6 + l * 0.35,
                pierce: 0,
            },
            Self::OrbitDrones => WeaponStats {
                cooldown: 0.5,
                damage: 9.0 + l * 2.0,
                range: 2.6,
                count: 2 + u32::from(level) - 1,
                speed: 2.6,
                radius: 0.7,
                pierce: 0,
            },
        }
    }
}

// ---------------------------------------------------------------------------
// Enemies
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum EnemyKind {
    Husk,
    Skitter,
    Bloater,
    Brute,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EnemyStats {
    pub health: f32,
    pub speed: f32,
    pub damage: f32,
    pub radius: f32,
    pub xp: u32,
}

impl EnemyKind {
    pub const ALL: [Self; 4] = [Self::Husk, Self::Skitter, Self::Bloater, Self::Brute];

    pub fn name(self) -> &'static str {
        match self {
            Self::Husk => "Husk",
            Self::Skitter => "Skitter",
            Self::Bloater => "Bloater",
            Self::Brute => "Brute",
        }
    }

    pub fn stats(self) -> EnemyStats {
        match self {
            Self::Husk => EnemyStats {
                health: 26.0,
                speed: 2.7,
                damage: 9.0,
                radius: 0.45,
                xp: 2,
            },
            Self::Skitter => EnemyStats {
                health: 10.0,
                speed: 5.0,
                damage: 5.0,
                radius: 0.4,
                xp: 1,
            },
            Self::Bloater => EnemyStats {
                health: 42.0,
                speed: 1.9,
                damage: 22.0,
                radius: 0.7,
                xp: 4,
            },
            Self::Brute => EnemyStats {
                health: 320.0,
                speed: 2.1,
                damage: 24.0,
                radius: 0.95,
                xp: 20,
            },
        }
    }
}

/// Bloater detonation radius.
pub const BLOATER_BLAST: f32 = 3.0;

// ---------------------------------------------------------------------------
// Ship upgrades (persistent)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ShipUpgrade {
    HullPlating,
    WeaponCalibration,
    DrillRig,
    TractorArray,
    NavComputer,
}

impl ShipUpgrade {
    pub const ALL: [Self; 5] = [
        Self::HullPlating,
        Self::WeaponCalibration,
        Self::DrillRig,
        Self::TractorArray,
        Self::NavComputer,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::HullPlating => "Med Bay",
            Self::WeaponCalibration => "Armory",
            Self::DrillRig => "Drill Workshop",
            Self::TractorArray => "Tractor Array",
            Self::NavComputer => "Reactor & Nav Core",
        }
    }

    pub fn effect(self) -> &'static str {
        match self {
            Self::HullPlating => "+25 max health per level",
            Self::WeaponCalibration => "+12% weapon damage per level",
            Self::DrillRig => "+35% mining speed per level",
            Self::TractorArray => "+1.25 m pickup range per level",
            Self::NavComputer => "Unlocks a more dangerous planet per level",
        }
    }

    pub fn max_level(self) -> u8 {
        match self {
            Self::NavComputer => 2,
            _ => 3,
        }
    }

    /// Cost of going from `level` to `level + 1`.
    pub fn cost(self, level: u8) -> Materials {
        let l = u32::from(level);
        match self {
            Self::HullPlating => Materials::new(10 + l * 14, 2 + l * 4, l * 2),
            Self::WeaponCalibration => Materials::new(14 + l * 14, 3 + l * 4, l * 3),
            Self::DrillRig => Materials::new(12 + l * 10, 2 + l * 3, l * 2),
            Self::TractorArray => Materials::new(8 + l * 10, 3 + l * 3, l * 2),
            Self::NavComputer => Materials::new(20 + l * 20, 8 + l * 8, 1 + l * 5),
        }
    }
}

// ---------------------------------------------------------------------------
// In-run perks (lost when the run ends)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Perk {
    NewWeapon(WeaponKind),
    WeaponLevel(WeaponKind),
    MaxHealth,
    MoveSpeed,
    Damage,
    FireRate,
    PickupRange,
    MiningSpeed,
    Regeneration,
}

impl Perk {
    pub const STATS: [Self; 7] = [
        Self::MaxHealth,
        Self::MoveSpeed,
        Self::Damage,
        Self::FireRate,
        Self::PickupRange,
        Self::MiningSpeed,
        Self::Regeneration,
    ];

    pub fn title(self) -> String {
        match self {
            Self::NewWeapon(w) => format!("NEW: {}", w.name()),
            Self::WeaponLevel(w) => format!("{} +1", w.name()),
            Self::MaxHealth => "Reinforced Suit".into(),
            Self::MoveSpeed => "Servo Boots".into(),
            Self::Damage => "Overcharge".into(),
            Self::FireRate => "Rapid Cycling".into(),
            Self::PickupRange => "Magnetic Field".into(),
            Self::MiningSpeed => "Hardened Drill".into(),
            Self::Regeneration => "Nanite Repair".into(),
        }
    }

    pub fn description(self) -> String {
        match self {
            Self::NewWeapon(w) => w.description().into(),
            Self::WeaponLevel(_) => "More damage, speed and projectiles.".into(),
            Self::MaxHealth => "+20 max health and heal 20.".into(),
            Self::MoveSpeed => "+10% movement speed.".into(),
            Self::Damage => "+15% damage for all weapons.".into(),
            Self::FireRate => "Weapons cycle 10% faster.".into(),
            Self::PickupRange => "+1.5 m pickup range.".into(),
            Self::MiningSpeed => "+40% mining speed.".into(),
            Self::Regeneration => "Regenerate 1 health per second.".into(),
        }
    }
}

/// XP needed to advance from `level` to `level + 1`.
pub fn xp_to_next(level: u32) -> u32 {
    5 + level * 4
}

#[cfg(test)]
mod tests {
    use super::*;

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
        for w in WeaponKind::ALL {
            let a = w.stats(1);
            let b = w.stats(MAX_WEAPON_LEVEL);
            assert!(b.damage > a.damage, "{w:?}");
            assert!(b.cooldown <= a.cooldown, "{w:?}");
        }
    }

    #[test]
    fn first_upgrade_is_affordable_after_tutorial_haul() {
        // The tutorial drop guarantees at least this much.
        let haul = Materials::new(12, 4, 0);
        assert!(haul.covers(ShipUpgrade::HullPlating.cost(0)));
    }
}
