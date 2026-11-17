//! Escalation and spawning: the threat level that rises all drop long,
//! surge waves, elites and the data-driven spawn tables (each enemy's
//! `spawn` rule in `enemies.toml`, scaled by the planet's `spawn_weights`).

use super::{Affix, Enemy, Extraction, Run};
use crate::content::{DT, EnemyId, content, seconds};
use crate::events::GameEvent;
use crate::math::V2;
use crate::terrain::Terrain;

impl Run {
    /// Raise the threat and run surge warnings and waves.
    pub(super) fn update_threat(&mut self, events: &mut Vec<GameEvent>) {
        let t = &content().tuning.threat;
        if !self.spawning_enabled || self.tutorial {
            return;
        }
        let before = self.threat_level();
        let rate = if matches!(self.extraction, Extraction::Idle) {
            1.0
        } else {
            t.extraction_rate
        };
        // Planet hazard already makes enemies tougher; the clock is the same.
        self.threat += t.per_minute * rate * DT / 60.0;
        let level = self.threat_level();
        if level > before {
            events.push(GameEvent::ThreatRose { level });
        }
        let surge = &mut self.surge;
        surge.active = surge.active.saturating_sub(1);
        if surge.warning > 0 {
            surge.warning -= 1;
            if surge.warning == 0 {
                surge.active = seconds(t.surge_duration);
                let burst = t.surge_burst + t.surge_burst_per_level * level;
                events.push(GameEvent::SurgeStarted { level });
                self.surge_burst(burst);
            }
        } else if level >= surge.next {
            surge.next = level + t.surge_every.max(1);
            surge.warning = seconds(t.surge_warning).max(1);
            events.push(GameEvent::SurgeWarning {
                seconds: t.surge_warning.ceil() as u32,
            });
        }
    }

    /// Spawn pressure: hazard, threat, tutorial and surges.
    fn intensity(&self) -> f32 {
        let tuning = &content().tuning;
        let mut k = self.hazard.sqrt() * (1.0 + self.threat * tuning.threat.spawn_per_level);
        if self.tutorial {
            k *= tuning.horde.tutorial_intensity;
        }
        if self.surge.active > 0 {
            k *= tuning.threat.surge_intensity;
        }
        if !matches!(self.extraction, Extraction::Idle) {
            k *= tuning.threat.extraction_intensity;
        }
        k
    }

    /// Enemies allowed to spawn right now, with their planet-scaled weights.
    pub(super) fn spawn_table(&self) -> Vec<(EnemyId, f32)> {
        let planet = self.planet.info();
        EnemyId::all()
            .filter_map(|id| {
                let rule = id.info().spawn;
                let allowed = self.threat >= rule.threat && (rule.tutorial || !self.tutorial);
                let scale = planet.spawn_weights.get(id.key()).copied().unwrap_or(1.0);
                let weight = rule.weight * scale;
                (allowed && weight > 0.0).then_some((id, weight))
            })
            .collect()
    }

    fn pick_kind(&mut self) -> Option<EnemyId> {
        let table = self.spawn_table();
        let total: f32 = table.iter().map(|(_, w)| w).sum();
        if total <= 0.0 {
            return None;
        }
        let mut roll = self.rng.next_f32() * total;
        Some(
            table
                .iter()
                .find(|(_, w)| {
                    roll -= w;
                    roll < 0.0
                })
                .map_or(table[table.len() - 1].0, |(id, _)| *id),
        )
    }

    pub(super) fn update_spawner(&mut self, _events: &mut Vec<GameEvent>) {
        let horde = &content().tuning.horde;
        if !self.spawning_enabled || matches!(self.extraction, Extraction::Departed { .. }) {
            return;
        }
        let cap = if self.tutorial {
            horde.tutorial_cap
        } else {
            horde.cap
        };
        if self.enemies.len() >= cap {
            return;
        }
        // One group every `spawn_period` seconds at intensity 1.
        self.spawn_timer += DT * self.intensity();
        if self.spawn_timer < horde.spawn_period {
            return;
        }
        self.spawn_timer -= horde.spawn_period;
        let Some(kind) = self.pick_kind() else {
            return;
        };
        let [low, high] = kind.info().spawn.count;
        let count = low + self.rng.range_u32(0..high - low + 1);
        let Some(center) = self.find_spawn_point() else {
            return;
        };
        self.spawn_group(kind, center, count);
    }

    /// A surge: groups from several directions at once.
    fn surge_burst(&mut self, total: u32) {
        let cap = content().tuning.horde.cap;
        let groups = 4;
        for _ in 0..groups {
            if self.enemies.len() >= cap {
                break;
            }
            let Some(kind) = self.pick_kind() else {
                return;
            };
            let Some(center) = self.find_spawn_point() else {
                continue;
            };
            self.spawn_group(kind, center, (total / groups).max(1));
        }
    }

    fn spawn_group(&mut self, kind: EnemyId, center: V2, count: u32) {
        let tuning = &content().tuning;
        let health = kind.info().health
            * (1.0 + self.threat_curve() * tuning.threat.health_per_level)
            * (0.6 + 0.4 * self.hazard);
        let elite_chance = if self.tutorial {
            0.0
        } else {
            (self.threat * tuning.elites.per_level).min(tuning.elites.max)
        };
        for n in 0..count {
            let jitter = V2::new(self.rng.range_f32(-1.2..1.2), self.rng.range_f32(-1.2..1.2));
            let pos = if self.terrain.solid_at(center + jitter) {
                center
            } else {
                center + jitter
            };
            let elite = (elite_chance > 0.0 && self.rng.chance(f64::from(elite_chance)))
                .then(|| Affix::ALL[self.rng.range_usize(0..Affix::ALL.len())]);
            let e = &tuning.elites;
            let health = health
                * match elite {
                    Some(Affix::Armored) => e.armored_health,
                    Some(Affix::Swift) => e.swift_health,
                    Some(Affix::Frenzied) => e.frenzied_health,
                    None => 1.0,
                };
            let speed = if elite == Some(Affix::Swift) {
                e.swift_speed
            } else {
                1.0
            };
            let id = self.next_id;
            self.next_id += 1;
            self.enemies.push(Enemy {
                id,
                kind,
                pos,
                yaw: self.rng.range_f32(0.0..std::f32::consts::TAU),
                health,
                max_health: health,
                hit_ticks: 0,
                attack_ticks: 0,
                walk: n as f32,
                spawn_ticks: 24,
                attack_cooldown: 30,
                drone_cooldown: 0,
                speed_scale: self.rng.range_f32(0.85..1.15) * speed,
                alerted: false,
                burning: 0,
                burn_dps: 0.0,
                corroded: 0,
                acid_dps: 0.0,
                chill: 0.0,
                frozen: 0,
                elite,
            });
        }
    }

    /// An open cell 17–26 m from a living player that the horde can path
    /// from.
    fn find_spawn_point(&mut self) -> Option<V2> {
        let alive: Vec<V2> = self
            .players
            .iter()
            .filter(|p| p.alive())
            .map(|p| p.pos)
            .collect();
        let around = match alive.len() {
            0 => return None,
            1 => alive[0],
            n => alive[self.rng.range_usize(0..n)],
        };
        for _ in 0..40 {
            let angle = self.rng.range_f32(0.0..std::f32::consts::TAU);
            let dist = self.rng.range_f32(17.0..26.0);
            let p = around + V2::new(angle.sin(), angle.cos()) * dist;
            let (x, y) = Terrain::tile_of(p);
            if self.terrain.tile(x, y).solid() {
                continue;
            }
            let reachable = self
                .flow
                .as_ref()
                .is_some_and(|f| f.distance_at(x, y) < 40.0);
            if reachable {
                return Some(Terrain::center_of(x, y));
            }
        }
        None
    }
}
