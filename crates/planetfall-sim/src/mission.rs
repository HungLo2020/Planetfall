//! Contracts and the mission board. A contract is a primary objective, an
//! optional secondary, a site condition and an optional mutator on one
//! planet, taken at a hazard level the player picks (1 to 5). The board
//! offers a few random contracts for the planet in orbit and is redrawn after
//! every drop and jump. Objective archetypes, sites and mutators are data
//! (`objectives.toml`, `sites.toml`, `mutators.toml`); their rules live in
//! `run::mission`.

use crate::content::{
    MutatorEffect, MutatorId, ObjectiveId, ObjectiveKind, ObjectiveRole, PlanetId, SiteId, content,
};
use quasar_core::Rng;
use serde::{Deserialize, Serialize};

/// Hazard levels run from 1 to this.
pub const MAX_HAZARD: u8 = 5;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Contract {
    pub planet: PlanetId,
    pub primary: ObjectiveId,
    pub secondary: Option<ObjectiveId>,
    pub site: SiteId,
    pub mutator: Option<MutatorId>,
}

impl Contract {
    /// The tutorial drop: an ore quota on the first planet, nothing extra.
    pub fn tutorial() -> Self {
        Self::tutorial_on(PlanetId::first())
    }

    /// A plain ore quota on `planet` (the tutorial's contract).
    pub fn tutorial_on(planet: PlanetId) -> Self {
        let primary = ObjectiveId::all()
            .find(|o| o.info().kind == ObjectiveKind::OreQuota)
            .expect("validated: an ore quota objective exists");
        Self {
            planet,
            primary,
            secondary: None,
            site: SiteId::all()
                .find(|s| available(&s.info().planets, planet))
                .expect("validated: every planet has a site"),
            mutator: None,
        }
    }

    pub fn has_mutator(&self, effect: MutatorEffect) -> Option<f32> {
        self.mutator
            .map(|m| m.info())
            .filter(|m| m.effect == effect)
            .map(|m| m.amount)
    }

    /// Extra share of banked materials from the mutator.
    pub fn mutator_reward(&self) -> f32 {
        self.mutator.map_or(0.0, |m| m.info().reward)
    }
}

/// Planet list check (empty: everywhere).
pub fn available(planets: &[String], planet: PlanetId) -> bool {
    planets.is_empty() || planets.iter().any(|p| p == planet.key())
}

impl ObjectiveId {
    /// The target at a hazard level (`count` plus `per_hazard` per level
    /// above 1).
    pub fn target(self, hazard: u8) -> u32 {
        let info = self.info();
        info.count + info.per_hazard * u32::from(hazard.saturating_sub(1))
    }

    /// Description with the target filled in.
    pub fn describe(self, hazard: u8) -> String {
        self.info()
            .description
            .replace("{count}", &self.target(hazard).to_string())
    }

    pub fn name(self) -> &'static str {
        &self.info().name
    }
}

impl SiteId {
    pub fn name(self) -> &'static str {
        &self.info().name
    }
}

impl MutatorId {
    pub fn name(self) -> &'static str {
        &self.info().name
    }
}

/// Hazard level names ("Easy" ... "Lethal").
pub fn hazard_name(level: u8) -> &'static str {
    let names = &content().tuning.hazard.names;
    &names[usize::from(level.clamp(1, MAX_HAZARD) - 1)]
}

fn pick<T: Copy>(rng: &mut Rng, choices: &[(T, u32)]) -> Option<T> {
    let total: u32 = choices.iter().map(|(_, w)| w).sum();
    if total == 0 {
        return None;
    }
    let mut roll = rng.range_u32(0..total);
    choices
        .iter()
        .find(|(_, w)| {
            if roll < *w {
                true
            } else {
                roll -= w;
                false
            }
        })
        .map(|(c, _)| *c)
}

/// A fresh board of contracts for `planet`, all with different primaries
/// where the data allows.
pub fn board(rng: &mut Rng, planet: PlanetId) -> Vec<Contract> {
    let tuning = &content().tuning.missions;
    let objectives = |role: ObjectiveRole| -> Vec<(ObjectiveId, u32)> {
        ObjectiveId::all()
            .filter(|o| {
                let info = o.info();
                info.role == role && available(&info.planets, planet)
            })
            .map(|o| (o, o.info().weight))
            .collect()
    };
    let sites: Vec<(SiteId, u32)> = SiteId::all()
        .filter(|s| available(&s.info().planets, planet))
        .map(|s| (s, s.info().weight))
        .collect();
    let mutators: Vec<(MutatorId, u32)> = MutatorId::all().map(|m| (m, m.info().weight)).collect();
    let mut primaries = objectives(ObjectiveRole::Primary);
    let secondaries = objectives(ObjectiveRole::Secondary);
    let mut board = Vec::new();
    for _ in 0..tuning.board {
        let Some(primary) = pick(rng, &primaries) else {
            break;
        };
        if primaries.len() > 1 {
            primaries.retain(|(o, _)| *o != primary);
        }
        let secondary = if rng.chance(f64::from(tuning.secondary_chance)) {
            pick(rng, &secondaries)
        } else {
            None
        };
        let mutator = if rng.chance(f64::from(tuning.mutator_chance)) {
            pick(rng, &mutators)
        } else {
            None
        };
        let Some(site) = pick(rng, &sites) else {
            break;
        };
        board.push(Contract {
            planet,
            primary,
            secondary,
            site,
            mutator,
        });
    }
    board
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boards_offer_varied_contracts_for_the_planet() {
        for planet in PlanetId::all() {
            let mut rng = Rng::seed_from_u64(9);
            let mut kinds = std::collections::BTreeSet::new();
            for _ in 0..30 {
                let board = board(&mut rng, planet);
                assert_eq!(board.len(), content().tuning.missions.board);
                let primaries: std::collections::BTreeSet<_> =
                    board.iter().map(|c| c.primary).collect();
                assert_eq!(primaries.len(), board.len(), "primaries repeat");
                for c in &board {
                    assert_eq!(c.planet, planet);
                    assert!(available(&c.site.info().planets, planet));
                    kinds.insert(format!("{:?}", c.primary.info().kind));
                }
            }
            assert!(kinds.len() >= 5, "{planet:?}: only {kinds:?}");
        }
        let quota = Contract::tutorial().primary;
        assert!(quota.target(3) > quota.target(1));
        assert!(quota.describe(1).contains('6'));
    }
}
