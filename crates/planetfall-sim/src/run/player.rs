//! Players: movement, dash, damage taken and auto-mining.

use super::{Mining, PickupKind, Run};
use crate::content::{DT, Material, content};
use crate::events::GameEvent;
use crate::input::GameInput;
use crate::math;
use crate::terrain::{Terrain, Tile};

impl Run {
    pub(super) fn update_player(
        &mut self,
        i: usize,
        input: &GameInput,
        events: &mut Vec<GameEvent>,
    ) {
        let tuning = &content().tuning.player;
        let p = &mut self.players[i];
        if p.down {
            return;
        }
        let movement = input.movement.clamp_length_max(1.0);
        let moving = movement.length() > 0.15;
        p.dash_cooldown = p.dash_cooldown.saturating_sub(1);
        p.invulnerable_ticks = p.invulnerable_ticks.saturating_sub(1);
        p.hurt_ticks = p.hurt_ticks.saturating_sub(1);
        // Recharge one ability charge at a time.
        if p.ability.charges < p.ability.max_charges {
            p.ability.cooldown = p.ability.cooldown.saturating_sub(1);
            if p.ability.cooldown == 0 {
                p.ability.charges += 1;
                if p.ability.charges < p.ability.max_charges {
                    p.ability.cooldown = p.ability.cooldown_total;
                }
            }
        }
        if input.dash && p.dash_cooldown == 0 && p.dash_ticks == 0 {
            p.dash_ticks = tuning.dash_ticks;
            p.dash_cooldown =
                (tuning.dash_cooldown_ticks as f32 * p.dash_cooldown_multiplier).round() as u32;
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
        let p = &mut self.players[i];
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
        self.mine(i, x, y, tuning.mining_rate, events);
    }

    fn mine(&mut self, i: usize, x: i32, y: i32, rate: f32, events: &mut Vec<GameEvent>) {
        let p = &mut self.players[i];
        let tile = self.terrain.tile(x, y);
        let amount = rate * p.mining_multiplier * DT;
        let at = Terrain::center_of(x, y);
        p.mine_sound += 1;
        if p.mine_sound % 14 == 1 {
            let material = match tile {
                Tile::Ore { material, .. } => Some(material),
                _ => None,
            };
            let face = p.pos + (at - p.pos).normalize_or_zero() * 0.8;
            events.push(GameEvent::MineHit { at: face, material });
        }
        match self.terrain.mine(x, y, amount) {
            Some(broken) => {
                self.players[i].mining = None;
                self.block_broken(i, at, broken.was, events);
            }
            None => {
                let max = tile.max_hp();
                let remaining = match self.terrain.tile(x, y) {
                    Tile::Rock { hp } | Tile::Ore { hp, .. } => hp,
                    _ => 0.0,
                };
                self.players[i].mining = Some(Mining {
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

    /// A block player `i` broke (by drilling or an ability): drops and events.
    pub(super) fn block_broken(
        &mut self,
        i: usize,
        at: math::V2,
        was: Tile,
        events: &mut Vec<GameEvent>,
    ) {
        self.blocks_mined += 1;
        let material = match was {
            Tile::Ore { material, .. } => Some(material),
            _ => None,
        };
        events.push(GameEvent::BlockBroken { at, material });
        let drops: &[(Material, u32)] = match material {
            Some(Material::Ferrite) => &[(Material::Ferrite, 2), (Material::Ferrite, 2)],
            Some(Material::Lumite) => &[(Material::Lumite, 1), (Material::Lumite, 1)],
            Some(Material::Voidstone) => &[(Material::Voidstone, 1)],
            None => &[],
        };
        for &(m, amount) in drops {
            self.drop_pickup(PickupKind::Material(m, amount), at);
        }
        let bonus = self.players[i].ore_bonus;
        if let Some(m) = material
            && bonus > 0.0
            && self.rng.chance(f64::from(bonus.min(1.0)))
        {
            self.drop_pickup(PickupKind::Material(m, 1), at);
        }
        let rock_ferrite = f64::from(content().tuning.player.rock_ferrite_chance);
        if material.is_none() && self.rng.chance(rock_ferrite) {
            self.drop_pickup(PickupKind::Material(Material::Ferrite, 1), at);
        }
    }

    pub(super) fn hurt_player(&mut self, i: usize, amount: f32, events: &mut Vec<GameEvent>) {
        let p = &mut self.players[i];
        if self.invulnerable || p.down || p.invulnerable_ticks > 0 || amount <= 0.0 {
            return;
        }
        p.health -= amount;
        p.hurt_ticks = 18;
        p.invulnerable_ticks = 30;
        events.push(GameEvent::PlayerHurt { amount });
    }
}
