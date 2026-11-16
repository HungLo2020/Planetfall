//! The FTL jump cinematic.

use super::*;

// ---------------------------------------------------------------------------
// FTL travel
// ---------------------------------------------------------------------------

pub fn ftl<'a>(p: &mut Painter<'a, '_>, ticks: u32, to: PlanetId) -> SceneOutput<'a> {
    let progress = 1.0 - ticks as f32 / FTL_TICKS as f32;
    let warp = if progress < 0.25 {
        progress / 0.25 * 0.3
    } else if progress < 0.85 {
        0.3 + (progress - 0.25) / 0.6 * 0.7
    } else {
        1.0 - (progress - 0.85) / 0.15
    };
    p.lights.push(sun([0.2, -0.6, 1.0], [0.8, 0.9, 1.0], 0.9));
    let shake = (p.time * 40.0).sin() * 0.04 * warp;
    p.rig(names::SHIP_EXTERIOR, t(shake, 0.0, 0.0), 0.0, |_| {
        Mat4::IDENTITY
    });
    p.point([0.0, 0.0, -4.5], [0.4, 0.9, 1.0], 2.0 + warp * 4.0, 8.0);
    let speed = 20.0 + warp * 600.0;
    for i in 0..340 {
        let h = |k: i32| tile_hash(i * 31 + k, i * 17 + k * 3);
        let angle = h(1) * std::f32::consts::TAU;
        let radius = 6.0 + h(2) * 60.0;
        let z_span = 300.0;
        let z = (h(3) * z_span - p.time * speed).rem_euclid(z_span) - 60.0;
        let len = 0.15 + warp * 14.0 * (0.5 + h(4));
        let color = mix3([0.6, 0.75, 1.0], [1.0, 1.0, 1.0], h(5));
        p.glow(
            t(angle.cos() * radius, angle.sin() * radius * 0.6, z) * s(0.06, 0.06, len),
            math_scale3(color, 1.2 + warp),
        );
    }
    if progress > 0.8 {
        let a = ((progress - 0.8) / 0.2).clamp(0.0, 1.0);
        let tint = to.info().palette.planet;
        p.model_tinted(
            names::PLANET,
            t(20.0, -18.0, 140.0 - a * 60.0) * s(40.0, 40.0, 40.0),
            tint,
            math_scale3(tint, 0.1),
        );
    }
    SceneOutput {
        draws: std::mem::take(&mut p.draws),
        lights: std::mem::take(&mut p.lights),
        ambient: [0.1, 0.1, 0.14],
        clear: [0.005, 0.008, 0.02 + warp * 0.03, 1.0],
        shadow_focus: [0.0, 0.0, 0.0],
        transparent: std::mem::take(&mut p.transparent),
        skinned: std::mem::take(&mut p.skinned),
        voxels: None,
        post: PostSettings {
            bloom: BloomSettings {
                intensity: 0.5 + warp * 0.7,
                ..base_post().bloom
            },
            ..base_post()
        },
    }
}
