//! Screen-space 2D effects drawn on the `quasar-render-2d` canvas beneath the UI:
//! mining progress, off-screen markers, boss bars and fades. (The damage
//! vignette is part of the post chain's grade.)

use crate::render::camera::Camera;
use planetfall_sim::content::EnemyKind;
use planetfall_sim::game::{ASCENT_TICKS, DESCENT_TICKS, FTL_TICKS, LOST_TICKS};
use planetfall_sim::math::V2;
use planetfall_sim::run::Extraction;
use planetfall_sim::terrain::Terrain;
use planetfall_sim::{Game, Phase};
use quasar_render_2d::{Canvas, Color, Rect};

fn rgba(r: u8, g: u8, b: u8, a: f32) -> Color {
    Color([r, g, b, (a.clamp(0.0, 1.0) * 255.0) as u8])
}

pub fn draw(c: &mut Canvas, camera: &Camera, game: &Game, time: f32) {
    let (w, h) = (c.width() as f32, c.height() as f32);
    let fill = |c: &mut Canvas, color: Color| c.rect(Rect::new(0.0, 0.0, w, h), color);
    match game.phase {
        Phase::Ftl { ticks, .. } => {
            let t = 1.0 - ticks as f32 / FTL_TICKS as f32;
            // Flash on entering and leaving the jump.
            let flash = (1.0 - ((t - 0.25).abs() / 0.05)).max(0.0)
                + (1.0 - ((t - 0.97).abs() / 0.04)).max(0.0);
            fill(c, rgba(220, 235, 255, flash));
        }
        Phase::Descent { ticks } => {
            let t = 1.0 - ticks as f32 / DESCENT_TICKS as f32;
            fill(
                c,
                rgba(255, 150, 60, 0.12 * (time * 20.0).sin().abs() * (1.0 - t)),
            );
            if t > 0.93 {
                fill(c, rgba(255, 240, 220, (t - 0.93) / 0.07));
            }
        }
        Phase::Ascent { ticks } => {
            let t = 1.0 - ticks as f32 / ASCENT_TICKS as f32;
            if t > 0.7 {
                fill(c, rgba(0, 0, 0, (t - 0.7) / 0.3));
            }
        }
        Phase::Lost { ticks } => {
            let t = 1.0 - ticks as f32 / LOST_TICKS as f32;
            fill(c, rgba(120, 10, 6, 0.25 + 0.25 * t));
            if t > 0.75 {
                fill(c, rgba(0, 0, 0, (t - 0.75) / 0.25));
            }
        }
        Phase::Ship | Phase::Surface => {}
    }
    let Some(run) = &game.run else {
        ship_marker(c, camera, game, time);
        return;
    };
    if game.phase != Phase::Surface {
        return;
    }
    let p = &run.player;
    // Mining progress over the block being drilled.
    if let Some(m) = p.mining {
        let center = Terrain::center_of(m.tile.0, m.tile.1);
        if let Some(s) = camera.project([center[0], 2.9, center[1]]) {
            let bw = 64.0;
            c.rect(
                Rect::new(s[0] - bw * 0.5 - 2.0, s[1] - 2.0, bw + 4.0, 10.0),
                rgba(0, 0, 0, 0.6),
            );
            c.rect(
                Rect::new(s[0] - bw * 0.5, s[1], bw * m.progress, 6.0),
                rgba(255, 170, 70, 0.95),
            );
        }
    }
    // Health bars over big enemies.
    for e in &run.enemies {
        if e.kind != EnemyKind::Brute && e.health >= e.max_health {
            continue;
        }
        let height = if e.kind == EnemyKind::Brute { 3.4 } else { 2.3 };
        if let Some(s) = camera.project([e.pos[0], height, e.pos[1]]) {
            let bw = if e.kind == EnemyKind::Brute {
                70.0
            } else {
                34.0
            };
            let f = (e.health / e.max_health).clamp(0.0, 1.0);
            c.rect(
                Rect::new(s[0] - bw * 0.5 - 1.0, s[1] - 1.0, bw + 2.0, 6.0),
                rgba(0, 0, 0, 0.55),
            );
            c.rect(
                Rect::new(s[0] - bw * 0.5, s[1], bw * f, 4.0),
                rgba(230, 70, 50, 0.9),
            );
        }
    }
    // Off-screen arrows: tutorial target, extraction zone, unopened caches.
    let mut targets: Vec<(V2, Color)> = Vec::new();
    if let Some(t) = game.tutorial_prompt().and_then(|p| p.target) {
        targets.push((t, rgba(255, 210, 90, 0.95)));
    }
    match run.extraction {
        Extraction::Inbound { pos, .. } | Extraction::Waiting { pos, .. } => {
            targets.push((pos, rgba(120, 255, 150, 0.95)))
        }
        _ => {}
    }
    for cache in run.caches.iter().filter(|c| !c.opened) {
        targets.push((cache.pos, rgba(255, 170, 70, 0.6)));
    }
    for (t, color) in targets {
        edge_arrow(c, camera, [t[0], 1.0, t[1]], color);
    }
}

fn ship_marker(c: &mut Canvas, camera: &Camera, game: &Game, _time: f32) {
    if game.phase != Phase::Ship {
        return;
    }
    if let Some(t) = game.tutorial_prompt().and_then(|p| p.target) {
        edge_arrow(c, camera, [t[0], 1.0, t[1]], rgba(255, 210, 90, 0.95));
    }
}

/// Arrow at the screen edge pointing to an off-screen world point.
fn edge_arrow(c: &mut Canvas, camera: &Camera, world: [f32; 3], color: Color) {
    let (w, h) = (c.width() as f32, c.height() as f32);
    let Some(s) = camera.project(world) else {
        return;
    };
    let margin = 40.0;
    if s[0] > margin && s[0] < w - margin && s[1] > margin && s[1] < h - margin {
        return;
    }
    let center = [w * 0.5, h * 0.5];
    let d = [s[0] - center[0], s[1] - center[1]];
    let k =
        ((w * 0.5 - margin) / d[0].abs().max(1e-3)).min((h * 0.5 - margin) / d[1].abs().max(1e-3));
    let tip = [center[0] + d[0] * k, center[1] + d[1] * k];
    let len = (d[0] * d[0] + d[1] * d[1]).sqrt().max(1e-3);
    let dir = [d[0] / len, d[1] / len];
    let perp = [-dir[1], dir[0]];
    let back = [tip[0] - dir[0] * 22.0, tip[1] - dir[1] * 22.0];
    let left = [back[0] + perp[0] * 11.0, back[1] + perp[1] * 11.0];
    let right = [back[0] - perp[0] * 11.0, back[1] - perp[1] * 11.0];
    c.line(tip, left, 4.0, color);
    c.line(tip, right, 4.0, color);
    c.line(left, right, 4.0, color);
}
