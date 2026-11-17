//! Projectiles, beams and blasts (particles live in `render::effects`).

use super::*;

pub(super) fn projectiles(p: &mut Painter<'_, '_>, run: &Run) {
    let mut lights = 0;
    for pr in &run.projectiles {
        let color = pr.weapon.info().color;
        match pr.kind {
            ProjectileKind::Bolt => {
                let d = pr.direction();
                let yaw = math::yaw_of(d);
                p.glow(
                    at(pr.pos, pr.height) * Mat4::rotation_y(yaw) * s(0.07, 0.07, 0.42),
                    color,
                );
                if lights < 16 {
                    p.point([pr.pos[0], pr.height, pr.pos[1]], color, 0.8, 4.0);
                    lights += 1;
                }
            }
            ProjectileKind::Pellet => {
                let yaw = math::yaw_of(pr.direction());
                p.glow(
                    at(pr.pos, pr.height) * Mat4::rotation_y(yaw) * s(0.06, 0.06, 0.18),
                    color,
                );
            }
            ProjectileKind::Mortar { .. } => {
                let k = 0.28 + (p.time * 20.0).sin() * 0.04;
                p.glow(at(pr.pos, pr.height) * s(k, k, k), color);
                p.point([pr.pos[0], pr.height, pr.pos[1]], color, 1.5, 6.0);
            }
        }
    }
    for b in &run.beams {
        let color = b.weapon.info().color;
        let fade = b.ttl as f32 / 9.0;
        for w in b.points.windows(2) {
            let (a, c) = (w[0], w[1]);
            // Jagged lightning: three jittered sub-segments.
            let mut prev = [a[0], 1.1, a[1]];
            for k in 1..=3 {
                let f = k as f32 / 3.0;
                let j = if k < 3 {
                    (tile_hash(b.ttl as i32 * 7 + k, (a[0] * 10.0) as i32) - 0.5) * 0.9
                } else {
                    0.0
                };
                let next = [
                    a[0] + (c[0] - a[0]) * f + j,
                    1.0 + j * 0.4,
                    a[1] + (c[1] - a[1]) * f - j,
                ];
                p.segment(
                    prev,
                    next,
                    0.05 * fade + 0.02,
                    math_scale3(color, 1.5 * fade + 0.5),
                );
                prev = next;
            }
        }
        if let Some(last) = b.points.last() {
            p.point([last[0], 1.2, last[1]], color, 2.5 * fade, 6.0);
        }
    }
}

pub(super) fn effects(p: &mut Painter<'_, '_>, run: &Run) {
    for b in &run.blasts {
        let t01 = 1.0 - b.ttl as f32 / b.total as f32;
        let color = match b.kind {
            BlastKind::Weapon(w) => w.info().color,
            // An enemy's burst is a brighter shade of its blood.
            BlastKind::Enemy(e) => math_scale3(e.info().blood, 1.4),
            BlastKind::PodLanding => [1.0, 0.75, 0.4],
        };
        let r = b.radius * (0.3 + 0.7 * t01.sqrt());
        let fade = 1.0 - t01;
        // Expanding shard ring plus a collapsing core.
        let n = 18;
        for i in 0..n {
            let a = i as f32 / n as f32 * std::f32::consts::TAU + b.pos[0];
            let q = b.pos + V2::new(a.sin(), a.cos()) * r;
            let k = 0.18 * fade + 0.04;
            p.glow(
                at(q, 0.4 + t01 * 0.8) * Mat4::rotation_y(a) * s(k, k, k * 2.5),
                math_scale3(color, 0.6 + fade),
            );
        }
        let core = b.radius * 0.45 * fade;
        if core > 0.02 {
            p.halo(
                at(b.pos, 0.6) * s(core, core * 0.8, core),
                color,
                0.9 * fade,
            );
            p.halo(at(b.pos, 0.4) * s(r, r * 0.35, r), color, 0.25 * fade);
        }
        p.point([b.pos[0], 1.5, b.pos[1]], color, 6.0 * fade, b.radius * 3.0);
    }
}
