//! The item icon atlas (`assets/ui/icons.png`, drawn by
//! `DevUtils/assets/generate_icons.py`) and its manifest of names.

use std::sync::OnceLock;

/// Image id of the atlas in `quasar-ui` image widgets.
pub const ATLAS: u64 = 1;
/// Image id of the PLANETFALL logo.
pub const LOGO: u64 = 2;

struct Atlas {
    size: f32,
    columns: usize,
    names: Vec<String>,
}

static ATLAS_INFO: OnceLock<Atlas> = OnceLock::new();

/// Read the manifest. Missing or broken manifests leave icons blank.
pub fn load(asset_root: &std::path::Path) -> Result<(), String> {
    let path = asset_root.join("ui").join("icons.toml");
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let number = |key: &str| {
        text.lines()
            .find_map(|l| l.strip_prefix(key))
            .and_then(|l| {
                l.trim()
                    .trim_start_matches('=')
                    .trim()
                    .parse::<usize>()
                    .ok()
            })
            .ok_or_else(|| format!("{}: missing {key}", path.display()))
    };
    let atlas = Atlas {
        size: number("size")? as f32,
        columns: number("columns")?.max(1),
        names: text
            .lines()
            .map(str::trim)
            .filter(|l| l.starts_with('"'))
            .map(|l| l.trim_end_matches(',').trim_matches('"').to_owned())
            .collect(),
    };
    let _ = ATLAS_INFO.set(atlas);
    Ok(())
}

/// Source rectangle of `name` in the atlas, in pixels.
pub fn source(name: &str) -> Option<quasar_ui::Rect> {
    let atlas = ATLAS_INFO.get()?;
    let index = atlas.names.iter().position(|n| n == name)?;
    let (col, row) = (index % atlas.columns, index / atlas.columns);
    Some(quasar_ui::Rect::new(
        col as f32 * atlas.size,
        row as f32 * atlas.size,
        atlas.size,
        atlas.size,
    ))
}

/// The logo image path.
pub fn logo_path(asset_root: &std::path::Path) -> std::path::PathBuf {
    asset_root.join("ui").join("logo.png")
}

/// The atlas image path.
pub fn image_path(asset_root: &std::path::Path) -> std::path::PathBuf {
    asset_root.join("ui").join("icons.png")
}
