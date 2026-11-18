//! Planetfall entry point: desktop play, headless simulation and offscreen
//! rendering (screenshots and Basalt cover art).

// The simulation-only build leaves the menu, slot and display plumbing unused.
#![cfg_attr(not(feature = "graphics"), allow(dead_code, unused_imports))]

#[cfg(feature = "desktop")]
mod audio;
#[cfg(feature = "graphics")]
mod bindings;
#[cfg(feature = "desktop")]
mod controls;
#[cfg(feature = "desktop")]
mod desktop;
mod headless;
#[cfg(feature = "desktop")]
mod interp;
#[cfg(feature = "graphics")]
mod render;
mod settings;
mod storage;
#[cfg(feature = "graphics")]
mod ui;

use planetfall_sim::{Game, Profile};
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
    install_content(&asset_root)?;
    // Item icons are looked up while menus are built, before any renderer.
    #[cfg(feature = "graphics")]
    ui::icons::load(&asset_root)?;
    storage.migrate_legacy();
    if let Some(path) = &settings.cover {
        return headless::cover(&asset_root, path);
    }
    if settings.headless {
        // `--slot N` runs (or renders) a saved game headless, read-only.
        let game = match settings.slot {
            Some(slot) => storage.load_slot(slot)?.restore(settings.seed),
            None => Game::new(Profile::default(), settings.seed),
        };
        return headless::run(&settings, game, &asset_root);
    }
    #[cfg(feature = "desktop")]
    {
        let _ = &mut storage;
        desktop::run(desktop::Launch {
            settings,
            storage,
            asset_root,
        })
    }
    #[cfg(not(feature = "desktop"))]
    {
        let _ = (&mut storage, asset_root);
        Err("this build has no desktop support; run with --headless".into())
    }
}

/// Load the content tables from `assets/data/` (falling back to the copy
/// compiled into the game when the directory is missing) and install them.
fn install_content(asset_root: &std::path::Path) -> Result<(), String> {
    use planetfall_sim::content::{self, Content, ContentFiles, FILE_NAMES};
    let dir = asset_root.join("data");
    if !dir.is_dir() {
        return Ok(());
    }
    let read = |name: &str| {
        std::fs::read_to_string(dir.join(name))
            .map_err(|e| format!("{}: {e}", dir.join(name).display()))
    };
    let texts: Vec<String> = FILE_NAMES
        .iter()
        .map(|n| read(n))
        .collect::<Result<_, _>>()?;
    let loaded = Content::from_toml(ContentFiles {
        planets: &texts[0],
        weapons: &texts[1],
        enemies: &texts[2],
        perks: &texts[3],
        upgrades: &texts[4],
        tuning: &texts[5],
        classes: &texts[6],
        mods: &texts[7],
        objectives: &texts[8],
        sites: &texts[9],
        mutators: &texts[10],
        gear: &texts[11],
        affixes: &texts[12],
    })
    .map_err(|e| format!("content data in {}: {e}", dir.display()))?;
    content::install(loaded).map_err(|_| "content was read before it was installed".to_owned())
}
