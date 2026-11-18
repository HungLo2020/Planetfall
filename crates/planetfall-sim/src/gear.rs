//! Equipment: randomised items for the four slots (helmet, suit, boots,
//! gadget). An item is a base (`gear.toml`) with its implicit stat, a
//! rarity and 1 to 4 random affixes (`affixes.toml`), all scaled by rarity
//! and the hazard level it dropped at. Found on dives (caches, elites,
//! bosses), kept in the profile's stash even on death, equipped per class
//! in the locker room and traded at the station.

use crate::content::{AffixId, GearId, GearSlot, PerkEffect, Rarity, content};
use crate::random::weighted;
use quasar_core::Rng;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Gear {
    /// Unique within a profile (0 until it reaches the stash).
    pub id: u32,
    pub base: GearId,
    pub rarity: Rarity,
    /// Hazard level it dropped at (scales its stats).
    pub hazard: u8,
    /// Affixes with their final rolled amounts.
    pub affixes: Vec<(AffixId, f32)>,
}

/// Stat scale for a rarity and hazard.
fn scale(rarity: Rarity, hazard: u8) -> f32 {
    let t = &content().tuning.equipment;
    t.scale[rarity.index()] * (1.0 + t.hazard_scale * f32::from(hazard.saturating_sub(1)))
}

/// Scale a stat's change (multiplicative effects scale their change from 1).
fn scaled(effect: PerkEffect, amount: f32, k: f32) -> f32 {
    if effect.multiplicative() {
        1.0 + (amount - 1.0) * k
    } else {
        amount * k
    }
}

/// A typical common affix's change per effect, for comparing items.
fn reference(effect: PerkEffect) -> f32 {
    use PerkEffect as E;
    match effect {
        E::MaxHealth => 10.0,
        E::MoveSpeed | E::FireRate => 0.035,
        E::Damage => 0.05,
        E::PickupRange => 0.7,
        E::Mining => 0.17,
        E::Regeneration => 0.3,
        E::OreBonus | E::Luck => 0.07,
        E::ExtractionSpeed => 0.15,
        E::DashCooldown | E::AbilityCooldown => 0.045,
        E::HeavyDamage => 0.1,
        E::Armor => 0.03,
        E::XpGain => 0.05,
    }
}

impl Gear {
    pub fn slot(&self) -> GearSlot {
        self.base.info().slot
    }

    /// Every stat the item grants: the implicit first, then the affixes.
    pub fn stats(&self) -> Vec<(PerkEffect, f32)> {
        let implicit = self.base.info().implicit;
        let k = scale(self.rarity, self.hazard);
        std::iter::once((implicit.effect, scaled(implicit.effect, implicit.amount, k)))
            .chain(self.affixes.iter().map(|(a, v)| (a.info().effect, *v)))
            .collect()
    }

    /// "Swift Runner Boots": the first affix names the item.
    pub fn name(&self) -> String {
        match self.affixes.first() {
            Some((a, _)) => format!("{} {}", a.info().name, self.base.info().name),
            None => self.base.info().name.clone(),
        }
    }

    /// Stat lines joined for one-line display.
    pub fn summary(&self) -> String {
        self.stats()
            .into_iter()
            .map(|(e, a)| e.describe(a))
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// Credits the station pays for it.
    pub fn sell_value(&self) -> u32 {
        let t = &content().tuning.trade;
        t.sell[self.rarity.index()] + t.sell_per_affix * self.affixes.len() as u32
    }

    /// Credits the station asks for it.
    pub fn price(&self) -> u32 {
        (self.sell_value() as f32 * content().tuning.trade.markup).round() as u32
    }

    /// Overall strength, for sorting and the autopilot's comparisons.
    pub fn score(&self) -> f32 {
        self.stats()
            .into_iter()
            .map(|(e, a)| {
                let change = if e.multiplicative() {
                    (a - 1.0).abs()
                } else {
                    a
                };
                change / reference(e)
            })
            .sum()
    }
}

/// Rarity odds shift toward rarer tiers with hazard and luck; `min` zeroes
/// everything below it.
pub fn roll_rarity(rng: &mut Rng, hazard: u8, min: Rarity, luck: f32) -> Rarity {
    let t = &content().tuning;
    let shift = (1.0 + luck.max(0.0))
        * (1.0 + t.rarity.hazard_bonus * f32::from(hazard.saturating_sub(1)) * 0.5);
    let weights: Vec<f32> = Rarity::ALL
        .iter()
        .map(|r| {
            if *r < min {
                0.0
            } else {
                t.rarity.weights[r.index()] * shift.powi(r.index() as i32)
            }
        })
        .collect();
    let total: f32 = weights.iter().sum();
    let mut roll = rng.next_f32() * total;
    for r in Rarity::ALL {
        roll -= weights[r.index()];
        if roll < 0.0 && weights[r.index()] > 0.0 {
            return r;
        }
    }
    Rarity::ALL
        .into_iter()
        .rev()
        .find(|r| weights[r.index()] > 0.0)
        .unwrap_or(min)
}

/// Fresh affixes for a base at a rarity and hazard (no repeats).
pub fn roll_affixes(
    rng: &mut Rng,
    base: GearId,
    rarity: Rarity,
    hazard: u8,
) -> Vec<(AffixId, f32)> {
    let slot = base.info().slot;
    let count = content().tuning.equipment.affixes[rarity.index()];
    let k = scale(rarity, hazard);
    let mut pool: Vec<(AffixId, u32)> = AffixId::all()
        .filter(|a| {
            let info = a.info();
            info.slots.is_empty() || info.slots.contains(&slot)
        })
        .map(|a| (a, a.info().weight))
        .collect();
    let mut out = Vec::new();
    while out.len() < count {
        let Some(a) = weighted(rng, &pool) else {
            break;
        };
        pool.retain(|(p, _)| *p != a);
        let info = a.info();
        let roll = info.range[0] + (info.range[1] - info.range[0]) * rng.next_f32();
        out.push((a, scaled(info.effect, roll, k)));
    }
    out
}

/// A random base for `slot` (any slot when `None`) that drops at `hazard`.
fn roll_base(rng: &mut Rng, hazard: u8, slot: Option<GearSlot>) -> GearId {
    let pool: Vec<(GearId, u32)> = GearId::all()
        .filter(|g| {
            let info = g.info();
            info.min_hazard <= hazard && slot.is_none_or(|s| info.slot == s)
        })
        .map(|g| (g, g.info().weight))
        .collect();
    weighted(rng, &pool).unwrap_or(GearId(0))
}

/// A new item at `hazard`, at least `min` rare (its `id` is set when it
/// reaches the stash).
pub fn roll(rng: &mut Rng, hazard: u8, min: Rarity, luck: f32) -> Gear {
    let hazard = hazard.clamp(1, crate::mission::MAX_HAZARD);
    let rarity = roll_rarity(rng, hazard, min, luck);
    let base = roll_base(rng, hazard, None);
    Gear {
        id: 0,
        base,
        rarity,
        hazard,
        affixes: roll_affixes(rng, base, rarity, hazard),
    }
}

/// New affixes for an item, keeping its base, rarity and hazard.
pub fn reroll(rng: &mut Rng, gear: &mut Gear) {
    gear.affixes = roll_affixes(rng, gear.base, gear.rarity, gear.hazard);
}

/// Fuse items of one rarity into a single item of the next rarity, in the
/// first item's slot, at the best hazard among them.
pub fn fuse(rng: &mut Rng, items: &[Gear]) -> Option<Gear> {
    let first = items.first()?;
    let next = Rarity::ALL.get(first.rarity.index() + 1).copied()?;
    let hazard = items.iter().map(|g| g.hazard).max().unwrap_or(1);
    let base = roll_base(rng, hazard, Some(first.slot()));
    Some(Gear {
        id: 0,
        base,
        rarity: next,
        hazard,
        affixes: roll_affixes(rng, base, next, hazard),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rolls_respect_rarity_slots_and_hazard() {
        let mut rng = Rng::seed_from_u64(5);
        let mut seen = std::collections::BTreeSet::new();
        for i in 0..400 {
            let hazard = 1 + (i % 5) as u8;
            let g = roll(&mut rng, hazard, Rarity::Uncommon, 0.0);
            assert!(g.rarity >= Rarity::Uncommon);
            assert_eq!(
                g.affixes.len(),
                content().tuning.equipment.affixes[g.rarity.index()]
            );
            let ids: std::collections::BTreeSet<_> = g.affixes.iter().map(|a| a.0).collect();
            assert_eq!(ids.len(), g.affixes.len(), "affixes repeat");
            for (a, _) in &g.affixes {
                let slots = &a.info().slots;
                assert!(slots.is_empty() || slots.contains(&g.slot()));
            }
            assert!(g.base.info().min_hazard <= hazard);
            assert!(g.score() > 0.0 && g.sell_value() > 0 && g.price() > g.sell_value());
            assert!(!g.summary().is_empty());
            seen.insert(g.slot());
        }
        assert_eq!(seen.len(), 4);
        // Higher rarity and hazard roll stronger items on average.
        let mean = |rarity: Rarity, hazard: u8, rng: &mut Rng| {
            (0..200)
                .map(|_| {
                    let base = roll_base(rng, hazard, None);
                    Gear {
                        id: 0,
                        base,
                        rarity,
                        hazard,
                        affixes: roll_affixes(rng, base, rarity, hazard),
                    }
                    .score()
                })
                .sum::<f32>()
                / 200.0
        };
        assert!(mean(Rarity::Rare, 1, &mut rng) > mean(Rarity::Common, 1, &mut rng));
        assert!(mean(Rarity::Rare, 5, &mut rng) > mean(Rarity::Rare, 1, &mut rng));
        let commons: Vec<Gear> = (0..3)
            .map(|_| roll(&mut rng, 1, Rarity::Common, 0.0))
            .collect();
        let mut low = commons.clone();
        for g in &mut low {
            g.rarity = Rarity::Common;
        }
        let fused = fuse(&mut rng, &low).unwrap();
        assert_eq!(fused.rarity, Rarity::Uncommon);
        assert_eq!(fused.slot(), low[0].slot());
    }
}
