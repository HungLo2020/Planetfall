//! Planet hazards: lava and water underfoot, spore clouds that fire
//! ignites, heat and cold vents, thin air with an oxygen meter (only on
//! oxygen-poor planets and sites) and quakes that bring rubble down (never
//! pits). Placement comes from `[planet.hazards]` and the site; numbers from
//! `[hazards]` in `tuning.toml`.

use super::{Hit, Run};
use crate::content::{DT, Element, VentKind, content, seconds};
use crate::events::GameEvent;
use crate::math::V2;
use crate::terrain::{Ground, Terrain};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum HazardKind {
    /// A drifting spore cloud: hurts inside, fire sets it off.
    Spores,
    /// Pulses heat or cold.
    Vent(VentKind),
    /// Refills oxygen on thin-air drops.
    Oxygen,
    /// Burning ground left by a boss; `timer` is its life.
    Embers,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Hazard {
    pub kind: HazardKind,
    pub pos: V2,
    /// Ticks to the next pulse (vents).
    pub timer: u32,
    /// Ticks until an ignited spore cloud grows back (0: active).
    pub dormant: u32,
}

impl Hazard {
    pub fn active(&self) -> bool {
        self.dormant == 0
    }
}

/// Quake timing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Quake {
    /// Ticks to the next warning (0: quakes off).
    pub next: u32,
    /// Ticks until a warned quake hits.
    pub warning: u32,
    /// Ticks since the last quake hit (for camera shake).
    pub since: u32,
}

/// Hazard objects for a drop: spores and vents from the planet, oxygen
/// vents on thin air. Spots are taken from the end of the layout's list
/// (objectives use the front).
pub(super) fn place(run: &Run, spots: &[V2]) -> (Vec<Hazard>, Quake) {
    let t = &content().tuning.hazards;
    let style = &run.planet.info().hazards;
    let mut free = spots.iter().rev().copied();
    let mut out = Vec::new();
    let mut add = |kind: HazardKind, n: u32, out: &mut Vec<Hazard>| {
        for i in 0..n {
            if let Some(pos) = free.next() {
                out.push(Hazard {
                    kind,
                    pos,
                    timer: seconds(t.vent_every) + i * 17,
                    dormant: 0,
                });
            }
        }
    };
    if !run.tutorial {
        add(HazardKind::Spores, style.spores, &mut out);
        if let Some(kind) = style.vents {
            add(HazardKind::Vent(kind), style.vent_count, &mut out);
        }
        if run.thin_air() {
            add(HazardKind::Oxygen, t.oxygen_vents, &mut out);
        }
    }
    let quake = Quake {
        next: if run.quakes() && !run.tutorial {
            seconds(t.quake_every[0])
        } else {
            0
        },
        ..Quake::default()
    };
    (out, quake)
}

impl Run {
    /// Thin air: the planet or the site is oxygen-poor.
    pub fn thin_air(&self) -> bool {
        self.planet.info().hazards.oxygen || self.mission.contract.site.info().oxygen
    }

    pub fn quakes(&self) -> bool {
        self.planet.info().hazards.quakes || self.mission.contract.site.info().quakes
    }

    /// Set off spore clouds near `at` (fire hits, fiery blasts).
    pub(super) fn ignite_spores(&mut self, at: V2, radius: f32, events: &mut Vec<GameEvent>) {
        let t = &content().tuning.hazards;
        let lit: Vec<usize> = (0..self.hazards.len())
            .filter(|&i| {
                let h = &self.hazards[i];
                h.kind == HazardKind::Spores
                    && h.active()
                    && h.pos.distance(at) <= radius + t.spore_radius
            })
            .collect();
        for i in lit {
            let pos = self.hazards[i].pos;
            self.hazards[i].dormant = seconds(t.spore_regrow);
            events.push(GameEvent::SporeBlast { at: pos });
            let blast = Hit {
                damage: t.spore_blast_damage,
                elements: crate::run::Elements::single(Element::Fire, 6.0),
                secondary: true,
                ..Hit::default()
            };
            for e in self.enemies_near(pos, t.spore_blast_radius) {
                self.damage_enemy(e, blast, events);
            }
            for p in 0..self.players.len() {
                if self.players[p].alive()
                    && self.players[p].pos.distance(pos) < t.spore_blast_radius
                {
                    self.hurt_player(p, t.spore_blast_damage / 3.0, events);
                }
            }
        }
    }

    /// Every tick: ground underfoot, clouds, vents, oxygen and quakes.
    pub(super) fn update_hazards(&mut self, events: &mut Vec<GameEvent>) {
        let t = &content().tuning.hazards;
        let thin = self.thin_air();
        let pulse_every = 30;
        // Players.
        for i in 0..self.players.len() {
            let p = &mut self.players[i];
            p.jammed = p.jammed.saturating_sub(1);
            p.chilled = p.chilled.saturating_sub(1);
            p.wet = p.wet.saturating_sub(1);
            p.hazard_ticks = p.hazard_ticks.saturating_sub(1);
            if !p.alive() {
                continue;
            }
            let pos = p.pos;
            let mut hurt = 0.0;
            match self.terrain.ground_at(pos) {
                Ground::Lava => hurt += t.lava_damage,
                Ground::Water => p.wet = seconds(t.wet_seconds),
                Ground::Plain => {}
            }
            let in_spores = self.hazards.iter().any(|h| {
                h.kind == HazardKind::Spores && h.active() && h.pos.distance(pos) < t.spore_radius
            });
            if in_spores {
                hurt += t.spore_damage;
            }
            if self
                .hazards
                .iter()
                .any(|h| h.kind == HazardKind::Embers && h.pos.distance(pos) < 1.6)
            {
                hurt += t.lava_damage;
            }
            if thin {
                let refill = self.landing.distance(pos) < 3.5
                    || self.hazards.iter().any(|h| {
                        h.kind == HazardKind::Oxygen && h.pos.distance(pos) < t.vent_radius
                    });
                let p = &mut self.players[i];
                let before = p.oxygen;
                p.oxygen = if refill {
                    (p.oxygen + t.oxygen_refill * DT).min(t.oxygen_max)
                } else {
                    (p.oxygen - t.oxygen_drain * DT).max(0.0)
                };
                if before > t.oxygen_max * 0.25 && p.oxygen <= t.oxygen_max * 0.25 {
                    events.push(GameEvent::OxygenLow);
                }
                if p.oxygen <= 0.0 {
                    hurt += t.oxygen_damage;
                }
            }
            let p = &mut self.players[i];
            if hurt > 0.0 && p.hazard_ticks == 0 {
                p.hazard_ticks = pulse_every;
                // Standing hazards ignore the hit flinch's invulnerability.
                p.invulnerable_ticks = 0;
                self.hurt_player(i, hurt * pulse_every as f32 * DT, events);
            }
        }
        // Enemies: lava sets them alight, water soaks and douses them.
        for e in &mut self.enemies {
            e.wet = e.wet.saturating_sub(1);
            if e.health <= 0.0 || e.burrowed || e.flying() {
                continue;
            }
            match self.terrain.ground_at(e.pos) {
                Ground::Lava => {
                    e.burning = seconds(2.0);
                    e.burn_dps = e.burn_dps.max(t.lava_damage * 0.5);
                }
                Ground::Water => {
                    e.wet = seconds(t.wet_seconds);
                    e.burning = 0;
                }
                Ground::Plain => {}
            }
        }
        self.update_hazard_objects(events);
        self.update_quake(events);
    }

    fn update_hazard_objects(&mut self, events: &mut Vec<GameEvent>) {
        let t = &content().tuning.hazards;
        // Embers burn out.
        self.hazards.retain_mut(|h| {
            if h.kind != HazardKind::Embers {
                return true;
            }
            h.timer = h.timer.saturating_sub(1);
            h.timer > 0
        });
        for i in 0..self.hazards.len() {
            let h = &mut self.hazards[i];
            h.dormant = h.dormant.saturating_sub(1);
            let HazardKind::Vent(kind) = h.kind else {
                continue;
            };
            h.timer = h.timer.saturating_sub(1);
            if h.timer > 0 {
                continue;
            }
            h.timer = seconds(t.vent_every);
            let at = h.pos;
            events.push(GameEvent::VentPulse { at, kind });
            for e in self.enemies_near(at, t.vent_radius) {
                let en = &mut self.enemies[e];
                match kind {
                    VentKind::Heat => {
                        en.burning = seconds(3.0);
                        en.burn_dps = en.burn_dps.max(t.heat_damage * 0.5);
                    }
                    VentKind::Cold => {
                        if en.frozen == 0 {
                            en.chill += t.cold_chill;
                            if en.chill >= 1.0 {
                                en.chill = 0.0;
                                en.frozen = seconds(1.6);
                            }
                        }
                    }
                }
            }
            for p in 0..self.players.len() {
                if !self.players[p].alive() || self.players[p].pos.distance(at) > t.vent_radius {
                    continue;
                }
                match kind {
                    VentKind::Heat => self.hurt_player(p, t.heat_damage, events),
                    VentKind::Cold => self.players[p].chilled = seconds(t.chill_seconds),
                }
            }
            if kind == VentKind::Heat {
                self.ignite_spores(at, t.vent_radius, events);
            }
        }
    }

    fn update_quake(&mut self, events: &mut Vec<GameEvent>) {
        let t = &content().tuning.hazards;
        self.quake.since = self.quake.since.saturating_add(1);
        if self.quake.next == 0 {
            return;
        }
        if self.quake.warning > 0 {
            self.quake.warning -= 1;
            if self.quake.warning == 0 {
                self.collapse(events);
                let [lo, hi] = t.quake_every;
                self.quake.next = seconds(self.rng.range_f32(lo..hi.max(lo + 0.1)));
                self.quake.since = 0;
            }
            return;
        }
        self.quake.next = self.quake.next.saturating_sub(1).max(1);
        if self.quake.next == 1 {
            self.quake.warning = seconds(t.quake_warning);
            events.push(GameEvent::QuakeWarning);
        }
    }

    /// Bring rubble down on open floor around a player, away from the pod,
    /// the mission sites and anyone standing there; anything caught under
    /// it is hurt.
    fn collapse(&mut self, events: &mut Vec<GameEvent>) {
        let t = &content().tuning.hazards;
        let alive: Vec<V2> = self
            .players
            .iter()
            .filter(|p| p.alive())
            .map(|p| p.pos)
            .collect();
        let Some(&center) = alive.get(self.rng.range_usize(0..alive.len().max(1))) else {
            return;
        };
        events.push(GameEvent::Quake { at: center });
        let keep_clear: Vec<V2> = std::iter::once(self.landing)
            .chain(self.sites.iter().copied())
            .chain(self.caches.iter().map(|c| c.pos))
            .chain(self.hazards.iter().map(|h| h.pos))
            .collect();
        let mut fallen = 0;
        for _ in 0..t.quake_tiles * 12 {
            if fallen >= t.quake_tiles {
                break;
            }
            let a = self.rng.range_f32(0.0..std::f32::consts::TAU);
            let r = self.rng.range_f32(5.0..14.0);
            let p = center + V2::new(a.cos(), a.sin()) * r;
            let (x, y) = Terrain::tile_of(p);
            let c = Terrain::center_of(x, y);
            if alive.iter().any(|q| q.distance(c) < 2.2)
                || keep_clear.iter().any(|k| k.distance(c) < 5.0)
                || self
                    .mission
                    .markers(self)
                    .iter()
                    .any(|m| m.distance(c) < 3.0)
            {
                continue;
            }
            if self.terrain.collapse(x, y) {
                fallen += 1;
                events.push(GameEvent::Rubble { at: c });
                let crush = Hit {
                    damage: t.quake_damage,
                    secondary: true,
                    ..Hit::default()
                };
                for e in self.enemies_near(c, 1.6) {
                    if !self.enemies[e].flying() {
                        self.damage_enemy(e, crush, events);
                    }
                }
            }
        }
    }
}
