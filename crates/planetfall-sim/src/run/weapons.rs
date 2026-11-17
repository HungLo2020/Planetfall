//! Weapons: targeting, firing by behaviour archetype, projectiles and
//! orbiting drones. Numbers come from `weapons.toml`.

use super::{Beam, Blast, BlastKind, Projectile, ProjectileKind, Run};
use crate::content::{DT, WeaponBehavior, WeaponId, WeaponStats, seconds};
use crate::events::GameEvent;
use crate::math::{self, FloatExt, V2};
use quasar_spatial::Aabb;

impl Run {
    pub(super) fn rebuild_index(&mut self) {
        self.index.clear();
        for (i, e) in self.enemies.iter().enumerate() {
            if e.health <= 0.0 {
                continue;
            }
            let r = f64::from(e.kind.info().radius);
            let (x, z) = (f64::from(e.pos[0]), f64::from(e.pos[1]));
            if let Ok(bounds) = Aabb::new([x - r, z - r], [x + r, z + r]) {
                self.index.insert(i as u32, bounds);
            }
        }
    }

    pub(super) fn nearest_enemy(&self, from: V2, range: f32) -> Option<usize> {
        self.index
            .nearest([f64::from(from[0]), f64::from(from[1])], f64::from(range))
            .map(|n| n.id as usize)
            .filter(|&i| self.enemies[i].health > 0.0)
    }

    pub(super) fn enemies_near(&mut self, at: V2, radius: f32) -> Vec<usize> {
        self.scratch.clear();
        self.index.query_radius_into(
            [f64::from(at[0]), f64::from(at[1])],
            f64::from(radius),
            &mut self.scratch,
        );
        self.scratch
            .iter()
            .map(|&i| i as usize)
            .filter(|&i| self.enemies[i].health > 0.0)
            .collect()
    }

    pub(super) fn fire_weapons(&mut self, events: &mut Vec<GameEvent>) {
        for slot in 0..self.weapons.len() {
            let w = self.weapons[slot];
            self.weapons[slot].since_fired = w.since_fired.saturating_add(1);
            let info = w.kind.info();
            if info.behavior == WeaponBehavior::Orbit {
                continue;
            }
            let stats = w.kind.stats(w.level);
            let cooldown = (w.cooldown - DT).max(0.0);
            self.weapons[slot].cooldown = cooldown;
            if cooldown > 0.0 {
                continue;
            }
            let origin = self.player.pos;
            let Some(target) = self.nearest_enemy(origin, stats.range) else {
                continue;
            };
            let target_pos = self.enemies[target].pos;
            if !info.ignores_walls && !self.terrain.line_clear(origin, target_pos) {
                continue;
            }
            let dir = (target_pos - origin).normalize_or_zero();
            let damage = stats.damage * self.player.damage_multiplier;
            match info.behavior {
                WeaponBehavior::Bolt => {
                    for i in 0..stats.count {
                        let spread = (i as f32 - (stats.count as f32 - 1.0) * 0.5) * 0.08;
                        let aim = math::rotate(dir, spread);
                        self.spawn_projectile(
                            ProjectileKind::Bolt,
                            w.kind,
                            origin,
                            aim,
                            &stats,
                            damage,
                        );
                    }
                }
                WeaponBehavior::Scatter => {
                    for i in 0..stats.count {
                        let t = i as f32 / (stats.count - 1).max(1) as f32 - 0.5;
                        let spread = t * 0.75 + self.rng.range_f32(-0.04..0.04);
                        let aim = math::rotate(dir, spread);
                        self.spawn_projectile(
                            ProjectileKind::Pellet,
                            w.kind,
                            origin,
                            aim,
                            &stats,
                            damage,
                        );
                    }
                }
                WeaponBehavior::Arc => {
                    self.fire_arc(w.kind, origin, target, &stats, damage, events)
                }
                WeaponBehavior::Mortar => self.fire_mortar(w.kind, origin, target, &stats, damage),
                WeaponBehavior::Orbit => unreachable!("orbit weapons are handled by update_drones"),
            }
            let slot = &mut self.weapons[slot];
            slot.cooldown = stats.cooldown * self.player.cooldown_multiplier;
            slot.since_fired = 0;
            slot.aim = math::yaw_of(dir);
            events.push(GameEvent::Fired {
                weapon: w.kind,
                at: origin,
            });
        }
    }

    fn fire_arc(
        &mut self,
        weapon: WeaponId,
        origin: V2,
        target: usize,
        stats: &WeaponStats,
        damage: f32,
        events: &mut Vec<GameEvent>,
    ) {
        let mut points = vec![origin];
        let mut hit = vec![target];
        let mut last = target;
        self.damage_enemy(target, damage, events);
        points.push(self.enemies[target].pos);
        for _ in 1..stats.count {
            let from = self.enemies[last].pos;
            let next = self
                .enemies_near(from, stats.radius)
                .into_iter()
                .filter(|i| !hit.contains(i))
                .min_by(|&a, &b| {
                    self.enemies[a]
                        .pos
                        .distance_squared(from)
                        .total_cmp(&self.enemies[b].pos.distance_squared(from))
                });
            let Some(next) = next else { break };
            self.damage_enemy(next, damage * 0.85, events);
            points.push(self.enemies[next].pos);
            hit.push(next);
            last = next;
        }
        self.beams.push(Beam {
            weapon,
            points,
            ttl: 9,
        });
    }

    fn fire_mortar(
        &mut self,
        weapon: WeaponId,
        origin: V2,
        target: usize,
        stats: &WeaponStats,
        damage: f32,
    ) {
        for _ in 0..stats.count {
            let candidates = self.enemies_near(origin, stats.range);
            let pick = if candidates.is_empty() {
                target
            } else {
                candidates[self.rng.range_usize(0..candidates.len())]
            };
            let to = self.enemies[pick].pos;
            let total = seconds(stats.speed).max(1);
            self.projectiles.push(Projectile {
                kind: ProjectileKind::Mortar {
                    from: origin,
                    to,
                    total,
                },
                weapon,
                pos: origin,
                height: 1.2,
                vel: (to - origin) / stats.speed,
                damage,
                radius: stats.radius,
                age: 0,
                ttl: total,
                pierce: 0,
                hit: Vec::new(),
            });
        }
    }

    fn spawn_projectile(
        &mut self,
        kind: ProjectileKind,
        weapon: WeaponId,
        origin: V2,
        dir: V2,
        stats: &WeaponStats,
        damage: f32,
    ) {
        self.projectiles.push(Projectile {
            kind,
            weapon,
            pos: origin + dir * 0.6,
            height: 1.1,
            vel: dir * stats.speed,
            damage,
            radius: stats.radius,
            age: 0,
            ttl: (stats.range / stats.speed * 60.0) as u32 + 2,
            pierce: stats.pierce,
            hit: Vec::new(),
        });
    }

    pub(super) fn update_projectiles(&mut self, events: &mut Vec<GameEvent>) {
        let mut projectiles = std::mem::take(&mut self.projectiles);
        projectiles.retain_mut(|p| {
            p.age += 1;
            match p.kind {
                ProjectileKind::Mortar { from, to, total } => {
                    let t = (p.age as f32 / total as f32).min(1.0);
                    p.pos = V2::new(from[0].lerp(to[0], t), from[1].lerp(to[1], t));
                    p.height = 1.2 + 6.0 * t * (1.0 - t) - 1.2 * t;
                    if p.age < total {
                        return true;
                    }
                    self.blasts.push(Blast {
                        kind: BlastKind::Weapon(p.weapon),
                        pos: to,
                        radius: p.radius,
                        ttl: 22,
                        total: 22,
                    });
                    for i in self.enemies_near(to, p.radius) {
                        self.damage_enemy(i, p.damage, events);
                    }
                    events.push(GameEvent::Explosion {
                        at: to,
                        radius: p.radius,
                    });
                    false
                }
                ProjectileKind::Bolt | ProjectileKind::Pellet => {
                    p.pos += p.vel * DT;
                    if p.age > p.ttl {
                        return false;
                    }
                    if self.terrain.solid_at(p.pos) {
                        events.push(GameEvent::Impact {
                            weapon: p.weapon,
                            at: p.pos,
                            height: p.height,
                        });
                        return false;
                    }
                    for i in self.enemies_near(p.pos, p.radius + 1.0) {
                        let e = self.enemies[i];
                        if p.hit.contains(&e.id)
                            || e.pos.distance(p.pos) > p.radius + e.kind.info().radius
                        {
                            continue;
                        }
                        p.hit.push(e.id);
                        self.damage_enemy(i, p.damage, events);
                        if p.pierce == 0 {
                            return false;
                        }
                        p.pierce -= 1;
                    }
                    true
                }
            }
        });
        projectiles.append(&mut self.projectiles);
        self.projectiles = projectiles;
    }

    /// The equipped orbit weapon, if any.
    fn orbit_weapon(&self) -> Option<(WeaponId, u8)> {
        self.weapons
            .iter()
            .find(|w| w.kind.info().behavior == WeaponBehavior::Orbit)
            .map(|w| (w.kind, w.level))
    }

    /// World positions of the orbit drones, if equipped.
    pub fn drone_positions(&self) -> Vec<V2> {
        let Some((kind, level)) = self.orbit_weapon() else {
            return Vec::new();
        };
        let stats = kind.stats(level);
        (0..stats.count)
            .map(|i| {
                let a = self.drone_angle + i as f32 * std::f32::consts::TAU / stats.count as f32;
                self.player.pos + V2::new(a.sin(), a.cos()) * stats.range
            })
            .collect()
    }

    pub(super) fn update_drones(&mut self, events: &mut Vec<GameEvent>) {
        let Some((kind, level)) = self.orbit_weapon() else {
            return;
        };
        let stats = kind.stats(level);
        self.drone_angle = (self.drone_angle + stats.speed * DT) % std::f32::consts::TAU;
        let damage = stats.damage * self.player.damage_multiplier;
        let mut struck = false;
        for drone in self.drone_positions() {
            for i in self.enemies_near(drone, stats.radius + 1.0) {
                let e = self.enemies[i];
                if e.drone_cooldown > 0
                    || e.pos.distance(drone) > stats.radius + e.kind.info().radius
                {
                    continue;
                }
                self.enemies[i].drone_cooldown =
                    seconds(stats.cooldown * self.player.cooldown_multiplier);
                self.damage_enemy(i, damage, events);
                struck = true;
            }
        }
        if struck && let Some(w) = self.weapons.iter_mut().find(|w| w.kind == kind) {
            if w.since_fired > 20 {
                events.push(GameEvent::Fired {
                    weapon: kind,
                    at: self.player.pos,
                });
            }
            w.since_fired = 0;
        }
    }

    pub(super) fn damage_enemy(&mut self, i: usize, amount: f32, events: &mut Vec<GameEvent>) {
        let e = &mut self.enemies[i];
        if e.health <= 0.0 {
            return;
        }
        e.health -= amount;
        e.hit_ticks = 6;
        // Deaths are resolved in `remove_dead`.
        if e.health > 0.0 {
            events.push(GameEvent::EnemyHit { at: e.pos });
        }
    }
}
