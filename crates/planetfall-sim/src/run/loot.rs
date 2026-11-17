//! Loot and progression within a drop: pickups, weapon caches, level-up
//! offers and perks (`perks.toml`, plus weapon choices from `weapons.toml`).
//! Weapons the player's class is familiar with are offered more often, and
//! class perks only to that class.

use super::{Pickup, PickupKind, Run, RunState, WeaponSlot};
use crate::content::{DT, Perk, PerkId, WeaponId, content};
use crate::events::GameEvent;
use crate::math::{FloatExt, V2};

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

    /// Weight for offering `weapon` to `player` (familiar weapons more often).
    fn weapon_weight(&self, player: usize, weapon: WeaponId, base: u32) -> u32 {
        let familiar = self.players[player].class.is_familiar(weapon);
        let k = if familiar {
            content().tuning.weapons.familiar_weight
        } else {
            1.0
        };
        ((base as f32 * k).round() as u32).max(1)
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

    /// Walking into a cache grants an unowned weapon (familiar ones are
    /// likelier), else upgrades one.
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
            let unowned: Vec<(WeaponId, u32)> = WeaponId::all()
                .filter(|k| self.players[player].weapon_level(*k).is_none())
                .map(|k| (k, self.weapon_weight(player, k, 1)))
                .collect();
            let carried = self.players[player].weapons.len();
            let weapon = match self.weighted(&unowned) {
                Some(kind) if carried < tuning.weapons.max_carried => {
                    self.players[player]
                        .weapons
                        .push(WeaponSlot::new(kind, 0.3));
                    kind
                }
                _ => {
                    let max = tuning.weapons.max_level;
                    let p = &mut self.players[player];
                    match p
                        .weapons
                        .iter_mut()
                        .filter(|w| w.level < max)
                        .min_by_key(|w| w.level)
                    {
                        Some(w) => {
                            w.level += 1;
                            w.kind
                        }
                        None => {
                            p.damage_multiplier += tuning.weapons.cache_overflow_damage;
                            p.weapons[0].kind
                        }
                    }
                }
            };
            self.drop_pickup(PickupKind::Xp(tuning.xp.cache_xp), c.pos);
            events.push(GameEvent::WeaponFound { weapon });
        }
    }

    /// The choices a level-up can offer `player` now, with their weights.
    pub(super) fn perk_pool(&self, player: usize) -> Vec<(Perk, u32)> {
        let tuning = &content().tuning.weapons;
        let p = &self.players[player];
        let mut pool = Vec::new();
        if p.weapons.len() < tuning.max_carried {
            for k in WeaponId::all().filter(|k| p.weapon_level(*k).is_none()) {
                pool.push((
                    Perk::NewWeapon(k),
                    self.weapon_weight(player, k, tuning.new_weapon_weight),
                ));
            }
        }
        for w in p.weapons.iter().filter(|w| w.level < tuning.max_level) {
            pool.push((Perk::WeaponLevel(w.kind), tuning.level_up_weight));
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
                pool.push((Perk::Field(perk), info.weight));
            }
        }
        pool
    }

    pub(super) fn offer_level_up(&mut self, events: &mut Vec<GameEvent>) {
        if !matches!(self.state, RunState::Active) {
            return;
        }
        let Some(player) = self
            .players
            .iter()
            .position(|p| p.alive() && p.pending_levels > 0)
        else {
            return;
        };
        self.players[player].pending_levels -= 1;
        let mut pool = self.perk_pool(player);
        let mut choices = Vec::new();
        while choices.len() < 3 && !pool.is_empty() {
            let Some(pick) = self.weighted(&pool) else {
                break;
            };
            let index = pool.iter().position(|(p, _)| *p == pick).unwrap_or(0);
            choices.push(pool.swap_remove(index).0);
        }
        events.push(GameEvent::LevelUp {
            level: self.players[player].level,
        });
        self.state = RunState::LevelUp { player, choices };
    }

    pub(super) fn apply_perk(&mut self, player: usize, perk: Perk, events: &mut Vec<GameEvent>) {
        let tuning = &content().tuning.weapons;
        let p = &mut self.players[player];
        match perk {
            Perk::NewWeapon(kind) => {
                if p.weapon_level(kind).is_none() && p.weapons.len() < tuning.max_carried {
                    p.weapons.push(WeaponSlot::new(kind, 0.2));
                    events.push(GameEvent::WeaponFound { weapon: kind });
                }
            }
            Perk::WeaponLevel(kind) => {
                if let Some(w) = p.weapons.iter_mut().find(|w| w.kind == kind) {
                    w.level = (w.level + 1).min(tuning.max_level);
                }
            }
            Perk::Field(id) => {
                let info = id.info();
                p.apply(info.effect, info.amount);
            }
        }
    }
}
