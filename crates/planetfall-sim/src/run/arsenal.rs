//! The newer weapon archetypes: flamer (cone with heat), ricochet discs,
//! proximity mines, melee sweeps, homing missiles and gravity vortices.
//! Firing goes through [`Run::fire`] like every weapon; their projectiles
//! update here. Numbers come from `weapons.toml` and `[arsenal]`.

use super::weapons::Shot;
use super::{Hit, Projectile, ProjectileKind, Run};
use crate::content::{DT, WeaponBehavior, content, seconds};
use crate::events::GameEvent;
use crate::math::{self, V2};

impl Run {
    /// Fire one of the newer archetypes. Returns false when it did not
    /// apply (the caller handles the classic ones).
    pub(super) fn fire_arsenal(
        &mut self,
        shot: &Shot,
        origin: V2,
        dir: V2,
        target: usize,
        events: &mut Vec<GameEvent>,
    ) {
        let stats = shot.stats;
        let t = &content().tuning.arsenal;
        match shot.weapon.info().behavior {
            WeaponBehavior::Flamer => {
                let cone = t.flamer_cone;
                for i in self.enemies_near(origin, stats.range) {
                    let to = self.enemies[i].pos - origin;
                    let angle = math::angle_between(dir, to.normalize_or_zero());
                    if angle <= cone && self.terrain.line_clear(origin, self.enemies[i].pos) {
                        self.damage_enemy(i, shot.hit, events);
                    }
                }
                // Fire through a spore cloud sets it off.
                self.ignite_spores(origin + dir * stats.range * 0.6, stats.range * 0.6, events);
            }
            WeaponBehavior::Ricochet => {
                self.spawn_projectile(
                    ProjectileKind::Ricochet {
                        bounces: stats.count,
                    },
                    shot,
                    origin,
                    dir,
                );
            }
            WeaponBehavior::Mines => {
                let live = self
                    .projectiles
                    .iter()
                    .filter(|p| p.kind == ProjectileKind::Mine && p.weapon == shot.weapon)
                    .count() as u32;
                if live < stats.count {
                    let mut p = self.make_projectile(ProjectileKind::Mine, shot, origin, V2::ZERO);
                    p.pos = origin;
                    p.height = 0.15;
                    p.ttl = seconds(t.mine_life);
                    self.projectiles.push(p);
                }
            }
            WeaponBehavior::Melee => {
                for i in self.enemies_near(origin, stats.range + 0.8) {
                    let e = self.enemies[i];
                    let to = e.pos - origin;
                    let reach = stats.range + e.kind.info().radius;
                    if to.length() > reach
                        || math::angle_between(dir, to.normalize_or_zero()) > t.melee_arc
                    {
                        continue;
                    }
                    self.damage_enemy(i, shot.hit, events);
                    // Knock the survivors back (bosses barely move).
                    if self.enemies[i].health > 0.0 {
                        let k = if e.kind.info().boss { 0.15 } else { 1.0 };
                        let push = to.normalize_or_zero() * t.melee_knockback * k;
                        let radius = e.kind.info().radius * 0.9;
                        let (pos, _) = self.terrain.resolve_circle(e.pos + push, radius);
                        if !e.flying() {
                            self.enemies[i].pos = pos;
                        }
                    }
                }
            }
            WeaponBehavior::Homing => {
                let near = self.enemies_near(origin, stats.range);
                for n in 0..stats.count {
                    let pick = if near.is_empty() {
                        target
                    } else {
                        near[n as usize % near.len()]
                    };
                    let spread = (n as f32 - (stats.count as f32 - 1.0) * 0.5) * 0.5;
                    let aim = math::rotate(dir, spread);
                    let id = self.enemies[pick].id;
                    self.spawn_projectile(ProjectileKind::Homing { target: id }, shot, origin, aim);
                }
            }
            WeaponBehavior::Gravity => {
                let to = self.enemies[target].pos;
                self.spawn_projectile(ProjectileKind::Well { to, active: 0 }, shot, origin, dir);
            }
            _ => {}
        }
    }

    pub(super) fn make_projectile(
        &self,
        kind: ProjectileKind,
        shot: &Shot,
        origin: V2,
        dir: V2,
    ) -> Projectile {
        let stats = &shot.stats;
        Projectile {
            kind,
            weapon: shot.weapon,
            pos: origin + dir * 0.6,
            height: 1.1,
            vel: dir * stats.speed,
            damage: shot.hit.damage,
            heavy: shot.hit.heavy,
            elements: shot.hit.elements,
            cluster: false,
            radius: stats.radius,
            age: 0,
            ttl: (stats.range / stats.speed.max(0.1) * 60.0) as u32 + 2,
            pierce: stats.pierce,
            hit: Vec::new(),
        }
    }

    /// One tick of a newer projectile; false removes it.
    pub(super) fn update_arsenal_projectile(
        &mut self,
        p: &mut Projectile,
        hit: Hit,
        events: &mut Vec<GameEvent>,
    ) -> bool {
        let t = &content().tuning.arsenal;
        match p.kind {
            ProjectileKind::Ricochet { bounces } => {
                let next = p.pos + p.vel * DT;
                if p.age > p.ttl * 2 {
                    return false;
                }
                if self.terrain.solid_at(next) {
                    // Bounce off the wall: flip whichever axis is blocked.
                    if bounces == 0 {
                        events.push(GameEvent::Impact {
                            weapon: p.weapon,
                            at: p.pos,
                            height: p.height,
                        });
                        return false;
                    }
                    let mut v = p.vel;
                    if self.terrain.solid_at(p.pos + V2::new(v[0] * DT, 0.0)) {
                        v[0] = -v[0];
                    }
                    if self.terrain.solid_at(p.pos + V2::new(0.0, v[1] * DT)) {
                        v[1] = -v[1];
                    }
                    if v == p.vel {
                        v = -v;
                    }
                    p.vel = v;
                    p.kind = ProjectileKind::Ricochet {
                        bounces: bounces - 1,
                    };
                    return true;
                }
                p.pos = next;
                for i in self.enemies_near(p.pos, p.radius + 1.0) {
                    let e = self.enemies[i];
                    if p.hit.last() == Some(&e.id)
                        || e.pos.distance(p.pos) > p.radius + e.kind.info().radius
                    {
                        continue;
                    }
                    p.hit.push(e.id);
                    self.damage_enemy(i, hit, events);
                    if bounces == 0 {
                        return false;
                    }
                    p.kind = ProjectileKind::Ricochet {
                        bounces: bounces - 1,
                    };
                    // On to the nearest other enemy in sight.
                    let speed = p.vel.length();
                    let from = p.pos;
                    let next = self
                        .enemies_near(from, t.ricochet_reach)
                        .into_iter()
                        .filter(|&j| self.enemies[j].id != e.id)
                        .filter(|&j| self.terrain.line_clear(from, self.enemies[j].pos))
                        .min_by(|&a, &b| {
                            self.enemies[a]
                                .pos
                                .distance_squared(from)
                                .total_cmp(&self.enemies[b].pos.distance_squared(from))
                        });
                    if let Some(j) = next {
                        p.vel = (self.enemies[j].pos - from).normalize_or_zero() * speed;
                        p.age = 0;
                    }
                    break;
                }
                true
            }
            ProjectileKind::Mine => {
                if p.age > p.ttl {
                    return false;
                }
                if p.age < seconds(t.mine_arm) {
                    return true;
                }
                let tripped = self.enemies_near(p.pos, p.radius).into_iter().any(|i| {
                    let e = &self.enemies[i];
                    !e.flying() && e.pos.distance(p.pos) < p.radius * 0.45 + e.kind.info().radius
                });
                if tripped {
                    self.burst(p.weapon, p.pos, p.radius, hit, events);
                    return false;
                }
                true
            }
            ProjectileKind::Homing { target } => {
                if p.age > p.ttl * 2 {
                    return false;
                }
                let goal = self
                    .enemies
                    .iter()
                    .find(|e| e.id == target && e.health > 0.0 && !e.burrowed)
                    .map(|e| e.pos)
                    .or_else(|| {
                        self.nearest_enemy(p.pos, 12.0).map(|i| {
                            let e = self.enemies[i];
                            p.kind = ProjectileKind::Homing { target: e.id };
                            e.pos
                        })
                    });
                if let Some(goal) = goal {
                    let speed = p.vel.length();
                    let want = math::yaw_of(goal - p.pos);
                    let yaw = math::turn_toward(math::yaw_of(p.vel), want, t.homing_turn * DT);
                    p.vel = math::from_yaw(yaw) * speed;
                }
                p.pos += p.vel * DT;
                let struck = self.terrain.solid_at(p.pos)
                    || self.enemies_near(p.pos, 1.5).into_iter().any(|i| {
                        let e = &self.enemies[i];
                        e.pos.distance(p.pos) < 0.35 + e.kind.info().radius
                    });
                if struck {
                    let at = p.pos - p.vel * DT;
                    self.burst(p.weapon, at, p.radius, hit, events);
                    return false;
                }
                true
            }
            ProjectileKind::Well { to, active } => {
                if active == 0 {
                    let step = p.vel * DT;
                    let arrived = p.pos.distance(to) <= step.length().max(0.2)
                        || self.terrain.solid_at(p.pos + step);
                    if arrived {
                        if !self.terrain.solid_at(p.pos + step) {
                            p.pos = to;
                        }
                        p.kind = ProjectileKind::Well {
                            to,
                            active: seconds(t.gravity_seconds),
                        };
                        events.push(GameEvent::Explosion {
                            at: p.pos,
                            radius: p.radius * 0.5,
                        });
                    } else {
                        p.pos += step;
                    }
                    return p.age < p.ttl * 3;
                }
                // Pull everything in and grind it four times a second.
                let grind = p.age.is_multiple_of(15);
                let center = p.pos;
                for i in self.enemies_near(center, p.radius) {
                    let e = self.enemies[i];
                    if e.kind.info().boss {
                        continue;
                    }
                    let to = center - e.pos;
                    let d = to.length();
                    if d > 0.4 {
                        let step = to / d * (t.gravity_pull * DT).min(d - 0.3);
                        let (pos, _) = self
                            .terrain
                            .resolve_circle(e.pos + step, e.kind.info().radius * 0.9);
                        self.enemies[i].pos = pos;
                    }
                    if grind {
                        self.damage_enemy(i, hit, events);
                    }
                }
                p.kind = ProjectileKind::Well {
                    to,
                    active: active - 1,
                };
                active > 1
            }
            _ => true,
        }
    }

    /// Flamer heat: cool down every tick; returns whether it may fire.
    pub(super) fn flamer_ready(&mut self, player: usize, slot: usize) -> bool {
        let t = &content().tuning.arsenal;
        let w = &mut self.players[player].weapons[slot];
        w.heat = (w.heat - t.flamer_cooling * DT).max(0.0);
        if w.overheated && w.heat <= 0.0 {
            w.overheated = false;
        }
        !w.overheated
    }

    /// Flamer heat after a burst; full heat locks it.
    pub(super) fn flamer_heat(&mut self, player: usize, slot: usize) {
        let t = &content().tuning.arsenal;
        let w = &mut self.players[player].weapons[slot];
        w.heat = (w.heat + t.flamer_heat).min(1.0);
        if w.heat >= 1.0 {
            w.overheated = true;
        }
    }
}
