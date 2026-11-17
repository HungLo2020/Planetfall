//! Skinned characters: glTF skins and clips imported by `quasar-assets`,
//! sampled with `quasar-animation`, posed on the GPU by `quasar-render-3d`'s
//! `SkinningPass`.
//!
//! Each character model carries a skeleton (one joint per body part), a lit
//! body primitive, an unlit glow primitive and its clips (see
//! `DevUtils/assets/generate_models.py`). The painter samples a pose and asks
//! for a skinned draw; every instance of a character shares one
//! `SkinnedCrowd` per primitive, posed by a single dispatch, and its slots
//! draw through the ordinary surface and shadow passes (instanced).

use super::models::Models;
use quasar_animation::model::{SkinBinding, clips_from_model, skeleton_from_model};
use quasar_animation::{Clip, Pose, Skeleton};
use quasar_assets::model::Model3D;
use quasar_frame::{Gpu, GraphicsResult, error};
use quasar_render_3d::{
    Mat4, OutlineDraw, ShadingModel, SkinnedCrowd, SkinningPass, SurfaceDraw3D,
};
use std::collections::BTreeMap;
use vulkanic_gal::CommandBuffer;

/// Upper bound on simultaneously posed instances of one character (the
/// horde cap is 160 enemies across every kind).
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
    /// Outline style (see `effects::outline`) to keep the character
    /// readable behind walls.
    pub outline: Option<u8>,
    /// Multiplies the lit body colour.
    pub tint: [f32; 3],
    /// Colour of the unlit glow parts (visor, lamps).
    pub glow: [f32; 3],
}

/// One frame's posed characters.
pub struct Skinned<'a> {
    /// Compute work to submit before any pass that draws the characters.
    pub commands: Option<CommandBuffer>,
    pub draws: Vec<SurfaceDraw3D<'a>>,
    pub outlines: Vec<OutlineDraw<'a>>,
}

/// Rigs plus one crowd per skinned primitive of each character: every
/// instance of a character is posed by one dispatch per primitive and
/// consecutive draws of a crowd merge into one instanced draw.
pub struct Skins {
    characters: BTreeMap<String, Character>,
    /// Per character: one crowd per skinned primitive.
    crowds: BTreeMap<&'static str, Vec<SkinnedCrowd>>,
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
            crowds: BTreeMap::new(),
            pass: SkinningPass::new(gal)?,
        })
    }

    pub fn character(&self, name: &str) -> &Character {
        self.characters
            .get(name)
            .unwrap_or_else(|| panic!("{name:?} is not a skinned character"))
    }

    /// Pose every request on the GPU and return the compute work plus the
    /// characters' surface and outline draws.
    pub fn prepare<'a>(
        &'a mut self,
        gal: &Gpu,
        models: &'a Models,
        requests: &[SkinRequest],
    ) -> GraphicsResult<Skinned<'a>> {
        if requests.is_empty() {
            return Ok(Skinned {
                commands: None,
                draws: Vec::new(),
                outlines: Vec::new(),
            });
        }
        // Group requests by character; extra instances beyond a crowd's
        // capacity are skipped.
        let mut groups: BTreeMap<&'static str, Vec<&SkinRequest>> = BTreeMap::new();
        for request in requests {
            let group = groups.entry(request.id).or_default();
            if group.len() < MAX_INSTANCES {
                group.push(request);
            }
        }
        for &id in groups.keys() {
            if self.crowds.contains_key(id) {
                continue;
            }
            let character = &self.characters[id];
            let crowds = character
                .primitives
                .iter()
                .map(|&p| {
                    SkinnedCrowd::from_model_primitive(
                        gal,
                        &character.model.primitives[p],
                        character.binding.inverse_bind_matrices.len(),
                        MAX_INSTANCES,
                    )
                })
                .collect::<GraphicsResult<Vec<_>>>()?;
            self.crowds.insert(id, crowds);
        }
        let mut encoder = gal.encoder("character crowds");
        for (id, group) in &groups {
            let palettes: Vec<&[Mat4]> = group.iter().map(|r| r.palette.as_slice()).collect();
            for crowd in self.crowds.get_mut(id).expect("created above") {
                self.pass
                    .record_crowd(gal, &mut encoder, crowd, &palettes)?;
            }
        }
        let commands = encoder.finish();

        // Draw each crowd's slots consecutively so they instance together.
        let mut draws = Vec::new();
        let mut outlines = Vec::new();
        for (id, group) in &groups {
            let character = &self.characters[*id];
            let uploaded = models.get(id);
            for (crowd, &primitive) in self.crowds[id].iter().zip(&character.primitives) {
                for (slot, request) in group.iter().enumerate() {
                    let Some(mut draw) = uploaded.primitive_draw(primitive, request.root) else {
                        continue;
                    };
                    draw.mesh = crowd.instance(slot);
                    let lit = &mut draw.material.lit;
                    let glow = lit.shading == ShadingModel::Unlit;
                    if glow {
                        let g = request.glow;
                        lit.base.tint = [GLOW * g[0], GLOW * g[1], GLOW * g[2], 1.0];
                        draw.material.receives_shadows = false;
                    } else {
                        for (c, k) in request.tint.iter().enumerate() {
                            lit.base.tint[c] *= k;
                        }
                    }
                    if request.flash > 0.0 {
                        let f = request.flash;
                        lit.emissive = [
                            lit.emissive[0] + f,
                            lit.emissive[1] + f * 0.35,
                            lit.emissive[2] + f * 0.3,
                        ];
                    }
                    if let Some(style) = request.outline
                        && !glow
                    {
                        outlines.push(OutlineDraw {
                            mesh: draw.mesh,
                            model: draw.model,
                            style,
                        });
                    }
                    draws.push(draw);
                }
            }
        }
        Ok(Skinned {
            commands: Some(commands),
            draws,
            outlines,
        })
    }

    pub fn release(&mut self, gal: &Gpu) -> GraphicsResult<()> {
        for crowds in self.crowds.values_mut() {
            for crowd in crowds {
                crowd.release(gal)?;
            }
        }
        self.crowds.clear();
        Ok(())
    }
}
