//! The horde: flow-field steering, separation, behaviour-specific attacks,
//! deaths, drops and explosions. Numbers come from `enemies.toml`.

use super::{Blast, BlastKind, Corpse, PickupKind, Run};
use crate::content::{DT, EnemyBehavior, content};
use crate::events::GameEvent;
use crate::math::{self, V2};
use crate::terrain::TILE;

impl Run {
    pub(super) fn update_enemies(&mut self, events: &mut Vec<GameEvent>) {
        let player = self.player.pos;
        let player_radius = content().tuning.player.radius;
        let mut melee = 0.0f32;
        let mut detonate = Vec::new();
        for i in 0..self.enemies.len() {
            let e = self.enemies[i];
            if e.health <= 0.0 {
                continue;
            }
            let info = e.kind.info();
            let to_player = player - e.pos;
            let dist = to_player.length();
            let direct = dist < TILE * 2.5 && self.terrain.line_clear(e.pos, player);
            let goal = if direct {
                player
            } else {
                self.flow
                    .as_ref()
                    .and_then(|f| f.next_waypoint(&self.terrain, e.pos))
                    .unwrap_or(player)
            };
            let dir = (goal - e.pos).normalize_or_zero();
            // Separation from neighbours so hordes spread around the player.
            let mut push = V2::ZERO;
            for j in self.enemies_near(e.pos, info.radius + 1.0) {
                if j == i {
                    continue;
                }
                let o = self.enemies[j];
                let min = info.radius + o.kind.info().radius;
                let d = e.pos - o.pos;
                let l = d.length();
                if l < min && l > 1e-4 {
                    push += d * (min - l) / l;
                } else if l <= 1e-4 {
                    push += V2::new(((i as f32) * 1.7).sin(), ((i as f32) * 1.3).cos());
                }
            }
            let reach = info.radius + player_radius + 0.2;
            let mut speed = info.speed * e.speed_scale;
            if dist < reach || e.spawn_ticks > 0 {
                speed = 0.0;
            }
            if let Some(lunge) = info.lunge
                && info.behavior == EnemyBehavior::Lunger
                && dist < lunge.range
                && dist > reach
            {
                speed *= lunge.speed;
            }
            let step = dir * speed * DT + push * 0.35;
            let (pos, _) = self.terrain.resolve_circle(e.pos + step, info.radius * 0.9);
            let en = &mut self.enemies[i];
            en.pos = pos;
            en.spawn_ticks = en.spawn_ticks.saturating_sub(1);
            en.hit_ticks = en.hit_ticks.saturating_sub(1);
            en.attack_ticks = en.attack_ticks.saturating_sub(1);
            en.attack_cooldown = en.attack_cooldown.saturating_sub(1);
            en.drone_cooldown = en.drone_cooldown.saturating_sub(1);
            if to_player.length() > 1e-3 {
                en.yaw = math::turn_toward(en.yaw, math::yaw_of(to_player), 0.18);
            }
            en.walk += speed * DT * 1.7;
            if !en.alerted && dist < 12.0 {
                en.alerted = true;
                if info.alert_sound.is_some() && self.rng.chance(f64::from(info.alert_chance)) {
                    events.push(GameEvent::EnemyAlerted {
                        kind: e.kind,
                        at: e.pos,
                    });
                }
            }
            if dist < reach && en.spawn_ticks == 0 {
                if info.behavior == EnemyBehavior::Exploder {
                    detonate.push(i);
                } else if en.attack_cooldown == 0 {
                    en.attack_cooldown = 60;
                    en.attack_ticks = 14;
                    melee += info.damage * self.damage_scale();
                }
            }
        }
        if melee > 0.0 {
            self.hurt_player(melee, events);
        }
        for i in detonate {
            self.enemies[i].health = 0.0;
        }
    }

    /// Enemy damage grows with planet hazard and time on the surface.
    pub(super) fn damage_scale(&self) -> f32 {
        let ramp = content().tuning.horde.damage_ramp_per_minute;
        (0.75 + 0.25 * self.hazard) * (1.0 + self.minutes() * ramp)
    }

    pub(super) fn remove_dead(&mut self, events: &mut Vec<GameEvent>) {
        let player_radius = content().tuning.player.radius;
        // Explosion chains can kill more enemies, so loop until stable.
        for _ in 0..4 {
            let dead: Vec<usize> = (0..self.enemies.len())
                .filter(|&i| self.enemies[i].health <= 0.0)
                .collect();
            if dead.is_empty() {
                break;
            }
            let mut blasts = Vec::new();
            for &i in &dead {
                let e = self.enemies[i];
                let info = e.kind.info();
                self.kills += 1;
                events.push(GameEvent::EnemyKilled {
                    kind: e.kind,
                    at: e.pos,
                });
                self.corpses.push(Corpse {
                    kind: e.kind,
                    pos: e.pos,
                    yaw: e.yaw,
                    ticks: 0,
                });
                self.drop_pickup(PickupKind::Xp(info.xp), e.pos);
                for drop in &info.drops {
                    if drop.chance >= 1.0 || self.rng.chance(f64::from(drop.chance)) {
                        self.drop_pickup(PickupKind::Material(drop.material, drop.amount), e.pos);
                    }
                }
                if let Some(explosion) = info.explosion {
                    blasts.push((e.kind, e.pos, explosion));
                }
            }
            self.enemies.retain(|e| e.health > 0.0);
            self.rebuild_index();
            for (kind, at, explosion) in blasts {
                self.blasts.push(Blast {
                    kind: BlastKind::Enemy(kind),
                    pos: at,
                    radius: explosion.radius,
                    ttl: 24,
                    total: 24,
                });
                events.push(GameEvent::Explosion {
                    at,
                    radius: explosion.radius,
                });
                if at.distance(self.player.pos) < explosion.radius + player_radius {
                    self.hurt_player(kind.info().damage * self.damage_scale(), events);
                }
                for j in self.enemies_near(at, explosion.radius) {
                    self.damage_enemy(j, explosion.enemy_damage, events);
                }
            }
        }
        for c in &mut self.corpses {
            c.ticks += 1;
        }
        self.corpses.retain(|c| c.ticks < 50);
    }
}
