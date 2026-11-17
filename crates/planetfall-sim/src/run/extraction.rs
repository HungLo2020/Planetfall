//! Extraction (pod inbound, hold the zone, liftoff) and short-lived visual
//! state (blasts, beams). Timings come from `tuning.toml`.

use super::{Blast, BlastKind, Extraction, Run, RunOutcome, RunState};
use crate::content::{content, seconds};
use crate::events::GameEvent;

/// Ticks from calling the pod to touchdown.
pub fn extraction_inbound() -> u32 {
    seconds(content().tuning.extraction.inbound).max(1)
}

/// Ticks the player must hold the zone.
pub fn extraction_hold() -> u32 {
    seconds(content().tuning.extraction.hold).max(1)
}

/// Radius of the hold zone in metres.
pub fn extraction_radius() -> f32 {
    content().tuning.extraction.radius
}

impl Run {
    pub(super) fn update_extraction(&mut self, call: bool, events: &mut Vec<GameEvent>) {
        if call && matches!(self.extraction, Extraction::Idle) {
            if self.objective_done() {
                self.extraction = Extraction::Inbound {
                    ticks: extraction_inbound(),
                    pos: self.landing,
                };
                self.spawning_enabled = true;
                events.push(GameEvent::ExtractionCalled);
            } else {
                events.push(GameEvent::ExtractionDenied);
            }
        }
        match self.extraction {
            Extraction::Inbound { ticks, pos } if ticks <= 1 => {
                self.extraction = Extraction::Waiting { pos, hold: 0 };
                self.blasts.push(Blast {
                    kind: BlastKind::PodLanding,
                    pos,
                    radius: 3.0,
                    ttl: 30,
                    total: 30,
                });
                for i in self.enemies_near(pos, 3.0) {
                    self.enemies[i].health = 0.0;
                }
                events.push(GameEvent::PodLanded { at: pos });
            }
            Extraction::Inbound { ticks, pos } => {
                self.extraction = Extraction::Inbound {
                    ticks: ticks - 1,
                    pos,
                };
            }
            Extraction::Waiting { pos, hold } => {
                // Every living player must be in the zone; the fastest
                // holder's extraction-speed bonus applies.
                let alive = self.players.iter().filter(|p| p.alive());
                let inside = alive
                    .clone()
                    .all(|p| p.pos.distance(pos) < extraction_radius());
                let speed = alive.map(|p| p.extraction_speed).fold(0.0, f32::max);
                let hold = if inside {
                    self.hold_carry += 1.0 + speed;
                    let step = self.hold_carry.floor();
                    self.hold_carry -= step;
                    hold + step as u32
                } else {
                    self.hold_carry = 0.0;
                    hold.saturating_sub(2)
                };
                if hold >= extraction_hold() {
                    self.extraction = Extraction::Departed { pos };
                    self.state = RunState::Ended(RunOutcome::Extracted);
                    events.push(GameEvent::Liftoff);
                    events.push(GameEvent::RunEnded { extracted: true });
                } else {
                    self.extraction = Extraction::Waiting { pos, hold };
                }
            }
            Extraction::Idle | Extraction::Departed { .. } => {}
        }
    }

    pub(super) fn update_blasts(&mut self) {
        for b in &mut self.blasts {
            b.ttl = b.ttl.saturating_sub(1);
        }
        self.blasts.retain(|b| b.ttl > 0);
    }

    pub(super) fn update_effects(&mut self) {
        for b in &mut self.beams {
            b.ttl = b.ttl.saturating_sub(1);
        }
        self.beams.retain(|b| b.ttl > 0);
    }
}
