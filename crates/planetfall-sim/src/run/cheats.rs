//! Applying developer console commands to a drop (see [`crate::cheats`]).

use super::*;
use crate::cheats::Cheat;
use crate::content::{Offer, Upgrade};

impl Run {
    /// Apply a console command to the local player. Returns the message to
    /// show, or why it could not be done.
    pub(crate) fn cheat(
        &mut self,
        cheat: Cheat,
        events: &mut Vec<GameEvent>,
    ) -> Result<String, String> {
        let max = content().tuning.weapons.max_carried;
        match cheat {
            Cheat::GiveWeapon(weapon) => {
                let p = self.player();
                if p.carries(weapon) || p.weapons.len() >= max {
                    return Err(format!("No free slot for {}", weapon.info().name));
                }
                self.apply_offer(
                    0,
                    Offer {
                        upgrade: Upgrade::NewWeapon(weapon),
                        rarity: Rarity::Common,
                    },
                    events,
                );
                Ok(format!("Gave {}", weapon.info().name))
            }
            Cheat::RemoveWeapon(weapon) => {
                let p = self.player_mut();
                let before = p.weapons.len();
                p.weapons.retain(|w| w.kind != weapon);
                if p.weapons.len() == before {
                    return Err(format!("Not carrying {}", weapon.info().name));
                }
                Ok(format!("Removed {}", weapon.info().name))
            }
            Cheat::WeaponMod {
                weapon,
                upgrade,
                rarity,
                count,
            } => {
                if !self.player().carries(weapon) {
                    return Err(format!("Not carrying {}", weapon.info().name));
                }
                for _ in 0..count {
                    self.apply_offer(
                        0,
                        Offer {
                            upgrade: Upgrade::WeaponMod(weapon, upgrade),
                            rarity,
                        },
                        events,
                    );
                }
                Ok(format!(
                    "{} {} x{count} on {}",
                    rarity.name(),
                    upgrade.info().title,
                    weapon.info().name
                ))
            }
            Cheat::AbilityMod {
                upgrade,
                rarity,
                count,
            } => {
                for _ in 0..count {
                    self.apply_offer(
                        0,
                        Offer {
                            upgrade: Upgrade::AbilityMod(upgrade),
                            rarity,
                        },
                        events,
                    );
                }
                Ok(format!(
                    "{} {} x{count}",
                    rarity.name(),
                    upgrade.info().title
                ))
            }
            Cheat::Perk {
                perk,
                rarity,
                count,
            } => {
                for _ in 0..count {
                    self.apply_offer(
                        0,
                        Offer {
                            upgrade: Upgrade::Field(perk),
                            rarity,
                        },
                        events,
                    );
                }
                Ok(format!("{} {} x{count}", rarity.name(), perk.info().title))
            }
            Cheat::Xp(amount) => {
                let p = self.player_mut();
                p.xp += amount;
                p.xp_total += amount;
                while p.xp >= p.xp_needed() {
                    p.xp -= p.xp_needed();
                    p.level += 1;
                    p.pending_levels += 1;
                }
                Ok(format!("+{amount} XP"))
            }
            Cheat::Heal => {
                for p in &mut self.players {
                    p.health = p.max_health;
                    p.down = false;
                }
                Ok("Healed".into())
            }
            Cheat::God => {
                self.invulnerable = !self.invulnerable;
                Ok(format!(
                    "God mode {}",
                    if self.invulnerable { "on" } else { "off" }
                ))
            }
            Cheat::Kill => {
                let hit = Hit {
                    damage: 1.0e9,
                    heavy: 0.0,
                    elements: Default::default(),
                    secondary: true,
                };
                let mut killed = 0;
                for i in 0..self.enemies.len() {
                    if self.enemies[i].health > 0.0 {
                        self.enemies[i].burrowed = false;
                        self.damage_enemy(i, hit, events);
                        killed += 1;
                    }
                }
                Ok(format!("Killed {killed}"))
            }
            Cheat::Spawn { enemy, count } => {
                // An open spot a few metres off, else beside the player.
                let pos = self.player().pos;
                let center = (0..16)
                    .map(|k| {
                        let a = k as f32 * std::f32::consts::TAU / 16.0;
                        pos + V2::new(a.cos(), a.sin()) * 6.0
                    })
                    .find(|&c| !self.terrain.solid_at(c) && self.terrain.line_clear(pos, c))
                    .unwrap_or(pos);
                self.spawn_group(enemy, center, u32::from(count));
                Ok(format!("Spawned {count} {}", enemy.info().name))
            }
            Cheat::Threat(level) => {
                self.threat = level as f32;
                Ok(format!("Threat {level}"))
            }
            Cheat::Material { .. } | Cheat::Credits(_) => Err("Not a drop command".into()),
        }
    }
}
