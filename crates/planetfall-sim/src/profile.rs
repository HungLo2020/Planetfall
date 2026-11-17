//! Persistent player progress. This module only defines the data, its rules
//! and migrations; the app stores it in a save slot (see `save`).

use crate::content::{
    ClassId, GearSlot, Material, Materials, PerkEffect, PlanetId, Rarity, ShipBlock, UpgradeEffect,
    UpgradeId, WeaponId, class_level, content,
};
use crate::gear::{self, Gear};
use crate::mission::{Contract, MAX_HAZARD};
use crate::run::Loadout;
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::BTreeMap;

/// Current schema. Bump and extend [`Profile::migrate`] when it changes.
///
/// - 1: first release (upgrades as named fields, planets as enum names).
/// - 2: player-built ship blocks (`ship_edits`).
/// - 3: content as data: upgrades keyed by module id, planets by id.
/// - 4: crew classes: the active class and a record per class.
/// - 5: missions: unlocked hazard level and the mission board.
/// - 6: equipment: the stash, gear equipped per class, credits, the trade
///   station (docked flag and its shelf).
pub const PROFILE_VERSION: u32 = 6;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Stats {
    pub drops: u32,
    pub extractions: u32,
    pub deaths: u32,
    pub kills: u32,
    pub best_level: u32,
    pub materials_banked: u32,
    /// Seconds spent on planet surfaces.
    pub surface_seconds: u32,
    /// Highest threat level reached.
    pub best_threat: u32,
}

/// One class's permanent progress (kept when switching classes).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ClassRecord {
    /// Class XP earned in total (the level is derived from it).
    pub xp: u32,
    /// Chosen starting weapon; `None` (or no longer allowed) means the
    /// signature weapon.
    #[serde(deserialize_with = "known_weapon")]
    pub start_weapon: Option<WeaponId>,
    /// Familiar weapons found on dives, unlocked as starting weapons.
    #[serde(deserialize_with = "known_weapons")]
    pub found_weapons: Vec<WeaponId>,
    /// Stash item ids worn in each slot.
    pub equipped: BTreeMap<GearSlot, u32>,
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
    /// The class chosen in the locker room.
    #[serde(deserialize_with = "known_class")]
    pub class: ClassId,
    /// Progress per class, by id. Classes no longer in the data are dropped.
    #[serde(deserialize_with = "known_classes")]
    pub classes: BTreeMap<ClassId, ClassRecord>,
    /// Highest hazard level that can be chosen (1 to 5).
    pub hazard_unlocked: u8,
    /// The hazard level last chosen on the mission board.
    pub hazard_choice: u8,
    /// Contracts on offer for the planet in orbit. Contracts naming
    /// content that no longer exists are dropped.
    #[serde(deserialize_with = "known_contracts")]
    pub board: Vec<Contract>,
    /// Equipment found on dives (kept even on death). Items naming content
    /// that no longer exists are dropped.
    #[serde(deserialize_with = "known_gear")]
    pub stash: Vec<Gear>,
    /// The next stash item id.
    pub next_gear: u32,
    /// Earned by selling at the trade station.
    pub credits: u32,
    /// Docked at the trade station (no planet below).
    pub docked: bool,
    /// The station's shelf this visit.
    #[serde(deserialize_with = "known_gear")]
    pub market: Vec<Gear>,
}

fn known_gear<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<Gear>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Maybe {
        Known(Gear),
        Unknown(serde::de::IgnoredAny),
    }
    Ok(Vec::<Maybe>::deserialize(d)?
        .into_iter()
        .filter_map(|m| match m {
            Maybe::Known(g) => Some(g),
            Maybe::Unknown(_) => None,
        })
        .collect())
}

fn known_contracts<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<Contract>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Maybe {
        Known(Contract),
        Unknown(serde::de::IgnoredAny),
    }
    Ok(Vec::<Maybe>::deserialize(d)?
        .into_iter()
        .filter_map(|m| match m {
            Maybe::Known(c) => Some(c),
            Maybe::Unknown(_) => None,
        })
        .collect())
}

fn known_weapon<'de, D: Deserializer<'de>>(d: D) -> Result<Option<WeaponId>, D::Error> {
    Ok(Option::<String>::deserialize(d)?.and_then(|k| WeaponId::from_key(&k)))
}

fn known_weapons<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<WeaponId>, D::Error> {
    Ok(Vec::<String>::deserialize(d)?
        .iter()
        .filter_map(|k| WeaponId::from_key(k))
        .collect())
}

fn known_class<'de, D: Deserializer<'de>>(d: D) -> Result<ClassId, D::Error> {
    Ok(ClassId::from_key(&String::deserialize(d)?).unwrap_or_else(ClassId::starter))
}

fn known_classes<'de, D: Deserializer<'de>>(
    d: D,
) -> Result<BTreeMap<ClassId, ClassRecord>, D::Error> {
    let raw = BTreeMap::<String, ClassRecord>::deserialize(d)?;
    Ok(raw
        .into_iter()
        .filter_map(|(k, v)| ClassId::from_key(&k).map(|id| (id, v)))
        .collect())
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
            class: ClassId::starter(),
            classes: BTreeMap::new(),
            hazard_unlocked: 1,
            hazard_choice: 1,
            board: Vec::new(),
            stash: Vec::new(),
            next_gear: 1,
            credits: 0,
            docked: false,
            market: Vec::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PurchaseError {
    MaxLevel,
    Unaffordable,
}

/// Why a trade or stash action was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TradeError {
    Missing,
    Credits,
    Materials,
    StashFull,
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
        self.hazard_unlocked = self.hazard_unlocked.clamp(1, MAX_HAZARD);
        self.hazard_choice = self.hazard_choice.clamp(1, self.hazard_unlocked);
        if self
            .orbiting
            .is_none_or(|p| self.board.iter().any(|c| c.planet != p))
        {
            self.board.clear();
        }
        // Item ids stay unique; equipped ids must exist and fit their slot.
        let mut seen = std::collections::BTreeSet::new();
        for g in &mut self.stash {
            if g.id == 0 || !seen.insert(g.id) {
                g.id = 0;
            }
        }
        let mut next = self.stash.iter().map(|g| g.id).max().unwrap_or(0) + 1;
        for g in &mut self.stash {
            if g.id == 0 {
                g.id = next;
                next += 1;
            }
        }
        self.next_gear = self.next_gear.max(next);
        let stash = self.stash.clone();
        for record in self.classes.values_mut() {
            record
                .equipped
                .retain(|slot, id| stash.iter().any(|g| g.id == *id && g.slot() == *slot));
        }
        if self.docked {
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

    // -- classes ------------------------------------------------------------

    pub fn class_record(&self, class: ClassId) -> ClassRecord {
        self.classes.get(&class).cloned().unwrap_or_default()
    }

    /// Class level and XP into that level.
    pub fn class_level(&self, class: ClassId) -> (u32, u32) {
        class_level(self.class_record(class).xp)
    }

    /// Weapons the class may start with: its signature, familiar weapons
    /// unlocked by level, and familiar weapons found on dives.
    pub fn start_options(&self, class: ClassId) -> Vec<WeaponId> {
        let (level, _) = self.class_level(class);
        let mut options = vec![class.signature()];
        let found = self.class_record(class).found_weapons;
        for w in class
            .level_unlocks(level)
            .chain(found.into_iter().filter(|&w| class.is_familiar(w)))
        {
            if !options.contains(&w) {
                options.push(w);
            }
        }
        options
    }

    /// The weapon the class starts dives with.
    pub fn start_weapon(&self, class: ClassId) -> WeaponId {
        self.class_record(class)
            .start_weapon
            .filter(|w| self.start_options(class).contains(w))
            .unwrap_or_else(|| class.signature())
    }

    /// The active class as it drops.
    pub fn loadout(&self) -> Loadout {
        Loadout {
            class: self.class,
            class_level: self.class_level(self.class).0,
            start_weapon: self.start_weapon(self.class),
            gear: self
                .equipped(self.class)
                .into_iter()
                .flat_map(|g| g.stats())
                .collect(),
            // Blueprints only for familiar weapons not yet startable.
            blueprints: {
                let options = self.start_options(self.class);
                self.class
                    .familiar()
                    .filter(|w| !options.contains(w))
                    .collect()
            },
        }
    }

    pub fn item(&self, id: u32) -> Option<&Gear> {
        self.stash.iter().find(|g| g.id == id)
    }

    /// Gear a class wears, in slot order.
    pub fn equipped(&self, class: ClassId) -> Vec<&Gear> {
        let record = self.classes.get(&class);
        GearSlot::ALL
            .iter()
            .filter_map(|s| record.and_then(|r| r.equipped.get(s)))
            .filter_map(|id| self.item(*id))
            .collect()
    }

    /// The class wearing an item, if any.
    pub fn wearer(&self, id: u32) -> Option<ClassId> {
        self.classes
            .iter()
            .find(|(_, r)| r.equipped.values().any(|e| *e == id))
            .map(|(c, _)| *c)
    }

    /// Put an item on the active class (taking it off any other class).
    pub fn equip(&mut self, id: u32) -> bool {
        let Some(slot) = self.item(id).map(Gear::slot) else {
            return false;
        };
        for record in self.classes.values_mut() {
            record.equipped.retain(|_, e| *e != id);
        }
        self.classes
            .entry(self.class)
            .or_default()
            .equipped
            .insert(slot, id);
        true
    }

    pub fn unequip(&mut self, slot: GearSlot) {
        if let Some(record) = self.classes.get_mut(&self.class) {
            record.equipped.remove(&slot);
        }
    }

    /// Bank a find in the stash. A full stash salvages it for credits
    /// instead; returns the new item id, or `None` when salvaged.
    pub fn add_gear(&mut self, mut item: Gear) -> Option<u32> {
        if self.stash.len() >= content().tuning.equipment.stash_size {
            self.credits += item.sell_value();
            return None;
        }
        item.id = self.next_gear;
        self.next_gear += 1;
        self.stash.push(item);
        Some(self.next_gear - 1)
    }

    /// Remove an item from the stash (and from whoever wears it).
    fn take(&mut self, id: u32) -> Option<Gear> {
        let i = self.stash.iter().position(|g| g.id == id)?;
        for record in self.classes.values_mut() {
            record.equipped.retain(|_, e| *e != id);
        }
        Some(self.stash.remove(i))
    }

    /// Sell an item at the station. Returns the credits paid.
    pub fn sell(&mut self, id: u32) -> Option<u32> {
        let item = self.take(id)?;
        let value = item.sell_value();
        self.credits += value;
        Some(value)
    }

    /// Buy item `index` off the station's shelf.
    pub fn buy(&mut self, index: usize) -> Result<u32, TradeError> {
        let item = self.market.get(index).ok_or(TradeError::Missing)?;
        let price = item.price();
        if self.credits < price {
            return Err(TradeError::Credits);
        }
        if self.stash.len() >= content().tuning.equipment.stash_size {
            return Err(TradeError::StashFull);
        }
        self.credits -= price;
        let item = self.market.remove(index);
        Ok(self.add_gear(item).unwrap_or(0))
    }

    /// Reroll an item's affixes for credits.
    pub fn reroll(&mut self, id: u32, rng: &mut quasar_core::Rng) -> Result<(), TradeError> {
        let cost = self
            .item(id)
            .map(|g| content().tuning.trade.reroll[g.rarity.index()])
            .ok_or(TradeError::Missing)?;
        if self.credits < cost {
            return Err(TradeError::Credits);
        }
        self.credits -= cost;
        if let Some(g) = self.stash.iter_mut().find(|g| g.id == id) {
            gear::reroll(rng, g);
        }
        Ok(())
    }

    /// Unworn items of `rarity`, weakest first: what fusing would consume.
    pub fn fusible(&self, rarity: Rarity) -> Vec<u32> {
        let mut items: Vec<&Gear> = self
            .stash
            .iter()
            .filter(|g| g.rarity == rarity && self.wearer(g.id).is_none())
            .collect();
        items.sort_by(|a, b| a.score().total_cmp(&b.score()).then(a.id.cmp(&b.id)));
        items.into_iter().map(|g| g.id).collect()
    }

    /// Fuse the weakest unworn items of `rarity` into one of the next.
    pub fn fuse(&mut self, rarity: Rarity, rng: &mut quasar_core::Rng) -> Result<u32, TradeError> {
        let t = &content().tuning.trade;
        if rarity == Rarity::Legendary {
            return Err(TradeError::Missing);
        }
        let ids: Vec<u32> = self.fusible(rarity).into_iter().take(t.fuse).collect();
        if ids.len() < t.fuse {
            return Err(TradeError::Missing);
        }
        let cost = t.fuse_cost[rarity.index()];
        if self.credits < cost {
            return Err(TradeError::Credits);
        }
        self.credits -= cost;
        let items: Vec<Gear> = ids.iter().filter_map(|id| self.take(*id)).collect();
        let fused = gear::fuse(rng, &items).ok_or(TradeError::Missing)?;
        Ok(self.add_gear(fused).unwrap_or(0))
    }

    /// Trade one lot of a material for credits (or back).
    pub fn trade_material(&mut self, material: Material, buy: bool) -> Result<(), TradeError> {
        let t = &content().tuning.trade;
        let i = Material::ALL
            .iter()
            .position(|m| *m == material)
            .unwrap_or(0);
        let mut lot = Materials::default();
        lot.add(material, t.lot);
        if buy {
            if self.credits < t.material_buy[i] {
                return Err(TradeError::Credits);
            }
            self.credits -= t.material_buy[i];
            self.materials.add_all(lot);
        } else {
            if !self.materials.spend(lot) {
                return Err(TradeError::Materials);
            }
            self.credits += t.material_sell[i];
        }
        Ok(())
    }

    /// Restock the station's shelf (on docking), at the best hazard
    /// unlocked.
    pub fn restock(&mut self, rng: &mut quasar_core::Rng) {
        let t = &content().tuning;
        self.market = (0..t.trade.stock)
            .map(|_| gear::roll(rng, self.hazard_unlocked, Rarity::Uncommon, 0.0))
            .collect();
    }

    /// A familiar weapon `class` has not found yet, if any (for blueprint
    /// finds), in data order.
    pub fn unfound_weapon(&self, class: ClassId) -> Option<WeaponId> {
        let record = self.class_record(class);
        class.familiar().find(|w| !record.found_weapons.contains(w))
    }

    /// Record a weapon blueprint found by `class`.
    pub fn find_weapon(&mut self, class: ClassId, weapon: WeaponId) {
        let record = self.classes.entry(class).or_default();
        if !record.found_weapons.contains(&weapon) {
            record.found_weapons.push(weapon);
        }
    }

    /// Total of one effect over a class's gear (for the locker readout).
    pub fn gear_total(&self, class: ClassId, effect: PerkEffect) -> f32 {
        self.equipped(class)
            .into_iter()
            .flat_map(|g| g.stats())
            .filter(|(e, _)| *e == effect)
            .fold(
                if effect.multiplicative() { 1.0 } else { 0.0 },
                |acc, (_, a)| {
                    if effect.multiplicative() {
                        acc * a
                    } else {
                        acc + a
                    }
                },
            )
    }

    /// Choose the starting weapon for the active class. Returns whether it
    /// is allowed.
    pub fn set_start_weapon(&mut self, weapon: WeaponId) -> bool {
        let class = self.class;
        if !self.start_options(class).contains(&weapon) {
            return false;
        }
        self.classes.entry(class).or_default().start_weapon =
            (weapon != class.signature()).then_some(weapon);
        true
    }

    /// Add class XP; returns the class level before and after.
    pub fn award_class_xp(&mut self, class: ClassId, xp: u32) -> (u32, u32) {
        let before = self.class_level(class).0;
        let record = self.classes.entry(class).or_default();
        record.xp = record.xp.saturating_add(xp);
        (before, self.class_level(class).0)
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
            class: ClassId::starter(),
            classes: BTreeMap::new(),
            hazard_unlocked: 1,
            hazard_choice: 1,
            board: Vec::new(),
            ..Profile::default()
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
    fn stash_equipment_and_trading() {
        use crate::content::Rarity;
        let mut rng = quasar_core::Rng::seed_from_u64(3);
        let mut p = Profile::default();
        let a = p
            .add_gear(gear::roll(&mut rng, 1, Rarity::Common, 0.0))
            .unwrap();
        let slot = p.item(a).unwrap().slot();
        assert!(p.equip(a));
        assert_eq!(p.equipped(p.class).len(), 1);
        assert_eq!(p.wearer(a), Some(p.class));
        let stats = p.loadout().gear;
        assert_eq!(stats, p.item(a).unwrap().stats());
        // Equipping on another class moves the item.
        let other = ClassId::all().find(|c| *c != p.class).unwrap();
        let first = p.class;
        p.class = other;
        assert!(p.equip(a));
        assert!(p.equipped(first).is_empty());
        assert_eq!(p.wearer(a), Some(other));
        p.unequip(slot);
        assert_eq!(p.wearer(a), None);
        // Selling pays credits; rerolling and buying spend them.
        let paid = p.sell(a).unwrap();
        assert_eq!(p.credits, paid);
        assert!(p.stash.is_empty());
        p.restock(&mut rng);
        assert_eq!(p.market.len(), content().tuning.trade.stock);
        assert_eq!(p.buy(0), Err(TradeError::Credits));
        p.credits = 10_000;
        let bought = p.buy(0).unwrap();
        assert_eq!(p.market.len(), content().tuning.trade.stock - 1);
        let before = p.item(bought).unwrap().affixes.clone();
        let credits = p.credits;
        p.reroll(bought, &mut rng).unwrap();
        assert!(p.credits < credits);
        assert_eq!(p.item(bought).unwrap().affixes.len(), before.len());
        // Fusing three commons gives one uncommon.
        for _ in 0..3 {
            let mut g = gear::roll(&mut rng, 1, Rarity::Common, 0.0);
            g.rarity = Rarity::Common;
            p.add_gear(g);
        }
        let n = p.stash.len();
        let fused = p.fuse(Rarity::Common, &mut rng).unwrap();
        assert_eq!(p.stash.len(), n - 2);
        assert_eq!(p.item(fused).unwrap().rarity, Rarity::Uncommon);
        // Materials for credits and back.
        assert_eq!(
            p.trade_material(Material::Lumite, false),
            Err(TradeError::Materials)
        );
        p.trade_material(Material::Lumite, true).unwrap();
        assert_eq!(p.materials.lumite, content().tuning.trade.lot);
        p.trade_material(Material::Lumite, false).unwrap();
        assert_eq!(p.materials.lumite, 0);
        // A full stash salvages finds for credits.
        while p.stash.len() < content().tuning.equipment.stash_size {
            p.add_gear(gear::roll(&mut rng, 1, Rarity::Common, 0.0));
        }
        let credits = p.credits;
        assert_eq!(
            p.add_gear(gear::roll(&mut rng, 1, Rarity::Common, 0.0)),
            None
        );
        assert!(p.credits > credits);
        // Round trip, with ids and equipment intact.
        p.equip(fused);
        let json = serde_json::to_string(&p).unwrap();
        let back: Profile = serde_json::from_str(&json).unwrap();
        assert_eq!(back.clone().migrate().unwrap(), p);
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

    /// Each class keeps its own progress and loadout across switches.
    #[test]
    fn classes_keep_their_progress() {
        let warden = ClassId::from_key("warden").unwrap();
        let engineer = ClassId::from_key("engineer").unwrap();
        let mut p = Profile {
            class: warden,
            ..Profile::default()
        };
        assert_eq!(p.start_weapon(warden), warden.signature());
        // Level 2 unlocks a familiar weapon as a starting option.
        let (before, after) = p.award_class_xp(warden, 10_000);
        assert_eq!(before, 1);
        assert!(after >= 5);
        let options = p.start_options(warden);
        assert!(options.len() > 1);
        assert!(p.set_start_weapon(options[1]));
        // A weapon the class is not familiar with is refused.
        let foreign = WeaponId::all().find(|&w| !warden.is_familiar(w)).unwrap();
        assert!(!p.set_start_weapon(foreign));
        p.class = engineer;
        assert_eq!(p.class_level(engineer).0, 1);
        p.class = warden;
        assert_eq!(p.start_weapon(warden), options[1]);
        let text = serde_json::to_string(&p).unwrap();
        let back: Profile = serde_json::from_str(&text).unwrap();
        assert_eq!(back.migrate().unwrap(), p);
        // Version 3 saves (no classes) start as the starter class.
        let v3: Profile = serde_json::from_str(r#"{"version":3}"#).unwrap();
        let v3 = v3.migrate().unwrap();
        assert_eq!(v3.class, ClassId::starter());
        assert!(v3.classes.is_empty());
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
