//! The player: movement, dash, damage taken and auto-mining.

use super::{Mining, PickupKind, Run, SparkKind, spawn_sparks};
use crate::content::{DT, Material, content};
use crate::events::GameEvent;
use crate::input::GameInput;
use crate::math;
use crate::terrain::{Terrain, Tile};

impl Run {
    pub(super) fn update_player(&mut self, input: &GameInput, events: &mut Vec<GameEvent>) {
        let tuning = &content().tuning.player;
        let p = &mut self.player;
        let movement = input.movement.clamp_length_max(1.0);
        let moving = movement.length() > 0.15;
        p.dash_cooldown = p.dash_cooldown.saturating_sub(1);
        p.invulnerable_ticks = p.invulnerable_ticks.saturating_sub(1);
        p.hurt_ticks = p.hurt_ticks.saturating_sub(1);
        if input.dash && p.dash_cooldown == 0 && p.dash_ticks == 0 {
            p.dash_ticks = tuning.dash_ticks;
            p.dash_cooldown = tuning.dash_cooldown_ticks;
            p.dash_dir = if moving {
                movement.normalize_or_zero()
            } else {
                math::from_yaw(p.yaw)
            };
            p.invulnerable_ticks = p.invulnerable_ticks.max(tuning.dash_ticks + 4);
            events.push(GameEvent::Dashed);
        }
        let velocity = if p.dash_ticks > 0 {
            p.dash_ticks -= 1;
            p.dash_dir * tuning.dash_speed
        } else {
            movement * p.move_speed
        };
        let target = p.pos + velocity * DT;
        let (resolved, hit) = self.terrain.resolve_circle(target, tuning.radius);
        p.pos = resolved;
        p.moving = moving;
        if moving {
            p.yaw = math::turn_toward(p.yaw, math::yaw_of(movement), 0.35);
            p.walk += velocity.length() * DT * 1.6;
        }
        if p.regeneration > 0.0 {
            p.health = (p.health + p.regeneration * DT).min(p.max_health);
        }
        // Auto-mine the block the player is pushing into.
        let mut mining = None;
        if moving
            && let Some((x, y)) = hit
            && self.terrain.tile(x, y).minable()
        {
            let to_block = (Terrain::center_of(x, y) - p.pos).normalize_or_zero();
            if to_block.dot(movement.normalize_or_zero()) > 0.35 {
                mining = Some((x, y));
            }
        }
        let Some((x, y)) = mining else {
            p.mining = None;
            return;
        };
        self.mine(x, y, tuning.mining_rate, events);
    }

    fn mine(&mut self, x: i32, y: i32, rate: f32, events: &mut Vec<GameEvent>) {
        let p = &mut self.player;
        let tile = self.terrain.tile(x, y);
        let amount = rate * p.mining_multiplier * DT;
        let at = Terrain::center_of(x, y);
        p.mine_sound += 1;
        if p.mine_sound % 14 == 1 {
            events.push(GameEvent::MineHit { at });
            let kind = match tile {
                Tile::Ore { material, .. } => SparkKind::Ore(material),
                _ => SparkKind::Rock,
            };
            let face = p.pos + (at - p.pos).normalize_or_zero() * 0.8;
            spawn_sparks(&mut self.sparks, &mut self.rng, kind, face, 1.0, 3);
        }
        match self.terrain.mine(x, y, amount) {
            Some(broken) => {
                self.player.mining = None;
                self.blocks_mined += 1;
                let material = match broken.was {
                    Tile::Ore { material, .. } => Some(material),
                    _ => None,
                };
                events.push(GameEvent::BlockBroken { at, material });
                let kind = material.map_or(SparkKind::Rock, SparkKind::Ore);
                spawn_sparks(&mut self.sparks, &mut self.rng, kind, at, 1.4, 12);
                let drops: &[(Material, u32)] = match material {
                    Some(Material::Ferrite) => &[(Material::Ferrite, 2), (Material::Ferrite, 2)],
                    Some(Material::Lumite) => &[(Material::Lumite, 1), (Material::Lumite, 1)],
                    Some(Material::Voidstone) => &[(Material::Voidstone, 1)],
                    None => &[],
                };
                for &(m, amount) in drops {
                    self.drop_pickup(PickupKind::Material(m, amount), at);
                }
                let rock_ferrite = f64::from(content().tuning.player.rock_ferrite_chance);
                if material.is_none() && self.rng.chance(rock_ferrite) {
                    self.drop_pickup(PickupKind::Material(Material::Ferrite, 1), at);
                }
            }
            None => {
                let max = tile.max_hp();
                let remaining = match self.terrain.tile(x, y) {
                    Tile::Rock { hp } | Tile::Ore { hp, .. } => hp,
                    _ => 0.0,
                };
                self.player.mining = Some(Mining {
                    tile: (x, y),
                    progress: if max > 0.0 {
                        1.0 - remaining / max
                    } else {
                        0.0
                    },
                });
            }
        }
    }

    pub(super) fn hurt_player(&mut self, amount: f32, events: &mut Vec<GameEvent>) {
        if self.invulnerable || self.player.invulnerable_ticks > 0 || amount <= 0.0 {
            return;
        }
        self.player.health -= amount;
        self.player.hurt_ticks = 18;
        self.player.invulnerable_ticks = 30;
        events.push(GameEvent::PlayerHurt { amount });
    }
}
