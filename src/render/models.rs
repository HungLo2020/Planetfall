//! Model registry: GLB files decoded by `quasar-assets` and uploaded with
//! `quasar-render-3d`'s model adapter, looked up by name.
//!
//! Every `assets/models/<name>.glb` is loaded under its file stem, plus the
//! named entries in `assets/data/models.toml` (Kenney props). Content data
//! refers to models by these names, so new content needs no code here.
//! Models that carry a skin are characters (see `skins`).

use quasar_assets::model::Model3D;
use quasar_assets::{AssetServer, CacheBudget, GltfLoader};
use quasar_frame::{Gpu, GraphicsResult, error};
use quasar_render_3d::{
    MaterialConversion, ModelUploadOptions, ShadowRenderState3D, SurfaceRenderState3D,
    UploadedModel,
};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::Path;

/// Prop models the scenes draw directly (content models come from data).
pub mod names {
    pub const ASTRONAUT: &str = "astronaut";
    pub const DROP_POD: &str = "drop-pod";
    pub const DRONE: &str = "drone";
    pub const PLANET: &str = "planet";
    pub const SHIP_EXTERIOR: &str = "ship-exterior";
    pub const NAV_CONSOLE: &str = "nav-console";
    pub const POD_CRADLE: &str = "pod-cradle";
    pub const WORKBENCH: &str = "workbench";
    pub const METAL_PANEL: &str = "metal-panel";
    pub const CRATE_MEDIUM: &str = "crate-medium";
    pub const CRATE_WIDE: &str = "crate-wide";
    pub const BARREL: &str = "barrel";
    pub const XP_SHARD: &str = "xp-shard";
    pub const FERRITE_ORE: &str = "ferrite-ore";
    pub const LUMITE_ORE: &str = "lumite-ore";
    pub const VOIDSTONE_ORE: &str = "voidstone-ore";
    pub const FERRITE_CHUNK: &str = "ferrite-chunk";
    pub const LUMITE_CHUNK: &str = "lumite-chunk";
    pub const VOIDSTONE_CHUNK: &str = "voidstone-chunk";

    pub const ALL: [&str; 19] = [
        ASTRONAUT,
        DROP_POD,
        DRONE,
        PLANET,
        SHIP_EXTERIOR,
        NAV_CONSOLE,
        POD_CRADLE,
        WORKBENCH,
        METAL_PANEL,
        CRATE_MEDIUM,
        CRATE_WIDE,
        BARREL,
        XP_SHARD,
        FERRITE_ORE,
        LUMITE_ORE,
        VOIDSTONE_ORE,
        FERRITE_CHUNK,
        LUMITE_CHUNK,
        VOIDSTONE_CHUNK,
    ];
}

#[derive(Deserialize)]
struct ModelsFile {
    models: BTreeMap<String, String>,
}

/// Every model name and its file, relative to the asset root.
pub fn catalog(root: &Path) -> Result<BTreeMap<String, String>, String> {
    let mut files = BTreeMap::new();
    let dir = root.join("models");
    let entries = std::fs::read_dir(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "glb")
            && let Some(stem) = path.file_stem().and_then(|s| s.to_str())
        {
            files.insert(stem.to_owned(), format!("models/{stem}.glb"));
        }
    }
    let aliases = root.join("data/models.toml");
    let named: ModelsFile = quasar_config::load_file_typed(&aliases)
        .map_err(|e| format!("{}: {e}", aliases.display()))?;
    files.extend(named.models);
    Ok(files)
}

/// Model names referenced by content data and the scenes.
pub fn required() -> Vec<String> {
    use planetfall_sim::content::{EnemyId, UpgradeId, WeaponId};
    let mut names: Vec<String> = names::ALL.iter().map(|s| (*s).to_owned()).collect();
    names.extend(EnemyId::all().map(|e| e.info().model.clone()));
    names.extend(WeaponId::all().map(|w| w.info().model.clone()));
    names.extend(UpgradeId::all().map(|u| u.info().station.clone()));
    names
}

pub struct Models {
    gpu: BTreeMap<String, UploadedModel>,
    /// Decoded skinned characters, kept for their skins and clips.
    sources: BTreeMap<String, Model3D>,
}

fn loader() -> GltfLoader {
    // Characters carry skins and clips; Kenney crates ship opening clips
    // that are imported but never played.
    GltfLoader {
        allow_animation_default_pose: true,
        ..GltfLoader::animated()
    }
}

impl Models {
    pub fn load(gal: &Gpu, root: &Path) -> GraphicsResult<Self> {
        let catalog = catalog(root).map_err(error)?;
        let missing: Vec<String> = required()
            .into_iter()
            .filter(|name| !catalog.contains_key(name))
            .collect();
        if !missing.is_empty() {
            return Err(error(format!("models not found: {}", missing.join(", "))));
        }
        let mut server = AssetServer::with_budget(CacheBudget::new(64 * 1024 * 1024, 64));
        server
            .mount_file("", root)
            .map_err(|e| error(format!("mount {}: {e}", root.display())))?;
        server.register(loader());
        let mut gpu = BTreeMap::new();
        let mut sources = BTreeMap::new();
        for (name, file) in catalog {
            let model = server
                .load::<GltfLoader>(&file)
                .map_err(|e| error(format!("load {file}: {e}")))?;
            let uploaded = UploadedModel::upload(
                gal,
                &model,
                ModelUploadOptions {
                    material_conversion: MaterialConversion::ClassicApproximation,
                    ..ModelUploadOptions::default()
                },
            )?;
            gpu.insert(name.clone(), uploaded);
            if !model.skins.is_empty() {
                sources.insert(name, (*model).clone());
            }
        }
        Ok(Self { gpu, sources })
    }

    /// A model by name. Names are checked at load (`required`), so a miss
    /// here is a scene bug.
    pub fn get(&self, name: &str) -> &UploadedModel {
        self.gpu
            .get(name)
            .unwrap_or_else(|| panic!("model {name:?} is not loaded"))
    }

    /// Decoded skinned characters by name.
    pub fn characters(&self) -> impl Iterator<Item = (&str, &Model3D)> {
        self.sources.iter().map(|(n, m)| (n.as_str(), m))
    }

    /// Samplers must be installed on each renderer before drawing.
    pub fn bind(
        &self,
        gal: &Gpu,
        world: &mut SurfaceRenderState3D,
        shadows: &mut ShadowRenderState3D,
    ) -> GraphicsResult<()> {
        for m in self.gpu.values() {
            m.bind_samplers(gal, world)?;
            m.bind_shadow_samplers(gal, shadows)?;
        }
        Ok(())
    }

    pub fn release(&mut self, gal: &Gpu) -> GraphicsResult<()> {
        for m in self.gpu.values_mut() {
            m.release(gal)?;
        }
        self.gpu.clear();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("assets")
    }

    fn server() -> AssetServer {
        let mut server = AssetServer::with_budget(CacheBudget::new(64 * 1024 * 1024, 64));
        server.mount_file("", root()).unwrap();
        server.register(loader());
        server
    }

    /// Every catalogued model decodes on the CPU, and every model the
    /// content data names exists.
    #[test]
    fn all_models_decode_and_content_models_exist() {
        let catalog = catalog(&root()).unwrap();
        for name in required() {
            assert!(catalog.contains_key(&name), "missing model {name:?}");
        }
        let mut server = server();
        for (name, file) in &catalog {
            let model = server
                .load::<GltfLoader>(file)
                .unwrap_or_else(|e| panic!("{name}: {e}"));
            assert!(!model.instances.is_empty(), "{name} has no instances");
        }
    }

    /// Characters are skinned with the clips the painter plays; props keep
    /// the named pivots the renderer turns.
    #[test]
    fn rigs_expose_their_joints_and_clips() {
        use quasar_animation::model::{clips_from_model, skeleton_from_model};
        let mut server = server();
        for (file, joints, clips) in [
            (
                "models/astronaut.glb",
                &["leg_l", "leg_r", "arm_l", "arm_r", "head", "torso"][..],
                &["idle", "walk", "aim"][..],
            ),
            (
                "models/husk.glb",
                &["leg_l", "arm_r", "head"][..],
                &["idle", "walk", "attack"][..],
            ),
            (
                "models/brute.glb",
                &["leg_r", "arm_l"][..],
                &["idle", "walk", "attack"][..],
            ),
            (
                "models/skitter.glb",
                &["leg_l0", "leg_r2", "head"][..],
                &["idle", "walk", "attack"][..],
            ),
            (
                "models/bloater.glb",
                &["body", "leg_l"][..],
                &["walk", "pulse"][..],
            ),
        ] {
            let model = server.load::<GltfLoader>(file).unwrap();
            let (skeleton, binding) = skeleton_from_model(&model, 0).unwrap();
            for joint in joints {
                assert!(
                    skeleton.bones().iter().any(|b| b.name == *joint),
                    "{file} missing joint {joint}"
                );
            }
            assert_eq!(binding.inverse_bind_matrices.len(), skeleton.bones().len());
            let names: Vec<String> = clips_from_model(&model)
                .unwrap()
                .iter()
                .map(|c| c.name().to_owned())
                .collect();
            for clip in clips {
                assert!(
                    names.iter().any(|n| n == clip),
                    "{file} missing clip {clip}"
                );
            }
            assert!(model.primitives.iter().all(|p| p.skin.is_some()));
        }
        let pod = server.load::<GltfLoader>("models/drop-pod.glb").unwrap();
        for joint in ["door", "thruster"] {
            assert!(pod.instances.iter().any(|i| i.name == joint));
        }
    }
}
