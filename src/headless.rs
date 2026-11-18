//! Windowless modes: the headless simulation (with its summary line and
//! `--trace`), offscreen screenshots of the game and its menus, `--bench`
//! frame timings and the Basalt cover art.

use crate::settings::Settings;
use crate::storage;
#[cfg(feature = "graphics")]
use crate::{bindings, render, ui};
use planetfall_sim::autoplay::Autopilot;
use planetfall_sim::{Game, GameInput, Phase, Profile};

/// Events of the last few seconds before an offscreen render, so a single
/// frame still shows the drop's recent scorch marks and splats. The final
/// tick's own events are passed separately (they also emit particles).
#[derive(Default)]
struct History {
    ticks: std::collections::VecDeque<Vec<planetfall_sim::events::GameEvent>>,
}

impl History {
    const TICKS: usize = 60 * 20;

    fn record(&mut self, game: &Game) {
        // Marks belong to one drop.
        if game.run.is_none() {
            self.ticks.clear();
            return;
        }
        if self.ticks.len() == Self::TICKS {
            self.ticks.pop_front();
        }
        self.ticks.push_back(game.events.clone());
    }

    /// Everything but the last tick, oldest first.
    fn events(mut self) -> Vec<planetfall_sim::events::GameEvent> {
        self.ticks.pop_back();
        self.ticks.into_iter().flatten().collect()
    }
}

/// The open menu, for headless summaries and traces.
fn menu_name(game: &Game) -> &'static str {
    let run = game.run.as_ref();
    match (&game.terminal, run.and_then(|r| r.level_up_choices())) {
        (_, Some(_)) => "level-up",
        (Some(planetfall_sim::Terminal::Navigation), _) => "navigation",
        (Some(planetfall_sim::Terminal::Fabricator), _) => "fabricator",
        (Some(planetfall_sim::Terminal::Debrief(_)), _) => "debrief",
        (Some(planetfall_sim::Terminal::Module(_)), _) => "module",
        (Some(planetfall_sim::Terminal::Locker), _) => "locker",
        (Some(planetfall_sim::Terminal::MissionBoard), _) => "mission-board",
        (Some(planetfall_sim::Terminal::Trade), _) => "trade",
        (None, None) => "none",
    }
}

/// Run the simulation without a window, optionally under the autopilot.
pub fn run(
    settings: &Settings,
    mut game: Game,
    asset_root: &std::path::Path,
) -> Result<(), String> {
    let mut pilot = Autopilot::new();
    let mut history = History::default();
    let mut last_trace = String::new();
    for _ in 0..settings.ticks {
        let input = if settings.autoplay {
            pilot.input(&game)
        } else {
            GameInput::default()
        };
        game.update(&input);
        history.record(&game);
        if settings.trace {
            let run = game.run.as_ref();
            let state = format!(
                "phase={:?} menu={} build={:?} surge={} finds={} tutorial={:?}",
                game.phase,
                menu_name(&game),
                game.ship.build,
                run.map_or("-", |r| if r.surge.warning > 0 {
                    "warning"
                } else if r.surge.active > 0 {
                    "active"
                } else {
                    "-"
                }),
                run.map_or(0, |r| r.finds.len()),
                game.tutorial_step(),
            );
            // Count-down phases change every tick; compare their kind only.
            let key = state.split(" {").next().unwrap_or(&state).to_owned()
                + &state[state.find(" menu=").unwrap_or(0)..];
            if key != last_trace {
                println!("trace tick={} {state}", game.tick);
                last_trace = key;
            }
        }
    }
    let history = history.events();
    println!("{}", summary(&game));
    #[cfg(feature = "graphics")]
    {
        if let Some(frames) = settings.bench {
            bench(settings, &mut game, &mut pilot, frames, asset_root)?;
        }
        if let Some(path) = &settings.screenshot {
            screenshot(settings, game, &history, asset_root, path)?;
        }
    }
    #[cfg(not(feature = "graphics"))]
    {
        let _ = (history, asset_root);
        if settings.bench.is_some() || settings.screenshot.is_some() {
            return Err("--bench and --screenshot need the `graphics` feature".into());
        }
    }
    Ok(())
}

/// The one-line summary a headless run prints (CI and agents parse it).
fn summary(game: &Game) -> String {
    let run = game.run.as_ref();
    format!(
        "planetfall ticks={} phase={:?} menu={} tutorial={:?} drops={} extractions={} deaths={} kills={} materials={:?} modules={} run_level={} class={} classes={} build={:?} avg_drop={:.1}min best_threat={} threat={} surge={} hazard={} stash={} worn={} credits={} docked={} found_weapons={} hash={:016x}",
        game.tick,
        game.phase,
        menu_name(game),
        game.tutorial_step(),
        game.profile.stats.drops,
        game.profile.stats.extractions,
        game.profile.stats.deaths,
        game.profile.stats.kills + run.map_or(0, |r| r.kills),
        game.profile.materials,
        game.ship.layout.rooms.len().saturating_sub(3),
        run.map_or(0, |r| r.player().level),
        game.profile.class.key(),
        planetfall_sim::content::ClassId::all()
            .map(|c| format!(
                "{}:{}/{}xp",
                c.key(),
                game.profile.class_level(c).0,
                game.profile.class_record(c).xp
            ))
            .collect::<Vec<_>>()
            .join(","),
        game.ship.build,
        game.profile.stats.surface_seconds as f32 / 60.0 / game.profile.stats.drops.max(1) as f32,
        game.profile.stats.best_threat,
        run.map_or(0, |r| r.threat_level()),
        run.map_or("-", |r| if r.surge.warning > 0 {
            "warning"
        } else if r.surge.active > 0 {
            "active"
        } else {
            "-"
        }),
        game.profile.hazard_unlocked,
        game.profile.stash.len(),
        planetfall_sim::content::ClassId::all()
            .map(|c| game.profile.equipped(c).len())
            .sum::<usize>(),
        game.profile.credits,
        game.profile.docked,
        planetfall_sim::content::ClassId::all()
            .map(|c| game.profile.class_record(c).found_weapons.len())
            .sum::<usize>(),
        game.state_hash().value(),
    )
}

/// `--bench N`: render N more frames offscreen, stepping the game (under
/// the autopilot with `--autoplay`), and print their timings.
#[cfg(feature = "graphics")]
fn bench(
    settings: &Settings,
    game: &mut Game,
    pilot: &mut Autopilot,
    frames: u64,
    asset_root: &std::path::Path,
) -> Result<(), String> {
    let size = settings.resolution.unwrap_or((1600, 900));
    let (measure, text) = ui::build_text(asset_root)?;
    let scale = size.1 as f32 / 900.0;
    let prompts = bindings::Prompts::default();
    let autoplay = settings.autoplay;
    let report = render::bench(
        game,
        render::Bench {
            asset_root,
            text: Some(text),
            size,
            frames,
            record: settings.record.clone(),
        },
        &mut |game| {
            if autoplay {
                pilot.input(game)
            } else {
                GameInput::default()
            }
        },
        &mut |game| {
            vec![ui::hud::build(&ui::hud::HudContext {
                game,
                measure: &measure,
                size: (size.0 as f32, size.1 as f32),
                scale,
                prompts: &prompts,
                menu_open: false,
            })]
        },
        &|game, canvas, camera| ui::overlay::draw(canvas, camera, game, game.tick as f32 / 60.0),
    )?;
    print!("{report}");
    Ok(())
}

/// `--screenshot FILE`: render the final state (or a `--menu` screen) offscreen.
#[cfg(feature = "graphics")]
fn screenshot(
    settings: &Settings,
    game: Game,
    history: &[planetfall_sim::events::GameEvent],
    asset_root: &std::path::Path,
    path: &std::path::Path,
) -> Result<(), String> {
    // `--menu gear|trade`: open that terminal on the ship as it is.
    let mut game = game;
    let view = match settings.menu.as_deref() {
        Some("gear") => {
            game.terminal = Some(planetfall_sim::Terminal::Locker);
            Some(ui::menu::View {
                tab: 1,
                slot: Some(planetfall_sim::content::GearSlot::Suit),
                page: 0,
            })
        }
        Some("trade") => {
            game.terminal = Some(planetfall_sim::Terminal::Trade);
            game.profile.docked = true;
            game.profile.orbiting = None;
            if game.profile.market.is_empty() {
                game.profile
                    .restock(&mut quasar_core::Rng::seed_from_u64(7));
            }
            Some(ui::menu::View::default())
        }
        _ => None,
    };
    if view.is_some() {
        game.phase = Phase::Ship;
        game.run = None;
    }
    let menu_screen = settings
        .menu
        .as_deref()
        .filter(|m| !matches!(*m, "gear" | "trade" | "console"));
    // `--menu console`: the console after a command, mid-way through
    // the next one (on a drop, with the sentries handed over).
    let console = (settings.menu.as_deref() == Some("console")).then(|| {
        ui::console::Console::preview(
            &mut game,
            &["/give weapon sentry", "/give upgrade sentry mobile-sentry"],
            "/give upgrade sentry ",
        )
    });
    let size = settings.resolution.unwrap_or((1600, 900));
    let (measure, text) = ui::build_text(asset_root)?;
    let scale = size.1 as f32 / 900.0;
    let prompts = bindings::Prompts::default();
    let hud = ui::hud::build(&ui::hud::HudContext {
        game: &game,
        measure: &measure,
        size: (size.0 as f32, size.1 as f32),
        scale,
        prompts: &prompts,
        menu_open: false,
    });
    // Terminals and level-up choices render as they would on screen.
    use ui::menu::{Content, MenuContext, SavesMode, Screen};
    let surface = game.phase == Phase::Surface;
    let content = if let Some(name) = menu_screen {
        Some(Content::Screen(match name {
            "main" => Screen::Main,
            "saves" => Screen::Saves(SavesMode::Load),
            "pause" => Screen::Pause { surface },
            "controls" => Screen::Controls { gamepad: false },
            _ => Screen::Settings,
        }))
    } else if game
        .run
        .as_ref()
        .is_some_and(|r| r.level_up_choices().is_some())
    {
        Some(ui::menu::Content::LevelUp)
    } else if game.terminal.is_some() && game.phase == Phase::Ship {
        Some(ui::menu::Content::Terminal)
    } else {
        None
    };
    let mut menu = ui::menu::Menu::new(ui::menu::SettingsState {
        fullscreen: false,
        shadows: true,
        bloom: true,
        master: 1.0,
        music: 1.0,
        effects: 1.0,
        shake: true,
    });
    // Sample slots for the menu screenshots (nothing is read from disk).
    let context = MenuContext {
        slots: screenshot_slots(&game),
        continue_slot: Some(1),
        active_slot: Some(1),
        error: None,
    };
    if let Some(view) = view {
        menu.set_view(view, &game.terminal);
    }
    menu.show(content, &game, &measure, scale, &prompts, &context);
    let mut layers = if matches!(settings.menu.as_deref(), Some("main" | "saves")) {
        vec![]
    } else {
        vec![&hud]
    };
    let mut menu_layer = menu.take_layer();
    if let Some(l) = &mut menu_layer {
        layout(l, &measure, size, scale);
        layers.push(l);
    }
    let console_layer =
        console.map(|c| c.build(&game, 0.0, (size.0 as f32, size.1 as f32), scale, &measure));
    if let Some(l) = &console_layer {
        layers.push(l);
    }
    let overlay = |c: &mut quasar_render_2d::Canvas, camera: &render::camera::Camera| {
        ui::overlay::draw(c, camera, &game, game.tick as f32 / 60.0);
    };
    render::screenshot(
        &game,
        history,
        asset_root,
        Some(text),
        size,
        &layers,
        &overlay,
        path,
    )?;
    println!("wrote {}", path.display());
    Ok(())
}

/// Example slots for menu screenshots: slot 1 holds the current game.
#[cfg(feature = "graphics")]
fn screenshot_slots(game: &Game) -> Vec<(usize, storage::SlotState)> {
    use storage::{SlotState, SlotSummary};
    (1..=planetfall_sim::save::SLOTS)
        .map(|slot| {
            let state = if slot == 1 {
                SlotState::Saved(SlotSummary {
                    meta: planetfall_sim::save::SlotMeta {
                        created: 0,
                        saved: 0,
                        play_seconds: game.tick / 60,
                    },
                    location: game
                        .profile
                        .orbiting
                        .map_or("Deep space".into(), |p| format!("Orbiting {}", p.name())),
                    drops: game.profile.stats.drops,
                    extractions: game.profile.stats.extractions,
                    modules: game.profile.upgrades.len(),
                    tutorial: !game.profile.tutorial_complete,
                    mid_drop: game.run.is_some(),
                })
            } else {
                SlotState::Empty
            };
            (slot, state)
        })
        .collect()
}

#[cfg(feature = "graphics")]
fn layout(
    layer: &mut ui::Layer,
    measure: &quasar_ui::ShapedTextMeasure,
    size: (u32, u32),
    scale: f32,
) {
    let viewport = quasar_ui::Rect::new(0.0, 0.0, size.0 as f32, size.1 as f32);
    layer.ui.update(
        &quasar_ui::UiInput::default(),
        viewport,
        scale,
        quasar_ui::Edges::ZERO,
        measure,
        0.0,
    );
    layer.refresh_overlays(viewport, scale, measure);
}

/// Basalt cover art: a live surface battle with the title over it (3:4, 900x1200).
pub fn cover(asset_root: &std::path::Path, path: &std::path::Path) -> Result<(), String> {
    #[cfg(feature = "graphics")]
    {
        let mut profile = Profile {
            tutorial_complete: true,
            orbiting: planetfall_sim::content::PlanetId::from_key("cinder"),
            ..Profile::default()
        };
        if let Some(reactor) = planetfall_sim::content::UpgradeId::from_key("reactor") {
            profile.upgrades.insert(reactor, 1);
        }
        let mut game = Game::new(profile, 77);
        let mut pilot = Autopilot::new();
        let mut history = History::default();
        // Play until a fight is under way on the surface.
        for _ in 0..60 * 60 * 10 {
            if let Some(run) = &mut game.run {
                run.invulnerable = true;
                if game.phase == Phase::Surface
                    && run.ticks > 60 * 45
                    && run
                        .enemies
                        .iter()
                        .filter(|e| e.pos.distance(run.player().pos) < 9.0)
                        .count()
                        >= 7
                    && !run.beams.is_empty()
                    && run.level_up_choices().is_none()
                {
                    break;
                }
            }
            let input = pilot.input(&game);
            game.update(&input);
            history.record(&game);
        }
        let history = history.events();
        println!(
            "cover scene: phase={:?} enemies={}",
            game.phase,
            game.run.as_ref().map_or(0, |r| r.enemies.len())
        );
        let size = (900, 1200);
        let (measure, text) = ui::build_text(asset_root)?;
        let scale = 2.6;
        let mut b = ui::Builder::default();
        let root = b.root(false);
        let title = b.card(
            root,
            quasar_ui::LayoutItem::size(quasar_ui::Length::fixed(300.0), quasar_ui::Length::Auto)
                .with_offset(quasar_ui::Vec2::new(23.0, 20.0)),
            2.0,
            12.0,
        );
        b.label(title, "PLANETFALL", quasar_ui::TextStyle::Heading);
        b.label(
            title,
            "Drop. Mine. Survive. Extract.",
            quasar_ui::TextStyle::Small,
        );
        let mut layer = b.finish();
        layout(&mut layer, &measure, size, scale);
        let overlay = |_: &mut quasar_render_2d::Canvas, _: &render::camera::Camera| {};
        render::screenshot(
            &game,
            &history,
            asset_root,
            Some(text),
            size,
            &[&layer],
            &overlay,
            path,
        )?;
        println!("wrote {}", path.display());
        Ok(())
    }
    #[cfg(not(feature = "graphics"))]
    {
        let _ = (asset_root, path);
        Err("cover art needs the `graphics` feature".into())
    }
}
