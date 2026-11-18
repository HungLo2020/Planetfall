//! Loot and progression within a drop: pickups, supply caches and upgrade
//! offers.
//!
//! - Dive levels in `tuning.weapons.pick_levels` offer a choice of new
//!   weapon (familiar weapons more often) while a slot is free.
//! - Every other level-up offers three upgrades, each rolled at a rarity:
//!   weapon mods and ability mods (`mods.toml`) and field or class perks
//!   (`perks.toml`). Luck and planet hazard push the odds toward rarer
//!   offers; elements need rare and behaviour changes legendary.
//! - A supply cache offers three upgrades at rare or better, and holds a
//!   find: gear at uncommon or better, or a familiar weapon blueprint.
//! - Elites sometimes drop gear; bosses and hunted elites always do.
//! - Finds go to the stash when the drop ends, whatever the outcome.

use super::{OfferSource, Pickup, PickupKind, Player, Run, RunState, WeaponSlot};
use crate::content::{
    ClassId, DT, ModId, ModTarget, Offer, PerkId, Rarity, Upgrade, WeaponId, content,
};
use crate::events::GameEvent;
use crate::gear::{self, Gear};
use crate::math::{FloatExt, V2};
use serde::{Deserialize, Serialize};

/// Something found on a dive that survives death.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Find {
    Gear(Gear),
    /// A familiar weapon blueprint: a starting-weapon option for the class.
    Weapon {
        class: ClassId,
        weapon: WeaponId,
    },
}

impl Find {
    pub fn name(&self) -> String {
        match self {
            Self::Gear(g) => format!("{} {}", g.rarity.name(), g.name()),
            Self::Weapon { weapon, .. } => format!("{} blueprint", weapon.name()),
        }
    }
}

/// Ticks a find stays on the HUD.
const FIND_BANNER: u32 = 240;

/// Offers per choice.
const CHOICES: usize = 3;

impl Run {
    pub(super) fn drop_pickup(&mut self, kind: PickupKind, at: V2) {
        let angle = self.rng.range_f32(0.0..std::f32::consts::TAU);
        let speed = self.rng.range_f32(1.0..3.0);
        self.pickups.push(Pickup {
            kind,
            pos: at,
            height: 0.6,
            vel: V2::new(angle.sin(), angle.cos()) * speed,
            vy: self.rng.range_f32(3.0..5.0),
            magnet: false,
            age: 0,
        });
    }

    pub(super) fn update_pickups(&mut self, events: &mut Vec<GameEvent>) {
        let mut collected = Vec::new();
        for (i, p) in self.pickups.iter_mut().enumerate() {
            p.age += 1;
            // Each pickup homes on the nearest living player.
            let Some((owner, player)) = self
                .players
                .iter()
                .enumerate()
                .filter(|(_, pl)| pl.alive())
                .min_by(|(_, a), (_, b)| {
                    a.pos
                        .distance_squared(p.pos)
                        .total_cmp(&b.pos.distance_squared(p.pos))
                })
            else {
                continue;
            };
            let to = player.pos - p.pos;
            let dist = to.length();
            if p.age > 20 && dist < player.pickup_range {
                p.magnet = true;
            }
            if p.magnet {
                let speed = 8.0 + p.age.min(240) as f32 * 0.05;
                p.pos += to.normalize_or_zero() * (speed * DT).min(dist);
                p.height = p.height.lerp(0.9, 0.2);
            } else {
                p.vy -= 18.0 * DT;
                p.height = (p.height + p.vy * DT).max(0.35);
                if p.height <= 0.35 {
                    p.vy = 0.0;
                    p.vel *= 0.8;
                }
                let next = p.pos + p.vel * DT;
                if !self.terrain.solid_at(next) {
                    p.pos = next;
                }
            }
            if dist < 0.75 && p.age > 10 {
                collected.push((i, owner));
            }
        }
        for &(i, owner) in collected.iter().rev() {
            let p = self.pickups.swap_remove(i);
            match p.kind {
                PickupKind::Xp(amount) => {
                    let player = &mut self.players[owner];
                    // Gear XP bonus, carrying the fraction to the next shard.
                    let exact = amount as f32 * (1.0 + player.xp_gain) + player.xp_fraction;
                    let amount = exact.floor() as u32;
                    player.xp_fraction = exact - amount as f32;
                    player.xp += amount;
                    player.xp_total += amount;
                    while player.xp >= player.xp_needed() {
                        player.xp -= player.xp_needed();
                        player.level += 1;
                        player.pending_levels += 1;
                    }
                    events.push(GameEvent::PickedUpXp);
                }
                PickupKind::Material(material, amount) => {
                    self.carried.add(material, amount);
                    events.push(GameEvent::PickedUpMaterial { material, amount });
                }
            }
        }
    }

    /// Pick from weighted choices with the run's RNG.
    fn weighted<T: Copy>(&mut self, choices: &[(T, u32)]) -> Option<T> {
        let total: u32 = choices.iter().map(|(_, w)| w).sum();
        if total == 0 {
            return None;
        }
        let mut roll = self.rng.range_u32(0..total);
        choices
            .iter()
            .find(|(_, w)| {
                if roll < *w {
                    true
                } else {
                    roll -= w;
                    false
                }
            })
            .map(|(c, _)| *c)
    }

    /// Equipment for `player` at the drop's hazard, at least `min` rare.
    pub(super) fn find_gear(&mut self, player: usize, min: Rarity, events: &mut Vec<GameEvent>) {
        let luck = self.players[player].luck;
        let item = gear::roll(&mut self.rng, self.mission.hazard, min, luck);
        events.push(GameEvent::GearFound {
            rarity: item.rarity,
            slot: item.slot(),
        });
        self.finds.push(Find::Gear(item));
        self.find_banner = FIND_BANNER;
    }

    /// A cache's find: sometimes a weapon blueprint the class still lacks,
    /// otherwise gear.
    fn cache_find(&mut self, player: usize, events: &mut Vec<GameEvent>) {
        let t = &content().tuning.equipment;
        // The tutorial's cache always has something to show.
        if !self.tutorial && !self.rng.chance(f64::from(t.cache_find)) {
            return;
        }
        let p = &self.players[player];
        let class = p.class;
        let left: Vec<WeaponId> = p
            .blueprints
            .iter()
            .copied()
            .filter(|w| {
                !self
                    .finds
                    .iter()
                    .any(|f| matches!(f, Find::Weapon { weapon, .. } if weapon == w))
            })
            .collect();
        if !left.is_empty() && !self.tutorial && self.rng.chance(f64::from(t.weapon_find)) {
            let weapon = left[self.rng.range_usize(0..left.len())];
            self.finds.push(Find::Weapon { class, weapon });
            self.find_banner = FIND_BANNER;
            events.push(GameEvent::BlueprintFound { weapon });
        } else {
            self.find_gear(player, t.cache_min, events);
        }
    }

    /// Walking into a supply cache queues a rare-or-better upgrade choice.
    pub(super) fn update_caches(&mut self, events: &mut Vec<GameEvent>) {
        let tuning = &content().tuning;
        for i in 0..self.caches.len() {
            let c = self.caches[i];
            if c.opened {
                self.caches[i].open_ticks += 1;
                continue;
            }
            let Some(player) = self
                .players
                .iter()
                .position(|p| p.alive() && c.pos.distance(p.pos) <= 1.9)
            else {
                continue;
            };
            self.caches[i].opened = true;
            self.caches_opened += 1;
            self.players[player].pending_caches += 1;
            self.drop_pickup(PickupKind::Xp(tuning.xp.cache_xp), c.pos);
            events.push(GameEvent::CacheOpened { at: c.pos });
            self.cache_find(player, events);
            // Caches carry spare air on thin-air drops.
            if self.thin_air() {
                let h = &content().tuning.hazards;
                let p = &mut self.players[player];
                p.oxygen = (p.oxygen + h.oxygen_cache).min(h.oxygen_max);
            }
        }
    }

    /// Rarity odds for a player: base weights shifted toward rarer tiers by
    /// luck and planet hazard. `min` zeroes everything below it.
    pub(super) fn rarity_weights(&self, player: usize, min: Rarity) -> [f32; 4] {
        let tuning = &content().tuning.rarity;
        let p = &self.players[player];
        let shift = (1.0 + p.luck)
            * (1.0 + tuning.hazard_bonus * (self.hazard - 1.0).max(0.0))
            * (1.0 + self.hazard_k(&content().tuning.hazard.rarity));
        let mut weights = tuning.weights;
        for (i, w) in weights.iter_mut().enumerate() {
            *w *= shift.powi(i as i32);
            if i < min.index() {
                *w = 0.0;
            }
        }
        weights
    }

    fn roll_rarity(&mut self, player: usize, min: Rarity) -> Rarity {
        let weights = self.rarity_weights(player, min);
        let total: f32 = weights.iter().sum();
        let mut roll = self.rng.next_f32() * total;
        for r in Rarity::ALL {
            roll -= weights[r.index()];
            if roll < 0.0 && weights[r.index()] > 0.0 {
                return r;
            }
        }
        Rarity::ALL
            .into_iter()
            .rev()
            .find(|r| weights[r.index()] > 0.0)
            .unwrap_or(Rarity::Common)
    }

    /// Upgrades `player` could take at `rarity`, with their weights.
    pub(super) fn upgrade_pool(&self, player: usize, rarity: Rarity) -> Vec<(Upgrade, u32)> {
        let p = &self.players[player];
        let mut pool = Vec::new();
        let ability = p.class.info().ability.kind;
        for m in ModId::all() {
            let info = m.info();
            if info.min_rarity > rarity {
                continue;
            }
            let room = |taken: u32| info.max_stacks.is_none_or(|max| taken < max);
            match info.target {
                ModTarget::Weapon => {
                    for w in &p.weapons {
                        let fits = info.behaviors.is_empty()
                            || info.behaviors.contains(&w.kind.info().behavior);
                        if fits && room(w.stacks(m)) {
                            pool.push((Upgrade::WeaponMod(w.kind, m), info.weight));
                        }
                    }
                }
                ModTarget::Ability => {
                    let fits = info.abilities.is_empty() || info.abilities.contains(&ability);
                    if fits && room(p.ability.stacks(m)) {
                        pool.push((Upgrade::AbilityMod(m), info.weight));
                    }
                }
            }
        }
        for perk in PerkId::all() {
            let info = perk.info();
            let for_class = match &info.class {
                Some(class) => class == p.class.key() && p.class_level >= info.class_level,
                None => true,
            };
            let capped = info
                .max
                .is_some_and(|max| p.effect_total(info.effect) >= max);
            if for_class && !capped && info.weight > 0 {
                pool.push((Upgrade::Field(perk), info.weight));
            }
        }
        pool
    }

    /// Three different upgrades, each at its own rolled rarity (never below
    /// `min`). A rarity with nothing to offer falls back to the next lower.
    fn roll_upgrades(&mut self, player: usize, min: Rarity) -> Vec<Offer> {
        let mut offers: Vec<Offer> = Vec::new();
        for _ in 0..CHOICES * 4 {
            if offers.len() == CHOICES {
                break;
            }
            let rolled = self.roll_rarity(player, min);
            for rarity in Rarity::ALL
                .into_iter()
                .rev()
                .filter(|r| *r <= rolled && *r >= min)
            {
                let pool: Vec<(Upgrade, u32)> = self
                    .upgrade_pool(player, rarity)
                    .into_iter()
                    .filter(|(u, _)| offers.iter().all(|o| o.upgrade != *u))
                    .collect();
                if let Some(upgrade) = self.weighted(&pool) {
                    offers.push(Offer { upgrade, rarity });
                    break;
                }
            }
        }
        offers
    }

    /// New weapons `player` could pick, weighted toward familiar ones.
    fn weapon_picks(&mut self, player: usize) -> Vec<Offer> {
        let familiar = content().tuning.weapons.familiar_weight;
        let p: &Player = &self.players[player];
        let mut pool: Vec<(WeaponId, u32)> = WeaponId::all()
            .filter(|w| !p.carries(*w))
            .map(|w| {
                let k = if p.class.is_familiar(w) {
                    familiar
                } else {
                    1.0
                };
                (w, (10.0 * k).round() as u32)
            })
            .collect();
        let mut offers = Vec::new();
        while offers.len() < CHOICES && !pool.is_empty() {
            let Some(pick) = self.weighted(&pool) else {
                break;
            };
            pool.retain(|(w, _)| *w != pick);
            offers.push(Offer {
                upgrade: Upgrade::NewWeapon(pick),
                rarity: Rarity::Common,
            });
        }
        offers
    }

    /// Put the next pending choice on screen: caches first, then levels.
    pub(super) fn offer_level_up(&mut self, events: &mut Vec<GameEvent>) {
        if !matches!(self.state, RunState::Active) {
            return;
        }
        let Some(player) = self
            .players
            .iter()
            .position(|p| p.alive() && (p.pending_caches > 0 || p.pending_levels > 0))
        else {
            return;
        };
        let tuning = &content().tuning;
        let p = &mut self.players[player];
        let (source, choices) = if p.pending_caches > 0 {
            p.pending_caches -= 1;
            let min = tuning.rarity.cache_min;
            (OfferSource::Cache, self.roll_upgrades(player, min))
        } else {
            // The level being offered (several can queue up at once).
            let level = p.level + 1 - p.pending_levels;
            p.pending_levels -= 1;
            let pick = tuning.weapons.pick_levels.contains(&level)
                && p.weapons.len() < tuning.weapons.max_carried;
            let picks = if pick {
                self.weapon_picks(player)
            } else {
                Vec::new()
            };
            if picks.is_empty() {
                (
                    OfferSource::LevelUp,
                    self.roll_upgrades(player, Rarity::Common),
                )
            } else {
                (OfferSource::WeaponPick, picks)
            }
        };
        if choices.is_empty() {
            return;
        }
        events.push(GameEvent::LevelUp {
            level: self.players[player].level,
        });
        self.state = RunState::LevelUp {
            player,
            choices,
            source,
        };
    }

    pub(super) fn apply_offer(&mut self, player: usize, offer: Offer, events: &mut Vec<GameEvent>) {
        let max = content().tuning.weapons.max_carried;
        let p = &mut self.players[player];
        match offer.upgrade {
            Upgrade::NewWeapon(kind) => {
                if !p.carries(kind) && p.weapons.len() < max {
                    p.weapons.push(WeaponSlot::new(kind, 0.2));
                    events.push(GameEvent::WeaponFound { weapon: kind });
                }
            }
            Upgrade::WeaponMod(kind, m) => {
                if let Some(w) = p.weapons.iter_mut().find(|w| w.kind == kind) {
                    w.mods.push((m, offer.rarity));
                }
            }
            Upgrade::AbilityMod(m) => {
                p.ability.mods.push((m, offer.rarity));
                p.refresh_ability();
            }
            Upgrade::Field(id) => {
                p.apply(id.info().effect, id.amount(offer.rarity));
            }
        }
        if offer.rarity >= Rarity::Rare {
            events.push(GameEvent::RareUpgrade {
                rarity: offer.rarity,
            });
        }
    }
}
