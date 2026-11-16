//! Native desktop runner: `quasar-app` owns the window/event loop; this file
//! wires the game, input, menus, audio and renderer together.

use crate::audio::Audio;
use crate::bindings;
use crate::controls::Controls;
use crate::render::{Frame, Graphics, camera::Camera};
use crate::settings::Settings;
use crate::storage::{Display, Preferences, Storage};
use crate::ui::menu::{Content, Menu, MenuAction, OptionsState, Screen};
use crate::ui::{Layer, build_text, hud, overlay};
use planetfall_sim::autoplay::Autopilot;
use planetfall_sim::events::GameEvent;
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
    pub profile: Profile,
    pub warning: Option<String>,
    pub asset_root: PathBuf,
}

pub fn run(launch: Launch) -> Result<(), String> {
    let display = launch.storage.load_display();
    let prefs = launch.storage.load_preferences();
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
    let mut game = Game::new(launch.profile, launch.settings.seed);
    if let Some(w) = &launch.warning {
        diagnostics.event_with(
            Level::Warn,
            "planetfall.save",
            "profile_warning",
            [Field::new("detail", w.clone())],
        );
        game.notice = Some(planetfall_sim::game::Notice {
            text: w.clone(),
            ticks: 600,
        });
    }
    let interactive = !launch.settings.autoplay;
    let options = OptionsState {
        fullscreen,
        master: prefs.master_volume,
        music: prefs.music_volume,
        effects: prefs.effects_volume,
        shake: prefs.screen_shake,
    };
    let app = App {
        autopilot: launch.settings.autoplay.then(Autopilot::new),
        screens: if interactive {
            vec![Screen::Title]
        } else {
            Vec::new()
        },
        menu: Menu::new(options),
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
        frames: 0,
        size: (size.0 as f32, size.1 as f32),
        scale: 1.0,
        failure: None,
        diagnostics,
        last_phase: None,
    };
    quasar_app::run(app, config).map_err(|e| e.to_string())
}

struct App {
    settings: Settings,
    storage: Storage,
    asset_root: PathBuf,
    prefs: Preferences,
    display: Display,
    game: Game,
    autopilot: Option<Autopilot>,
    /// Stack of open app-level screens (title, pause, options, confirmations).
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
    frames: u64,
    size: (f32, f32),
    scale: f32,
    failure: Option<String>,
    diagnostics: Diagnostics,
    last_phase: Option<std::mem::Discriminant<Phase>>,
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
        self.graphics = Some(
            Graphics::new(&gal, &self.asset_root, Some(text))
                .map_err(|e| format!("create renderer: {e}"))?,
        );
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

    fn save_profile(&mut self) {
        if let Err(e) = self.storage.save_profile(&self.game.profile) {
            self.diagnostics.event_with(
                Level::Error,
                "planetfall.save",
                "profile_save_failed",
                [Field::new("error", e.to_string())],
            );
            self.game.notice = Some(planetfall_sim::game::Notice {
                text: format!("Could not save progress: {e}"),
                ticks: 400,
            });
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
        let on_surface = self.game.phase == Phase::Surface;
        match action {
            MenuAction::None => {}
            MenuAction::Launch | MenuAction::Resume => self.screens.clear(),
            MenuAction::OpenOptions => {
                let from_title = self.screens.last() == Some(&Screen::Title);
                self.menu.options.fullscreen = self.display.fullscreen;
                self.screens.push(Screen::Options { from_title });
            }
            MenuAction::Back => {
                self.screens.pop();
            }
            MenuAction::Apply(_) => {
                let o = self.menu.options.clone();
                self.prefs.master_volume = o.master;
                self.prefs.music_volume = o.music;
                self.prefs.effects_volume = o.effects;
                self.prefs.screen_shake = o.shake;
                if o.fullscreen != self.display.fullscreen {
                    self.display.fullscreen = o.fullscreen;
                    if let Some(w) = c.window {
                        w.set_fullscreen(o.fullscreen);
                    }
                }
                if let Some(audio) = &self.audio {
                    audio.apply(&self.prefs);
                }
                let _ = self.storage.save_preferences(&self.prefs);
                let _ = self.storage.save_display(&self.display);
                self.screens.pop();
            }
            MenuAction::AskReset => self.screens.push(Screen::ConfirmReset),
            MenuAction::ResetProgress => {
                self.game = Game::new(Profile::default(), self.settings.seed.wrapping_add(1));
                self.save_profile();
                self.screens = vec![Screen::Title];
            }
            MenuAction::AskAbandon => self.screens.push(Screen::ConfirmAbandon),
            MenuAction::Abandon => {
                self.screens.clear();
                self.controls.command = Some(Command::Abandon);
            }
            MenuAction::QuitToTitle => {
                self.save_profile();
                self.screens = vec![Screen::Title];
            }
            MenuAction::AskQuit => {
                let from_title = self.screens.last() == Some(&Screen::Title);
                self.screens.push(Screen::ConfirmQuit { from_title });
            }
            MenuAction::Quit => {
                if !on_surface {
                    self.save_profile();
                }
                return Some(Flow::Exit);
            }
            MenuAction::Game(command) => self.controls.command = Some(command),
        }
        None
    }

    fn react(&mut self) {
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
            self.save_profile();
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
        );
        if self.menu.is_open() {
            let input = self.controls.ui_input(c.input);
            let viewport = Rect::new(0.0, 0.0, self.size.0, self.size.1);
            let action = self
                .menu
                .update(&input, viewport, self.scale, &self.measure, self.dt);
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
        let title = self.screens.first() == Some(&Screen::Title);
        if let Some(audio) = &mut self.audio {
            audio.set_scene(self.game.phase, title);
            let focus = match (&self.game.run, self.game.phase) {
                (Some(run), Phase::Surface | Phase::Lost { .. }) => run.player.pos,
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
        let show_hud = !matches!(self.screens.first(), Some(Screen::Title));
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
        let frame = Frame {
            game,
            time,
            dt: self.dt,
            shake,
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
        // Mid-drop progress is never banked; the profile is already saved at every change.
        self.save_profile();
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

    fn configure_window(&mut self, window: &mut quasar_window::GameWindow) -> Result<(), AppError> {
        window
            .input
            .set_bindings(bindings::keyboard())
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
