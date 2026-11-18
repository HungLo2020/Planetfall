//! Turns game state into draws, lights and post settings. Pure presentation:
//! reads the simulation, never changes it.
//!
//! [`Painter`] holds the drawing primitives (blocks, glows, transparent
//! volumes, models, rigs, skinned characters, lights); each scene lives in
//! its own module: `ship`, `space` (FTL), `surface` (planet, pods, loot),
//! `characters` (player and horde) and `effects` (projectiles, beams,
//! blasts, sparks).

mod characters;
mod effects;
mod ship;
mod space;
mod surface;

pub use ship::ship;
pub use space::ftl;
pub use surface::surface;

use super::geometry::{Textures3D, tile_hash};
use super::models::{Models, names};
use super::skins::{Layer, Posed, SkinRequest, Skins};
use super::voxels::{self, VoxelScene, WorldKey};
use crate::render::camera::{ascent_height, descent_height};
use crate::render::effects::outline;
use planetfall_sim::content::ShipBlock;
use planetfall_sim::content::{
    Axis, ClassId, DecorKind, EnemyBehavior, Material, Palette, PlanetId, VentKind, WeaponBehavior,
    WeaponId,
};
use planetfall_sim::game::{FTL_TICKS, LOST_TICKS};
use planetfall_sim::math::{self, V2};
use planetfall_sim::run::{
    Affix, BlastKind, DeployableKind, Extraction, HazardKind, Player, ProjectileKind, Run,
    extraction_hold, extraction_inbound, extraction_radius,
};
use planetfall_sim::ship::TILE as SHIP_TILE;
use planetfall_sim::ship::{BUILD_AREA, Room, ShipLayout, StationKind};
use planetfall_sim::terrain::{TILE, Terrain, Tile};
use planetfall_sim::voxel;
use planetfall_sim::{Game, Phase};
use quasar_render_3d::{
    Attenuation, BlendMode, BloomSettings, ColorGrade, Light, LitMaterial, Mat4, Mesh, NormalMap,
    PostSettings, ShadingModel, SurfaceDraw3D, SurfaceMaterial, SurfaceTransparentDraw3D,
    VignetteSettings,
};
use quasar_render_voxel::{Fog, OverlayBox};

/// Everything the 3D passes need for one frame.
pub struct SceneOutput<'a> {
    pub draws: Vec<SurfaceDraw3D<'a>>,
    /// Blended draws, sorted back to front by the renderer.
    pub transparent: Vec<SurfaceTransparentDraw3D<'a>>,
    pub lights: Vec<Light>,
    pub ambient: [f32; 3],
    pub clear: [f32; 4],
    /// Center of the directional shadow volume.
    pub shadow_focus: [f32; 3],
    pub post: PostSettings,
    /// Characters to pose on the GPU (drawn after skinning).
    pub skinned: Vec<SkinRequest>,
    /// Terrain or hull blocks to draw, if the scene has any.
    pub voxels: Option<VoxelScene>,
}

#[derive(Clone, Copy)]
enum Surface {
    Plain,
    Metal,
}

/// The player's look, smoothed across frames by the renderer: facing eases
/// toward where you move or shoot instead of snapping between targets, walk
/// and idle cross-fade, and the body leans into a run or dash.
#[derive(Clone, Copy, Debug, Default)]
pub struct PlayerPose {
    pub yaw: f32,
    /// 0 standing to 1 walking.
    pub motion: f32,
    /// Forward lean, 0 to 1.
    pub lean: f32,
    ready: bool,
}

impl PlayerPose {
    pub fn update(&mut self, game: &Game, dt: f32) {
        let (yaw, moving, dashing) = match (&game.run, game.phase) {
            (Some(run), Phase::Surface | Phase::Lost { .. } | Phase::Ascent { .. }) => {
                let pl = run.player();
                // Face the most recent shot while standing still.
                let yaw = if pl.moving {
                    pl.yaw
                } else {
                    pl.weapons
                        .iter()
                        .filter(|w| w.since_fired < 30)
                        .map(|w| w.aim)
                        .next()
                        .unwrap_or(pl.yaw)
                };
                (yaw, pl.moving, pl.dash_ticks > 0)
            }
            _ => (game.ship.player.yaw, game.ship.player.moving, false),
        };
        if !self.ready {
            *self = PlayerPose {
                yaw,
                ready: true,
                ..PlayerPose::default()
            };
        }
        let k = |rate: f32| 1.0 - (-rate * dt).exp();
        // About a third of a second for a full about-face.
        self.yaw = math::turn_toward(self.yaw, yaw, 10.0 * dt);
        self.motion += (f32::from(u8::from(moving)) - self.motion) * k(12.0);
        let lean = if dashing {
            1.0
        } else if moving {
            0.45
        } else {
            0.0
        };
        self.lean += (lean - self.lean) * k(10.0);
    }
}

pub struct Painter<'a, 's> {
    cube: &'a Mesh,
    sphere: &'a Mesh,
    tex: &'a Textures3D,
    models: &'a Models,
    skins: &'s Skins,
    pub draws: Vec<SurfaceDraw3D<'a>>,
    pub transparent: Vec<SurfaceTransparentDraw3D<'a>>,
    /// Characters to pose on the GPU this frame.
    pub skinned: Vec<SkinRequest>,
    pub lights: Vec<Light>,
    pub time: f32,
    pub pose: PlayerPose,
}

pub fn rot_z(a: f32) -> Mat4 {
    let (s, c) = a.sin_cos();
    Mat4([
        [c, s, 0., 0.],
        [-s, c, 0., 0.],
        [0., 0., 1., 0.],
        [0., 0., 0., 1.],
    ])
}

fn t(x: f32, y: f32, z: f32) -> Mat4 {
    Mat4::translation(x, y, z)
}

fn s(x: f32, y: f32, z: f32) -> Mat4 {
    Mat4::scale(x, y, z)
}

fn at(p: V2, y: f32) -> Mat4 {
    Mat4::translation(p[0], y, p[1])
}

impl<'a, 's> Painter<'a, 's> {
    pub fn new(
        cube: &'a Mesh,
        sphere: &'a Mesh,
        tex: &'a Textures3D,
        models: &'a Models,
        skins: &'s Skins,
        time: f32,
    ) -> Self {
        Self {
            cube,
            sphere,
            tex,
            models,
            skins,
            skinned: Vec::with_capacity(128),
            draws: Vec::with_capacity(2048),
            transparent: Vec::with_capacity(512),
            lights: Vec::with_capacity(super::MAX_LIGHTS),
            time,
            pose: PlayerPose::default(),
        }
    }

    fn material(&self, color: [f32; 3], surface: Surface, uv: f32) -> SurfaceMaterial {
        let mut lit = LitMaterial::textured(&self.tex.white);
        lit.base.tint = [color[0], color[1], color[2], 1.0];
        lit.specular = match surface {
            Surface::Metal => [0.35; 3],
            _ => [0.06; 3],
        };
        lit.shininess = match surface {
            Surface::Metal => 64.0,
            _ => 12.0,
        };
        let mut m = SurfaceMaterial::from(lit);
        m.normal_map = match surface {
            Surface::Plain => None,
            Surface::Metal => Some(NormalMap {
                uv_scale: [uv; 2],
                strength: 1.0,
                ..NormalMap::new(&self.tex.metal_normal)
            }),
        };
        m
    }

    fn block(&mut self, model: Mat4, color: [f32; 3], surface: Surface, uv: f32) {
        let material = self.material(color, surface, uv);
        self.draws.push(SurfaceDraw3D {
            mesh: self.cube,
            model,
            material,
        });
    }

    fn cube(&mut self, model: Mat4, color: [f32; 3]) {
        self.block(model, color, Surface::Plain, 1.0);
    }

    /// Unlit emissive cube (projectiles, sparks, lights). Emission above 1.0
    /// blooms in the post chain.
    fn glow(&mut self, model: Mat4, color: [f32; 3]) {
        let mut lit = LitMaterial::textured(&self.tex.white);
        lit.base.tint = [0.0, 0.0, 0.0, 1.0];
        lit.emissive = math_scale3(color, GLOW);
        lit.shading = ShadingModel::Unlit;
        let mut m = SurfaceMaterial::from(lit);
        m.receives_shadows = false;
        self.draws.push(SurfaceDraw3D {
            mesh: self.cube,
            model,
            material: m,
        });
    }

    /// Additive light volume: flames, beams, holograms, light shafts.
    fn ghost(&mut self, model: Mat4, color: [f32; 3], alpha: f32) {
        self.blend(self.cube, model, color, alpha, BlendMode::Additive);
    }

    /// Additive sphere: blast cores and glow halos.
    fn halo(&mut self, model: Mat4, color: [f32; 3], alpha: f32) {
        self.blend(self.sphere, model, color, alpha, BlendMode::Additive);
    }

    /// See-through tinted surface (glass), alpha-blended over the scene.
    fn glass(&mut self, model: Mat4, color: [f32; 3], alpha: f32) {
        let mut lit = LitMaterial::textured(&self.tex.white);
        lit.base.tint = [color[0], color[1], color[2], alpha];
        lit.specular = [0.6; 3];
        lit.shininess = 96.0;
        self.transparent.push(SurfaceTransparentDraw3D::new(
            self.cube,
            model,
            lit,
            BlendMode::Alpha,
        ));
    }

    fn blend(&mut self, mesh: &'a Mesh, model: Mat4, color: [f32; 3], alpha: f32, mode: BlendMode) {
        let mut lit = LitMaterial::textured(&self.tex.white);
        lit.base.tint = [0.0, 0.0, 0.0, alpha.clamp(0.0, 1.0)];
        lit.emissive = math_scale3(color, GLOW);
        lit.shading = ShadingModel::Unlit;
        self.transparent
            .push(SurfaceTransparentDraw3D::new(mesh, model, lit, mode));
    }

    fn model(&mut self, id: &str, root: Mat4) {
        let draws = self.models.get(id).draws(root);
        self.draws.extend(draws);
    }

    fn model_tinted(&mut self, id: &str, root: Mat4, tint: [f32; 3], emissive: [f32; 3]) {
        for mut d in self.models.get(id).draws(root) {
            for (c, k) in tint.iter().enumerate() {
                d.material.lit.base.tint[c] *= k;
            }
            for (c, e) in emissive.iter().enumerate() {
                d.material.lit.emissive[c] += e;
            }
            self.draws.push(d);
        }
    }

    /// Pose a skinned character from clip layers and queue it for GPU
    /// skinning, optionally outlined where walls hide it. Returns the pose
    /// for attachments.
    fn character(
        &mut self,
        id: &'static str,
        root: Mat4,
        layers: &[Layer],
        flash: f32,
        outline: Option<u8>,
    ) -> Posed {
        self.character_tinted(id, root, layers, flash, outline, [1.0; 3], [1.0; 3])
    }

    /// [`Self::character`] with the body multiplied by `tint` and the glow
    /// parts lit in `glow`.
    #[allow(
        clippy::too_many_arguments,
        reason = "a skinned draw is fully described by these values"
    )]
    fn character_tinted(
        &mut self,
        id: &'static str,
        root: Mat4,
        layers: &[Layer],
        flash: f32,
        outline: Option<u8>,
        tint: [f32; 3],
        glow: [f32; 3],
    ) -> Posed {
        let posed = self.skins.character(id).pose(layers);
        self.skinned.push(SkinRequest {
            id,
            palette: posed.palette.clone(),
            root,
            flash,
            outline,
            tint,
            glow,
        });
        posed
    }

    /// Draw a prop with named parts: `pose(node)` rotates each part about its pivot.
    fn rig(&mut self, id: &str, root: Mat4, flash: f32, pose: impl Fn(&str) -> Mat4) {
        let model = self.models.get(id);
        for (i, inst) in model.instances().iter().enumerate() {
            let local = Mat4(inst.transform) * pose(&inst.name);
            if let Some(mut d) = model.primitive_draw(inst.primitive, root * local) {
                if flash > 0.0 {
                    d.material.lit.emissive = [
                        d.material.lit.emissive[0] + flash,
                        d.material.lit.emissive[1] + flash * 0.35,
                        d.material.lit.emissive[2] + flash * 0.3,
                    ];
                }
                self.draws.push(d);
            }
            let _ = i;
        }
    }

    fn point(&mut self, p: [f32; 3], color: [f32; 3], intensity: f32, range: f32) {
        if self.lights.len() >= super::MAX_LIGHTS {
            return;
        }
        // A bad light fails the whole frame in the renderer; a glitched
        // effect should only lose its light.
        if !p.iter().chain(&color).all(|v| v.is_finite())
            || !intensity.is_finite()
            || !range.is_finite()
            || range <= 0.0
        {
            return;
        }
        let color = color.map(|c| c.max(0.0));
        let intensity = intensity.max(0.0);
        self.lights.push(Light::Point {
            position: p,
            color,
            intensity,
            range,
            attenuation: Attenuation::Smooth,
        });
    }

    /// Beam segment between two points at a height.
    fn segment(&mut self, a: [f32; 3], b: [f32; 3], width: f32, color: [f32; 3]) {
        let d = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt().max(1e-3);
        let yaw = d[0].atan2(d[2]);
        let pitch = -(d[1] / len).asin();
        let mid = [
            (a[0] + b[0]) * 0.5,
            (a[1] + b[1]) * 0.5,
            (a[2] + b[2]) * 0.5,
        ];
        let model = t(mid[0], mid[1], mid[2]) * Mat4::rotation_y(yaw) * Mat4::rotation_x(pitch);
        self.glow(model * s(width * 0.45, width * 0.45, len * 0.5), color);
        self.ghost(model * s(width * 2.2, width * 2.2, len * 0.5), color, 0.35);
    }
}

/// HDR boost for emissive effects so the post chain blooms them.
const GLOW: f32 = 2.2;

fn math_scale3(c: [f32; 3], k: f32) -> [f32; 3] {
    [c[0] * k, c[1] * k, c[2] * k]
}

fn mix3(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}

fn sun(direction: [f32; 3], color: [f32; 3], intensity: f32) -> Light {
    Light::Directional {
        direction,
        color,
        intensity,
    }
}

pub const SUN_DIRECTION: [f32; 3] = [0.35, -1.0, -0.45];

/// Bloom on emissive effects, filmic tonemapping and a soft vignette.
fn base_post() -> PostSettings {
    PostSettings {
        exposure: 0.85,
        bloom: BloomSettings {
            threshold: 1.25,
            knee: 0.4,
            intensity: 0.4,
            levels: 5,
            ..BloomSettings::default()
        },
        vignette: VignetteSettings {
            strength: 0.28,
            ..VignetteSettings::default()
        },
        grade: ColorGrade {
            saturation: 1.08,
            contrast: 1.06,
            ..ColorGrade::default()
        },
        ..PostSettings::default()
    }
}

/// Phase-dependent grading: danger reddens and darkens the edges; losing the
/// signal drains the colour.
fn post_settings(game: &Game, time: f32) -> PostSettings {
    let mut post = base_post();
    let Some(run) = &game.run else {
        return post;
    };
    match game.phase {
        Phase::Surface => {
            let p = &run.player();
            let danger = (1.0 - p.health / p.max_health).max(0.0);
            let hurt = p.hurt_ticks as f32 / 18.0;
            let pulse = 0.8 + 0.2 * (time * 6.0).sin();
            let edge = ((danger * danger * 0.5 + hurt * 0.4) * pulse).min(0.75);
            post.vignette.strength = (post.vignette.strength + edge).min(1.0);
            post.grade.gain = [1.0 + edge * 0.6, 1.0 - edge * 0.45, 1.0 - edge * 0.45];
        }
        Phase::Lost { ticks } => {
            let t = 1.0 - ticks as f32 / LOST_TICKS as f32;
            post.grade.saturation = 1.0 - t * 0.8;
            post.vignette.strength = 0.5 + t * 0.4;
        }
        _ => {}
    }
    post
}
