//! The contract during a drop: objective progress and the set pieces that
//! go with it (boss arena, survivor, drill rig, wreck and parts, hunted
//! elite, glowcaps, relays), plus the contract's modifiers (hazard level,
//! site condition, mutator) applied to the run.

use super::{Affix, Enemy, PickupKind, Run};
use crate::content::{DT, EnemyId, Material, MutatorEffect, ObjectiveKind, content, seconds};
use crate::events::GameEvent;
use crate::math::{self, V2};
use crate::mission::Contract;
use crate::terrain::{Layout, Terrain};
use serde::{Deserialize, Serialize};

/// Progress toward one objective.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Progress {
    pub count: u32,
    pub need: u32,
    pub done: bool,
}

impl Progress {
    fn new(need: u32) -> Self {
        Self {
            count: 0,
            need: need.max(1),
            done: false,
        }
    }

    /// Add `n`; returns true when this completes it.
    fn add(&mut self, n: u32) -> bool {
        if self.done {
            return false;
        }
        self.count = (self.count + n).min(self.need);
        self.done = self.count >= self.need;
        self.done
    }
}

/// The stranded prospector of a rescue.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Survivor {
    pub pos: V2,
    pub yaw: f32,
    pub walk: f32,
    pub moving: bool,
    pub health: f32,
    pub max_health: f32,
    /// Found and walking with the nearest player.
    pub following: bool,
    pub down: bool,
    /// 0..1 progress of a revive.
    pub revive: f32,
    /// Reached the pod.
    pub boarded: bool,
    pub hurt_ticks: u32,
}

/// The drill rig of a defend objective.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Rig {
    pub pos: V2,
    pub health: f32,
    pub max_health: f32,
    /// 0..1 progress of starting it (players standing close).
    pub start: f32,
    pub active: bool,
    /// Ticks drilled, and the ticks it needs.
    pub drilled: u32,
    pub need: u32,
    pub hurt_ticks: u32,
}

impl Rig {
    pub fn broken(&self) -> bool {
        self.health <= 0.0
    }
}

/// A relay to scan for a survey.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Relay {
    pub pos: V2,
    /// 0..1 scan progress.
    pub scan: f32,
    pub done: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MissionState {
    pub contract: Contract,
    pub hazard: u8,
    pub primary: Progress,
    pub secondary: Option<Progress>,
    /// Boss arena centre; the boss wakes when a player comes close.
    pub arena: Option<V2>,
    /// The boss or hunted elite (enemy id) once spawned.
    pub target: Option<u32>,
    pub survivor: Option<Survivor>,
    pub rig: Option<Rig>,
    /// The crashed wreck and the parts still lying around it.
    pub wreck: Option<V2>,
    pub parts: Vec<V2>,
    pub glowcaps: Vec<V2>,
    pub relays: Vec<Relay>,
}

/// How close a player must be to pick something up or work a set piece.
const REACH: f32 = 2.2;
/// A boss wakes when a player is this close to its arena.
const WAKE: f32 = 16.0;

impl MissionState {
    pub(super) fn new(
        contract: Contract,
        hazard: u8,
        layout: &Layout,
        terrain: &Terrain,
        rng: &mut quasar_core::Rng,
    ) -> Self {
        let tuning = &content().tuning.missions;
        let primary = contract.primary;
        let kind = primary.info().kind;
        let need = primary.target(hazard);
        let site = |i: usize| layout.sites.get(i).copied().unwrap_or(layout.landing);
        let mut spots = layout.spots.iter().copied();
        let mut state = Self {
            contract,
            hazard,
            primary: Progress::new(need),
            secondary: contract.secondary.map(|s| Progress::new(s.target(hazard))),
            arena: None,
            target: None,
            survivor: None,
            rig: None,
            wreck: None,
            parts: Vec::new(),
            glowcaps: Vec::new(),
            relays: Vec::new(),
        };
        match kind {
            ObjectiveKind::Boss => state.arena = Some(site(0)),
            ObjectiveKind::Rescue => {
                state.survivor = Some(Survivor {
                    pos: site(0),
                    yaw: 0.0,
                    walk: 0.0,
                    moving: false,
                    health: tuning.survivor_health,
                    max_health: tuning.survivor_health,
                    following: false,
                    down: false,
                    revive: 0.0,
                    boarded: false,
                    hurt_ticks: 0,
                })
            }
            ObjectiveKind::Defend => {
                state.rig = Some(Rig {
                    pos: site(0),
                    health: tuning.rig_health,
                    max_health: tuning.rig_health,
                    start: 0.0,
                    active: false,
                    drilled: 0,
                    need: seconds(need as f32),
                    hurt_ticks: 0,
                });
                state.primary = Progress::new(1);
            }
            ObjectiveKind::Salvage => {
                let wreck = site(0);
                state.wreck = Some(wreck);
                // Most parts scattered around the wreck, the rest further out.
                for i in 0..need {
                    let a = i as f32 * 2.4 + rng.range_f32(0.0..0.6);
                    let r = 3.0 + rng.range_f32(0.0..2.5);
                    let near = wreck + V2::new(a.sin(), a.cos()) * r;
                    // Parts lie on open floor (never inside rock).
                    let pos = if i % 3 == 2 || terrain.solid_at(near) {
                        spots.next().unwrap_or(wreck)
                    } else {
                        near
                    };
                    state.parts.push(pos);
                }
            }
            _ => {}
        }
        if let Some(secondary) = contract.secondary {
            let n = secondary.target(hazard) as usize;
            match secondary.info().kind {
                ObjectiveKind::Glowcaps => state.glowcaps = spots.by_ref().take(n).collect(),
                ObjectiveKind::Scan => {
                    state.relays = (1..=n)
                        .map(|i| Relay {
                            pos: site(i % layout.sites.len().max(1)) + V2::new(1.5, 0.0),
                            scan: 0.0,
                            done: false,
                        })
                        .collect()
                }
                _ => {}
            }
        }
        state
    }

    pub fn primary_kind(&self) -> ObjectiveKind {
        self.contract.primary.info().kind
    }

    /// Where the HUD points for the current objective, if anywhere.
    pub fn markers(&self, run: &Run) -> Vec<V2> {
        let mut out = Vec::new();
        if !self.primary.done {
            match self.primary_kind() {
                ObjectiveKind::Boss | ObjectiveKind::Hunt => {
                    if let Some(e) = self
                        .target
                        .and_then(|id| run.enemies.iter().find(|e| e.id == id))
                    {
                        out.push(e.pos);
                    } else if let Some(arena) = self.arena {
                        out.push(arena);
                    }
                }
                ObjectiveKind::Rescue => {
                    if let Some(s) = self.survivor {
                        out.push(if s.following && !s.down {
                            run.landing
                        } else {
                            s.pos
                        });
                    }
                }
                ObjectiveKind::Defend => out.extend(self.rig.map(|r| r.pos)),
                ObjectiveKind::Salvage => out.extend(self.parts.iter().copied()),
                _ => {}
            }
        }
        if self.secondary.is_some_and(|s| !s.done) {
            out.extend(self.relays.iter().filter(|r| !r.done).map(|r| r.pos));
        }
        out
    }
}

impl Run {
    pub fn objective_done(&self) -> bool {
        self.mission.primary.done
    }

    /// Hazard-level table value for this drop.
    pub(super) fn hazard_k(&self, table: &[f32; 5]) -> f32 {
        table[usize::from(self.mission.hazard.clamp(1, 5) - 1)]
    }

    /// Ore drop multiplier (site and mutator).
    pub(super) fn ore_multiplier(&self) -> f32 {
        let c = &self.mission.contract;
        c.site.info().ore * c.has_mutator(MutatorEffect::RichVeins).unwrap_or(1.0)
    }

    /// Progress the contract: set pieces, pickups and completion events.
    pub(super) fn update_mission(&mut self, events: &mut Vec<GameEvent>) {
        let tuning = &content().tuning.missions;
        let alive: Vec<V2> = self
            .players
            .iter()
            .filter(|p| p.alive())
            .map(|p| p.pos)
            .collect();
        let near = |at: V2, r: f32| alive.iter().any(|p| p.distance(at) < r);
        let kind = self.mission.primary_kind();
        // Ore quota: lumite carried.
        if kind == ObjectiveKind::OreQuota {
            let have = self.carried.lumite;
            let p = &mut self.mission.primary;
            if !p.done {
                p.count = have.min(p.need);
                if have >= p.need {
                    p.done = true;
                    events.push(GameEvent::ObjectiveComplete);
                }
            }
        }
        // Boss: wakes when someone approaches the arena.
        if let Some(arena) = self.mission.arena
            && self.mission.target.is_none()
            && near(arena, WAKE)
            && let Some(kind) = self.mission.contract.target_enemy()
        {
            let id = self.spawn_special(kind, arena, None, 1.0);
            self.mission.target = Some(id);
            events.push(GameEvent::BossAwakened { at: arena });
        }
        // Hunt: the marked elite is out there from the start.
        if kind == ObjectiveKind::Hunt
            && self.mission.target.is_none()
            && !self.mission.primary.done
            && let Some(kind) = self.mission.contract.target_enemy()
        {
            let site = self.landing_site_far();
            let id = self.spawn_special(kind, site, Some(Affix::Armored), 2.5);
            self.mission.target = Some(id);
        }
        self.update_survivor(events, &alive);
        self.update_rig(events, &alive);
        // Salvage parts, glowcaps and relays.
        let before = self.mission.parts.len();
        self.mission
            .parts
            .retain(|p| !alive.iter().any(|a| a.distance(*p) < REACH * 0.6));
        let found = (before - self.mission.parts.len()) as u32;
        if found > 0 {
            events.push(GameEvent::ObjectiveProgress);
            if self.mission.primary.add(found) {
                events.push(GameEvent::ObjectiveComplete);
            }
        }
        let before = self.mission.glowcaps.len();
        self.mission
            .glowcaps
            .retain(|p| !alive.iter().any(|a| a.distance(*p) < REACH * 0.5));
        let found = (before - self.mission.glowcaps.len()) as u32;
        if found > 0 {
            events.push(GameEvent::PickedUpXp);
            self.secondary_progress(found, events);
        }
        let scan = DT / tuning.scan_seconds.max(0.1);
        let mut scanned = 0;
        for relay in &mut self.mission.relays {
            if relay.done || !alive.iter().any(|a| a.distance(relay.pos) < REACH) {
                continue;
            }
            relay.scan += scan;
            if relay.scan >= 1.0 {
                relay.done = true;
                scanned += 1;
            }
        }
        if scanned > 0 {
            self.secondary_progress(scanned, events);
        }
    }

    fn secondary_progress(&mut self, n: u32, events: &mut Vec<GameEvent>) {
        if let Some(s) = &mut self.mission.secondary
            && s.add(n)
        {
            events.push(GameEvent::SecondaryComplete);
        }
    }

    /// Kills feed boss, hunt and cull objectives.
    pub(super) fn mission_kill(&mut self, enemy: &Enemy, events: &mut Vec<GameEvent>) {
        if self.mission.target == Some(enemy.id) {
            self.mission.target = None;
            self.mission.arena = None;
            if self.mission.primary.add(1) {
                events.push(GameEvent::ObjectiveComplete);
            }
        }
        if let Some(secondary) = self.mission.contract.secondary
            && secondary.info().kind == ObjectiveKind::Cull
            && secondary.info().enemy.as_deref() == Some(enemy.kind.key())
        {
            self.secondary_progress(1, events);
        }
    }

    /// The farthest mission site from the pod (for hunted elites).
    fn landing_site_far(&self) -> V2 {
        self.sites
            .iter()
            .copied()
            .max_by(|a, b| {
                a.distance(self.landing)
                    .total_cmp(&b.distance(self.landing))
            })
            .unwrap_or(self.landing)
    }

    /// Spawn a boss or a marked elite; returns its id.
    fn spawn_special(&mut self, kind: EnemyId, at: V2, elite: Option<Affix>, health: f32) -> u32 {
        let threat = content().tuning.threat.health_per_level;
        let hp = kind.info().health
            * health
            * self.hazard_k(&content().tuning.hazard.enemy_health)
            * (1.0 + self.threat_curve() * threat);
        let id = self.next_id;
        self.next_id += 1;
        self.enemies.push(Enemy {
            spawn_ticks: 30,
            elite,
            ..Enemy::fresh(id, kind, at, hp)
        });
        id
    }

    fn update_survivor(&mut self, events: &mut Vec<GameEvent>, alive: &[V2]) {
        let tuning = &content().tuning.missions;
        let landing = self.landing;
        let Some(mut s) = self.mission.survivor else {
            return;
        };
        if s.boarded {
            return;
        }
        s.hurt_ticks = s.hurt_ticks.saturating_sub(1);
        let nearest = alive
            .iter()
            .copied()
            .min_by(|a, b| a.distance(s.pos).total_cmp(&b.distance(s.pos)));
        if s.down {
            // Revive by standing close.
            if nearest.is_some_and(|p| p.distance(s.pos) < REACH) {
                s.revive += DT / tuning.revive_seconds.max(0.1);
                if s.revive >= 1.0 {
                    s.down = false;
                    s.revive = 0.0;
                    s.health = s.max_health * 0.5;
                    events.push(GameEvent::SurvivorFound);
                }
            } else {
                s.revive = (s.revive - DT).max(0.0);
            }
        } else if !s.following {
            if nearest.is_some_and(|p| p.distance(s.pos) < REACH * 1.5) {
                s.following = true;
                events.push(GameEvent::SurvivorFound);
            }
        } else if let Some(leader) = nearest {
            // Walk behind the nearest player, around corners along the
            // horde's flow field (which leads to the players) when the way
            // is not straight.
            let to = leader - s.pos;
            let dist = to.length();
            s.moving = dist > 1.6;
            if s.moving {
                let toward = if self.terrain.line_clear(s.pos, leader) {
                    to
                } else {
                    self.flow
                        .as_ref()
                        .and_then(|f| f.next_waypoint(&self.terrain, s.pos))
                        .map_or(to, |w| w - s.pos)
                };
                let step = toward.normalize_or_zero() * (5.2 * DT).min(dist - 1.4);
                let (pos, _) = self.terrain.resolve_circle(s.pos + step, 0.4);
                s.pos = pos;
                s.yaw = math::turn_toward(s.yaw, math::yaw_of(to), 0.3);
                s.walk += 5.2 * DT * 1.6;
            }
        }
        if s.following && !s.down && s.pos.distance(landing) < 3.5 {
            s.boarded = true;
            if self.mission.primary.add(1) {
                events.push(GameEvent::ObjectiveComplete);
            }
        }
        self.mission.survivor = Some(s);
    }

    fn update_rig(&mut self, events: &mut Vec<GameEvent>, alive: &[V2]) {
        let tuning = &content().tuning.missions;
        let Some(mut rig) = self.mission.rig else {
            return;
        };
        rig.hurt_ticks = rig.hurt_ticks.saturating_sub(1);
        let attended = alive.iter().any(|p| p.distance(rig.pos) < REACH);
        if !rig.active {
            if attended {
                rig.start += DT / tuning.rig_start_seconds.max(0.1);
                if rig.start >= 1.0 {
                    rig.active = true;
                    events.push(GameEvent::RigStarted { at: rig.pos });
                }
            }
        } else if rig.broken() {
            // Repair by standing close.
            if attended {
                rig.health = (rig.health + rig.max_health * 0.25 * DT).min(rig.max_health);
            }
        } else if !self.mission.primary.done {
            rig.drilled += 1;
            if rig.drilled.is_multiple_of(30) {
                events.push(GameEvent::MineHit {
                    at: rig.pos,
                    material: None,
                });
            }
            if rig.drilled >= rig.need {
                self.mission.primary.add(1);
                events.push(GameEvent::ObjectiveComplete);
                // The finished rig coughs up a little ore.
                for _ in 0..3 {
                    self.drop_pickup(PickupKind::Material(Material::Lumite, 2), rig.pos);
                }
            }
        }
        self.mission.rig = Some(rig);
    }

    /// Something the horde can attack besides players: a following survivor
    /// or an active rig, as (position, radius).
    pub(super) fn mission_targets(&self) -> Vec<(V2, f32)> {
        let mut out = Vec::new();
        if let Some(s) = self.mission.survivor
            && s.following
            && !s.down
            && !s.boarded
        {
            out.push((s.pos, 0.4));
        }
        if let Some(r) = self.mission.rig
            && r.active
            && !r.broken()
            && !self.mission.primary.done
        {
            out.push((r.pos, 0.9));
        }
        out
    }

    /// Damage a mission target near `at` (from `mission_targets`).
    pub(super) fn hurt_mission_target(&mut self, at: V2, amount: f32, events: &mut Vec<GameEvent>) {
        if let Some(s) = &mut self.mission.survivor
            && s.pos.distance(at) < 0.5
            && !s.down
        {
            s.health -= amount;
            s.hurt_ticks = 18;
            if s.health <= 0.0 {
                s.health = 0.0;
                s.down = true;
                events.push(GameEvent::SurvivorDown);
            }
            return;
        }
        if let Some(r) = &mut self.mission.rig
            && r.pos.distance(at) < 0.5
        {
            r.health = (r.health - amount).max(0.0);
            r.hurt_ticks = 18;
            if r.broken() {
                events.push(GameEvent::RigBroken);
            }
        }
    }
}
