//! Interface icons: one hand-editable PNG per icon in `assets/ui/icons/`
//! (named by the content id or glyph name, e.g. `scattergun.png`,
//! `obj-hunt.png`, `xbox-a.png`), packed into one atlas texture when the game
//! starts. Any size works; each icon draws scaled into its widget's box.

use quasar_render_2d::Image;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::OnceLock;

/// Image id of the atlas in `quasar-ui` image widgets.
pub const ATLAS: u64 = 1;
/// Image id of the PLANETFALL logo.
pub const LOGO: u64 = 2;

/// Icons per atlas row.
const COLUMNS: u32 = 16;

struct Atlas {
    rects: BTreeMap<String, quasar_ui::Rect>,
    image: Image,
}

static ATLAS_INFO: OnceLock<Atlas> = OnceLock::new();

/// Read and pack every icon in `assets/ui/icons`. Unreadable files are
/// skipped (their widgets stay blank) and reported.
pub fn load(asset_root: &Path) -> Result<(), String> {
    if ATLAS_INFO.get().is_some() {
        return Ok(());
    }
    let dir = asset_root.join("ui").join("icons");
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "png"))
        .collect();
    files.sort();
    let mut icons = Vec::new();
    for path in files {
        let Some(name) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        match Image::load_png(&path) {
            Ok(image) => icons.push((name.to_owned(), image)),
            Err(e) => eprintln!("planetfall: icon {}: {e}", path.display()),
        }
    }
    // Uniform cells as large as the largest icon, row-major.
    let cell = icons
        .iter()
        .map(|(_, i)| i.width().max(i.height()))
        .max()
        .unwrap_or(1);
    let rows = (icons.len() as u32).div_ceil(COLUMNS).max(1);
    let (width, height) = (COLUMNS * cell, rows * cell);
    let mut rgba = vec![0u8; (width * height * 4) as usize];
    let mut rects = BTreeMap::new();
    for (index, (name, icon)) in icons.iter().enumerate() {
        let (x0, y0) = (
            (index as u32 % COLUMNS) * cell,
            (index as u32 / COLUMNS) * cell,
        );
        let src = icon.rgba();
        let row = (icon.width() * 4) as usize;
        for y in 0..icon.height() {
            let from = (y * icon.width() * 4) as usize;
            let to = (((y0 + y) * width + x0) * 4) as usize;
            rgba[to..to + row].copy_from_slice(&src[from..from + row]);
        }
        rects.insert(
            name.clone(),
            quasar_ui::Rect::new(
                x0 as f32,
                y0 as f32,
                icon.width() as f32,
                icon.height() as f32,
            ),
        );
    }
    let image = Image::new(width, height, rgba).map_err(|e| format!("icon atlas: {e}"))?;
    let _ = ATLAS_INFO.set(Atlas { rects, image });
    Ok(())
}

/// Source rectangle of `name` in the atlas, in pixels.
pub fn source(name: &str) -> Option<quasar_ui::Rect> {
    ATLAS_INFO.get()?.rects.get(name).copied()
}

/// The packed atlas (after [`load`]).
pub fn atlas() -> Option<Image> {
    ATLAS_INFO.get().map(|a| a.image.clone())
}

/// The logo image path.
pub fn logo_path(asset_root: &Path) -> std::path::PathBuf {
    asset_root.join("ui").join("logo.png")
}
