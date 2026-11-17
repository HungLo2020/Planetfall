//! Native desktop runner: `quasar-app` owns the window/event loop; this file
//! wires the game, input, menus, audio and renderer together.

use crate::audio::Audio;
use crate::bindings;
use crate::controls::Controls;
use crate::render::{Frame, Graphics, camera::Camera};
use crate::settings::Settings;
use crate::storage::{Display, Preferences, SlotState, Storage, new_meta, unix_time};
use crate::ui::menu::{Content, Menu, MenuAction, MenuContext, SavesMode, Screen, SettingsState};
use crate::ui::{Layer, build_text, hud, overlay};
use planetfall_sim::autoplay::Autopilot;
use planetfall_sim::events::GameEvent;
use planetfall_sim::save::{SaveGame, SlotMeta};
use planetfall_sim::{Command, Game, Phase, Profile};
use quasar_app::{
    AppConfig, AppContext, Application, DesktopApplication, Flow, RenderPolicy, TimingConfig,
};
use quasar_core::FixedStepConfig;
use quasar_diagnostics::{Diagnostics, DiagnosticsConfig, Field, Level, TextSink};
use quasar_frame::{CompletionPolicy, FrameSurface, Gpu};
use quasar_ui::{Rect, ShapedTextMeasure, TextRenderer};
use quasar_window::{DesktopConfig, WindowConfig, winit::event::WindowEvent};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use vulkanic_gal::PresentMode;

pub struct Launch {
    pub settings: Settings,
    pub storage: Storage,
    pub asset_root: PathBuf,
}

/// Seconds of play between autosaves.
const AUTOSAVE_SECONDS: u32 = 60;

pub fn run(launch: Launch) -> Result<(), String> {
    let display = launch.storage.load_display();
    let prefs = launch.storage.load_preferences();
    let bindings = launch.storage.load_controls(&bindings::defaults());
    let size = launch
        .settings
        .resolution
        .unwrap_or((display.width, display.height));
    let fullscreen = launch.settings.fullscreen.unwrap_or(display.fullscreen);
    let config = AppConfig {
        window: WindowConfig {
            title: "Planetfall".into(),
            application_id: "planetfall".into(),
            size,
            minimum_size: (960, 540),
            fullscreen,
            resizable: true,
        },
        desktop: DesktopConfig::for_application("planetfall"),
        timing: TimingConfig::Fixed(FixedStepConfig::for_rate(60.0, 8)),
        render: RenderPolicy::EveryFrame,
        max_frame_wait: Duration::from_millis(16),
        frame_interval: None,
    };
    let diagnostics = Diagnostics::with_config(
        Arc::new(TextSink::new(std::io::stderr())),
        DiagnosticsConfig {
            min_level: launch.settings.log_level,
            ..DiagnosticsConfig::default()
        },
    );
    diagnostics.event_with(
        Level::Info,
        "planetfall",
        "startup",
        [
            Field::new("version", env!("CARGO_PKG_VERSION")),
            Field::new("root", launch.storage.paths.root.display().to_string()),
            Field::new("save", launch.storage.paths.save.display().to_string()),
            Field::new("assets", launch.asset_root.display().to_string()),
        ],
    );
    let (measure, text) = build_text(&launch.asset_root)?;
    // The main menu shows a fresh ship behind it; `--slot`, `--fresh` and
    // `--autoplay` skip straight into a game.
    let mut game = Game::new(Profile::default(), launch.settings.seed);
    let mut slot = None;
    let mut meta = SlotMeta::default();
    let mut warning = None;
    if let Some(chosen) = launch.settings.slot {
        match launch.storage.load_slot(chosen) {
            Ok(save) => {
                meta = save.meta.clone();
                game = save.restore(launch.settings.seed);
                slot = Some(chosen);
            }
            Err(e) => warning = Some(format!("slot {chosen}: {e}")),
        }
    }
    if let Some(w) = &warning {
        diagnostics.event_with(
            Level::Warn,
            "planetfall.save",
            "load_failed",
            [Field::new("detail", w.clone())],
        );
    }
    let skip_menu = launch.settings.autoplay || launch.settings.fresh || slot.is_some();
    let settings_state = SettingsState {
        fullscreen,
        shadows: display.shadows,
        bloom: display.bloom,
        master: prefs.master_volume,
        music: prefs.music_volume,
        effects: prefs.effects_volume,
        shake: prefs.screen_shake,
    };
    let slots = launch.storage.slots();
    let mut app = App {
        autopilot: launch.settings.autoplay.then(Autopilot::new),
        screens: if skip_menu {
            Vec::new()
        } else {
            vec![Screen::Main]
        },
        menu: Menu::new(settings_state),
        slot,
        meta,
        slots,
        load_error: warning,
        tick_count: 0,
        settings: launch.settings,
        storage: launch.storage,
        asset_root: launch.asset_root,
        prefs,
        display: Display {
            fullscreen,
            width: size.0,
            height: size.1,
            ..display
        },
        game,
        measure,
        text: Some(text),
        controls: Controls::new(),
        audio: None,
        gal: None,
        surface: None,
        graphics: None,
        time: 0.0,
        dt: 1.0 / 60.0,
        shake: 0.0,
        frame_events: Vec::new(),
        frames: 0,
        size: (size.0 as f32, size.1 as f32),
        scale: 1.0,
        failure: None,
        diagnostics,
        last_phase: None,
        keyboard_bindings: Default::default(),
        pending_bindings: None,
    };
    app.install_bindings(bindings);
    app.pending_bindings = None;
    quasar_app::run(app, config).map_err(|e| e.to_string())
}

struct App {
    /// The save slot being played; None at the main menu or when unsaved.
    slot: Option<usize>,
    meta: SlotMeta,
    /// Slot summaries for the menus (refreshed when they change).
    slots: Vec<(usize, SlotState)>,
    load_error: Option<String>,
    /// Ticks played since the last save, for play time and autosave.
    tick_count: u32,
    settings: Settings,
    storage: Storage,
    asset_root: PathBuf,
    prefs: Preferences,
    display: Display,
    game: Game,
    autopilot: Option<Autopilot>,
    /// Stack of open app-level screens (main menu, saves, settings, pause, confirmations).
    screens: Vec<Screen>,
    menu: Menu,
    measure: ShapedTextMeasure,
    text: Option<TextRenderer>,
    controls: Controls,
    audio: Option<Audio>,
    gal: Option<Gpu>,
    surface: Option<FrameSurface>,
    graphics: Option<Graphics>,
    time: f32,
    dt: f32,
    shake: f32,
    /// Game events since the last rendered frame (particles and marks).
    frame_events: Vec<GameEvent>,
    frames: u64,
    size: (f32, f32),
    scale: f32,
    failure: Option<String>,
    diagnostics: Diagnostics,
    last_phase: Option<std::mem::Discriminant<Phase>>,
    /// The keyboard/mouse part of the player's bindings (window input).
    keyboard_bindings: quasar_input::Bindings,
    /// Rebound keyboard bindings for the runner to install (`take_bindings`).
    pending_bindings: Option<quasar_input::Bindings>,
}

impl App {
    fn initialize(&mut self, c: &AppContext<'_>) -> Result<(), String> {
        if self.gal.is_some() {
            return Ok(());
        }
        let window = c.window.ok_or("missing desktop window")?;
        let size = window.native().inner_size();
        let (gal, mut surface) =
            FrameSurface::for_window(window.shared_native(), size.width, size.height)
                .map_err(|e| format!("create graphics: {e}"))?;
        surface.config.present_mode = PresentMode::AutoVsync;
        surface.reconfigure(&gal).map_err(|e| e.to_string())?;
        // The text renderer moves into the graphics layer on first init; a
        // resumed surface rebuilds fonts from disk.
        let text = match self.text.take() {
            Some(t) => t,
            None => build_text(&self.asset_root)?.1,
        };
        let mut graphics = Graphics::new(&gal, &self.asset_root, Some(text))
            .map_err(|e| format!("create renderer: {e}"))?;
        graphics.shadows_enabled = self.display.shadows;
        graphics.bloom_enabled = self.display.bloom;
        self.graphics = Some(graphics);
        self.surface = Some(surface);
        self.gal = Some(gal);
        if self.settings.audio && self.audio.is_none() {
            self.audio = Audio::open(
                self.settings.audio_device.as_deref(),
                &self.asset_root,
                &self.prefs,
            );
        }
        Ok(())
    }

    fn release(&mut self) -> Result<(), String> {
        if let Some(gpu) = self.gal.take() {
            gpu.wait().map_err(|e| e.to_string())?;
            if let Some(mut g) = self.graphics.take() {
                g.release(&gpu).map_err(|e| e.to_string())?;
            }
            drop(self.surface.take());
            gpu.wait().map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    /// Write the game to its slot (no-op when unsaved).
    fn save_game(&mut self) {
        let Some(slot) = self.slot else { return };
        self.meta.play_seconds += u64::from(self.tick_count / 60);
        self.tick_count %= 60;
        self.meta.saved = unix_time();
        let save = SaveGame::capture(&self.game, self.meta.clone());
        if let Err(e) = self.storage.save_slot(slot, &save) {
            self.diagnostics.event_with(
                Level::Error,
                "planetfall.save",
                "save_failed",
                [
                    Field::new("slot", slot as u64),
                    Field::new("error", e.to_string()),
                ],
            );
            self.game.notice = Some(planetfall_sim::game::Notice {
                text: format!("Could not save slot {slot}: {e}"),
                ticks: 400,
            });
        }
    }

    fn refresh_slots(&mut self) {
        self.slots = self.storage.slots();
    }

    fn remember_slot(&mut self, slot: Option<usize>) {
        self.prefs.last_slot = slot;
        let _ = self.storage.save_preferences(&self.prefs);
    }

    /// Swap in a different game (new or loaded); cached render state from
    /// the old one must not leak into the new one.
    fn replace_game(&mut self, game: Game) {
        self.game = game;
        if let Some(graphics) = &mut self.graphics {
            graphics.reset_world();
        }
    }

    /// Install the player's bindings on every device and in the prompts.
    fn install_bindings(&mut self, all: quasar_input::Bindings) {
        let keyboard = self.controls.set_bindings(&all);
        self.keyboard_bindings = keyboard.clone();
        self.pending_bindings = Some(keyboard);
        self.menu.bindings = all;
    }

    /// Start a new commander in `slot`.
    fn start_new(&mut self, slot: usize) {
        self.replace_game(Game::new(Profile::default(), unix_time()));
        self.meta = new_meta();
        self.tick_count = 0;
        self.slot = Some(slot);
        self.save_game();
        self.remember_slot(Some(slot));
        self.screens.clear();
    }

    /// Load `slot`, or stay on the menu with the error shown.
    fn load(&mut self, slot: usize) {
        match self.storage.load_slot(slot) {
            Ok(save) => {
                self.meta = save.meta.clone();
                self.replace_game(save.restore(unix_time()));
                self.tick_count = 0;
                self.slot = Some(slot);
                self.load_error = None;
                self.remember_slot(Some(slot));
                self.screens.clear();
            }
            Err(e) => {
                self.load_error = Some(e);
                self.refresh_slots();
            }
        }
    }

    fn menu_context(&self) -> MenuContext {
        MenuContext {
            slots: self.slots.clone(),
            continue_slot: self.prefs.last_slot,
            active_slot: self.slot,
            error: self.load_error.clone(),
        }
    }

    fn content(&self) -> Option<Content> {
        if let Some(&screen) = self.screens.last() {
            return Some(Content::Screen(screen));
        }
        if self.autopilot.is_some() {
            return None;
        }
        if self
            .game
            .run
            .as_ref()
            .is_some_and(|r| r.level_up_choices().is_some())
            && self.game.phase == Phase::Surface
        {
            return Some(Content::LevelUp);
        }
        if self.game.terminal.is_some() && self.game.phase == Phase::Ship {
            return Some(Content::Terminal);
        }
        None
    }

    fn apply_menu(&mut self, action: MenuAction, c: &AppContext<'_>) -> Option<Flow> {
        if action != MenuAction::None
            && let Some(audio) = &mut self.audio
        {
            audio.ui("ui_confirm");
        }
        match action {
            MenuAction::None => {}
            MenuAction::Resume => self.screens.clear(),
            MenuAction::Continue => {
                if let Some(slot) = self.prefs.last_slot {
                    self.load(slot);
                }
            }
            MenuAction::OpenSaves(mode) => {
                self.load_error = None;
                self.refresh_slots();
                if mode == SavesMode::Delete {
                    self.screens.pop();
                }
                self.screens.push(Screen::Saves(mode));
            }
            MenuAction::PickSlot(slot) => {
                let state = self
                    .slots
                    .iter()
                    .find(|(s, _)| *s == slot)
                    .map(|(_, state)| state.clone());
                match (self.screens.last(), state) {
                    (Some(Screen::Saves(SavesMode::New)), Some(SlotState::Saved(_))) => {
                        self.screens.push(Screen::ConfirmOverwrite(slot));
                    }
                    (Some(Screen::Saves(SavesMode::New)), _) => self.start_new(slot),
                    (Some(Screen::Saves(SavesMode::Load)), _) => self.load(slot),
                    (Some(Screen::Saves(SavesMode::Delete)), _) => {
                        self.screens.push(Screen::ConfirmDelete(slot));
                    }
                    _ => {}
                }
            }
            MenuAction::Overwrite(slot) => self.start_new(slot),
            MenuAction::DeleteSlot(slot) => {
                let _ = self.storage.delete_slot(slot);
                if self.prefs.last_slot == Some(slot) {
                    self.remember_slot(None);
                }
                self.refresh_slots();
                self.screens.pop();
            }
            MenuAction::OpenSettings => {
                self.menu.settings = SettingsState {
                    fullscreen: self.display.fullscreen,
                    shadows: self.display.shadows,
                    bloom: self.display.bloom,
                    master: self.prefs.master_volume,
                    music: self.prefs.music_volume,
                    effects: self.prefs.effects_volume,
                    shake: self.prefs.screen_shake,
                };
                self.screens.push(Screen::Settings);
            }
            MenuAction::OpenControls { gamepad } => {
                if matches!(self.screens.last(), Some(Screen::Controls { .. })) {
                    self.screens.pop();
                }
                self.screens.push(Screen::Controls { gamepad });
            }
            MenuAction::ApplyBindings(all) => {
                self.install_bindings(all);
                if let Err(e) = self.storage.save_controls(&self.menu.bindings) {
                    self.diagnostics.event_with(
                        Level::Error,
                        "planetfall.storage",
                        "controls_save_failed",
                        [Field::new("error", e)],
                    );
                }
            }
            MenuAction::Back => {
                self.screens.pop();
            }
            MenuAction::ApplySettings => {
                let o = self.menu.settings.clone();
                self.prefs.master_volume = o.master;
                self.prefs.music_volume = o.music;
                self.prefs.effects_volume = o.effects;
                self.prefs.screen_shake = o.shake;
                self.display.shadows = o.shadows;
                self.display.bloom = o.bloom;
                if o.fullscreen != self.display.fullscreen {
                    self.display.fullscreen = o.fullscreen;
                    if let Some(w) = c.window {
                        w.set_fullscreen(o.fullscreen);
                    }
                }
                if let Some(audio) = &self.audio {
                    audio.apply(&self.prefs);
                }
                if let Some(graphics) = &mut self.graphics {
                    graphics.shadows_enabled = self.display.shadows;
                    graphics.bloom_enabled = self.display.bloom;
                }
                for result in [
                    self.storage.save_preferences(&self.prefs),
                    self.storage.save_display(&self.display),
                ] {
                    if let Err(e) = result {
                        self.diagnostics.event_with(
                            Level::Error,
                            "planetfall.settings",
                            "save_failed",
                            [Field::new("error", e)],
                        );
                    }
                }
                self.screens.pop();
            }
            MenuAction::AskAbandon => self.screens.push(Screen::ConfirmAbandon),
            MenuAction::Abandon => {
                self.screens.clear();
                self.controls.command = Some(Command::Abandon);
            }
            MenuAction::SaveAndQuit => {
                self.save_game();
                self.slot = None;
                self.refresh_slots();
                self.screens = vec![Screen::Main];
            }
            MenuAction::AskQuit => self.screens.push(Screen::ConfirmQuit),
            MenuAction::Quit => {
                self.save_game();
                return Some(Flow::Exit);
            }
            MenuAction::Game(command) => self.controls.command = Some(command),
        }
        None
    }

    fn react(&mut self) {
        // Keep a bounded backlog in case frames stop (minimised window).
        if self.frame_events.len() > 4096 {
            self.frame_events.clear();
        }
        self.frame_events.extend_from_slice(&self.game.events);
        let mut save = false;
        for e in &self.game.events {
            match e {
                GameEvent::ProfileChanged => save = true,
                GameEvent::Explosion { .. } => self.shake = self.shake.max(0.6),
                GameEvent::PlayerHurt { .. } => self.shake = self.shake.max(0.45),
                GameEvent::Landed | GameEvent::PodLanded { .. } => self.shake = self.shake.max(1.0),
                GameEvent::Liftoff | GameEvent::FtlStarted { .. } => {
                    self.shake = self.shake.max(0.5)
                }
                _ => {}
            }
        }
        if let Some(audio) = &mut self.audio {
            audio.handle(&self.game.events);
        }
        if save {
            self.save_game();
        }
    }

    fn hud(&self, menu_open: bool) -> Layer {
        hud::build(&hud::HudContext {
            game: &self.game,
            measure: &self.measure,
            size: self.size,
            scale: self.scale,
            prompts: &self.controls.prompts,
            menu_open,
        })
    }
}

/// Runner error: a human-readable message.
#[derive(Debug)]
pub struct AppError(String);
impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for AppError {}
impl From<String> for AppError {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl Application for App {
    type Error = AppError;

    fn setup(&mut self, c: &mut AppContext<'_>) -> Result<(), AppError> {
        Ok(self.initialize(c)?)
    }

    fn frame_update(&mut self, c: &mut AppContext<'_>) -> Result<Flow, AppError> {
        if let Some(e) = self.failure.take() {
            return Err(e.into());
        }
        self.dt = c.elapsed.as_secs_f32().min(0.1);
        self.time += self.dt;
        self.size = (c.size.0.max(1) as f32, c.size.1.max(1) as f32);
        self.scale = (self.size.1 / 900.0).clamp(0.75, 2.5);
        self.shake = (self.shake - self.dt * 2.5).max(0.0);
        self.controls.begin_frame(c.input);
        if c.input.pressed(quasar_input::Key::F11) {
            self.display.fullscreen = !self.display.fullscreen;
            if let Some(w) = c.window {
                w.set_fullscreen(self.display.fullscreen);
            }
            c.input.consume_pressed(quasar_input::Key::F11);
        }
        let surface_phase = self.game.phase == Phase::Surface;
        if c.focus_changed == Some(false) && self.screens.is_empty() && self.autopilot.is_none() {
            self.screens.push(Screen::Pause {
                surface: surface_phase,
            });
        }
        let content = self.content();
        self.menu.show(
            content,
            &self.game,
            &self.measure,
            self.scale,
            &self.controls.prompts,
            &self.menu_context(),
        );
        if self.menu.is_open() {
            let action = if self.menu.is_capturing() {
                // A controls row waits for a button: raw presses, no navigation.
                let pressed = self.controls.pressed_buttons(c.input);
                self.menu.capture(&pressed)
            } else {
                let input = self.controls.ui_input(c.input);
                let viewport = Rect::new(0.0, 0.0, self.size.0, self.size.1);
                self.menu
                    .update(&input, viewport, self.scale, &self.measure, self.dt)
            };
            c.input.consume_all();
            self.controls.clear();
            if let Some(flow) = self.apply_menu(action, c) {
                return Ok(flow);
            }
        } else if self.controls.pause_pressed(c.input) && self.autopilot.is_none() {
            self.screens.push(Screen::Pause {
                surface: surface_phase,
            });
            c.input.consume_all();
        } else {
            self.controls.latch_gameplay(c.input);
        }
        self.controls.end_frame();
        let title = self.screens.first() == Some(&Screen::Main);
        if let Some(audio) = &mut self.audio {
            audio.set_scene(self.game.phase, title);
            let focus = match (&self.game.run, self.game.phase) {
                (Some(run), Phase::Surface | Phase::Lost { .. }) => run.player().pos,
                _ => self.game.ship.player.pos,
            };
            audio.set_listener([focus[0], 6.0, focus[1]]);
        }
        Ok(Flow::Continue)
    }

    fn tick(&mut self, c: &mut AppContext<'_>) -> Result<Flow, AppError> {
        if !self.screens.is_empty() {
            return Ok(Flow::Deferred);
        }
        let input = match &mut self.autopilot {
            Some(pilot) => pilot.input(&self.game),
            None => self.controls.take_game_input(c.input),
        };
        self.game.update(&input);
        self.tick_count += 1;
        if self.tick_count >= AUTOSAVE_SECONDS * 60 {
            self.save_game();
        }
        let phase = std::mem::discriminant(&self.game.phase);
        if self.last_phase != Some(phase) {
            self.last_phase = Some(phase);
            self.diagnostics.event_with(
                Level::Debug,
                "planetfall.game",
                "phase",
                [
                    Field::new("phase", format!("{:?}", self.game.phase)),
                    Field::new("tick", self.game.tick),
                ],
            );
        }
        self.react();
        Ok(Flow::Continue)
    }

    fn render(&mut self, _c: &mut AppContext<'_>) -> Result<Flow, AppError> {
        let menu_open = self.menu.is_open();
        let hud = self.hud(menu_open && self.content() != Some(Content::Terminal));
        let (Some(gal), Some(surface), Some(graphics)) =
            (&mut self.gal, &mut self.surface, &mut self.graphics)
        else {
            return Ok(Flow::Continue);
        };
        let mut layers: Vec<&Layer> = Vec::with_capacity(2);
        let show_hud = !matches!(self.screens.first(), Some(Screen::Main));
        if show_hud {
            layers.push(&hud);
        }
        if let Some(menu) = self.menu.layer() {
            layers.push(menu);
        }
        let game = &self.game;
        let time = self.time;
        let overlay = |canvas: &mut quasar_render_2d::Canvas, camera: &Camera| {
            if show_hud {
                overlay::draw(canvas, camera, game, time);
            }
        };
        let shake = if self.prefs.screen_shake {
            self.shake
        } else {
            0.0
        };
        let events = std::mem::take(&mut self.frame_events);
        let frame = Frame {
            game,
            time,
            dt: self.dt,
            shake,
            events: &events,
            layers: &layers,
            overlay: &overlay,
        };
        let report = surface
            .render(gal, CompletionPolicy::Poll, |gal, target| {
                graphics.commands(gal, target, &frame)
            })
            .map_err(|e| AppError(format!("render: {e}")))?;
        graphics.submitted(gal);
        if report.is_some() {
            self.frames += 1;
            if self.settings.frames.is_some_and(|n| self.frames >= n) {
                return Ok(Flow::Exit);
            }
        }
        Ok(Flow::Continue)
    }

    fn suspended(&mut self, _c: &mut AppContext<'_>) -> Result<(), AppError> {
        self.save_game();
        if self.screens.is_empty() && self.autopilot.is_none() {
            self.screens.push(Screen::Pause {
                surface: self.game.phase == Phase::Surface,
            });
        }
        Ok(self.release()?)
    }

    fn shutdown(&mut self, _c: &mut AppContext<'_>) -> Result<(), AppError> {
        self.release()?;
        self.audio.take();
        // The slot keeps the whole session, including a drop in progress.
        self.save_game();
        let _ = self.storage.save_display(&self.display);
        println!(
            "planetfall frames={} ticks={} phase={:?} drops={} extractions={} materials={:?}",
            self.frames,
            self.game.tick,
            self.game.phase,
            self.game.profile.stats.drops,
            self.game.profile.stats.extractions,
            self.game.profile.materials,
        );
        Ok(())
    }
}

impl DesktopApplication for App {
    fn resumed(&mut self, c: &mut AppContext<'_>) -> Result<(), AppError> {
        Ok(self.initialize(c)?)
    }

    fn take_bindings(&mut self) -> Option<quasar_input::Bindings> {
        self.pending_bindings.take()
    }

    fn configure_window(&mut self, window: &mut quasar_window::GameWindow) -> Result<(), AppError> {
        window
            .input
            .set_bindings(self.keyboard_bindings.clone())
            .map_err(|e| AppError(format!("install controls: {e}")))?;
        let _ = window.capture_cursor(false);
        Ok(())
    }

    fn window_event(
        &mut self,
        c: &mut AppContext<'_>,
        event: &WindowEvent,
    ) -> Result<(), AppError> {
        if matches!(
            event,
            WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. }
        ) && let (Some(gal), Some(surface), Some(graphics)) =
            (&mut self.gal, &mut self.surface, &mut self.graphics)
        {
            surface.finish(gal).map_err(|e| AppError(e.to_string()))?;
            graphics
                .release_targets(gal)
                .map_err(|e| AppError(e.to_string()))?;
            if let Some(window) = c.window {
                let size = window.native().inner_size();
                surface.resize(size.width, size.height);
                if !self.display.fullscreen && size.width >= 640 && size.height >= 360 {
                    self.display.width = size.width;
                    self.display.height = size.height;
                }
            }
        }
        Ok(())
    }
}
