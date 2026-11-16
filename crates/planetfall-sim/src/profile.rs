//! Persistent player progress. Serialized to `data/save/profile.json` by the
//! desktop app; this module only defines the data, its rules and migrations.

use crate::content::{Materials, PlanetId, ShipBlock, ShipUpgrade};
use serde::{Deserialize, Serialize};

/// Current on-disk schema. Bump and extend [`Profile::migrate`] when it changes.
pub const PROFILE_VERSION: u32 = 2;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct UpgradeLevels {
    pub hull_plating: u8,
    pub weapon_calibration: u8,
    pub drill_rig: u8,
    pub tractor_array: u8,
    pub nav_computer: u8,
}

impl UpgradeLevels {
    pub fn get(&self, upgrade: ShipUpgrade) -> u8 {
        match upgrade {
            ShipUpgrade::HullPlating => self.hull_plating,
            ShipUpgrade::WeaponCalibration => self.weapon_calibration,
            ShipUpgrade::DrillRig => self.drill_rig,
            ShipUpgrade::TractorArray => self.tractor_array,
            ShipUpgrade::NavComputer => self.nav_computer,
        }
    }

    fn slot(&mut self, upgrade: ShipUpgrade) -> &mut u8 {
        match upgrade {
            ShipUpgrade::HullPlating => &mut self.hull_plating,
            ShipUpgrade::WeaponCalibration => &mut self.weapon_calibration,
            ShipUpgrade::DrillRig => &mut self.drill_rig,
            ShipUpgrade::TractorArray => &mut self.tractor_array,
            ShipUpgrade::NavComputer => &mut self.nav_computer,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Stats {
    pub drops: u32,
    pub extractions: u32,
    pub deaths: u32,
    pub kills: u32,
    pub best_level: u32,
    pub materials_banked: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Profile {
    pub version: u32,
    pub materials: Materials,
    pub upgrades: UpgradeLevels,
    pub tutorial_complete: bool,
    /// The planet the ship is orbiting, if any. Travel happens at the nav console.
    pub orbiting: Option<PlanetId>,
    pub stats: Stats,
    /// Player-built ship blocks over the base layout, one entry per cell
    /// (version 2+). `block: None` removes a base block.
    pub ship_edits: Vec<ShipEdit>,
}

/// One cell of free-form ship construction.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShipEdit {
    pub x: i32,
    pub z: i32,
    /// `-1` deck layer, `0` wall layer.
    pub layer: i32,
    pub block: Option<ShipBlock>,
}

impl Default for Profile {
    fn default() -> Self {
        Self {
            version: PROFILE_VERSION,
            materials: Materials::default(),
            upgrades: UpgradeLevels::default(),
            tutorial_complete: false,
            orbiting: None,
            stats: Stats::default(),
            ship_edits: Vec::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PurchaseError {
    MaxLevel,
    Unaffordable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProfileError {
    /// Written by a newer Planetfall; refuse rather than silently downgrade.
    Newer(u32),
}

impl std::fmt::Display for ProfileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Newer(v) => write!(
                f,
                "profile version {v} is newer than this build supports ({PROFILE_VERSION})"
            ),
        }
    }
}

impl std::error::Error for ProfileError {}

impl Profile {
    /// Bring an older profile up to [`PROFILE_VERSION`]. Version 0 means the
    /// field was missing, which only happens for hand-edited files.
    pub fn migrate(mut self) -> Result<Self, ProfileError> {
        if self.version > PROFILE_VERSION {
            return Err(ProfileError::Newer(self.version));
        }
        // Future migrations go here, one `if self.version == N { ...; self.version = N + 1 }` per step.
        if self.version == 0 {
            self.version = 1;
        }
        if self.version == 1 {
            // v2 adds free-form ship building; older ships have no edits.
            self.ship_edits.clear();
            self.version = 2;
        }
        for upgrade in ShipUpgrade::ALL {
            let slot = self.upgrades.slot(upgrade);
            *slot = (*slot).min(upgrade.max_level());
        }
        if self.orbiting.is_some_and(|p| !self.unlocked(p)) {
            self.orbiting = None;
        }
        Ok(self)
    }

    pub fn level(&self, upgrade: ShipUpgrade) -> u8 {
        self.upgrades.get(upgrade)
    }

    /// Cost of the next level, or None when maxed.
    pub fn next_cost(&self, upgrade: ShipUpgrade) -> Option<Materials> {
        let level = self.level(upgrade);
        (level < upgrade.max_level()).then(|| upgrade.cost(level))
    }

    pub fn purchase(&mut self, upgrade: ShipUpgrade) -> Result<u8, PurchaseError> {
        let cost = self.next_cost(upgrade).ok_or(PurchaseError::MaxLevel)?;
        if !self.materials.spend(cost) {
            return Err(PurchaseError::Unaffordable);
        }
        let slot = self.upgrades.slot(upgrade);
        *slot += 1;
        Ok(*slot)
    }

    pub fn unlocked(&self, planet: PlanetId) -> bool {
        self.level(ShipUpgrade::NavComputer) >= planet.info().nav_level
    }

    pub fn max_health(&self) -> f32 {
        100.0 + 25.0 * f32::from(self.level(ShipUpgrade::HullPlating))
    }

    pub fn damage_multiplier(&self) -> f32 {
        1.0 + 0.12 * f32::from(self.level(ShipUpgrade::WeaponCalibration))
    }

    pub fn mining_multiplier(&self) -> f32 {
        1.0 + 0.35 * f32::from(self.level(ShipUpgrade::DrillRig))
    }

    pub fn pickup_bonus(&self) -> f32 {
        1.25 * f32::from(self.level(ShipUpgrade::TractorArray))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_json() {
        let mut p = Profile {
            materials: Materials::new(40, 9, 2),
            orbiting: Some(PlanetId::Verdant),
            ..Profile::default()
        };
        p.purchase(ShipUpgrade::HullPlating).unwrap();
        let text = serde_json::to_string(&p).unwrap();
        let back: Profile = serde_json::from_str(&text).unwrap();
        assert_eq!(back.migrate().unwrap(), p);
    }

    #[test]
    fn missing_fields_take_defaults() {
        let p: Profile = serde_json::from_str(r#"{"materials":{"ferrite":3}}"#).unwrap();
        let p = p.migrate().unwrap();
        assert_eq!(p.materials.ferrite, 3);
        assert_eq!(p.version, PROFILE_VERSION);
        assert!(!p.tutorial_complete);
    }

    /// A save written before free-form building loads with an untouched ship.
    #[test]
    fn version_one_saves_migrate() {
        let v1 = r#"{"version":1,"materials":{"ferrite":12,"lumite":4,"voidstone":0},
            "upgrades":{"drill_rig":1},"tutorial_complete":true,"orbiting":"Verdant",
            "stats":{"drops":3}}"#;
        let p: Profile = serde_json::from_str(v1).unwrap();
        let p = p.migrate().unwrap();
        assert_eq!(p.version, 2);
        assert_eq!(p.materials, Materials::new(12, 4, 0));
        assert_eq!(p.level(ShipUpgrade::DrillRig), 1);
        assert!(p.ship_edits.is_empty());
        let mut edited = p.clone();
        edited.ship_edits.push(ShipEdit {
            x: 3,
            z: 4,
            layer: 0,
            block: Some(ShipBlock::Lamp),
        });
        let text = serde_json::to_string(&edited).unwrap();
        let back: Profile = serde_json::from_str(&text).unwrap();
        assert_eq!(back.migrate().unwrap(), edited);
    }

    #[test]
    fn newer_profiles_are_refused() {
        let p = Profile {
            version: PROFILE_VERSION + 1,
            ..Profile::default()
        };
        assert_eq!(p.migrate(), Err(ProfileError::Newer(PROFILE_VERSION + 1)));
    }

    #[test]
    fn purchase_rules() {
        let mut p = Profile::default();
        assert_eq!(
            p.purchase(ShipUpgrade::NavComputer),
            Err(PurchaseError::Unaffordable)
        );
        p.materials = Materials::new(1000, 1000, 1000);
        for _ in 0..ShipUpgrade::NavComputer.max_level() {
            p.purchase(ShipUpgrade::NavComputer).unwrap();
        }
        assert_eq!(
            p.purchase(ShipUpgrade::NavComputer),
            Err(PurchaseError::MaxLevel)
        );
        assert!(PlanetId::ALL.iter().all(|&planet| p.unlocked(planet)));
    }
}
