//! Skinned characters: glTF skins and clips imported by `quasar-assets`,
//! sampled with `quasar-animation`, posed on the GPU by `quasar-render-3d`'s
//! `SkinningPass`.
//!
//! Each character model carries a skeleton (one joint per body part), a lit
//! body primitive, an unlit glow primitive and its clips (see
//! `DevUtils/assets/generate_models.py`). The painter samples a pose and asks
//! for a skinned draw; the renderer gives every on-screen character its own
//! pooled `SkinnedMesh` per primitive, records one compute dispatch per mesh
//! and draws the posed vertices through the ordinary surface and shadow
//! passes.

use super::models::Models;
use quasar_animation::model::{SkinBinding, clips_from_model, skeleton_from_model};
use quasar_animation::{Clip, Pose, Skeleton};
use quasar_assets::model::Model3D;
use quasar_frame::{Gpu, GraphicsResult, error};
use quasar_render_3d::{Mat4, ShadingModel, SkinnedMesh, SkinningPass, SurfaceDraw3D};
use std::collections::BTreeMap;
use vulkanic_gal::CommandBuffer;

/// Upper bound on simultaneously posed instances of one character.
const MAX_INSTANCES: usize = 160;

/// HDR boost for the unlit glow primitive so eyes and lamps bloom.
const GLOW: f32 = 2.4;

/// Rig data for one character model.
pub struct Character {
    model: Model3D,
    skeleton: Skeleton,
    binding: SkinBinding,
    clips: BTreeMap<String, Clip>,
    /// Skinned primitives (indices into the model and its upload).
    primitives: Vec<usize>,
}

/// One clip contribution to a pose. Clips with disjoint tracks layer; clips
/// sharing tracks crossfade by `weight`.
#[derive(Clone, Copy, Debug)]
pub struct Layer {
    pub clip: &'static str,
    pub time: f32,
    pub weight: f32,
}

impl Layer {
    pub fn full(clip: &'static str, time: f32) -> Self {
        Self {
            clip,
            time,
            weight: 1.0,
        }
    }
}

impl Character {
    fn load(model: Model3D) -> GraphicsResult<Self> {
        let (skeleton, binding) =
            skeleton_from_model(&model, 0).map_err(|e| error(format!("skeleton: {e}")))?;
        let clips = clips_from_model(&model)
            .map_err(|e| error(format!("clips: {e}")))?
            .into_iter()
            .map(|clip| (clip.name().to_owned(), clip))
            .collect();
        let primitives = (0..model.primitives.len())
            .filter(|&i| model.primitives[i].skin.is_some())
            .collect::<Vec<_>>();
        if primitives.is_empty() {
            return Err(error("character has no skinned primitives"));
        }
        Ok(Self {
            model,
            skeleton,
            binding,
            clips,
            primitives,
        })
    }

    pub fn has(&self, clip: &str) -> bool {
        self.clips.contains_key(clip)
    }

    /// Sample and combine `layers` into a skeleton pose; returns the joint
    /// palette and each bone's model-space matrix.
    pub fn pose(&self, layers: &[Layer]) -> Posed {
        let mut pose: Option<Pose> = None;
        for layer in layers {
            let Some(clip) = self.clips.get(layer.clip) else {
                continue;
            };
            let time = layer.time.rem_euclid(clip.duration().max(1e-3));
            let sampled = clip.sample(time);
            pose = Some(match pose {
                None => sampled,
                Some(base) => base.blend(&sampled, layer.weight.clamp(0.0, 1.0)),
            });
        }
        let skeleton_pose = self.skeleton.sample(&pose.unwrap_or_default());
        let palette = self
            .binding
            .joint_matrices(&skeleton_pose)
            .into_iter()
            .map(Mat4)
            .collect();
        let bones = self.binding.bone_world(&skeleton_pose);
        let names = self
            .skeleton
            .bones()
            .iter()
            .map(|b| b.name.to_string())
            .zip(bones.into_iter().map(Mat4))
            .collect();
        Posed {
            palette,
            bones: names,
        }
    }
}

/// A sampled pose ready for the GPU.
pub struct Posed {
    pub palette: Vec<Mat4>,
    bones: Vec<(String, Mat4)>,
}

impl Posed {
    /// Model-space matrix of a bone, for attachments (held weapons).
    pub fn bone(&self, name: &str) -> Option<Mat4> {
        self.bones.iter().find(|(n, _)| n == name).map(|(_, m)| *m)
    }
}

/// A skinned draw requested by the painter.
pub struct SkinRequest {
    /// Character model name.
    pub id: &'static str,
    pub palette: Vec<Mat4>,
    pub root: Mat4,
    /// Added red-white emission (hit flash).
    pub flash: f32,
}

/// Rigs plus the per-instance GPU meshes they pose into.
pub struct Skins {
    characters: BTreeMap<String, Character>,
    /// Per character: pooled instances, each one mesh per skinned primitive.
    pool: BTreeMap<&'static str, Vec<Vec<SkinnedMesh>>>,
    pass: SkinningPass,
}

impl Skins {
    pub fn new(gal: &Gpu, models: &Models) -> GraphicsResult<Self> {
        let mut characters = BTreeMap::new();
        for (name, model) in models.characters() {
            characters.insert(name.to_owned(), Character::load(model.clone())?);
        }
        Ok(Self {
            characters,
            pool: BTreeMap::new(),
            pass: SkinningPass::new(gal)?,
        })
    }

    pub fn character(&self, name: &str) -> &Character {
        self.characters
            .get(name)
            .unwrap_or_else(|| panic!("{name:?} is not a skinned character"))
    }

    /// Pose every request on the GPU and return the command buffer to submit
    /// before the passes that draw them, plus their draws.
    pub fn prepare<'a>(
        &'a mut self,
        gal: &Gpu,
        models: &'a Models,
        requests: &[SkinRequest],
    ) -> GraphicsResult<(Option<CommandBuffer>, Vec<SurfaceDraw3D<'a>>)> {
        if requests.is_empty() {
            return Ok((None, Vec::new()));
        }
        // Grow pools to this frame's counts, then record one dispatch per mesh.
        let mut counts: BTreeMap<&'static str, usize> = BTreeMap::new();
        let mut slots = Vec::with_capacity(requests.len());
        for request in requests {
            let n = counts.entry(request.id).or_default();
            slots.push((*n < MAX_INSTANCES).then_some(*n));
            *n += 1;
        }
        for (&id, &count) in &counts {
            let character = &self.characters[id];
            let pool = self.pool.entry(id).or_default();
            while pool.len() < count.min(MAX_INSTANCES) {
                let meshes = character
                    .primitives
                    .iter()
                    .map(|&p| {
                        SkinnedMesh::from_model_primitive(
                            gal,
                            &character.model.primitives[p],
                            character.binding.inverse_bind_matrices.len(),
                        )
                    })
                    .collect::<GraphicsResult<Vec<_>>>()?;
                pool.push(meshes);
            }
        }
        let mut encoder = gal.encoder("character skinning");
        for (request, slot) in requests.iter().zip(&slots) {
            let Some(slot) = slot else { continue };
            for mesh in &mut self.pool.get_mut(request.id).expect("grown above")[*slot] {
                self.pass
                    .record(gal, &mut encoder, mesh, &request.palette)?;
            }
        }
        let commands = encoder.finish();

        let mut draws = Vec::new();
        for (request, slot) in requests.iter().zip(slots) {
            let Some(slot) = slot else { continue };
            let character = &self.characters[request.id];
            let uploaded = models.get(request.id);
            for (mesh, &primitive) in self.pool[request.id][slot]
                .iter()
                .zip(&character.primitives)
            {
                let Some(mut draw) = uploaded.primitive_draw(primitive, request.root) else {
                    continue;
                };
                draw.mesh = mesh.mesh();
                let lit = &mut draw.material.lit;
                if lit.shading == ShadingModel::Unlit {
                    lit.base.tint = [GLOW, GLOW, GLOW, 1.0];
                    draw.material.receives_shadows = false;
                }
                if request.flash > 0.0 {
                    let f = request.flash;
                    lit.emissive = [
                        lit.emissive[0] + f,
                        lit.emissive[1] + f * 0.35,
                        lit.emissive[2] + f * 0.3,
                    ];
                }
                draws.push(draw);
            }
        }
        Ok((Some(commands), draws))
    }

    pub fn release(&mut self, gal: &Gpu) -> GraphicsResult<()> {
        for instances in self.pool.values_mut() {
            for meshes in instances {
                for mesh in meshes {
                    mesh.release(gal)?;
                }
            }
        }
        self.pool.clear();
        Ok(())
    }
}
