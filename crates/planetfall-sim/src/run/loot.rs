//! Loot and progression within a drop: pickups, weapon caches, level-up
//! offers and perks (`perks.toml`, plus weapon choices from `weapons.toml`).

use super::{Pickup, PickupKind, Run, RunState, WeaponSlot};
use crate::content::{DT, Perk, PerkEffect, PerkId, WeaponId, content};
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
        let player = self.player.pos;
        let range = self.player.pickup_range;
        let mut collected = Vec::new();
        for (i, p) in self.pickups.iter_mut().enumerate() {
            p.age += 1;
            let to = player - p.pos;
            let dist = to.length();
            if p.age > 20 && dist < range {
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
                collected.push(i);
            }
        }
        for &i in collected.iter().rev() {
            let p = self.pickups.swap_remove(i);
            match p.kind {
                PickupKind::Xp(amount) => {
                    self.player.xp += amount;
                    while self.player.xp >= self.player.xp_needed() {
                        self.player.xp -= self.player.xp_needed();
                        self.player.level += 1;
                        self.player.pending_levels += 1;
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

    /// Walking into a cache grants an unowned weapon, else upgrades one.
    pub(super) fn update_caches(&mut self, events: &mut Vec<GameEvent>) {
        let tuning = &content().tuning;
        for i in 0..self.caches.len() {
            let c = self.caches[i];
            if c.opened {
                self.caches[i].open_ticks += 1;
                continue;
            }
            if c.pos.distance(self.player.pos) > 1.9 {
                continue;
            }
            self.caches[i].opened = true;
            self.caches_opened += 1;
            let unowned: Vec<WeaponId> = WeaponId::all()
                .filter(|k| self.weapon_level(*k).is_none())
                .collect();
            let weapon = if self.weapons.len() < tuning.weapons.max_carried && !unowned.is_empty() {
                let kind = unowned[self.rng.range_usize(0..unowned.len())];
                self.weapons.push(WeaponSlot::new(kind, 0.3));
                kind
            } else {
                let max = tuning.weapons.max_level;
                match self
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
                        self.player.damage_multiplier += tuning.weapons.cache_overflow_damage;
                        self.weapons[0].kind
                    }
                }
            };
            self.drop_pickup(PickupKind::Xp(tuning.xp.cache_xp), c.pos);
            events.push(GameEvent::WeaponFound { weapon });
        }
    }

    /// The choices a level-up can offer now, with their weights.
    fn perk_pool(&self) -> Vec<(Perk, u32)> {
        let tuning = &content().tuning.weapons;
        let mut pool = Vec::new();
        if self.weapons.len() < tuning.max_carried {
            for k in WeaponId::all().filter(|k| self.weapon_level(*k).is_none()) {
                pool.push((Perk::NewWeapon(k), tuning.new_weapon_weight));
            }
        }
        for w in self.weapons.iter().filter(|w| w.level < tuning.max_level) {
            pool.push((Perk::WeaponLevel(w.kind), tuning.level_up_weight));
        }
        for perk in PerkId::all() {
            let info = perk.info();
            let capped = info
                .max
                .is_some_and(|max| self.perk_total(info.effect) >= max);
            if !capped && info.weight > 0 {
                pool.push((Perk::Field(perk), info.weight));
            }
        }
        pool
    }

    /// The current value of an additive perk effect (for caps).
    fn perk_total(&self, effect: PerkEffect) -> f32 {
        match effect {
            PerkEffect::Regeneration => self.player.regeneration,
            PerkEffect::PickupRange => self.player.pickup_range,
            PerkEffect::Mining => self.player.mining_multiplier,
            PerkEffect::Damage => self.player.damage_multiplier,
            PerkEffect::MaxHealth => self.player.max_health,
            PerkEffect::MoveSpeed => self.player.move_speed,
            PerkEffect::FireRate => self.player.cooldown_multiplier,
        }
    }

    pub(super) fn offer_level_up(&mut self, events: &mut Vec<GameEvent>) {
        if self.player.pending_levels == 0 || !matches!(self.state, RunState::Active) {
            return;
        }
        self.player.pending_levels -= 1;
        let mut pool = self.perk_pool();
        let mut choices = Vec::new();
        while choices.len() < 3 && !pool.is_empty() {
            let total: u32 = pool.iter().map(|(_, w)| w).sum();
            let mut roll = self.rng.range_u32(0..total);
            let index = pool
                .iter()
                .position(|(_, w)| {
                    if roll < *w {
                        true
                    } else {
                        roll -= w;
                        false
                    }
                })
                .unwrap_or(0);
            choices.push(pool.swap_remove(index).0);
        }
        events.push(GameEvent::LevelUp {
            level: self.player.level,
        });
        self.state = RunState::LevelUp(choices);
    }

    pub(super) fn apply_perk(&mut self, perk: Perk, events: &mut Vec<GameEvent>) {
        let tuning = &content().tuning.weapons;
        match perk {
            Perk::NewWeapon(kind) => {
                if self.weapon_level(kind).is_none() && self.weapons.len() < tuning.max_carried {
                    self.weapons.push(WeaponSlot::new(kind, 0.2));
                    events.push(GameEvent::WeaponFound { weapon: kind });
                }
            }
            Perk::WeaponLevel(kind) => {
                if let Some(w) = self.weapons.iter_mut().find(|w| w.kind == kind) {
                    w.level = (w.level + 1).min(tuning.max_level);
                }
            }
            Perk::Field(id) => {
                let info = id.info();
                let p = &mut self.player;
                let a = info.amount;
                match info.effect {
                    PerkEffect::MaxHealth => {
                        p.max_health += a;
                        p.health = (p.health + a).min(p.max_health);
                    }
                    PerkEffect::MoveSpeed => p.move_speed *= a,
                    PerkEffect::Damage => p.damage_multiplier += a,
                    PerkEffect::FireRate => p.cooldown_multiplier *= a,
                    PerkEffect::PickupRange => p.pickup_range += a,
                    PerkEffect::Mining => p.mining_multiplier += a,
                    PerkEffect::Regeneration => p.regeneration += a,
                }
            }
        }
    }
}
