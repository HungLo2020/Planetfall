//! Weapons: targeting, firing by behaviour archetype, projectiles and
//! orbiting drones, for every player, plus how hits land: heavy-enemy
//! bonuses, element statuses (burn, corrode, chill and freeze, shock arcs)
//! and legendary behaviour changes. Base numbers come from `weapons.toml`,
//! upgrades from `mods.toml`.

use super::{Beam, Blast, BlastKind, Elements, Hit, Projectile, ProjectileKind, Run};
use crate::content::{DT, Element, Special, WeaponBehavior, WeaponId, WeaponStats, seconds};
use crate::events::GameEvent;
use crate::math::{self, FloatExt, V2};
use quasar_spatial::Aabb;

/// Enemies with at least this base health count as heavy.
pub const HEAVY_HEALTH: f32 = 100.0;

/// Seconds a burn or corrosion lasts after the last hit.
const BURN_SECONDS: f32 = 3.0;
const CORRODE_SECONDS: f32 = 4.0;
/// Extra damage taken while corroded.
pub const CORRODE_BONUS: f32 = 0.25;
/// Chill gained per point of cold potency; at 1 the enemy freezes.
const CHILL_PER_POTENCY: f32 = 0.06;
const FREEZE_SECONDS: f32 = 1.6;
/// Chance per hit that shock arcs, and how far.
const SHOCK_CHANCE: f64 = 0.35;
const SHOCK_REACH: f32 = 3.5;
pub const SHOCK_COLOR: [f32; 3] = [0.65, 0.8, 1.0];

/// What one shot carries: the weapon's stats, elements and specials.
#[derive(Clone, Copy)]
pub(super) struct Shot {
    pub weapon: WeaponId,
    pub stats: WeaponStats,
    pub hit: Hit,
    pub twin: bool,
    pub double: bool,
    pub overload: bool,
    pub cluster: bool,
}

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
        for player in 0..self.players.len() {
            if self.players[player].alive() {
                self.fire_player_weapons(player, events);
            }
        }
    }

    fn fire_player_weapons(&mut self, player: usize, events: &mut Vec<GameEvent>) {
        for slot in 0..self.players[player].weapons.len() {
            let p = &self.players[player];
            let w = &p.weapons[slot];
            let (origin, multiplier, heavy, cooldown_multiplier) = (
                p.pos,
                p.damage_multiplier,
                p.heavy_damage,
                p.cooldown_multiplier,
            );
            let kind = w.kind;
            let info = kind.info();
            let stats = w.stats();
            let shot = Shot {
                weapon: kind,
                stats,
                hit: Hit {
                    damage: stats.damage * multiplier,
                    heavy,
                    elements: w.elements(),
                    secondary: false,
                },
                twin: w.has(Special::TwinBarrel),
                double: w.has(Special::DoubleBlast),
                overload: w.has(Special::Overload),
                cluster: w.has(Special::Cluster),
            };
            let since = w.since_fired;
            let cooldown = (w.cooldown - DT).max(0.0);
            let w = &mut self.players[player].weapons[slot];
            w.since_fired = since.saturating_add(1);
            if info.behavior == WeaponBehavior::Orbit {
                continue;
            }
            w.cooldown = cooldown;
            if cooldown > 0.0 {
                continue;
            }
            let Some(target) = self.nearest_enemy(origin, stats.range) else {
                continue;
            };
            let target_pos = self.enemies[target].pos;
            if !info.ignores_walls && !self.terrain.line_clear(origin, target_pos) {
                continue;
            }
            let dir = (target_pos - origin).normalize_or_zero();
            self.fire(&shot, origin, dir, target, events);
            // Twin Barrel: the same volley at a second target.
            if shot.twin
                && let Some(second) = self.second_target(origin, stats.range, target)
            {
                let to = (self.enemies[second].pos - origin).normalize_or_zero();
                self.fire(&shot, origin, to, second, events);
            }
            let w = &mut self.players[player].weapons[slot];
            w.cooldown = stats.cooldown * cooldown_multiplier;
            w.since_fired = 0;
            w.aim = math::yaw_of(dir);
            events.push(GameEvent::Fired {
                weapon: kind,
                at: origin,
            });
        }
    }

    /// The nearest enemy in range other than `not` (with line of sight).
    fn second_target(&mut self, from: V2, range: f32, not: usize) -> Option<usize> {
        let near = self.enemies_near(from, range);
        near.into_iter()
            .filter(|&i| i != not && self.terrain.line_clear(from, self.enemies[i].pos))
            .min_by(|&a, &b| {
                self.enemies[a]
                    .pos
                    .distance_squared(from)
                    .total_cmp(&self.enemies[b].pos.distance_squared(from))
            })
    }

    /// One volley of `shot` from `origin` at enemy `target` (players and
    /// turrets share this).
    pub(super) fn fire(
        &mut self,
        shot: &Shot,
        origin: V2,
        dir: V2,
        target: usize,
        events: &mut Vec<GameEvent>,
    ) {
        let stats = &shot.stats;
        match shot.weapon.info().behavior {
            WeaponBehavior::Bolt => {
                for i in 0..stats.count {
                    let spread = (i as f32 - (stats.count as f32 - 1.0) * 0.5) * 0.08;
                    let aim = math::rotate(dir, spread);
                    self.spawn_projectile(ProjectileKind::Bolt, shot, origin, aim);
                }
            }
            WeaponBehavior::Scatter => {
                let fans: &[V2] = if shot.double { &[dir, -dir] } else { &[dir] };
                for &fan in fans {
                    for i in 0..stats.count {
                        let t = i as f32 / (stats.count - 1).max(1) as f32 - 0.5;
                        let spread = t * 0.75 + self.rng.range_f32(-0.04..0.04);
                        let aim = math::rotate(fan, spread);
                        self.spawn_projectile(ProjectileKind::Pellet, shot, origin, aim);
                    }
                }
            }
            WeaponBehavior::Arc => self.fire_arc(shot, origin, target, events),
            WeaponBehavior::Mortar => self.fire_mortar(shot, origin, target),
            WeaponBehavior::Orbit => {}
        }
    }

    fn fire_arc(&mut self, shot: &Shot, origin: V2, target: usize, events: &mut Vec<GameEvent>) {
        let stats = &shot.stats;
        let color = shot.weapon.info().color;
        let mut points = vec![origin];
        let mut hit = vec![target];
        let mut last = target;
        self.damage_enemy(target, shot.hit, events);
        points.push(self.enemies[target].pos);
        let mut forks = Vec::new();
        let near_unhit = |run: &mut Run, from: V2, hit: &[usize]| {
            run.enemies_near(from, stats.radius)
                .into_iter()
                .filter(|i| !hit.contains(i))
                .min_by(|&a, &b| {
                    run.enemies[a]
                        .pos
                        .distance_squared(from)
                        .total_cmp(&run.enemies[b].pos.distance_squared(from))
                })
        };
        let chained = Hit {
            damage: shot.hit.damage * 0.85,
            ..shot.hit
        };
        for _ in 1..stats.count {
            let from = self.enemies[last].pos;
            let Some(next) = near_unhit(self, from, &hit) else {
                break;
            };
            self.damage_enemy(next, chained, events);
            points.push(self.enemies[next].pos);
            hit.push(next);
            // Overload: every jump also forks to another enemy.
            if shot.overload
                && let Some(fork) = near_unhit(self, from, &hit)
            {
                self.damage_enemy(fork, chained, events);
                forks.push(vec![from, self.enemies[fork].pos]);
                hit.push(fork);
            }
            last = next;
        }
        self.beams.push(Beam {
            color,
            points,
            ttl: 9,
        });
        for points in forks {
            self.beams.push(Beam {
                color,
                points,
                ttl: 9,
            });
        }
    }

    fn fire_mortar(&mut self, shot: &Shot, origin: V2, target: usize) {
        let stats = &shot.stats;
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
                weapon: shot.weapon,
                pos: origin,
                height: 1.2,
                vel: (to - origin) / stats.speed,
                damage: shot.hit.damage,
                heavy: shot.hit.heavy,
                elements: shot.hit.elements,
                cluster: shot.cluster,
                radius: stats.radius,
                age: 0,
                ttl: total,
                pierce: 0,
                hit: Vec::new(),
            });
        }
    }

    fn spawn_projectile(&mut self, kind: ProjectileKind, shot: &Shot, origin: V2, dir: V2) {
        let stats = &shot.stats;
        self.projectiles.push(Projectile {
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
            ttl: (stats.range / stats.speed * 60.0) as u32 + 2,
            pierce: stats.pierce,
            hit: Vec::new(),
        });
    }

    /// A blast at `at`: damage everything in `radius`.
    fn burst(
        &mut self,
        weapon: WeaponId,
        at: V2,
        radius: f32,
        hit: Hit,
        events: &mut Vec<GameEvent>,
    ) {
        self.blasts.push(Blast {
            kind: BlastKind::Weapon(weapon),
            pos: at,
            radius,
            ttl: 22,
            total: 22,
        });
        for i in self.enemies_near(at, radius) {
            self.damage_enemy(i, hit, events);
        }
        events.push(GameEvent::Explosion { at, radius });
    }

    pub(super) fn update_projectiles(&mut self, events: &mut Vec<GameEvent>) {
        let mut projectiles = std::mem::take(&mut self.projectiles);
        projectiles.retain_mut(|p| {
            p.age += 1;
            let hit = Hit {
                damage: p.damage,
                heavy: p.heavy,
                elements: p.elements,
                secondary: false,
            };
            match p.kind {
                ProjectileKind::Mortar { from, to, total } => {
                    let t = (p.age as f32 / total as f32).min(1.0);
                    p.pos = V2::new(from[0].lerp(to[0], t), from[1].lerp(to[1], t));
                    p.height = 1.2 + 6.0 * t * (1.0 - t) - 1.2 * t;
                    if p.age < total {
                        return true;
                    }
                    self.burst(p.weapon, to, p.radius, hit, events);
                    // Cluster Shells: three bomblets around the impact.
                    if p.cluster {
                        let small = Hit {
                            damage: hit.damage * 0.5,
                            ..hit
                        };
                        for k in 0..3 {
                            let a = k as f32 * std::f32::consts::TAU / 3.0 + p.age as f32;
                            let at = to + V2::new(a.sin(), a.cos()) * p.radius * 0.9;
                            self.burst(p.weapon, at, p.radius * 0.6, small, events);
                        }
                    }
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
                        let e = &self.enemies[i];
                        if p.hit.contains(&e.id)
                            || e.pos.distance(p.pos) > p.radius + e.kind.info().radius
                        {
                            continue;
                        }
                        p.hit.push(e.id);
                        self.damage_enemy(i, hit, events);
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

    /// A player's equipped orbit weapon slot, if any.
    fn orbit_slot(&self, player: usize) -> Option<usize> {
        self.players[player]
            .weapons
            .iter()
            .position(|w| w.kind.info().behavior == WeaponBehavior::Orbit)
    }

    /// World positions of a player's orbit drones, if equipped (Second Ring
    /// adds a wider ring turning the other way).
    pub fn drone_positions(&self, player: usize) -> Vec<V2> {
        let Some(slot) = self.orbit_slot(player) else {
            return Vec::new();
        };
        let p = &self.players[player];
        let w = &p.weapons[slot];
        let stats = w.stats();
        let ring = |angle: f32, range: f32| {
            (0..stats.count).map(move |i| {
                let a = angle + i as f32 * std::f32::consts::TAU / stats.count as f32;
                p.pos + V2::new(a.sin(), a.cos()) * range
            })
        };
        let mut drones: Vec<V2> = ring(p.drone_angle, stats.range).collect();
        if w.has(Special::SecondRing) {
            drones.extend(ring(-p.drone_angle * 0.8, stats.range * 1.6));
        }
        drones
    }

    pub(super) fn update_drones(&mut self, events: &mut Vec<GameEvent>) {
        for player in 0..self.players.len() {
            if self.players[player].alive() {
                self.update_player_drones(player, events);
            }
        }
    }

    fn update_player_drones(&mut self, player: usize, events: &mut Vec<GameEvent>) {
        let Some(slot) = self.orbit_slot(player) else {
            return;
        };
        let p = &mut self.players[player];
        let w = &p.weapons[slot];
        let (kind, stats, elements) = (w.kind, w.stats(), w.elements());
        p.drone_angle = (p.drone_angle + stats.speed * DT) % std::f32::consts::TAU;
        let hit = Hit {
            damage: stats.damage * p.damage_multiplier,
            heavy: p.heavy_damage,
            elements,
            secondary: false,
        };
        let cooldown = p.cooldown_multiplier;
        let mut struck = false;
        for drone in self.drone_positions(player) {
            for i in self.enemies_near(drone, stats.radius + 1.0) {
                let e = &self.enemies[i];
                if e.drone_cooldown > 0
                    || e.pos.distance(drone) > stats.radius + e.kind.info().radius
                {
                    continue;
                }
                self.enemies[i].drone_cooldown = seconds(stats.cooldown * cooldown);
                self.damage_enemy(i, hit, events);
                struck = true;
            }
        }
        let pos = self.players[player].pos;
        let w = &mut self.players[player].weapons[slot];
        if struck {
            if w.since_fired > 20 {
                events.push(GameEvent::Fired {
                    weapon: kind,
                    at: pos,
                });
            }
            w.since_fired = 0;
        }
    }

    /// Land a hit on enemy `i`: heavy and corrosion bonuses, then statuses.
    pub(super) fn damage_enemy(&mut self, i: usize, hit: Hit, events: &mut Vec<GameEvent>) {
        let e = &mut self.enemies[i];
        if e.health <= 0.0 {
            return;
        }
        let mut amount = hit.damage;
        if e.heavy() {
            amount *= 1.0 + hit.heavy;
        }
        if e.corroded > 0 {
            amount *= 1.0 + CORRODE_BONUS;
        }
        e.health -= amount;
        e.hit_ticks = 6;
        let at = e.pos;
        if !hit.elements.any() {
            if e.health > 0.0 {
                events.push(GameEvent::EnemyHit { at });
            }
            return;
        }
        let fire = hit.elements.get(Element::Fire);
        if fire > 0.0 {
            if e.burning == 0 {
                events.push(GameEvent::Status {
                    element: Element::Fire,
                    at,
                });
            }
            e.burning = seconds(BURN_SECONDS);
            e.burn_dps = e.burn_dps.max(fire);
        }
        let acid = hit.elements.get(Element::Acid);
        if acid > 0.0 {
            if e.corroded == 0 {
                events.push(GameEvent::Status {
                    element: Element::Acid,
                    at,
                });
            }
            e.corroded = seconds(CORRODE_SECONDS);
            e.acid_dps = e.acid_dps.max(acid);
        }
        let cold = hit.elements.get(Element::Cold);
        if cold > 0.0 && e.frozen == 0 {
            e.chill += cold * CHILL_PER_POTENCY;
            if e.chill >= 1.0 {
                e.chill = 0.0;
                e.frozen = seconds(FREEZE_SECONDS);
                events.push(GameEvent::Status {
                    element: Element::Cold,
                    at,
                });
            }
        }
        if e.health > 0.0 {
            events.push(GameEvent::EnemyHit { at });
        }
        let shock = hit.elements.get(Element::Shock);
        if shock > 0.0 && !hit.secondary && self.rng.chance(SHOCK_CHANCE) {
            let jump = self
                .enemies_near(at, SHOCK_REACH)
                .into_iter()
                .filter(|&j| j != i)
                .min_by(|&a, &b| {
                    self.enemies[a]
                        .pos
                        .distance_squared(at)
                        .total_cmp(&self.enemies[b].pos.distance_squared(at))
                });
            if let Some(j) = jump {
                let to = self.enemies[j].pos;
                self.damage_enemy(
                    j,
                    Hit {
                        damage: shock,
                        heavy: hit.heavy,
                        elements: Elements::default(),
                        secondary: true,
                    },
                    events,
                );
                self.beams.push(Beam {
                    color: SHOCK_COLOR,
                    points: vec![at, to],
                    ttl: 6,
                });
                events.push(GameEvent::Status {
                    element: Element::Shock,
                    at: to,
                });
            }
        }
    }

    /// Burn and corrosion damage, chill decay and thawing (every tick).
    pub(super) fn update_statuses(&mut self, events: &mut Vec<GameEvent>) {
        // Damage over time lands twice a second so hits stay readable.
        let pulse = self.ticks.is_multiple_of(30);
        for i in 0..self.enemies.len() {
            let e = &mut self.enemies[i];
            if e.health <= 0.0 {
                continue;
            }
            e.burning = e.burning.saturating_sub(1);
            e.corroded = e.corroded.saturating_sub(1);
            e.frozen = e.frozen.saturating_sub(1);
            e.chill = (e.chill - 0.4 * DT).max(0.0);
            if e.burning == 0 {
                e.burn_dps = 0.0;
            }
            if e.corroded == 0 {
                e.acid_dps = 0.0;
            }
            let dot = (e.burn_dps + e.acid_dps) * 0.5;
            if pulse && dot > 0.0 {
                self.damage_enemy(
                    i,
                    Hit {
                        damage: dot,
                        secondary: true,
                        ..Hit::default()
                    },
                    events,
                );
            }
        }
    }
}
