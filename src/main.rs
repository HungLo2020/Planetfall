//! Planetfall entry point: desktop play, headless simulation and offscreen
//! rendering (screenshots and Basalt cover art).

#[cfg(feature = "desktop")]
mod audio;
#[cfg(feature = "desktop")]
mod controls;
#[cfg(feature = "desktop")]
mod desktop;
#[cfg(feature = "graphics")]
mod render;
mod settings;
mod storage;
#[cfg(feature = "graphics")]
mod ui;

use planetfall_sim::autoplay::Autopilot;
use planetfall_sim::{Game, GameInput, Phase, Profile};
use settings::Settings;
use storage::Storage;

fn main() {
    match run() {
        Ok(()) => {}
        Err(e) => {
            eprintln!("planetfall: {e}");
            std::process::exit(1);
        }
    }
}

fn run() -> Result<(), String> {
    let Some(settings) = Settings::parse(std::env::args().skip(1))? else {
        return Ok(());
    };
    let headless_output = settings.headless || settings.cover.is_some();
    let mut storage = Storage::discover(settings.no_save || headless_output)
        .map_err(|e| format!("data directory: {e}"))?;
    let asset_root = storage.asset_root(settings.asset_root.as_deref());
    if let Some(path) = &settings.cover {
        return cover(&asset_root, path);
    }
    let (profile, warning) = if settings.fresh || settings.headless {
        (Profile::default(), None)
    } else {
        storage.load_profile()
    };
    if settings.headless {
        return headless(&settings, profile, &asset_root);
    }
    #[cfg(feature = "desktop")]
    {
        desktop::run(desktop::Launch {
            settings,
            storage,
            profile,
            warning,
            asset_root,
        })
    }
    #[cfg(not(feature = "desktop"))]
    {
        let _ = (warning, asset_root);
        Err("this build has no desktop support; run with --headless".into())
    }
}

/// Run the simulation without a window, optionally under the autopilot.
fn headless(
    settings: &Settings,
    profile: Profile,
    asset_root: &std::path::Path,
) -> Result<(), String> {
    let mut game = Game::new(profile, settings.seed);
    let mut pilot = Autopilot::new();
    for _ in 0..settings.ticks {
        let input = if settings.autoplay {
            pilot.input(&game)
        } else {
            GameInput::default()
        };
        game.update(&input);
    }
    let run = game.run.as_ref();
    println!(
        "planetfall ticks={} phase={:?} menu={} tutorial={:?} drops={} extractions={} deaths={} kills={} materials={:?} modules={} run_level={} hash={:016x}",
        game.tick,
        game.phase,
        match (&game.terminal, run.and_then(|r| r.level_up_choices())) {
            (_, Some(_)) => "level-up",
            (Some(planetfall_sim::Terminal::Navigation), _) => "navigation",
            (Some(planetfall_sim::Terminal::Fabricator), _) => "fabricator",
            (Some(planetfall_sim::Terminal::Debrief(_)), _) => "debrief",
            (Some(planetfall_sim::Terminal::Module(_)), _) => "module",
            (None, None) => "none",
        },
        game.tutorial_step(),
        game.profile.stats.drops,
        game.profile.stats.extractions,
        game.profile.stats.deaths,
        game.profile.stats.kills + run.map_or(0, |r| r.kills),
        game.profile.materials,
        game.ship.layout.rooms.len().saturating_sub(3),
        run.map_or(0, |r| r.player.level),
        game.state_hash(),
    );
    if let Some(path) = &settings.screenshot {
        #[cfg(feature = "graphics")]
        {
            let size = settings.resolution.unwrap_or((1600, 900));
            let (measure, text) = ui::build_text(asset_root)?;
            let scale = size.1 as f32 / 900.0;
            let mut hud = ui::hud::build(&ui::hud::HudContext {
                game: &game,
                measure: &measure,
                size: (size.0 as f32, size.1 as f32),
                scale,
                gamepad: false,
                menu_open: false,
            });
            layout(&mut hud, &measure, size, scale);
            // Terminals and level-up choices render as they would on screen.
            use ui::menu::{Content, Screen};
            let surface = game.phase == Phase::Surface;
            let content = if let Some(name) = &settings.menu {
                Some(Content::Screen(match name.as_str() {
                    "title" => Screen::Title,
                    "pause" => Screen::Pause { surface },
                    _ => Screen::Options { from_title: true },
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
            let mut menu = ui::menu::Menu::new(ui::menu::OptionsState {
                fullscreen: false,
                master: 1.0,
                music: 1.0,
                effects: 1.0,
                shake: true,
            });
            menu.show(content, &game, &measure, scale, false);
            let mut layers = if settings.menu.as_deref() == Some("title") {
                vec![]
            } else {
                vec![&hud]
            };
            let mut menu_layer = menu.take_layer();
            if let Some(l) = &mut menu_layer {
                layout(l, &measure, size, scale);
                layers.push(l);
            }
            let overlay = |c: &mut quasar_render_2d::Canvas, camera: &render::camera::Camera| {
                ui::overlay::draw(c, camera, &game, game.tick as f32 / 60.0);
            };
            render::screenshot(&game, asset_root, Some(text), size, &layers, &overlay, path)?;
            println!("wrote {}", path.display());
        }
        #[cfg(not(feature = "graphics"))]
        {
            let _ = (path, asset_root);
            return Err("screenshots need the `graphics` feature".into());
        }
    }
    Ok(())
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
}

/// Basalt cover art: a live surface battle with the title over it (3:4, 900x1200).
fn cover(asset_root: &std::path::Path, path: &std::path::Path) -> Result<(), String> {
    #[cfg(feature = "graphics")]
    {
        let mut profile = Profile {
            tutorial_complete: true,
            orbiting: Some(planetfall_sim::content::PlanetId::Cinder),
            ..Profile::default()
        };
        profile.upgrades.nav_computer = 1;
        let mut game = Game::new(profile, 77);
        let mut pilot = Autopilot::new();
        // Play until a fight is under way on the surface.
        for _ in 0..60 * 60 * 10 {
            if let Some(run) = &mut game.run {
                run.invulnerable = true;
                if game.phase == Phase::Surface
                    && run.ticks > 60 * 45
                    && run
                        .enemies
                        .iter()
                        .filter(|e| planetfall_sim::math::distance(e.pos, run.player.pos) < 9.0)
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
        }
        println!(
            "cover scene: phase={:?} enemies={}",
            game.phase,
            game.run.as_ref().map_or(0, |r| r.enemies.len())
        );
        let size = (900, 1200);
        let (measure, text) = ui::build_text(asset_root)?;
        let scale = 2.6;
        let mut b = ui::Builder::new(&measure, scale);
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
