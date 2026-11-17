//! Spawn pressure and data-driven spawn tables: each enemy's `spawn` rule in
//! `enemies.toml`, scaled by the planet's `spawn_weights`.

use super::{Enemy, Extraction, Run};
use crate::content::{DT, EnemyId, content};
use crate::math::V2;
use crate::terrain::Terrain;

impl Run {
    /// Spawn pressure: hazard, time on the surface, tutorial and extraction.
    fn intensity(&self) -> f32 {
        let horde = &content().tuning.horde;
        let extraction = !matches!(self.extraction, Extraction::Idle);
        self.hazard.sqrt()
            * (1.0 + self.minutes() * horde.minutes_ramp)
            * if self.tutorial {
                horde.tutorial_intensity
            } else {
                1.0
            }
            * if extraction {
                horde.extraction_intensity
            } else {
                1.0
            }
    }

    /// Enemies allowed to spawn right now, with their planet-scaled weights.
    pub(super) fn spawn_table(&self) -> Vec<(EnemyId, f32)> {
        let horde = &content().tuning.horde;
        let minutes = self.minutes();
        let late = minutes > horde.late_minutes || !matches!(self.extraction, Extraction::Idle);
        let planet = self.planet.info();
        EnemyId::all()
            .filter_map(|id| {
                let rule = id.info().spawn;
                let allowed = minutes >= rule.after_minutes
                    && (!rule.late || late)
                    && (rule.tutorial || !self.tutorial);
                let scale = planet.spawn_weights.get(id.key()).copied().unwrap_or(1.0);
                let weight = rule.weight * scale;
                (allowed && weight > 0.0).then_some((id, weight))
            })
            .collect()
    }

    pub(super) fn update_spawner(&mut self) {
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
        let table = self.spawn_table();
        let total: f32 = table.iter().map(|(_, w)| w).sum();
        if total <= 0.0 {
            return;
        }
        let mut roll = self.rng.next_f32() * total;
        let kind = table
            .iter()
            .find(|(_, w)| {
                roll -= w;
                roll < 0.0
            })
            .map_or(table[table.len() - 1].0, |(id, _)| *id);
        let [low, high] = kind.info().spawn.count;
        let count = low + self.rng.range_u32(0..high - low + 1);
        let Some(center) = self.find_spawn_point() else {
            return;
        };
        let health_scale =
            (1.0 + self.minutes() * horde.health_ramp_per_minute) * (0.6 + 0.4 * self.hazard);
        let health = kind.info().health * health_scale;
        for n in 0..count {
            let jitter = V2::new(self.rng.range_f32(-1.2..1.2), self.rng.range_f32(-1.2..1.2));
            let pos = if self.terrain.solid_at(center + jitter) {
                center
            } else {
                center + jitter
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
                speed_scale: self.rng.range_f32(0.85..1.15),
                alerted: false,
                burning: 0,
                burn_dps: 0.0,
                corroded: 0,
                acid_dps: 0.0,
                chill: 0.0,
                frozen: 0,
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
