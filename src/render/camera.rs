//! Top-down follow camera, cinematic framings and world-to-screen projection.

use planetfall_sim::game::{ASCENT_TICKS, DESCENT_TICKS, FTL_TICKS, LOST_TICKS};
use planetfall_sim::math::V2;
use planetfall_sim::{Game, Phase};
use quasar_math::{Mat4, Vec3, Viewport};

#[derive(Clone, Copy, Debug)]
pub struct Camera {
    pub view_projection: Mat4,
    pub view: Mat4,
    pub projection: Mat4,
    pub eye: Vec3,
    pub target: Vec3,
    pub viewport: Viewport,
}

impl Camera {
    /// World point to pixel coordinates; None when behind the camera.
    pub fn project(&self, p: [f32; 3]) -> Option<[f32; 2]> {
        quasar_math::project(self.view_projection, Vec3::from(p), self.viewport).map(|s| [s.x, s.y])
    }
}

/// Where the camera wants to be for the current phase.
#[derive(Clone, Copy, Debug)]
pub struct Framing {
    pub target: [f32; 3],
    pub offset: [f32; 3],
    pub fov: f32,
    /// Snap instead of easing (cuts between scenes).
    pub cut: bool,
}

/// Height of the drop pod above the landing site during descent/ascent.
pub fn descent_height(ticks: u32) -> f32 {
    let t = ticks as f32 / DESCENT_TICKS as f32;
    70.0 * t * t
}

pub fn ascent_height(ticks: u32) -> f32 {
    let t = 1.0 - ticks as f32 / ASCENT_TICKS as f32;
    55.0 * t * t
}

pub fn framing(game: &Game) -> Framing {
    let top_down = [0.0, 19.0, 11.5];
    match game.phase {
        Phase::Ship => {
            let p = game.ship.player.pos;
            Framing {
                target: [p[0], 0.0, p[1]],
                offset: [0.0, 16.5, 10.0],
                fov: 42f32.to_radians(),
                cut: false,
            }
        }
        Phase::Ftl { ticks, .. } => {
            let t = 1.0 - ticks as f32 / FTL_TICKS as f32;
            Framing {
                target: [0.0, 0.0, 0.0],
                offset: [6.0 - t * 4.0, 4.5 + t * 1.5, -12.0 + t * 2.0],
                fov: (50.0 + 25.0 * (t * std::f32::consts::PI).sin()).to_radians(),
                cut: true,
            }
        }
        Phase::Descent { ticks } => {
            let landing = game.run.as_ref().map_or(V2::ZERO, |r| r.landing);
            let h = descent_height(ticks);
            Framing {
                target: [landing[0], h * 0.85, landing[1]],
                offset: [
                    0.0,
                    24.0 - (ticks as f32 / DESCENT_TICKS as f32) * 6.0,
                    13.0,
                ],
                fov: 46f32.to_radians(),
                cut: true,
            }
        }
        Phase::Ascent { ticks } => {
            let landing = game.run.as_ref().map_or(V2::ZERO, |r| r.landing);
            let h = ascent_height(ticks);
            Framing {
                target: [landing[0], h * 0.7, landing[1]],
                offset: top_down,
                fov: 44f32.to_radians(),
                cut: false,
            }
        }
        Phase::Lost { ticks } => {
            let p = game.run.as_ref().map_or(V2::ZERO, |r| r.player().pos);
            let t = 1.0 - ticks as f32 / LOST_TICKS as f32;
            Framing {
                target: [p[0], 0.0, p[1]],
                offset: [0.0, 19.0 - 8.0 * t, 11.5 - 4.0 * t],
                fov: 42f32.to_radians(),
                cut: false,
            }
        }
        Phase::Surface => {
            let p = game.run.as_ref().map_or(V2::ZERO, |r| r.player().pos);
            Framing {
                target: [p[0], 0.0, p[1]],
                offset: top_down,
                fov: 42f32.to_radians(),
                cut: false,
            }
        }
    }
}

/// Smoothed camera state owned by the renderer.
#[derive(Default)]
pub struct CameraRig {
    target: Vec3,
    offset: Vec3,
    fov: f32,
    ready: bool,
    last_phase: Option<std::mem::Discriminant<Phase>>,
}

impl CameraRig {
    pub fn update(
        &mut self,
        game: &Game,
        dt: f32,
        shake: f32,
        time: f32,
        width: f32,
        height: f32,
    ) -> Camera {
        let f = framing(game);
        let (target, offset) = (Vec3::from(f.target), Vec3::from(f.offset));
        let phase = std::mem::discriminant(&game.phase);
        let changed = self.last_phase != Some(phase);
        self.last_phase = Some(phase);
        if !self.ready || f.cut || changed {
            self.target = target;
            self.offset = offset;
            self.fov = f.fov;
            self.ready = true;
        } else {
            let k = quasar_math::damp_factor(8.0, dt);
            self.target = self.target.lerp(target, k);
            self.offset = self.offset.lerp(offset, k);
            self.fov += (f.fov - self.fov) * k;
        }
        // Trauma model: the offset grows faster than shake (power 1.5), so
        // small bumps stay subtle while hits and quakes clearly jolt the view.
        let trauma = shake.clamp(0.0, 1.5);
        let jitter = Vec3::new(
            (time * 23.0).sin() + 0.5 * (time * 37.0).sin(),
            0.5 * (time * 29.0).cos(),
            (time * 19.0).cos() + 0.5 * (time * 41.0).sin(),
        ) * (0.42 * trauma * trauma.sqrt());
        let target = self.target + jitter;
        let eye = target + self.offset;
        let viewport = Viewport::new(width, height.max(1.0));
        let view = quasar_math::look_at(eye, target, Vec3::Y).unwrap_or(Mat4::IDENTITY);
        let projection = quasar_math::perspective(self.fov, viewport.aspect().max(0.1), 0.3, 400.0)
            .unwrap_or(Mat4::IDENTITY);
        Camera {
            view_projection: projection * view,
            view,
            projection,
            eye,
            target,
            viewport,
        }
    }
}
