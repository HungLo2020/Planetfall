//! Per-table field checks and cross-table references for
//! `quasar-config`'s `ContentRegistry` (one `ContentRecord` per definition).

use super::defs::{
    AbilityKind, AffixDef, BossSpecial, ClassDef, EnemyBehavior, EnemyDef, GearDef, ModDef,
    ModStat, ModTarget, MutatorDef, ObjectiveDef, ObjectiveKind, ObjectiveRole, PerkDef, PlanetDef,
    SiteDef, UpgradeDef, WeaponDef,
};
use quasar_config::content::{ContentRecord, Validator};

impl ContentRecord for PlanetDef {
    fn id(&self) -> &str {
        &self.id
    }
    fn validate(&self, v: &mut Validator) {
        v.non_empty("name", &self.name)
            .range("hazard", self.hazard, 0.1, 10.0);
        for (enemy, weight) in &self.spawn_weights {
            v.reference("spawn_weights", "enemy", enemy)
                .finite("spawn_weights", f64::from(*weight))
                .check(
                    *weight >= 0.0,
                    "spawn_weights",
                    "weights cannot be negative",
                );
        }
        for boss in &self.bosses {
            v.reference("bosses", "enemy", boss);
        }
        let h = &self.hazards;
        v.range("hazards.lava", h.lava, 0.0, 0.3)
            .range("hazards.water", h.water, 0.0, 0.5)
            .check(
                h.vents.is_some() || h.vent_count == 0,
                "hazards.vent_count",
                "vents need a `vents` kind",
            );
    }
}

impl ContentRecord for WeaponDef {
    fn id(&self) -> &str {
        &self.id
    }
    fn validate(&self, v: &mut Validator) {
        let l = &self.stats;
        v.non_empty("name", &self.name)
            .non_empty("model", &self.model)
            .non_empty("sound", &self.sound)
            .check(
                l.cooldown > 0.0 && l.range > 0.0,
                "stats",
                "cooldown and range must be positive",
            )
            .check(l.count > 0, "stats.count", "must be at least 1");
        if let Some(e) = self.element {
            v.check(e.potency > 0.0, "element.potency", "must be positive");
        }
    }
}

impl ContentRecord for EnemyDef {
    fn id(&self) -> &str {
        &self.id
    }
    fn validate(&self, v: &mut Validator) {
        v.non_empty("name", &self.name)
            .non_empty("model", &self.model)
            .check(self.health > 0.0, "health", "must be positive")
            .check(self.radius > 0.0, "radius", "must be positive")
            .check(
                self.behavior != EnemyBehavior::Exploder || self.explosion.is_some(),
                "explosion",
                "exploders need an `explosion`",
            )
            .check(
                self.behavior != EnemyBehavior::Lunger || self.lunge.is_some(),
                "lunge",
                "lungers need a `lunge`",
            )
            .check(
                self.lunge.is_none_or(|l| {
                    [l.range, l.windup, l.speed, l.time, l.recover, l.cooldown]
                        .iter()
                        .all(|v| v.is_finite() && *v > 0.0)
                }),
                "lunge",
                "lunge times, range and speed must be positive",
            )
            .check(
                self.behavior != EnemyBehavior::Spitter || self.ranged.is_some(),
                "ranged",
                "spitters need `ranged`",
            )
            .check(
                self.behavior != EnemyBehavior::Disruptor || self.pulse.is_some(),
                "pulse",
                "disruptors need a `pulse`",
            )
            .check(
                self.behavior != EnemyBehavior::Burrower || self.burrow.is_some(),
                "burrow",
                "burrowers need a `burrow`",
            )
            .check(
                self.spawn.count[0] > 0 && self.spawn.count[0] <= self.spawn.count[1],
                "spawn.count",
                "must be 1..=max",
            );
        for (i, d) in self.drops.iter().enumerate() {
            v.range(&format!("drops[{i}].chance"), d.chance, 0.0, 1.0);
        }
        if let Some(BossSpecial::Brood { enemy, .. }) = &self.special {
            v.reference("special.enemy", "enemy", enemy);
        }
    }
}

impl ContentRecord for PerkDef {
    fn id(&self) -> &str {
        &self.id
    }
    fn validate(&self, v: &mut Validator) {
        v.non_empty("title", &self.title)
            .finite("amount", f64::from(self.amount))
            .optional_reference("class", "class", self.class.as_deref());
    }
}

impl ContentRecord for ModDef {
    fn id(&self) -> &str {
        &self.id
    }
    fn validate(&self, v: &mut Validator) {
        v.non_empty("title", &self.title)
            .check(self.weight > 0, "weight", "must be at least 1")
            .check(
                self.amounts.iter().all(|a| a.is_finite() && *a >= 0.0),
                "amounts",
                "must be non-negative",
            )
            .check(
                (self.stat == ModStat::Element) == self.element.is_some(),
                "element",
                "element mods (and only they) name an element",
            )
            .check(
                (self.stat == ModStat::Special) == self.special.is_some(),
                "special",
                "special mods (and only they) name a special",
            );
        let weapon_stat = matches!(
            self.stat,
            ModStat::Damage
                | ModStat::FireRate
                | ModStat::Count
                | ModStat::Range
                | ModStat::Area
                | ModStat::Pierce
                | ModStat::Element
        );
        let ability_stat = matches!(
            self.stat,
            ModStat::Cooldown | ModStat::Power | ModStat::Reach
        );
        match self.target {
            ModTarget::Weapon => v.check(
                !ability_stat && self.abilities.is_empty(),
                "stat",
                "weapon mods change weapon stats",
            ),
            ModTarget::Ability => v.check(
                !weapon_stat && self.behaviors.is_empty(),
                "stat",
                "ability mods change ability stats",
            ),
        };
    }
}

impl ContentRecord for ObjectiveDef {
    fn id(&self) -> &str {
        &self.id
    }
    fn validate(&self, v: &mut Validator) {
        use ObjectiveKind as K;
        let primary = matches!(
            self.kind,
            K::OreQuota | K::Boss | K::Rescue | K::Defend | K::Salvage | K::Hunt
        );
        let needs_enemy = matches!(self.kind, K::Boss | K::Hunt | K::Cull);
        v.non_empty("name", &self.name)
            .check(self.count > 0, "count", "must be at least 1")
            .check(self.weight > 0, "weight", "must be at least 1")
            .check(
                primary == (self.role == ObjectiveRole::Primary),
                "role",
                "the kind decides whether it is primary or secondary",
            )
            .check(
                needs_enemy == self.enemy.is_some(),
                "enemy",
                "boss, hunt and cull objectives (only) name an enemy",
            )
            .optional_reference("enemy", "enemy", self.enemy.as_deref());
        for planet in &self.planets {
            v.reference("planets", "planet", planet);
        }
    }
}

impl ContentRecord for SiteDef {
    fn id(&self) -> &str {
        &self.id
    }
    fn validate(&self, v: &mut Validator) {
        v.non_empty("name", &self.name)
            .check(self.weight > 0, "weight", "must be at least 1")
            .check(
                [self.ore, self.enemy_health, self.spawn, self.threat]
                    .iter()
                    .all(|k| *k > 0.0),
                "multipliers",
                "must be positive",
            );
        for planet in &self.planets {
            v.reference("planets", "planet", planet);
        }
        for enemy in self.spawn_weights.keys() {
            v.reference("spawn_weights", "enemy", enemy);
        }
    }
}

impl ContentRecord for MutatorDef {
    fn id(&self) -> &str {
        &self.id
    }
    fn validate(&self, v: &mut Validator) {
        v.non_empty("name", &self.name)
            .check(self.weight > 0, "weight", "must be at least 1")
            .check(self.reward >= 0.0, "reward", "cannot be negative");
    }
}

impl ContentRecord for GearDef {
    fn id(&self) -> &str {
        &self.id
    }
    fn validate(&self, v: &mut Validator) {
        v.non_empty("name", &self.name)
            .check(self.weight > 0, "weight", "must be at least 1")
            .finite("implicit.amount", f64::from(self.implicit.amount))
            .range("min_hazard", f32::from(self.min_hazard), 1.0, 5.0);
    }
}

impl ContentRecord for AffixDef {
    fn id(&self) -> &str {
        &self.id
    }
    fn validate(&self, v: &mut Validator) {
        v.non_empty("name", &self.name)
            .check(self.weight > 0, "weight", "must be at least 1")
            .check(
                self.range[0].is_finite() && self.range[0] <= self.range[1],
                "range",
                "must be [low, high]",
            );
    }
}

impl ContentRecord for ClassDef {
    fn id(&self) -> &str {
        &self.id
    }
    fn validate(&self, v: &mut Validator) {
        v.non_empty("name", &self.name)
            .reference("signature", "weapon", &self.signature)
            .check(
                !self.ability.tiers.is_empty()
                    && self.ability.tiers.len() == self.ability_levels.len(),
                "ability_levels",
                "needs one level per ability tier",
            )
            .check(
                !self.passive_levels.is_empty(),
                "passive_levels",
                "needs at least one level",
            )
            .check(
                self.ability_levels.first() == Some(&1) && self.passive_levels.first() == Some(&1),
                "ability_levels",
                "the first tiers apply from class level 1",
            );
        for weapon in &self.familiar {
            v.reference("familiar", "weapon", weapon);
        }
        for (i, unlock) in self.unlocks.iter().enumerate() {
            v.reference(&format!("unlocks[{i}].weapon"), "weapon", &unlock.weapon)
                .check(
                    self.familiar.contains(&unlock.weapon),
                    &format!("unlocks[{i}].weapon"),
                    "unlocks must be familiar weapons",
                );
        }
        for (i, tier) in self.ability.tiers.iter().enumerate() {
            v.check(
                tier.cooldown > 0.0,
                &format!("ability.tiers[{i}].cooldown"),
                "must be positive",
            )
            .optional_reference(
                &format!("ability.tiers[{i}].weapon"),
                "weapon",
                tier.weapon.as_deref(),
            );
            if self.ability.kind == AbilityKind::Dome {
                v.check(
                    tier.duration > 0.0 && tier.range > 0.0,
                    &format!("ability.tiers[{i}]"),
                    "a dome needs a duration and a radius",
                );
            }
        }
    }
}

impl ContentRecord for UpgradeDef {
    fn id(&self) -> &str {
        &self.id
    }
    fn validate(&self, v: &mut Validator) {
        let [x0, y0, x1, y1] = self.room;
        v.non_empty("name", &self.name)
            .non_empty("station", &self.station)
            .check(!self.costs.is_empty(), "costs", "needs at least one cost")
            .check(x0 <= x1 && y0 <= y1, "room", "rect is inverted");
    }
}
