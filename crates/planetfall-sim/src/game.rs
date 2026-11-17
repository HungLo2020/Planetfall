//! The full game loop as one state machine:
//! ship → nav console → FTL → drop pod → surface run → extraction → debrief → upgrades.

use crate::content::{
    ClassId, Material, Materials, PlanetId, ShipBlock, UpgradeId, WeaponId, content,
};
use crate::events::GameEvent;
use crate::input::{Command, GameInput};
use crate::math::V2;
use crate::profile::{Profile, PurchaseError};
use crate::run::{Extraction, Run, RunConfig, RunOutcome, RunState};
use crate::ship::{Ship, StationKind};
use crate::terrain::{Terrain, Tile};
use crate::tutorial::{Prompt, Tutorial, TutorialStep};
use quasar_core::Rng;
use quasar_sim::{Simulation, StateHash, StateHasher, Tick};

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
    /// Hull salvage the ship AI adds after the tutorial drop so the first
    /// module is always affordable.
    pub salvage: Materials,
    pub kills: u32,
    pub level: u32,
    pub seconds: u32,
    pub weapons: Vec<(WeaponId, u8)>,
    /// The class that dropped, the class XP it earned and its level before
    /// and after.
    pub class: ClassId,
    pub class_xp: u32,
    pub class_levels: (u32, u32),
    /// Threat level reached.
    pub threat: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Terminal {
    Navigation,
    Fabricator,
    Debrief(RunSummary),
    Module(UpgradeId),
    Locker,
}

/// A short status message, e.g. why an action was refused.
#[derive(Clone, Debug, PartialEq)]
pub struct Notice {
    pub text: String,
    pub ticks: u32,
}

#[derive(Clone)]
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
            rng: Rng::seed_from_u64(seed),
        }
    }

    pub(crate) fn rng(&self) -> &Rng {
        &self.rng
    }

    pub(crate) fn set_rng(&mut self, rng: Rng) {
        self.rng = rng;
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
            self.ship.update(V2::new(0.0, 0.0));
            let Some(command) = input.command else { return };
            match (terminal, command) {
                (_, Command::Close) => {
                    self.terminal = None;
                    self.events.push(GameEvent::TerminalClosed);
                }
                (Terminal::Navigation, Command::TravelTo(planet)) => self.travel(planet),
                (Terminal::Fabricator, Command::Purchase(upgrade)) => self.purchase(upgrade),
                (Terminal::Locker, Command::SelectClass(class)) => {
                    if self.profile.class != class {
                        self.profile.class = class;
                        self.events.push(GameEvent::ClassChanged { class });
                        self.events.push(GameEvent::ProfileChanged);
                    }
                }
                (Terminal::Locker, Command::SetStartWeapon(weapon)) => {
                    if self.profile.set_start_weapon(weapon) {
                        self.events.push(GameEvent::ProfileChanged);
                    } else {
                        self.events.push(GameEvent::Denied);
                        self.notify(format!(
                            "The {} has not unlocked the {} yet.",
                            self.profile.class.name(),
                            weapon.name()
                        ));
                    }
                }
                _ => {}
            }
            return;
        }
        if self.ship.update(input.movement) {
            self.ship.rebuild(&self.profile);
        }
        if input.build && self.profile.tutorial_complete {
            let on = self.ship.build.is_none();
            self.ship.build = on.then_some(ShipBlock::Deck);
            self.events.push(GameEvent::BuildMode { on });
            return;
        }
        if self.ship.build.is_some() {
            self.update_building(input);
            return;
        }
        if !input.interact {
            return;
        }
        let Some(station) = self.ship.focused_station() else {
            return;
        };
        match station.kind {
            StationKind::NavConsole => self.open(Terminal::Navigation),
            StationKind::Fabricator => self.open(Terminal::Fabricator),
            StationKind::Locker => self.open(Terminal::Locker),
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

    /// Build mode: interact places, dash removes, extract cycles blocks.
    fn update_building(&mut self, input: &GameInput) {
        if input.call_extraction
            && let Some(block) = self.ship.build
        {
            self.ship.build = Some(block.next());
        }
        let result = if input.interact {
            self.ship
                .place(&mut self.profile)
                .map(|block| GameEvent::BlockPlaced { block })
        } else if input.dash {
            self.ship
                .remove(&mut self.profile)
                .map(|_| GameEvent::BlockRemoved)
        } else {
            return;
        };
        match result {
            Ok(event) => {
                self.events.push(event);
                self.events.push(GameEvent::ProfileChanged);
            }
            Err(e) => {
                self.events.push(GameEvent::Denied);
                self.notify(e.message());
            }
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

    fn purchase(&mut self, upgrade: UpgradeId) {
        match self.profile.purchase(upgrade) {
            Ok(level) => {
                if level == 1 {
                    self.ship.start_construction(upgrade, &self.profile);
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
        let tutorial = self.tutorial.is_some() && planet == PlanetId::first();
        let mut run = Run::new(RunConfig {
            planet,
            seed: self.rng.next_u64(),
            tutorial,
            players: vec![self.profile.loadout()],
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

    /// Top the hold up so the cheapest first module is affordable: the
    /// tutorial ends by building one, whatever the drop happened to yield.
    fn tutorial_salvage(&mut self) -> Materials {
        let have = self.profile.materials;
        let Some(cost) = UpgradeId::all()
            .filter_map(|u| self.profile.next_cost(u))
            .min_by_key(|c| c.total())
        else {
            return Materials::default();
        };
        let mut salvage = Materials::default();
        for m in Material::ALL {
            salvage.add(m, cost.get(m).saturating_sub(have.get(m)));
        }
        self.profile.materials.add_all(salvage);
        salvage
    }

    fn finish_run(&mut self) {
        let Some(run) = self.run.take() else {
            self.phase = Phase::Ship;
            return;
        };
        let outcome = run.outcome().unwrap_or(RunOutcome::Abandoned);
        let extracted = outcome == RunOutcome::Extracted;
        let player = run.player();
        // Class XP from everything collected; a failed drop keeps a share.
        let share = if extracted {
            1.0
        } else {
            content().tuning.class.failed_share
        };
        let class = player.class;
        let bonus = if extracted {
            content().tuning.class.extraction_bonus
        } else {
            0
        };
        let class_xp = (player.xp_total as f32 * share).round() as u32 + bonus;
        let class_levels = self.profile.award_class_xp(class, class_xp);
        if class_levels.1 > class_levels.0 {
            self.events.push(GameEvent::ClassLevelUp {
                class,
                level: class_levels.1,
            });
        }
        let stats = &mut self.profile.stats;
        stats.kills += run.kills;
        stats.best_level = stats.best_level.max(player.level);
        stats.surface_seconds += run.ticks / 60;
        stats.best_threat = stats.best_threat.max(run.threat_level());
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
        let salvage = if extracted && run.tutorial {
            self.tutorial_salvage()
        } else {
            Materials::default()
        };
        let summary = RunSummary {
            planet: run.planet,
            outcome,
            banked,
            lost,
            salvage,
            kills: run.kills,
            level: run.player().level,
            seconds: run.ticks / 60,
            weapons: run
                .player()
                .weapons
                .iter()
                .map(|w| (w.kind, w.level() as u8))
                .collect(),
            class,
            class_xp,
            class_levels,
            threat: run.threat_level(),
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
            TutorialStep::Welcome if tutorial.ticks > 60 && input.movement.length() > 0.2 => {
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
            // The drop can outrun the lesson (no level-up or cache before the
            // objective is met); catch up instead of waiting forever.
            step if (TutorialStep::Survive..TutorialStep::Debrief).contains(&step)
                && matches!(self.phase, Phase::Ascent { .. }) =>
            {
                Some(TutorialStep::Debrief)
            }
            step if (TutorialStep::Survive..TutorialStep::HoldZone).contains(&step)
                && self
                    .run
                    .as_ref()
                    .is_some_and(|r| !matches!(r.extraction, Extraction::Idle)) =>
            {
                Some(TutorialStep::HoldZone)
            }
            step if (TutorialStep::Mine..TutorialStep::CallPod).contains(&step)
                && self.run.as_ref().is_some_and(|r| r.objective_done) =>
            {
                Some(TutorialStep::CallPod)
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
            TutorialStep::Collect if self.run.as_ref().is_some_and(|r| r.player().level > 1) => {
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
        if let Some(block) = self.ship.build {
            let cost = block.cost();
            let status = match self.ship.can_place(&self.profile) {
                Ok(()) => String::new(),
                Err(e) => format!("  ({})", e.message()),
            };
            return Some(format!(
                "BUILD: {} — {} ferrite, {} lumite{status}\n{{interact}} place · {{dash}} remove · {{extract}} next block · {{build}} done",
                block.name(),
                cost.ferrite,
                cost.lumite,
            ));
        }
        let Some(station) = self.ship.focused_station() else {
            return self
                .profile
                .tutorial_complete
                .then(|| "{build} Build mode".to_owned());
        };
        Some(match station.kind {
            StationKind::DropPod => match self.profile.orbiting {
                Some(p) => format!("{{interact}} Deploy to {}", p.info().name),
                None => "Drop Pod (no planet in orbit)".into(),
            },
            kind => format!("{{interact}} {}", kind.label()),
        })
    }

    /// Deterministic hash of the whole game, as `quasar-sim` replays compare it.
    pub fn state_hash(&self) -> StateHash {
        let mut h = StateHasher::new();
        h.write_u64(self.tick);
        h.write_rng(&self.rng);
        for m in Material::ALL {
            h.write_u32(self.profile.materials.get(m));
        }
        for v in [self.ship.player.pos.x, self.ship.player.pos.y] {
            h.write_f32_bits(v);
        }
        if let Some(run) = &self.run {
            run.hash_into(&mut h);
        }
        h.finish()
    }
}

/// Nearest exposed ore of a material, for markers and the autopilot.
pub fn nearest_ore(run: &Run, material: Material) -> Option<V2> {
    let player = run.player().pos;
    run.terrain
        .tiles()
        .filter(|(_, t)| matches!(t, Tile::Ore { material: m, .. } if *m == material))
        .map(|((x, y), _)| Terrain::center_of(x, y))
        .min_by(|a, b| {
            (*a).distance_squared(player)
                .total_cmp(&(*b).distance_squared(player))
        })
}

/// `quasar-sim` drives the game for recording, replay and divergence checks.
impl Simulation for Game {
    type Input = GameInput;
    type Snapshot = Box<Game>;
    type Error = std::convert::Infallible;

    fn update(&mut self, _tick: Tick, input: &GameInput) -> Result<(), Self::Error> {
        Game::update(self, input);
        Ok(())
    }

    fn state_hash(&self) -> StateHash {
        Game::state_hash(self)
    }

    fn snapshot(&self) -> Box<Game> {
        Box::new(self.clone())
    }

    fn restore(&mut self, snapshot: &Box<Game>) -> Result<(), Self::Error> {
        *self = (**snapshot).clone();
        Ok(())
    }
}
