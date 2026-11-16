//! Rendering: one caller-owned VulkanicGAL device shared by a lit 3D world
//! pass with directional shadows (`quasar-render-3d`) and a 2D HUD/UI pass
//! (`quasar-render-2d` + `quasar-ui`), submitted together each frame.

pub mod camera;
mod geometry;
mod models;
pub mod scene;

use crate::ui::Layer;
use camera::{Camera, CameraRig};
use planetfall_sim::{Game, Phase};
use quasar_frame::{Gpu, GraphicsResult, RenderTarget, error};
use quasar_render_2d::{Canvas, DisplayOptions, LoadOp, RenderState2D, Textures};
use quasar_render_2d::{Color, Rect};
use quasar_render_3d::{
    ColorSpace, Lighting, LitCamera3D, Mat4, Mesh, PassOptions, PreparedShadows, ShadingModel,
    ShadowConfig, ShadowRenderState3D, ShadowSettings, ShadowTarget, ShadowView, SubmissionConfig,
    SurfaceDraw3D, SurfaceRenderState3D,
};
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
    shadows: ShadowRenderState3D,
    shadow_target: Option<ShadowTarget>,
    two_d: RenderState2D,
    textures_2d: Textures,
    ui: UiRenderer,
    cube: Mesh,
    tex: geometry::Textures3D,
    models: models::Models,
    rig: CameraRig,
    pub shadows_enabled: bool,
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
        Ok(Self {
            world,
            shadows,
            shadow_target: None,
            two_d: RenderState2D::new(gal).map_err(|e| error(e.to_string()))?,
            textures_2d: Textures::default(),
            ui,
            cube: geometry::cube(gal)?,
            tex: geometry::Textures3D::new(gal)?,
            models: models::Models::load(gal, asset_root)?,
            rig: CameraRig::default(),
            shadows_enabled: true,
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

        let mut painter = Painter::new(&self.cube, &self.tex, &self.models, frame.time);
        let game = frame.game;
        let mut out = match (game.phase, &game.run) {
            (Phase::Ftl { ticks, to }, _) => scene::ftl(&mut painter, ticks, to),
            (Phase::Ship, _) => scene::ship(&mut painter, game),
            (_, Some(run)) => {
                let center = [camera.target[0], camera.target[2]];
                scene::surface(&mut painter, game, run, center)
            }
            (_, None) => scene::ship(&mut painter, game),
        };
        if out.draws.len() > MAX_DRAWS {
            out.draws.truncate(MAX_DRAWS);
        }

        // Directional shadows follow the view.
        let focus = out.shadow_focus;
        let shadow_frame: Option<PreparedShadows> =
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
                Some(self.shadows.prepare_into(gal, &views, target)?)
            } else {
                None
            };

        let world = self.world.prepare(
            gal,
            target,
            LitCamera3D {
                view_projection: camera.view_projection(),
                position: camera.eye,
            },
            Lighting {
                ambient: out.ambient,
                lights: &out.lights,
                output_color_space: ColorSpace::Srgb,
            },
            &out.draws,
            shadow_frame.as_ref().map(PreparedShadows::binding),
            PassOptions {
                clear_color: Some(out.clear),
                clear_depth: true,
            },
        )?;

        let mut canvas =
            Canvas::new(target.width, target.height).map_err(|e| error(e.to_string()))?;
        (frame.overlay)(&mut canvas, &camera);
        for layer in frame.layers {
            paint_boxes(&mut canvas, layer);
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
        let mut commands: Vec<CommandBuffer> =
            shadow_frame.into_iter().map(|s| s.commands).collect();
        commands.push(world.commands);
        commands.push(hud.commands);
        Ok(commands)
    }

    pub fn release_targets(&mut self, gal: &Gpu) -> GraphicsResult<()> {
        self.world.release_targets(gal)?;
        self.two_d.release_targets(gal)
    }

    pub fn release(&mut self, gal: &Gpu) -> GraphicsResult<()> {
        self.world.release(gal)?;
        self.shadows.release(gal)?;
        self.two_d.release(gal)?;
        self.models.release(gal)?;
        self.cube.release(gal)?;
        self.shadow_target = None;
        Ok(())
    }
}

fn paint_boxes(canvas: &mut Canvas, layer: &Layer) {
    canvas.set_clip(None);
    for b in &layer.boxes {
        let Some(entry) = layer.ui.layout_entries().iter().find(|e| e.id == b.node) else {
            continue;
        };
        let r = entry.rect;
        canvas.rect(Rect::new(r.x, r.y, r.width, r.height), Color(b.fill));
        if let Some(border) = b.border {
            canvas.rect(Rect::new(r.x, r.y, 3.0, r.height), Color(border));
            canvas.outline_rect(
                Rect::new(r.x, r.y, r.width, r.height),
                1.0,
                Color([border[0], border[1], border[2], border[3] / 3]),
            );
        }
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
    let target =
        quasar_frame::OffscreenTarget::new(&gal, size.0, size.1).map_err(|e| e.to_string())?;
    let image = target
        .render(&gal, |gpu, target| {
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
    quasar_render_2d::write_png(path, size.0, size.1, &image.rgba).map_err(|e| e.to_string())?;
    graphics.release(&gal).map_err(|e| e.to_string())?;
    Ok(())
}
