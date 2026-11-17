//! Class abilities: one button on a cooldown, each a code archetype with its
//! numbers from `classes.toml` at the player's tier, plus the things they
//! leave in the world (barrier posts and turrets).

use super::{BARRIER_RADIUS, Deployable, DeployableKind, Run};
use crate::content::{AbilityKind, DT, WeaponId, content, seconds};
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
        if p.ability.cooldown > 0 {
            events.push(GameEvent::AbilityNotReady);
            return;
        }
        let class = p.class;
        let tier = class.ability(p.ability.tier).clone();
        let kind = class.info().ability.kind;
        let from = p.pos;
        let facing = math::from_yaw(p.yaw);
        let damage = tier.damage * p.damage_multiplier;
        let heavy = p.heavy_damage;
        let to = match kind {
            AbilityKind::Bore => self.bore(
                i, from, facing, tier.range, tier.width, damage, heavy, events,
            ),
            AbilityKind::Blink => self.blink(i, from, facing, tier.range, damage, heavy, events),
            AbilityKind::Barrier => {
                self.raise_barrier(i, from, facing, tier.count, tier.duration, damage, heavy);
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
                self.deployables.push(Deployable {
                    kind: DeployableKind::Turret { weapon },
                    owner: i,
                    pos: at,
                    yaw: self.players[i].yaw,
                    ttl: total,
                    total,
                    damage,
                    range: tier.range,
                    heavy,
                    cooldown: 0.4,
                    since_fired: 999,
                });
                at
            }
        };
        let p = &mut self.players[i];
        let cooldown = seconds(tier.cooldown * p.ability_cooldown_multiplier).max(1);
        p.ability.cooldown = cooldown;
        p.ability.cooldown_total = cooldown;
        events.push(GameEvent::AbilityUsed {
            player: i,
            kind,
            from,
            to,
        });
    }

    /// Tunnel straight ahead: every minable block in the bore breaks (with
    /// its drops) and enemies inside it take damage. Stops at bedrock.
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
        damage: f32,
        heavy: f32,
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
                self.damage_enemy(e, damage, heavy, events);
            }
        }
        end
    }

    /// Teleport up to `range` ahead (stopping at walls), damaging enemies
    /// along the way.
    #[allow(
        clippy::too_many_arguments,
        reason = "ability inputs read straight from the tier"
    )]
    fn blink(
        &mut self,
        i: usize,
        from: V2,
        dir: V2,
        range: f32,
        damage: f32,
        heavy: f32,
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
                self.damage_enemy(e, damage, heavy, events);
            }
        }
        let p = &mut self.players[i];
        p.pos = to;
        p.invulnerable_ticks = p.invulnerable_ticks.max(20);
        p.mining = None;
        to
    }

    /// A row of shock posts across the player's facing, two metres ahead.
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
        damage: f32,
        heavy: f32,
    ) {
        let side = V2::new(-dir.y, dir.x);
        // Two metres ahead, closer when a wall is in the way.
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
                damage,
                range: 0.0,
                heavy,
                cooldown: 0.0,
                since_fired: 999,
            });
        }
    }

    /// Age deployables; turrets shoot and barrier posts zap what touches them.
    pub(super) fn update_deployables(&mut self, events: &mut Vec<GameEvent>) {
        for d in 0..self.deployables.len() {
            let dep = &mut self.deployables[d];
            dep.ttl = dep.ttl.saturating_sub(1);
            dep.since_fired = dep.since_fired.saturating_add(1);
            let dep = *dep;
            match dep.kind {
                DeployableKind::Turret { weapon } => {
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
                    let stats = weapon.stats(1);
                    let dir = (target_pos - dep.pos).normalize_or_zero();
                    self.fire(
                        weapon, &stats, dep.pos, dir, target, dep.damage, dep.heavy, events,
                    );
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
                    let zap = dep.damage * BARRIER_PULSE as f32 * DT;
                    let mut hit = false;
                    for e in self.enemies_near(dep.pos, BARRIER_RADIUS + 1.2) {
                        let r = self.enemies[e].kind.info().radius;
                        if self.enemies[e].pos.distance(dep.pos) < BARRIER_RADIUS + r + 0.2 {
                            self.damage_enemy(e, zap, dep.heavy, events);
                            hit = true;
                        }
                    }
                    if hit {
                        self.deployables[d].since_fired = 0;
                    }
                }
            }
        }
        self.deployables.retain(|d| d.ttl > 0);
    }
}
