//! Class abilities: one button with charges and a cooldown, each a code
//! archetype with its numbers from `classes.toml` at the player's tier,
//! scaled by ability upgrades (`mods.toml`), plus the things they leave in
//! the world (barrier posts and turrets).

use super::weapons::Shot;
use super::{BARRIER_RADIUS, Deployable, DeployableKind, Hit, Run};
use crate::content::{AbilityKind, DT, Element, Special, WeaponId, content, seconds};
use crate::events::GameEvent;
use crate::math::{self, V2};
use crate::terrain::{TILE, Terrain};

/// Barrier posts zap enemies touching them this often (ticks).
const BARRIER_PULSE: u32 = 15;

impl Run {
    /// Player `i` pressed the ability button.
    pub(super) fn use_ability(&mut self, i: usize, events: &mut Vec<GameEvent>) {
        let p = &self.players[i];
        if p.down {
            return;
        }
        if p.ability.charges == 0 {
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
        let mobile = p.ability.has(Special::MobileSentry);
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
            AbilityKind::Turret => {
                let weapon = tier
                    .weapon
                    .as_deref()
                    .and_then(WeaponId::from_key)
                    .unwrap_or_else(|| class.signature());
                let ahead = from + facing * 1.2;
                let at = if self.terrain.solid_at(ahead) {
                    from
                } else {
                    ahead
                };
                let total = seconds(tier.duration).max(1);
                // One turret per player: a new one replaces the old.
                self.deployables
                    .retain(|d| d.owner != i || !matches!(d.kind, DeployableKind::Turret { .. }));
                self.deployables.push(Deployable {
                    kind: DeployableKind::Turret { weapon, mobile },
                    owner: i,
                    pos: at,
                    yaw: self.players[i].yaw,
                    ttl: total,
                    total,
                    damage: hit.damage,
                    range: tier.range,
                    heavy: hit.heavy,
                    elements: hit.elements,
                    cooldown: 0.4,
                    since_fired: 999,
                });
                at
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
                    if mobile {
                        let to = self.players[dep.owner].pos - dep.pos;
                        if to.length() > 2.5 {
                            let step = to.normalize_or_zero() * 5.5 * DT;
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
                    let Some(target) = self.nearest_enemy(dep.pos, dep.range) else {
                        continue;
                    };
                    let target_pos = self.enemies[target].pos;
                    if !self.terrain.line_clear(dep.pos, target_pos) {
                        continue;
                    }
                    let stats = weapon.base();
                    let dir = (target_pos - dep.pos).normalize_or_zero();
                    let shot = Shot {
                        weapon,
                        stats,
                        hit: Hit {
                            damage: dep.damage,
                            heavy: dep.heavy,
                            elements: dep.elements,
                            secondary: false,
                        },
                        twin: false,
                        double: false,
                        overload: false,
                        cluster: false,
                    };
                    self.fire(&shot, dep.pos, dir, target, events);
                    let dep = &mut self.deployables[d];
                    dep.cooldown = stats.cooldown;
                    dep.since_fired = 0;
                    dep.yaw = math::yaw_of(dir);
                    events.push(GameEvent::Fired {
                        weapon,
                        at: dep.pos,
                    });
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
