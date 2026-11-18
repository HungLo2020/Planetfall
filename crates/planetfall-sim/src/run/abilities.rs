//! Class abilities: one button with charges and a cooldown, each a code
//! archetype with its numbers from `classes.toml` at the player's tier,
//! scaled by ability upgrades (`mods.toml`), plus the things they leave in
//! the world (barrier posts, shield domes), and the sentry turrets the
//! Engineer's Sentry weapon keeps planted.

use super::weapons::Shot;
use super::{BARRIER_RADIUS, Deployable, DeployableKind, Hit, Run};
use crate::content::{AbilityKind, DT, Element, Special, content, seconds};
use crate::events::GameEvent;
use crate::math::{self, V2};
use crate::terrain::{TILE, Terrain};

/// Barrier posts zap enemies touching them this often (ticks).
const BARRIER_PULSE: u32 = 15;
/// Share of damage a shield dome stops for players inside it.
pub const DOME_PROTECTION: f32 = 0.6;
/// Enemies inside a dome move at this share of their speed.
pub const DOME_SLOW: f32 = 0.55;
/// An overcharged dome shocks enemies inside it this often (ticks).
const DOME_PULSE: u32 = 30;
/// A sentry farther than this from its owner is re-planted at their side.
const SENTRY_LEASH: f32 = 10.0;
/// Seconds between plantings.
const SENTRY_REPLANT: f32 = 0.8;

impl Run {
    /// Player `i` pressed the ability button.
    pub(super) fn use_ability(&mut self, i: usize, events: &mut Vec<GameEvent>) {
        let p = &self.players[i];
        if p.down {
            return;
        }
        if p.ability.charges == 0 || p.jammed > 0 {
            events.push(GameEvent::AbilityNotReady);
            return;
        }
        let class = p.class;
        let mut tier = class.ability(p.ability.tier).clone();
        let kind = class.info().ability.kind;
        let from = p.pos;
        let facing = math::from_yaw(p.yaw);
        // Upgrades: power scales damage; reach scales range, duration and
        // barrier posts.
        let (power, reach) = (p.ability.power(), p.ability.reach());
        tier.range *= reach;
        tier.duration *= reach;
        tier.count = (tier.count as f32 * reach).round() as u32;
        let mut hit = Hit {
            damage: tier.damage * p.damage_multiplier * power,
            heavy: p.heavy_damage,
            ..Hit::default()
        };
        let molten = p.ability.has(Special::MoltenBore);
        let fence = p.ability.has(Special::ArcFence);
        let overcharge = p.ability.has(Special::Overcharge);
        let to = match kind {
            AbilityKind::Bore => {
                let mut width = tier.width;
                if molten {
                    width *= 1.5;
                    hit.elements.add(Element::Fire, 8.0 * power);
                }
                self.bore(i, from, facing, tier.range, width, hit, events)
            }
            AbilityKind::Blink => self.blink(i, from, facing, tier.range, hit, events),
            AbilityKind::Barrier => {
                let mut zap = 1.0;
                if fence {
                    zap = 2.0;
                    hit.elements.add(Element::Shock, 6.0 * power);
                }
                self.raise_barrier(i, from, facing, tier.count, tier.duration, zap, hit);
                from + facing * 2.2
            }
            AbilityKind::Dome => {
                let total = seconds(tier.duration).max(1);
                if overcharge {
                    hit.elements.add(Element::Shock, 4.0 * power);
                }
                // One dome per player: a new one replaces the old.
                self.deployables
                    .retain(|d| d.owner != i || !matches!(d.kind, DeployableKind::Dome { .. }));
                self.deployables.push(Deployable {
                    kind: DeployableKind::Dome { overcharge },
                    owner: i,
                    pos: from,
                    yaw: 0.0,
                    ttl: total,
                    total,
                    damage: hit.damage,
                    range: tier.range,
                    heavy: hit.heavy,
                    elements: hit.elements,
                    cooldown: 0.0,
                    since_fired: 999,
                });
                from
            }
        };
        let p = &mut self.players[i];
        p.ability.charges -= 1;
        let cooldown = seconds(tier.cooldown * p.ability_cooldown()).max(1);
        p.ability.cooldown_total = cooldown;
        if p.ability.cooldown == 0 {
            p.ability.cooldown = cooldown;
        }
        events.push(GameEvent::AbilityUsed {
            player: i,
            kind,
            from,
            to,
        });
    }

    /// Tunnel straight ahead: every minable block in the bore breaks (with
    /// its drops) and enemies inside it take the hit. Stops at bedrock.
    #[allow(
        clippy::too_many_arguments,
        reason = "ability inputs read straight from the tier"
    )]
    fn bore(
        &mut self,
        i: usize,
        from: V2,
        dir: V2,
        length: f32,
        width: f32,
        hit: Hit,
        events: &mut Vec<GameEvent>,
    ) -> V2 {
        let side = V2::new(-dir.y, dir.x);
        let half = (width * 0.5).max(0.3);
        let mut reached = 0.0f32;
        let mut t = 0.6;
        'bore: while t <= length {
            for k in [-1.0, 0.0, 1.0] {
                let p = from + dir * t + side * (k * half * 0.8);
                let (x, y) = Terrain::tile_of(p);
                let tile = self.terrain.tile(x, y);
                if tile.minable() {
                    if let Some(broken) = self.terrain.mine(x, y, f32::MAX) {
                        let at = Terrain::center_of(x, y);
                        self.block_broken(i, at, broken.was, events);
                    }
                } else if tile.solid() && k == 0.0 {
                    break 'bore;
                }
            }
            reached = t;
            t += TILE * 0.25;
        }
        let end = from + dir * reached;
        for e in 0..self.enemies.len() {
            let pos = self.enemies[e].pos;
            let along = (pos - from).dot(dir).clamp(0.0, reached);
            let closest = from + dir * along;
            let r = self.enemies[e].kind.info().radius;
            if pos.distance(closest) < half + r {
                self.damage_enemy(e, hit, events);
            }
        }
        end
    }

    /// Teleport up to `range` ahead (stopping at walls), hitting enemies
    /// along the way.
    fn blink(
        &mut self,
        i: usize,
        from: V2,
        dir: V2,
        range: f32,
        hit: Hit,
        events: &mut Vec<GameEvent>,
    ) -> V2 {
        let radius = content().tuning.player.radius;
        let mut to = from;
        let mut t = 0.2;
        while t <= range {
            let next = from + dir * t;
            if self.terrain.solid_at(next) {
                break;
            }
            to = next;
            t += 0.2;
        }
        let (to, _) = self.terrain.resolve_circle(to, radius);
        let path = to - from;
        let length = path.length();
        let dir = path.normalize_or_zero();
        for e in 0..self.enemies.len() {
            let pos = self.enemies[e].pos;
            let along = (pos - from).dot(dir).clamp(0.0, length);
            if pos.distance(from + dir * along) < 1.2 {
                self.damage_enemy(e, hit, events);
            }
        }
        let p = &mut self.players[i];
        p.pos = to;
        p.invulnerable_ticks = p.invulnerable_ticks.max(20);
        p.mining = None;
        to
    }

    /// A row of shock posts across the player's facing, two metres ahead
    /// (closer when a wall is in the way). `zap` scales how far they reach.
    #[allow(
        clippy::too_many_arguments,
        reason = "ability inputs read straight from the tier"
    )]
    fn raise_barrier(
        &mut self,
        i: usize,
        from: V2,
        dir: V2,
        count: u32,
        duration: f32,
        zap: f32,
        hit: Hit,
    ) {
        let side = V2::new(-dir.y, dir.x);
        let center = [2.2, 1.6, 1.0]
            .into_iter()
            .map(|d| from + dir * d)
            .find(|c| !self.terrain.solid_at(*c))
            .unwrap_or(from);
        let total = seconds(duration).max(1);
        let count = count.max(1);
        let yaw = math::yaw_of(dir);
        for n in 0..count {
            let offset = (n as f32 - (count as f32 - 1.0) * 0.5) * 0.95;
            let pos = center + side * offset;
            if self.terrain.solid_at(pos) {
                continue;
            }
            self.deployables.push(Deployable {
                kind: DeployableKind::Barrier,
                owner: i,
                pos,
                yaw,
                ttl: total,
                total,
                damage: hit.damage,
                range: zap,
                heavy: hit.heavy,
                elements: hit.elements,
                cooldown: 0.0,
                since_fired: 999,
            });
        }
    }

    /// Whether `at` is inside a shield dome.
    pub(super) fn in_dome(&self, at: V2) -> bool {
        self.deployables
            .iter()
            .any(|d| matches!(d.kind, DeployableKind::Dome { .. }) && d.pos.distance(at) < d.range)
    }

    /// The Sentry weapon in `slot`: keep `count` turrets planted near the
    /// player, re-planting the farthest one when they move on. `timer` is
    /// the weapon's cooldown, reused as the planting timer.
    pub(super) fn tend_sentries(&mut self, player: usize, slot: usize, timer: f32) {
        let p = &self.players[player];
        let w = &p.weapons[slot];
        let (weapon, stats, mobile, pos) = (w.kind, w.stats(), w.has(Special::MobileSentry), p.pos);
        self.players[player].weapons[slot].cooldown = timer;
        if timer > 0.0 {
            return;
        }
        let mine: Vec<usize> = (0..self.deployables.len())
            .filter(|&d| {
                let dep = &self.deployables[d];
                dep.owner == player
                    && matches!(dep.kind, DeployableKind::Turret { weapon: k, .. } if k == weapon)
            })
            .collect();
        // Upgrades take effect on the turrets already planted.
        for &d in &mine {
            self.deployables[d].kind = DeployableKind::Turret { weapon, mobile };
        }
        let farthest = mine.iter().copied().max_by(|&a, &b| {
            let da = self.deployables[a].pos.distance_squared(pos);
            let db = self.deployables[b].pos.distance_squared(pos);
            da.total_cmp(&db)
        });
        let replant = if (mine.len() as u32) < stats.count {
            None
        } else {
            match farthest {
                Some(d) if !mobile && self.deployables[d].pos.distance(pos) > SENTRY_LEASH => {
                    Some(d)
                }
                _ => return,
            }
        };
        // Plant beside the player, away from the other turrets.
        let spot = (0..8)
            .map(|k| {
                let a = k as f32 * std::f32::consts::TAU / 8.0;
                pos + V2::new(a.cos(), a.sin()) * 1.4
            })
            .filter(|&c| !self.terrain.solid_at(c))
            .max_by(|a, b| {
                let gap = |c: V2| {
                    mine.iter()
                        .filter(|&&d| Some(d) != replant)
                        .map(|&d| self.deployables[d].pos.distance(c))
                        .fold(f32::INFINITY, f32::min)
                };
                gap(*a).total_cmp(&gap(*b))
            })
            .unwrap_or(pos);
        let turret = Deployable {
            kind: DeployableKind::Turret { weapon, mobile },
            owner: player,
            pos: spot,
            yaw: self.players[player].yaw,
            ttl: u32::MAX,
            total: u32::MAX,
            damage: 0.0,
            range: stats.range,
            heavy: 0.0,
            elements: Default::default(),
            cooldown: 0.3,
            since_fired: 999,
        };
        match replant {
            Some(d) => self.deployables[d] = turret,
            None => self.deployables.push(turret),
        }
        self.players[player].weapons[slot].cooldown = SENTRY_REPLANT;
    }

    /// Age deployables; turrets shoot (and mobile ones follow their owner)
    /// and barrier posts zap what touches them.
    pub(super) fn update_deployables(&mut self, events: &mut Vec<GameEvent>) {
        for d in 0..self.deployables.len() {
            let dep = &mut self.deployables[d];
            dep.ttl = dep.ttl.saturating_sub(1);
            dep.since_fired = dep.since_fired.saturating_add(1);
            let dep = *dep;
            match dep.kind {
                DeployableKind::Turret { weapon, mobile } => {
                    // A sentry fires with its owner's weapon as upgraded now.
                    let owner = &self.players[dep.owner];
                    let Some(w) = owner.weapons.iter().find(|w| w.kind == weapon) else {
                        self.deployables[d].ttl = 0;
                        continue;
                    };
                    let stats = w.stats();
                    let hit = Hit {
                        damage: stats.damage * owner.damage_multiplier,
                        heavy: owner.heavy_damage,
                        elements: w.elements(),
                        secondary: false,
                    };
                    let twin = w.has(Special::TwinBarrel);
                    let rate = owner.cooldown_multiplier;
                    if mobile {
                        // Mobile sentries trot along, spread around their owner.
                        let index = self.deployables[..d]
                            .iter()
                            .filter(|o| {
                                o.owner == dep.owner
                                    && matches!(o.kind, DeployableKind::Turret { .. })
                            })
                            .count();
                        let a = index as f32 * 2.1 + 0.6;
                        let goal = owner.pos + V2::new(a.cos(), a.sin()) * 1.8;
                        let to = goal - dep.pos;
                        if to.length() > 0.4 {
                            let step = to.normalize_or_zero() * 6.0_f32.min(to.length() / DT) * DT;
                            let (pos, _) = self.terrain.resolve_circle(dep.pos + step, 0.35);
                            self.deployables[d].pos = pos;
                        }
                    }
                    let dep = self.deployables[d];
                    let cooldown = (dep.cooldown - DT).max(0.0);
                    self.deployables[d].cooldown = cooldown;
                    if cooldown > 0.0 {
                        continue;
                    }
                    let Some(target) = self.nearest_enemy(dep.pos, stats.range) else {
                        continue;
                    };
                    let target_pos = self.enemies[target].pos;
                    if !self.terrain.line_clear(dep.pos, target_pos) {
                        continue;
                    }
                    let dir = (target_pos - dep.pos).normalize_or_zero();
                    let shot = Shot {
                        weapon,
                        stats,
                        hit,
                        twin,
                        double: false,
                        overload: false,
                        cluster: false,
                    };
                    self.fire(&shot, dep.pos, dir, target, events);
                    if twin && let Some(second) = self.second_target(dep.pos, stats.range, target) {
                        let to = (self.enemies[second].pos - dep.pos).normalize_or_zero();
                        self.fire(&shot, dep.pos, to, second, events);
                    }
                    let dep = &mut self.deployables[d];
                    dep.cooldown = stats.cooldown * rate;
                    dep.since_fired = 0;
                    dep.yaw = math::yaw_of(dir);
                    events.push(GameEvent::Fired {
                        weapon,
                        at: dep.pos,
                    });
                }
                DeployableKind::Dome { overcharge } => {
                    if !overcharge || !dep.ttl.is_multiple_of(DOME_PULSE) {
                        continue;
                    }
                    let zap = Hit {
                        damage: dep.damage,
                        heavy: dep.heavy,
                        elements: dep.elements,
                        secondary: true,
                    };
                    let mut struck = false;
                    for e in self.enemies_near(dep.pos, dep.range) {
                        self.damage_enemy(e, zap, events);
                        struck = true;
                    }
                    if struck {
                        self.deployables[d].since_fired = 0;
                    }
                }
                DeployableKind::Barrier => {
                    if !dep.ttl.is_multiple_of(BARRIER_PULSE) {
                        continue;
                    }
                    let zap = Hit {
                        damage: dep.damage * BARRIER_PULSE as f32 * DT,
                        heavy: dep.heavy,
                        elements: dep.elements,
                        secondary: false,
                    };
                    // `range` is the reach multiplier (Arc Fence doubles it).
                    let reach = 0.2 + (dep.range - 1.0).max(0.0) * 0.9;
                    let mut struck = false;
                    for e in self.enemies_near(dep.pos, BARRIER_RADIUS + 1.2 + reach) {
                        let r = self.enemies[e].kind.info().radius;
                        if self.enemies[e].pos.distance(dep.pos) < BARRIER_RADIUS + r + reach {
                            self.damage_enemy(e, zap, events);
                            struck = true;
                        }
                    }
                    if struck {
                        self.deployables[d].since_fired = 0;
                    }
                }
            }
        }
        self.deployables.retain(|d| d.ttl > 0);
    }
}
