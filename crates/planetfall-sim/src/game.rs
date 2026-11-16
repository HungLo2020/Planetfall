//! The full game loop as one state machine:
//! ship → nav console → FTL → drop pod → surface run → extraction → debrief → upgrades.

use crate::content::{Material, Materials, PlanetId, ShipUpgrade, WeaponKind};
use crate::events::GameEvent;
use crate::input::{Command, GameInput};
use crate::math::V2;
use crate::profile::{Profile, PurchaseError};
use crate::rng::Rng;
use crate::run::{Extraction, Run, RunConfig, RunOutcome, RunState};
use crate::ship::{Ship, StationKind};
use crate::terrain::{Terrain, Tile};
use crate::tutorial::{Prompt, Tutorial, TutorialStep};

pub const FTL_TICKS: u32 = 210;
pub const DESCENT_TICKS: u32 = 170;
pub const ASCENT_TICKS: u32 = 170;
pub const LOST_TICKS: u32 = 150;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// Walking around the ship.
    Ship,
    /// FTL cinematic.
    Ftl { ticks: u32, to: PlanetId },
    /// Drop pod falling to the surface.
    Descent { ticks: u32 },
    /// On the planet surface; see [`Game::run`].
    Surface,
    /// Extraction pod climbing back to orbit.
    Ascent { ticks: u32 },
    /// Player went down; brief "signal lost" before returning to the ship.
    Lost { ticks: u32 },
}

#[derive(Clone, Debug, PartialEq)]
pub struct RunSummary {
    pub planet: PlanetId,
    pub outcome: RunOutcome,
    pub banked: Materials,
    pub lost: Materials,
    pub kills: u32,
    pub level: u32,
    pub seconds: u32,
    pub weapons: Vec<(WeaponKind, u8)>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Terminal {
    Navigation,
    Fabricator,
    Debrief(RunSummary),
    Module(ShipUpgrade),
}

/// A short status message, e.g. why an action was refused.
#[derive(Clone, Debug, PartialEq)]
pub struct Notice {
    pub text: String,
    pub ticks: u32,
}

pub struct Game {
    pub profile: Profile,
    pub phase: Phase,
    pub ship: Ship,
    pub run: Option<Run>,
    pub terminal: Option<Terminal>,
    pub tutorial: Option<Tutorial>,
    pub notice: Option<Notice>,
    /// Events produced by the most recent [`Game::update`].
    pub events: Vec<GameEvent>,
    pub tick: u64,
    rng: Rng,
}

impl Game {
    pub fn new(profile: Profile, seed: u64) -> Self {
        let tutorial = (!profile.tutorial_complete).then(|| {
            Tutorial::new(if profile.orbiting.is_some() {
                TutorialStep::ToPod
            } else {
                TutorialStep::Welcome
            })
        });
        Self {
            ship: Ship::new(&profile),
            profile,
            phase: Phase::Ship,
            run: None,
            terminal: None,
            tutorial,
            notice: None,
            events: Vec::new(),
            tick: 0,
            rng: Rng::new(seed),
        }
    }

    pub fn tutorial_step(&self) -> Option<TutorialStep> {
        self.tutorial.as_ref().map(|t| t.step)
    }

    fn notify(&mut self, text: impl Into<String>) {
        self.notice = Some(Notice {
            text: text.into(),
            ticks: 210,
        });
    }

    pub fn update(&mut self, input: &GameInput) {
        self.events.clear();
        self.tick += 1;
        if let Some(n) = &mut self.notice {
            n.ticks = n.ticks.saturating_sub(1);
            if n.ticks == 0 {
                self.notice = None;
            }
        }
        match self.phase {
            Phase::Ship => self.update_ship(input),
            Phase::Ftl { ticks, to } => {
                if ticks <= 1 {
                    self.profile.orbiting = Some(to);
                    self.phase = Phase::Ship;
                    self.events.push(GameEvent::FtlArrived { at: to });
                    self.events.push(GameEvent::ProfileChanged);
                } else {
                    self.phase = Phase::Ftl {
                        ticks: ticks - 1,
                        to,
                    };
                }
            }
            Phase::Descent { ticks } => {
                if ticks <= 1 {
                    self.phase = Phase::Surface;
                    self.events.push(GameEvent::Landed);
                } else {
                    self.phase = Phase::Descent { ticks: ticks - 1 };
                }
            }
            Phase::Surface => self.update_surface(input),
            Phase::Ascent { ticks } => {
                if let Some(run) = &mut self.run {
                    // Keep the world animating under the departing pod.
                    let mut ignored = Vec::new();
                    run.state = RunState::Active;
                    run.spawning_enabled = false;
                    run.invulnerable = true;
                    run.update(&GameInput::default(), &mut ignored);
                    run.state = RunState::Ended(RunOutcome::Extracted);
                }
                if ticks <= 1 {
                    self.finish_run();
                } else {
                    self.phase = Phase::Ascent { ticks: ticks - 1 };
                }
            }
            Phase::Lost { ticks } => {
                if ticks <= 1 {
                    self.finish_run();
                } else {
                    self.phase = Phase::Lost { ticks: ticks - 1 };
                }
            }
        }
        self.update_tutorial(input);
    }

    fn update_ship(&mut self, input: &GameInput) {
        if let Some(terminal) = self.terminal.clone() {
            self.ship.update([0.0, 0.0]);
            let Some(command) = input.command else { return };
            match (terminal, command) {
                (_, Command::Close) => {
                    self.terminal = None;
                    self.events.push(GameEvent::TerminalClosed);
                }
                (Terminal::Navigation, Command::TravelTo(planet)) => self.travel(planet),
                (Terminal::Fabricator, Command::Purchase(upgrade)) => self.purchase(upgrade),
                _ => {}
            }
            return;
        }
        self.ship.update(input.movement);
        if !input.interact {
            return;
        }
        let Some(station) = self.ship.focused_station() else {
            return;
        };
        match station.kind {
            StationKind::NavConsole => self.open(Terminal::Navigation),
            StationKind::Fabricator => self.open(Terminal::Fabricator),
            StationKind::Module(u) => self.open(Terminal::Module(u)),
            StationKind::DropPod => match self.profile.orbiting {
                Some(planet) => self.deploy(planet),
                None => {
                    self.events.push(GameEvent::Denied);
                    self.notify(
                        "No planet below us. Plot a course at the Navigation Console first.",
                    );
                }
            },
        }
    }

    fn open(&mut self, terminal: Terminal) {
        self.terminal = Some(terminal);
        self.events.push(GameEvent::TerminalOpened);
    }

    fn travel(&mut self, planet: PlanetId) {
        if !self.profile.unlocked(planet) {
            self.events.push(GameEvent::Denied);
            self.notify(format!(
                "{} is out of range. Upgrade the Reactor & Nav Core.",
                planet.info().name
            ));
            return;
        }
        if self.profile.orbiting == Some(planet) {
            self.events.push(GameEvent::Denied);
            self.notify(format!("Already in orbit of {}.", planet.info().name));
            return;
        }
        self.terminal = None;
        self.phase = Phase::Ftl {
            ticks: FTL_TICKS,
            to: planet,
        };
        self.events.push(GameEvent::FtlStarted { to: planet });
    }

    fn purchase(&mut self, upgrade: ShipUpgrade) {
        match self.profile.purchase(upgrade) {
            Ok(level) => {
                if level == 1 {
                    self.ship.rebuild(&self.profile);
                    self.ship.start_construction(upgrade);
                }
                self.events.push(GameEvent::UpgradeBuilt { upgrade, level });
                self.events.push(GameEvent::ProfileChanged);
            }
            Err(PurchaseError::MaxLevel) => {
                self.events.push(GameEvent::Denied);
                self.notify(format!("{} is fully upgraded.", upgrade.name()));
            }
            Err(PurchaseError::Unaffordable) => {
                self.events.push(GameEvent::Denied);
                self.notify("Not enough materials in the hold.");
            }
        }
    }

    fn deploy(&mut self, planet: PlanetId) {
        let tutorial = self.tutorial.is_some() && planet == PlanetId::Verdant;
        let mut run = Run::new(RunConfig {
            planet,
            seed: self.rng.next_u64(),
            tutorial,
            max_health: self.profile.max_health(),
            damage_multiplier: self.profile.damage_multiplier(),
            mining_multiplier: self.profile.mining_multiplier(),
            pickup_bonus: self.profile.pickup_bonus(),
        });
        run.spawning_enabled = !tutorial;
        self.run = Some(run);
        self.profile.stats.drops += 1;
        self.phase = Phase::Descent {
            ticks: DESCENT_TICKS,
        };
        self.events.push(GameEvent::DescentStarted);
        self.events.push(GameEvent::ProfileChanged);
    }

    fn update_surface(&mut self, input: &GameInput) {
        let Some(run) = &mut self.run else {
            self.phase = Phase::Ship;
            return;
        };
        run.update(input, &mut self.events);
        match run.outcome() {
            Some(RunOutcome::Extracted) => {
                self.phase = Phase::Ascent {
                    ticks: ASCENT_TICKS,
                }
            }
            Some(RunOutcome::Died) => self.phase = Phase::Lost { ticks: LOST_TICKS },
            Some(RunOutcome::Abandoned) => self.finish_run(),
            None => {}
        }
    }

    fn finish_run(&mut self) {
        let Some(run) = self.run.take() else {
            self.phase = Phase::Ship;
            return;
        };
        let outcome = run.outcome().unwrap_or(RunOutcome::Abandoned);
        let extracted = outcome == RunOutcome::Extracted;
        let stats = &mut self.profile.stats;
        stats.kills += run.kills;
        stats.best_level = stats.best_level.max(run.player.level);
        let (banked, lost) = if extracted {
            stats.extractions += 1;
            stats.materials_banked += run.carried.total();
            self.profile.materials.add_all(run.carried);
            (run.carried, Materials::default())
        } else {
            if outcome == RunOutcome::Died {
                stats.deaths += 1;
            }
            (Materials::default(), run.carried)
        };
        let summary = RunSummary {
            planet: run.planet,
            outcome,
            banked,
            lost,
            kills: run.kills,
            level: run.player.level,
            seconds: run.ticks / 60,
            weapons: run.weapons.iter().map(|w| (w.kind, w.level)).collect(),
        };
        self.phase = Phase::Ship;
        self.ship.place_at_drop_bay();
        self.terminal = Some(Terminal::Debrief(summary));
        self.events.push(GameEvent::TerminalOpened);
        self.events.push(GameEvent::ProfileChanged);
    }

    // -- tutorial -------------------------------------------------------------

    fn update_tutorial(&mut self, input: &GameInput) {
        let Some(tutorial) = &mut self.tutorial else {
            return;
        };
        tutorial.ticks += 1;
        let events = &self.events;
        let has = |f: &dyn Fn(&GameEvent) -> bool| events.iter().any(f);
        let died = has(&|e| matches!(e, GameEvent::RunEnded { extracted: false }));
        let current = tutorial.step;
        let next = match current {
            TutorialStep::Welcome
                if tutorial.ticks > 60 && crate::math::length(input.movement) > 0.2 =>
            {
                Some(TutorialStep::ToConsole)
            }
            TutorialStep::Welcome if tutorial.ticks > 600 => Some(TutorialStep::ToConsole),
            TutorialStep::ToConsole if self.terminal == Some(Terminal::Navigation) => {
                Some(TutorialStep::PlotCourse)
            }
            TutorialStep::PlotCourse if matches!(self.phase, Phase::Ftl { .. }) => {
                Some(TutorialStep::Jumping)
            }
            TutorialStep::PlotCourse if self.terminal.is_none() => Some(TutorialStep::ToConsole),
            TutorialStep::Jumping if self.phase == Phase::Ship => Some(TutorialStep::ToPod),
            TutorialStep::ToPod if matches!(self.phase, Phase::Descent { .. }) => {
                Some(TutorialStep::Dropping)
            }
            TutorialStep::Dropping if self.phase == Phase::Surface => Some(TutorialStep::Survive),
            _ if died => {
                tutorial.retry = true;
                Some(TutorialStep::ToPod)
            }
            TutorialStep::Survive if tutorial.ticks > 600 => Some(TutorialStep::Mine),
            TutorialStep::Mine
                if has(&|e| {
                    matches!(
                        e,
                        GameEvent::BlockBroken {
                            material: Some(Material::Lumite),
                            ..
                        }
                    )
                }) || self.run.as_ref().is_some_and(|r| r.carried.lumite > 0) =>
            {
                Some(TutorialStep::Collect)
            }
            TutorialStep::Collect if self.run.as_ref().is_some_and(|r| r.player.level > 1) => {
                Some(TutorialStep::Cache)
            }
            TutorialStep::Cache if self.run.as_ref().is_some_and(|r| r.caches_opened > 0) => {
                Some(TutorialStep::Objective)
            }
            TutorialStep::Objective if self.run.as_ref().is_some_and(|r| r.objective_done) => {
                Some(TutorialStep::CallPod)
            }
            TutorialStep::CallPod
                if self
                    .run
                    .as_ref()
                    .is_some_and(|r| !matches!(r.extraction, Extraction::Idle)) =>
            {
                Some(TutorialStep::HoldZone)
            }
            TutorialStep::HoldZone if matches!(self.phase, Phase::Ascent { .. }) => {
                Some(TutorialStep::Debrief)
            }
            TutorialStep::Debrief if self.phase == Phase::Ship && self.terminal.is_none() => {
                Some(TutorialStep::Fabricate)
            }
            TutorialStep::Fabricate
                if has(&|e| matches!(e, GameEvent::UpgradeBuilt { .. }))
                    || (tutorial.ticks > 120
                        && has(&|e| matches!(e, GameEvent::TerminalClosed))) =>
            {
                Some(TutorialStep::Done)
            }
            TutorialStep::Done if tutorial.ticks > 480 => Some(TutorialStep::Done),
            _ => None,
        };
        // Tutorial pacing on the surface.
        if let Some(run) = &mut self.run
            && run.tutorial
        {
            run.spawning_enabled = tutorial.step >= TutorialStep::Survive;
        }
        if current == TutorialStep::Done && tutorial.ticks > 480 {
            self.tutorial = None;
            self.profile.tutorial_complete = true;
            self.events.push(GameEvent::TutorialComplete);
            self.events.push(GameEvent::ProfileChanged);
            return;
        }
        if let Some(step) = next
            && tutorial.advance(step)
        {
            self.events.push(GameEvent::TutorialStep);
        }
    }

    /// The tutorial prompt for the current step with a world-space target.
    pub fn tutorial_prompt(&self) -> Option<Prompt> {
        let tutorial = self.tutorial.as_ref()?;
        let (have, need) = self
            .run
            .as_ref()
            .map_or((0, 0), |r| (r.carried.lumite, r.objective));
        let (title, body) = tutorial.text(have, need)?;
        let station = |kind| self.ship.layout.station(kind).map(|s| s.pos);
        let target = match tutorial.step {
            TutorialStep::ToConsole => station(StationKind::NavConsole),
            TutorialStep::ToPod => station(StationKind::DropPod),
            TutorialStep::Fabricate if self.terminal.is_none() => station(StationKind::Fabricator),
            TutorialStep::Mine => self
                .run
                .as_ref()
                .and_then(|r| nearest_ore(r, Material::Lumite)),
            TutorialStep::Cache => self
                .run
                .as_ref()
                .and_then(|r| r.caches.iter().find(|c| !c.opened).map(|c| c.pos)),
            TutorialStep::HoldZone => self.run.as_ref().map(|r| r.landing),
            _ => None,
        };
        Some(Prompt {
            title,
            body,
            target,
        })
    }

    /// Hint shown near the player for the focused ship station.
    pub fn interaction_hint(&self) -> Option<String> {
        if self.phase != Phase::Ship || self.terminal.is_some() {
            return None;
        }
        let station = self.ship.focused_station()?;
        Some(match station.kind {
            StationKind::DropPod => match self.profile.orbiting {
                Some(p) => format!("{{interact}} Deploy to {}", p.info().name),
                None => "Drop Pod (no planet in orbit)".into(),
            },
            kind => format!("{{interact}} {}", kind.label()),
        })
    }

    pub fn state_hash(&self) -> u64 {
        let mut h = self.tick.wrapping_mul(0x9E37_79B9_7F4A_7C15);
        h ^= u64::from(self.profile.materials.total()).rotate_left(17);
        h ^= u64::from(self.ship.player.pos[0].to_bits()).rotate_left(7);
        if let Some(run) = &self.run {
            h ^= run.state_hash().rotate_left(29);
        }
        h
    }
}

/// Nearest exposed ore of a material, for markers and the autopilot.
pub fn nearest_ore(run: &Run, material: Material) -> Option<V2> {
    let player = run.player.pos;
    run.terrain
        .tiles()
        .filter(|(_, t)| matches!(t, Tile::Ore { material: m, .. } if *m == material))
        .map(|((x, y), _)| Terrain::center_of(x, y))
        .min_by(|a, b| {
            crate::math::distance_sq(*a, player).total_cmp(&crate::math::distance_sq(*b, player))
        })
}
