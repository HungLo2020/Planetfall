//! Top-down follow camera, cinematic framings and world-to-screen projection.

use planetfall_sim::game::{ASCENT_TICKS, DESCENT_TICKS, FTL_TICKS, LOST_TICKS};
use planetfall_sim::{Game, Phase};
use quasar_render_3d::Mat4;

#[derive(Clone, Copy, Debug)]
pub struct Camera {
    pub view: Mat4,
    pub projection: Mat4,
    pub eye: [f32; 3],
    pub target: [f32; 3],
    pub width: f32,
    pub height: f32,
}

impl Camera {
    pub fn view_projection(&self) -> Mat4 {
        self.projection * self.view
    }

    /// World point to pixel coordinates; None when behind the camera.
    pub fn project(&self, p: [f32; 3]) -> Option<[f32; 2]> {
        let clip = self.view_projection().transform([p[0], p[1], p[2], 1.0]);
        if clip[3] <= 1e-4 {
            return None;
        }
        let ndc = [clip[0] / clip[3], clip[1] / clip[3]];
        Some([
            (ndc[0] * 0.5 + 0.5) * self.width,
            (0.5 - ndc[1] * 0.5) * self.height,
        ])
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
            let landing = game.run.as_ref().map_or([0.0, 0.0], |r| r.landing);
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
            let landing = game.run.as_ref().map_or([0.0, 0.0], |r| r.landing);
            let h = ascent_height(ticks);
            Framing {
                target: [landing[0], h * 0.7, landing[1]],
                offset: top_down,
                fov: 44f32.to_radians(),
                cut: false,
            }
        }
        Phase::Lost { ticks } => {
            let p = game.run.as_ref().map_or([0.0, 0.0], |r| r.player.pos);
            let t = 1.0 - ticks as f32 / LOST_TICKS as f32;
            Framing {
                target: [p[0], 0.0, p[1]],
                offset: [0.0, 19.0 - 8.0 * t, 11.5 - 4.0 * t],
                fov: 42f32.to_radians(),
                cut: false,
            }
        }
        Phase::Surface => {
            let p = game.run.as_ref().map_or([0.0, 0.0], |r| r.player.pos);
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
    target: [f32; 3],
    offset: [f32; 3],
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
        let phase = std::mem::discriminant(&game.phase);
        let changed = self.last_phase != Some(phase);
        self.last_phase = Some(phase);
        if !self.ready || f.cut || changed {
            self.target = f.target;
            self.offset = f.offset;
            self.fov = f.fov;
            self.ready = true;
        } else {
            let k = 1.0 - (-dt * 8.0).exp();
            for i in 0..3 {
                self.target[i] += (f.target[i] - self.target[i]) * k;
                self.offset[i] += (f.offset[i] - self.offset[i]) * k;
            }
            self.fov += (f.fov - self.fov) * k;
        }
        let jitter = [
            (time * 61.0).sin() * shake * 0.35,
            (time * 47.0).cos() * shake * 0.2,
            (time * 53.0).sin() * shake * 0.35,
        ];
        let target = [
            self.target[0] + jitter[0],
            self.target[1] + jitter[1],
            self.target[2] + jitter[2],
        ];
        let eye = [
            target[0] + self.offset[0],
            target[1] + self.offset[1],
            target[2] + self.offset[2],
        ];
        let aspect = (width / height.max(1.0)).max(0.1);
        Camera {
            view: Mat4::look_at(eye, target, [0.0, 1.0, 0.0]).unwrap_or(Mat4::IDENTITY),
            projection: Mat4::perspective(self.fov, aspect, 0.3, 400.0).unwrap_or(Mat4::IDENTITY),
            eye,
            target,
            width,
            height,
        }
    }
}
