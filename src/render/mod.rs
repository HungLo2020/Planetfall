//! Rendering: one caller-owned VulkanicGAL device shared by every pass,
//! submitted together each frame:
//!
//! 1. GPU skinning for characters;
//! 2. one directional shadow map with mesh and voxel casters;
//! 3. opaque voxel terrain / ship hull (`quasar-render-voxel`), then opaque
//!    lit surfaces on the same depth, into a linear HDR [`SceneTarget`];
//! 4. translucent voxels (windows), then sorted transparent draws (glows,
//!    beams, holograms, previews);
//! 5. the post chain (bloom, ACES tonemap, grade, vignette, one sRGB encode)
//!    onto the output;
//! 6. the 2D overlay and `quasar-ui` layers on top.

pub mod camera;
mod geometry;
mod models;
pub mod scene;
mod skins;
mod voxels;

use crate::ui::Layer;
use camera::{Camera, CameraRig};
use planetfall_sim::math::V2;
use planetfall_sim::{Game, Phase};
use quasar_frame::{Gpu, GraphicsResult, RenderTarget, error};
use quasar_render_2d::{Canvas, DisplayOptions, LoadOp, RenderState2D, Textures};
use quasar_render_3d::{
    ColorSpace, Lighting, LitCamera3D, LitRenderState3D, Mat4, Mesh, PassOptions, PostChain,
    PreparedShadows, SceneTarget, ShadingModel, ShadowConfig, ShadowRenderState3D, ShadowSettings,
    ShadowTarget, ShadowView, SubmissionConfig, SurfaceDraw3D, SurfaceRenderState3D,
};
use quasar_render_voxel::PreparedVoxels;
use quasar_ui::{TextRenderer, UiRenderer};
use scene::{Painter, SUN_DIRECTION};
use std::path::Path;
use vulkanic_gal::CommandBuffer;

const MAX_DRAWS: usize = 4000;

/// Per-frame inputs from the application.
pub struct Frame<'a> {
    pub game: &'a Game,
    /// Seconds since start, for animation.
    pub time: f32,
    pub dt: f32,
    pub shake: f32,
    /// UI trees drawn in order over the 3D scene.
    pub layers: &'a [&'a Layer],
    /// Custom 2D drawing beneath the UI (bars, markers, fades).
    pub overlay: &'a dyn Fn(&mut Canvas, &Camera),
}

pub struct Graphics {
    world: SurfaceRenderState3D,
    /// Blended draws after the opaque pass.
    transparent: LitRenderState3D,
    post: PostChain,
    scene: Option<SceneTarget>,
    shadows: ShadowRenderState3D,
    shadow_target: Option<ShadowTarget>,
    two_d: RenderState2D,
    textures_2d: Textures,
    ui: UiRenderer,
    cube: Mesh,
    sphere: Mesh,
    tex: geometry::Textures3D,
    models: models::Models,
    skins: skins::Skins,
    voxels: voxels::VoxelWorld,
    rig: CameraRig,
    pub shadows_enabled: bool,
    pub bloom_enabled: bool,
}

impl Graphics {
    pub fn new(gal: &Gpu, asset_root: &Path, text: Option<TextRenderer>) -> GraphicsResult<Self> {
        let submission = SubmissionConfig {
            instancing: true,
            frustum_culling: true,
            max_instances: 4096,
        };
        let world = SurfaceRenderState3D::with_submission(gal, 24, 1, submission)?;
        let shadows = ShadowRenderState3D::with_submission(
            gal,
            ShadowConfig {
                resolution: 2048,
                max_maps: 1,
                max_depth_bytes: 64 * 1024 * 1024,
                ..ShadowConfig::default()
            },
            submission,
        )?;
        let mut ui = UiRenderer::new();
        if let Some(text) = text {
            ui.enable_text(text);
        }
        let models = models::Models::load(gal, asset_root)?;
        let skins = skins::Skins::new(gal, &models)?;
        Ok(Self {
            world,
            transparent: LitRenderState3D::new(gal, 24)?,
            post: PostChain::new(gal)?,
            scene: None,
            shadows,
            shadow_target: None,
            two_d: RenderState2D::new(gal).map_err(|e| error(e.to_string()))?,
            textures_2d: Textures::default(),
            ui,
            cube: geometry::cube(gal)?,
            sphere: geometry::sphere(gal)?,
            tex: geometry::Textures3D::new(gal)?,
            models,
            skins,
            voxels: voxels::VoxelWorld::new(),
            rig: CameraRig::default(),
            shadows_enabled: true,
            bloom_enabled: true,
        })
    }

    pub fn commands(
        &mut self,
        gal: &Gpu,
        target: &RenderTarget,
        frame: &Frame,
    ) -> GraphicsResult<Vec<CommandBuffer>> {
        let (w, h) = (target.width as f32, target.height as f32);
        let camera = self
            .rig
            .update(frame.game, frame.dt, frame.shake, frame.time, w, h);
        self.models.bind(gal, &mut self.world, &mut self.shadows)?;

        let mut painter = Painter::new(
            &self.cube,
            &self.sphere,
            &self.tex,
            &self.models,
            &self.skins,
            frame.time,
        );
        let game = frame.game;
        let mut out = match (game.phase, &game.run) {
            (Phase::Ftl { ticks, to }, _) => scene::ftl(&mut painter, ticks, to),
            (Phase::Ship, _) => scene::ship(&mut painter, game),
            (_, Some(run)) => {
                let center = V2::new(camera.target[0], camera.target[2]);
                scene::surface(&mut painter, game, run, center)
            }
            (_, None) => scene::ship(&mut painter, game),
        };
        drop(painter);
        if !self.bloom_enabled {
            out.post.bloom.enabled = false;
        }
        // Characters: pose on the GPU, then draw like any other surface.
        let (skinning, skinned) = self.skins.prepare(gal, &self.models, &out.skinned)?;
        out.draws.extend(skinned);
        if out.draws.len() > MAX_DRAWS {
            out.draws.truncate(MAX_DRAWS);
        }
        // Terrain or hull blocks: swap worlds and re-mesh edited chunks.
        if let Some(world) = &out.voxels {
            match (world.key, &game.run) {
                (voxels::WorldKey::Planet { .. }, Some(run)) => self.voxels.sync(
                    gal,
                    world.key,
                    world.palette,
                    run.terrain.blocks(),
                    run.terrain.chunk_revisions(),
                )?,
                _ => {
                    let ship = &game.ship;
                    self.voxels.sync(
                        gal,
                        world.key,
                        world.palette,
                        &ship.layout.blocks,
                        ship.layout
                            .blocks
                            .volume
                            .chunk_coords()
                            .into_iter()
                            .map(|c| (c, ship.revision)),
                    )?
                }
            }
        }

        // Directional shadows follow the view.
        let focus = out.shadow_focus;
        let shadow_frame: Option<(PreparedShadows, Option<PreparedVoxels>)> =
            if self.shadows_enabled && !matches!(game.phase, Phase::Ftl { .. }) {
                let casters: Vec<_> = out
                    .draws
                    .iter()
                    .filter(|d| {
                        d.material.lit.shading != ShadingModel::Unlit
                            && !d.material.lit.base.transparent
                    })
                    .map(SurfaceDraw3D::shadow_caster)
                    .collect();
                let d = SUN_DIRECTION;
                let eye = [
                    focus[0] - d[0] * 50.0,
                    focus[1] - d[1] * 50.0,
                    focus[2] - d[2] * 50.0,
                ];
                let view = Mat4::look_at(eye, focus, [0.0, 1.0, 0.0])
                    .ok_or_else(|| error("shadow view"))?;
                let projection = Mat4::orthographic(-30.0, 30.0, -30.0, 30.0, 1.0, 120.0)
                    .ok_or_else(|| error("shadow projection"))?;
                let views = [ShadowView {
                    light_index: 0,
                    view_projection: projection * view,
                    settings: ShadowSettings {
                        bias: 0.0008,
                        normal_bias: 0.04,
                        strength: 0.8,
                        ..ShadowSettings::default()
                    },
                    casters: &casters,
                }];
                if self.shadow_target.is_none() {
                    self.shadow_target = Some(self.shadows.create_target(gal)?);
                }
                let target = self.shadow_target.as_ref().expect("created above");
                let meshes = self.shadows.prepare_into(gal, &views, target)?;
                let blocks = match (&out.voxels, target.layer_view(0)) {
                    (Some(_), Some(layer)) => {
                        self.voxels
                            .prepare_depth(gal, layer, (projection * view).to_glam())?
                    }
                    _ => None,
                };
                Some((meshes, blocks))
            } else {
                None
            };

        // Opaque and transparent passes share one linear HDR target.
        let scene = match &mut self.scene {
            Some(scene) => {
                scene.resize(gal, target.width, target.height)?;
                scene
            }
            none => none.insert(SceneTarget::new(gal, target.width, target.height)?),
        };
        let lit_camera = LitCamera3D {
            view_projection: Mat4::from_glam(camera.view_projection),
            position: camera.eye.to_array(),
        };
        let lighting = Lighting {
            ambient: out.ambient,
            lights: &out.lights,
            output_color_space: ColorSpace::Linear,
        };
        let clear = out.clear.map(srgb_to_linear);
        let clear = Some([clear[0], clear[1], clear[2], 1.0]);
        let shadow_binding = shadow_frame.as_ref().map(|(s, _)| s.binding());
        let voxel_camera = voxels::camera(camera.eye, camera.target, camera.projection)?;
        let voxel_pass = |overlays: Vec<_>, clear| voxels::VoxelPass {
            camera: &voxel_camera,
            lighting,
            shadow: shadow_binding,
            fog: out.voxels.as_ref().and_then(|v| v.fog),
            clear,
            overlays,
        };
        // Opaque blocks clear the scene; meshes then share their depth.
        let blocks = match &out.voxels {
            Some(world) => self.voxels.prepare(
                gal,
                scene.target(),
                voxel_pass(world.overlays.clone(), clear),
                false,
            )?,
            None => None,
        };
        let world = self.world.prepare(
            gal,
            scene.target(),
            lit_camera,
            lighting,
            &out.draws,
            shadow_binding,
            if blocks.is_some() {
                PassOptions::load()
            } else {
                PassOptions {
                    clear_color: clear,
                    clear_depth: true,
                }
            },
        )?;
        let windows = match &out.voxels {
            Some(_) => {
                self.voxels
                    .prepare(gal, scene.target(), voxel_pass(Vec::new(), None), true)?
            }
            None => None,
        };
        let blended = self.transparent.prepare_transparent(
            gal,
            scene.target(),
            lit_camera,
            lighting,
            &out.transparent,
            PassOptions::load(),
        )?;
        let post = self.post.prepare_scene(gal, scene, target, &out.post)?;

        let mut canvas =
            Canvas::new(target.width, target.height).map_err(|e| error(e.to_string()))?;
        (frame.overlay)(&mut canvas, &camera);
        for layer in frame.layers {
            self.ui
                .prepare_text(&mut self.two_d, gal, &mut self.textures_2d, &layer.ui)?;
            self.ui.draw(&mut canvas, &layer.ui);
        }
        let hud = self.two_d.prepare(
            gal,
            &canvas,
            &self.textures_2d,
            target,
            DisplayOptions::default(),
            LoadOp::Load,
        )?;
        let mut commands: Vec<CommandBuffer> = skinning.into_iter().collect();
        if let Some((meshes, blocks)) = shadow_frame {
            commands.push(meshes.commands);
            commands.extend(blocks.map(|b| b.commands));
        }
        commands.extend(blocks.map(|b| b.commands));
        commands.push(world.commands);
        commands.extend(windows.map(|b| b.commands));
        commands.push(blended.commands);
        commands.push(post);
        commands.push(hud.commands);
        Ok(commands)
    }

    /// Rasterise every glyph the layers will draw. quasar-ui 0.2.24 warms
    /// glyphs from unwrapped layouts, so wrapped lines would otherwise appear
    /// one frame late; offscreen renders only get one frame.
    /// The game was replaced: re-mesh the voxel world from scratch and cut
    /// the camera instead of easing from the old game's framing.
    pub fn reset_world(&mut self) {
        self.voxels.invalidate();
        self.rig = CameraRig::default();
    }

    pub fn warm_text(&mut self, gal: &Gpu, layers: &[&Layer]) -> GraphicsResult<()> {
        let mut scratch = Canvas::new(16, 16).map_err(|e| error(e.to_string()))?;
        for layer in layers {
            self.ui
                .prepare_text(&mut self.two_d, gal, &mut self.textures_2d, &layer.ui)?;
            self.ui.draw(&mut scratch, &layer.ui);
        }
        Ok(())
    }

    /// Call after the frame's command buffers were submitted.
    pub fn submitted(&mut self, gal: &Gpu) {
        self.voxels.submitted(gal);
    }

    pub fn release_targets(&mut self, gal: &Gpu) -> GraphicsResult<()> {
        self.scene = None;
        self.world.release_targets(gal)?;
        self.two_d.release_targets(gal)
    }

    pub fn release(&mut self, gal: &Gpu) -> GraphicsResult<()> {
        self.world.release(gal)?;
        self.transparent.release(gal)?;
        self.post.release(gal)?;
        self.scene = None;
        self.shadows.release(gal)?;
        self.two_d.release(gal)?;
        self.skins.release(gal)?;
        self.voxels.release(gal)?;
        self.models.release(gal)?;
        self.cube.release(gal)?;
        self.sphere.release(gal)?;
        self.shadow_target = None;
        Ok(())
    }
}

/// Display-referred (sRGB) colour channel to linear.
pub(crate) fn srgb_to_linear(v: f32) -> f32 {
    if v <= 0.040_45 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

/// Render one frame offscreen and write it as a PNG (no window needed).
pub fn screenshot(
    game: &Game,
    asset_root: &Path,
    text: Option<TextRenderer>,
    size: (u32, u32),
    layers: &[&Layer],
    overlay: &dyn Fn(&mut Canvas, &Camera),
    path: &Path,
) -> Result<(), String> {
    let gal = Gpu::headless().map_err(|e| format!("headless GPU: {e}"))?;
    let mut graphics = Graphics::new(&gal, asset_root, text).map_err(|e| e.to_string())?;
    graphics
        .warm_text(&gal, layers)
        .map_err(|e| e.to_string())?;
    let image = quasar_frame::render_offscreen(&gal, size.0, size.1, |gpu, target| {
        graphics.commands(
            gpu,
            target,
            &Frame {
                game,
                time: game.tick as f32 / 60.0,
                dt: 1.0 / 60.0,
                shake: 0.0,
                layers,
                overlay,
            },
        )
    })
    .map_err(|e| e.to_string())?;
    image.write_png(path).map_err(|e| e.to_string())?;
    graphics.release(&gal).map_err(|e| e.to_string())?;
    Ok(())
}
