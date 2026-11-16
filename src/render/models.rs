//! Model registry: GLB files decoded by `quasar-assets` and uploaded with
//! `quasar-render-3d`'s model adapter.

use quasar_assets::model::Model3D;
use quasar_assets::{AssetServer, CacheBudget, GltfLoader};
use quasar_frame::{Gpu, GraphicsResult, error};
use quasar_render_3d::{
    MaterialConversion, ModelUploadOptions, ShadowRenderState3D, SurfaceRenderState3D,
    UploadedModel,
};
use std::collections::BTreeMap;
use std::path::Path;

macro_rules! models {
    ($($id:ident => $file:literal,)*) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
        pub enum ModelId { $($id,)* }
        impl ModelId {
            pub const ALL: &'static [ModelId] = &[$(ModelId::$id,)*];
            pub fn file(self) -> &'static str {
                match self { $(ModelId::$id => $file,)* }
            }
        }
    };
}

models! {
    Astronaut => "models/astronaut.glb",
    Husk => "models/husk.glb",
    Brute => "models/brute.glb",
    Skitter => "models/skitter.glb",
    Bloater => "models/bloater.glb",
    DropPod => "models/drop-pod.glb",
    ArcCaster => "models/arc-caster.glb",
    PlasmaMortar => "models/plasma-mortar.glb",
    Drone => "models/drone.glb",
    FerriteOre => "models/ferrite-ore.glb",
    LumiteOre => "models/lumite-ore.glb",
    VoidstoneOre => "models/voidstone-ore.glb",
    XpShard => "models/xp-shard.glb",
    FerriteChunk => "models/ferrite-chunk.glb",
    LumiteChunk => "models/lumite-chunk.glb",
    VoidstoneChunk => "models/voidstone-chunk.glb",
    Beacon => "models/beacon.glb",
    NavConsole => "models/nav-console.glb",
    PodCradle => "models/pod-cradle.glb",
    ReactorCore => "models/reactor-core.glb",
    MedBay => "models/med-bay.glb",
    WeaponRack => "models/weapon-rack.glb",
    DrillRig => "models/drill-rig.glb",
    ShipExterior => "models/ship-exterior.glb",
    Planet => "models/planet.glb",
    Blaster => "kenney/blaster-kit/blaster-d.glb",
    Scattergun => "kenney/blaster-kit/blaster-f.glb",
    CrateMedium => "kenney/blaster-kit/crate-medium.glb",
    CrateWide => "kenney/blaster-kit/crate-wide.glb",
    Barrel => "kenney/survival-kit/barrel.glb",
    Workbench => "kenney/survival-kit/workbench.glb",
    MetalPanel => "kenney/survival-kit/metal-panel.glb",
}

pub struct Models {
    gpu: BTreeMap<ModelId, UploadedModel>,
    /// Decoded skinned characters, kept for their skins and clips.
    sources: BTreeMap<ModelId, Model3D>,
}

impl Models {
    pub fn load(gal: &Gpu, root: &Path) -> GraphicsResult<Self> {
        let mut server = AssetServer::with_budget(CacheBudget::new(64 * 1024 * 1024, 64));
        server
            .mount_file("", root)
            .map_err(|e| error(format!("mount {}: {e}", root.display())))?;
        // Characters carry skins and clips; Kenney crates ship opening clips
        // that are imported but never played.
        server.register(GltfLoader {
            allow_animation_default_pose: true,
            ..GltfLoader::animated()
        });
        let mut gpu = BTreeMap::new();
        let mut sources = BTreeMap::new();
        for &id in ModelId::ALL {
            let model = server
                .load::<GltfLoader>(id.file())
                .map_err(|e| error(format!("load {}: {e}", id.file())))?;
            let uploaded = UploadedModel::upload(
                gal,
                &model,
                ModelUploadOptions {
                    material_conversion: MaterialConversion::ClassicApproximation,
                    ..ModelUploadOptions::default()
                },
            )?;
            gpu.insert(id, uploaded);
            if super::skins::CHARACTERS.contains(&id) {
                sources.insert(id, (*model).clone());
            }
        }
        Ok(Self { gpu, sources })
    }

    pub fn get(&self, id: ModelId) -> &UploadedModel {
        &self.gpu[&id]
    }

    /// The decoded glTF of a skinned character.
    pub fn source(&self, id: ModelId) -> &Model3D {
        &self.sources[&id]
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

    /// Every model decodes on the CPU (no GPU needed) and has drawable parts.
    fn server() -> AssetServer {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets");
        let mut server = AssetServer::with_budget(CacheBudget::new(64 * 1024 * 1024, 64));
        server.mount_file("", &root).unwrap();
        server.register(GltfLoader {
            allow_animation_default_pose: true,
            ..GltfLoader::animated()
        });
        server
    }

    #[test]
    fn all_models_decode() {
        let mut server = server();
        for &id in ModelId::ALL {
            let model = server
                .load::<GltfLoader>(id.file())
                .unwrap_or_else(|e| panic!("{}: {e}", id.file()));
            assert!(
                !model.instances.is_empty(),
                "{} has no instances",
                id.file()
            );
        }
    }

    /// Characters are skinned with the clips the painter plays; props keep
    /// the named pivots the renderer turns.
    #[test]
    fn rigs_expose_their_joints_and_clips() {
        use quasar_animation::model::{clips_from_model, skeleton_from_model};
        let mut server = server();
        for (id, joints, clips) in [
            (
                ModelId::Astronaut,
                &["leg_l", "leg_r", "arm_l", "arm_r", "head", "torso"][..],
                &["idle", "walk", "aim"][..],
            ),
            (
                ModelId::Husk,
                &["leg_l", "arm_r", "head"][..],
                &["idle", "walk", "attack"][..],
            ),
            (
                ModelId::Brute,
                &["leg_r", "arm_l"][..],
                &["idle", "walk", "attack"][..],
            ),
            (
                ModelId::Skitter,
                &["leg_l0", "leg_r2", "head"][..],
                &["idle", "walk", "attack"][..],
            ),
            (
                ModelId::Bloater,
                &["body", "leg_l"][..],
                &["walk", "pulse"][..],
            ),
        ] {
            let model = server.load::<GltfLoader>(id.file()).unwrap();
            let (skeleton, binding) = skeleton_from_model(&model, 0).unwrap();
            for joint in joints {
                assert!(
                    skeleton.bones().iter().any(|b| b.name == *joint),
                    "{id:?} missing joint {joint}"
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
                    "{id:?} missing clip {clip}"
                );
            }
            assert!(model.primitives.iter().all(|p| p.skin.is_some()));
        }
        let pod = server.load::<GltfLoader>(ModelId::DropPod.file()).unwrap();
        for joint in ["door", "thruster"] {
            assert!(pod.instances.iter().any(|i| i.name == joint));
        }
    }
}
