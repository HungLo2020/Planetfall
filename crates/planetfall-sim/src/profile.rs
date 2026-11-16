//! Persistent player progress. This module only defines the data, its rules
//! and migrations; the app stores it in a save slot (see `save`).

use crate::content::{Materials, PlanetId, ShipBlock, UpgradeEffect, UpgradeId, content};
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::BTreeMap;

/// Current schema. Bump and extend [`Profile::migrate`] when it changes.
///
/// - 1: first release (upgrades as named fields, planets as enum names).
/// - 2: player-built ship blocks (`ship_edits`).
/// - 3: content as data: upgrades keyed by module id, planets by id.
pub const PROFILE_VERSION: u32 = 3;

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

/// One cell of free-form ship construction.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShipEdit {
    pub x: i32,
    pub z: i32,
    /// `-1` deck layer, `0` wall layer.
    pub layer: i32,
    pub block: Option<ShipBlock>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Profile {
    pub version: u32,
    pub materials: Materials,
    /// Module levels by id. Modules no longer in the data are dropped.
    #[serde(deserialize_with = "known_upgrades")]
    pub upgrades: BTreeMap<UpgradeId, u8>,
    pub tutorial_complete: bool,
    /// The planet the ship is orbiting, if any. Travel happens at the nav console.
    #[serde(deserialize_with = "known_planet")]
    pub orbiting: Option<PlanetId>,
    pub stats: Stats,
    /// Player-built ship blocks over the base layout, one entry per cell.
    /// `block: None` removes a base block.
    pub ship_edits: Vec<ShipEdit>,
}

fn known_upgrades<'de, D: Deserializer<'de>>(d: D) -> Result<BTreeMap<UpgradeId, u8>, D::Error> {
    let raw = BTreeMap::<String, u8>::deserialize(d)?;
    Ok(raw
        .into_iter()
        .filter_map(|(k, v)| UpgradeId::from_key(&k).map(|id| (id, v)))
        .collect())
}

fn known_planet<'de, D: Deserializer<'de>>(d: D) -> Result<Option<PlanetId>, D::Error> {
    Ok(Option::<String>::deserialize(d)?.and_then(|k| PlanetId::from_key(&k)))
}

impl Default for Profile {
    fn default() -> Self {
        Self {
            version: PROFILE_VERSION,
            materials: Materials::default(),
            upgrades: BTreeMap::new(),
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
                "save version {v} is newer than this build supports ({PROFILE_VERSION})"
            ),
        }
    }
}

impl std::error::Error for ProfileError {}

impl Profile {
    /// Bring the profile up to [`PROFILE_VERSION`] and clamp it to the
    /// installed content. Older on-disk layouts go through [`LegacyProfile`].
    pub fn migrate(mut self) -> Result<Self, ProfileError> {
        if self.version > PROFILE_VERSION {
            return Err(ProfileError::Newer(self.version));
        }
        self.version = PROFILE_VERSION;
        for (id, level) in &mut self.upgrades {
            *level = (*level).min(id.max_level());
        }
        self.upgrades.retain(|_, level| *level > 0);
        if self.orbiting.is_some_and(|p| !self.unlocked(p)) {
            self.orbiting = None;
        }
        Ok(self)
    }

    pub fn level(&self, upgrade: UpgradeId) -> u8 {
        self.upgrades.get(&upgrade).copied().unwrap_or(0)
    }

    /// Cost of the next level, or None when maxed.
    pub fn next_cost(&self, upgrade: UpgradeId) -> Option<Materials> {
        upgrade.cost(self.level(upgrade))
    }

    pub fn purchase(&mut self, upgrade: UpgradeId) -> Result<u8, PurchaseError> {
        let cost = self.next_cost(upgrade).ok_or(PurchaseError::MaxLevel)?;
        if !self.materials.spend(cost) {
            return Err(PurchaseError::Unaffordable);
        }
        let level = self.upgrades.entry(upgrade).or_insert(0);
        *level += 1;
        Ok(*level)
    }

    /// Sum of `per_level × level` over modules with `effect`.
    fn bonus(&self, effect: UpgradeEffect) -> f32 {
        self.upgrades
            .iter()
            .filter(|(id, _)| id.info().effect == effect)
            .map(|(id, &level)| id.info().per_level * f32::from(level))
            .sum()
    }

    pub fn nav_level(&self) -> u8 {
        self.bonus(UpgradeEffect::NavLevel).round() as u8
    }

    pub fn unlocked(&self, planet: PlanetId) -> bool {
        self.nav_level() >= planet.info().nav_level
    }

    pub fn max_health(&self) -> f32 {
        content().tuning.player.health + self.bonus(UpgradeEffect::MaxHealth)
    }

    pub fn damage_multiplier(&self) -> f32 {
        1.0 + self.bonus(UpgradeEffect::Damage)
    }

    pub fn mining_multiplier(&self) -> f32 {
        1.0 + self.bonus(UpgradeEffect::Mining)
    }

    pub fn pickup_bonus(&self) -> f32 {
        self.bonus(UpgradeEffect::PickupRange)
    }
}

/// The on-disk shape of version 1 and 2 profiles (`data/save/profile.json`
/// before save slots). Converted with [`LegacyProfile::into_profile`].
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct LegacyProfile {
    pub version: u32,
    pub materials: Materials,
    pub upgrades: LegacyUpgrades,
    pub tutorial_complete: bool,
    pub orbiting: Option<String>,
    pub stats: Stats,
    pub ship_edits: Vec<ShipEdit>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize)]
#[serde(default)]
pub struct LegacyUpgrades {
    pub hull_plating: u8,
    pub weapon_calibration: u8,
    pub drill_rig: u8,
    pub tractor_array: u8,
    pub nav_computer: u8,
}

impl LegacyProfile {
    pub fn into_profile(self) -> Result<Profile, ProfileError> {
        if self.version > 2 {
            return Err(ProfileError::Newer(self.version));
        }
        let u = self.upgrades;
        let upgrades = [
            ("med-bay", u.hull_plating),
            ("armory", u.weapon_calibration),
            ("drill-workshop", u.drill_rig),
            ("tractor-array", u.tractor_array),
            ("reactor", u.nav_computer),
        ]
        .into_iter()
        .filter_map(|(key, level)| UpgradeId::from_key(key).map(|id| (id, level)))
        .collect();
        let orbiting = self
            .orbiting
            .and_then(|name| PlanetId::from_key(&name.to_lowercase()));
        Profile {
            version: PROFILE_VERSION,
            materials: self.materials,
            upgrades,
            tutorial_complete: self.tutorial_complete,
            orbiting,
            stats: self.stats,
            // Version 1 had no ship building.
            ship_edits: if self.version >= 2 {
                self.ship_edits
            } else {
                Vec::new()
            },
        }
        .migrate()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(key: &str) -> UpgradeId {
        UpgradeId::from_key(key).unwrap()
    }

    #[test]
    fn round_trips_through_json() {
        let mut p = Profile {
            materials: Materials::new(40, 9, 2),
            orbiting: Some(PlanetId::first()),
            ..Profile::default()
        };
        p.purchase(id("med-bay")).unwrap();
        let text = serde_json::to_string(&p).unwrap();
        assert!(text.contains("\"med-bay\":1"), "{text}");
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

    /// Saves from before content-as-data keep their modules, orbit and builds.
    #[test]
    fn legacy_profiles_migrate() {
        let v2 = r#"{"version":2,"materials":{"ferrite":12,"lumite":4,"voidstone":0},
            "upgrades":{"drill_rig":1,"nav_computer":1},"tutorial_complete":true,
            "orbiting":"Cinder","stats":{"drops":3},
            "ship_edits":[{"x":3,"z":4,"layer":0,"block":"Lamp"}]}"#;
        let legacy: LegacyProfile = serde_json::from_str(v2).unwrap();
        let p = legacy.into_profile().unwrap();
        assert_eq!(p.version, PROFILE_VERSION);
        assert_eq!(p.materials, Materials::new(12, 4, 0));
        assert_eq!(p.level(id("drill-workshop")), 1);
        assert_eq!(p.nav_level(), 1);
        assert_eq!(p.orbiting.map(PlanetId::key), Some("cinder"));
        assert_eq!(p.ship_edits.len(), 1);
        assert_eq!(p.stats.drops, 3);
        let v1 = r#"{"version":1,"upgrades":{"hull_plating":2},"orbiting":"Verdant"}"#;
        let p = serde_json::from_str::<LegacyProfile>(v1)
            .unwrap()
            .into_profile()
            .unwrap();
        assert_eq!(p.level(id("med-bay")), 2);
        assert!(p.ship_edits.is_empty());
    }

    #[test]
    fn unknown_content_is_dropped_not_fatal() {
        let text = r#"{"version":3,"upgrades":{"warp-drive":2,"armory":1},"orbiting":"atlantis"}"#;
        let p: Profile = serde_json::from_str(text).unwrap();
        let p = p.migrate().unwrap();
        assert_eq!(p.upgrades.len(), 1);
        assert_eq!(p.orbiting, None);
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
        let reactor = id("reactor");
        let mut p = Profile::default();
        assert_eq!(p.purchase(reactor), Err(PurchaseError::Unaffordable));
        p.materials = Materials::new(1000, 1000, 1000);
        for _ in 0..reactor.max_level() {
            p.purchase(reactor).unwrap();
        }
        assert_eq!(p.purchase(reactor), Err(PurchaseError::MaxLevel));
        assert!(PlanetId::all().all(|planet| p.unlocked(planet)));
    }

    #[test]
    fn first_module_is_affordable_after_tutorial_haul() {
        let haul = Materials::new(12, 4, 0);
        assert!(UpgradeId::all().any(|u| haul.covers(u.cost(0).unwrap())));
    }
}
