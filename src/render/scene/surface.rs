//! The planet surface: terrain decorations, pods, caches, pickups and the
//! tutorial marker. Characters and effects come from sibling modules.

use super::characters::enemies;
use super::effects::{effects, projectiles};
use super::*;

// ---------------------------------------------------------------------------
// Planet surface (also used for descent, ascent and death)
// ---------------------------------------------------------------------------

pub fn surface<'a>(
    p: &mut Painter<'a, '_>,
    game: &Game,
    run: &Run,
    view_center: V2,
) -> SceneOutput<'a> {
    let palette: Palette = run.planet.info().palette;
    let time = p.time;
    p.lights.push(sun(SUN_DIRECTION, palette.sun, 0.85));
    let wide = !matches!(game.phase, Phase::Surface | Phase::Lost { .. });
    let mining = terrain(p, run, view_center, if wide { 6 } else { 0 });
    let hide_player = matches!(game.phase, Phase::Descent { .. } | Phase::Ascent { .. });
    pods(p, game, run);
    caches(p, run);
    pickups(p, run);
    enemies(p, run);
    if !hide_player {
        for (index, pl) in run.players.iter().enumerate() {
            player(p, game, run, index, pl);
        }
    }
    deployables(p, run);
    projectiles(p, run);
    effects(p, run);
    objective_markers(p, game, run);
    SceneOutput {
        draws: std::mem::take(&mut p.draws),
        lights: std::mem::take(&mut p.lights),
        ambient: palette.ambient,
        clear: [palette.sky[0], palette.sky[1], palette.sky[2], 1.0],
        shadow_focus: [view_center[0], 0.0, view_center[1]],
        transparent: std::mem::take(&mut p.transparent),
        skinned: std::mem::take(&mut p.skinned),
        post: post_settings(game, time),
        voxels: Some(VoxelScene {
            key: WorldKey::Planet {
                planet: run.planet,
                drop: game.profile.stats.drops,
            },
            palette: Some((run.planet, palette)),
            overlays: mining.into_iter().collect(),
            fog: Some(Fog {
                color: palette.sky.map(crate::render::srgb_to_linear),
                start: 38.0,
                end: 95.0,
            }),
        }),
    }
}

/// One player: the suited character (or their fall), dash trail and drones.
fn player(p: &mut Painter<'_, '_>, game: &Game, run: &Run, index: usize, pl: &Player) {
    let time = p.time;
    let dying = match game.phase {
        Phase::Lost { ticks } => 1.0 - ticks as f32 / LOST_TICKS as f32,
        _ if pl.down => 1.0,
        _ => 0.0,
    };
    let flash = if pl.hurt_ticks > 0 {
        pl.hurt_ticks as f32 / 18.0 * 0.8
    } else {
        0.0
    };
    if dying > 0.0 {
        let root = at(pl.pos, 0.0)
            * Mat4::rotation_y(pl.yaw)
            * Mat4::rotation_x((dying * 3.0).min(1.0) * 1.45);
        p.suit(pl.class, root, &[Layer::full("idle", 0.0)], 0.0, None);
        return;
    }
    let held = pl
        .weapons
        .iter()
        .find(|w| w.kind.info().behavior != WeaponBehavior::Orbit)
        .map(|w| w.kind);
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
    p.astronaut(pl.class, pl.pos, 0.0, yaw, pl.walk, pl.moving, flash, held);
    if pl.dash_ticks > 0 {
        let back = math::from_yaw(pl.yaw);
        for i in 1..4 {
            let q = pl.pos - (back * i as f32 * 0.5);
            let k = 0.12 - i as f32 * 0.025;
            p.glow(at(q, 0.9) * s(k, k, k), [0.5, 0.85, 1.0]);
        }
    }
    for (i, d) in run.drone_positions(index).into_iter().enumerate() {
        let bob = (time * 3.0 + i as f32).sin() * 0.15;
        p.rig(names::DRONE, at(d, 1.3 + bob), 0.0, |n| {
            if n == "rotor" {
                Mat4::rotation_y(time * 14.0)
            } else {
                Mat4::IDENTITY
            }
        });
    }
}

/// Barrier posts and turrets left by class abilities.
fn deployables(p: &mut Painter<'_, '_>, run: &Run) {
    let time = p.time;
    for d in &run.deployables {
        let color = run.players[d.owner].class.info().color;
        // Fade in over a few ticks and flicker out at the end.
        let age = d.total - d.ttl;
        let rise = (age as f32 / 8.0).min(1.0);
        let fading = d.ttl < 60 && (d.ttl / 4) % 2 == 0;
        let zap = (1.0 - d.since_fired as f32 / 10.0).max(0.0);
        match d.kind {
            DeployableKind::Barrier => {
                let h = 1.5 * rise;
                p.block(
                    at(d.pos, h * 0.5) * s(0.16, h * 0.5, 0.16),
                    [0.3, 0.32, 0.36],
                    Surface::Metal,
                    1.0,
                );
                if !fading {
                    let pulse = 0.6 + 0.4 * (time * 9.0 + d.pos[0]).sin() + zap;
                    p.glow(at(d.pos, h) * s(0.12, 0.12, 0.12), color);
                    p.ghost(
                        at(d.pos, h * 0.5) * s(0.42, h * 0.5, 0.42),
                        color,
                        0.12 * pulse,
                    );
                }
            }
            DeployableKind::Turret { weapon, mobile } => {
                let root = at(d.pos, 0.0);
                // A mobile sentry stands on little legs.
                if mobile {
                    for k in 0..3 {
                        let a = k as f32 * std::f32::consts::TAU / 3.0 + time * 6.0;
                        let leg = root * Mat4::rotation_y(a) * t(0.0, 0.12, 0.32);
                        p.block(
                            leg * s(0.05, 0.14, 0.05),
                            [0.25, 0.27, 0.3],
                            Surface::Metal,
                            1.0,
                        );
                    }
                }
                p.block(
                    root * t(0.0, 0.25, 0.0) * s(0.35, 0.25, 0.35),
                    [0.3, 0.32, 0.36],
                    Surface::Metal,
                    1.0,
                );
                let head = root * t(0.0, 0.75 * rise, 0.0) * Mat4::rotation_y(d.yaw);
                p.block(head * s(0.28, 0.2, 0.36), color, Surface::Metal, 1.0);
                p.glow(
                    head * t(0.0, 0.05, 0.42) * s(0.06, 0.06, 0.12),
                    weapon.info().color,
                );
                if zap > 0.0 {
                    p.point(
                        [d.pos[0], 1.2, d.pos[1]],
                        weapon.info().color,
                        1.2 * zap,
                        4.0,
                    );
                }
                if !fading {
                    p.ghost(
                        root * t(0.0, 0.02, 0.0) * s(d.range * 0.04, 0.01, d.range * 0.04),
                        color,
                        0.05,
                    );
                }
            }
        }
    }
}

/// Decorations on the voxel terrain (pebbles, ore crystals and their glow)
/// and the overlay marking the block being drilled.
fn terrain(p: &mut Painter<'_, '_>, run: &Run, center: V2, extra: i32) -> Option<OverlayBox> {
    let (cx, cy) = Terrain::tile_of(center);
    let mut glows: Vec<(f32, [f32; 3], [f32; 3])> = Vec::new();
    for y in cy - 10 - extra..=cy + 8 + extra {
        for x in cx - 12 - extra..=cx + 12 + extra {
            let tile = run.terrain.tile(x, y);
            let c = Terrain::center_of(x, y);
            let h = tile_hash(x, y);
            match tile {
                // Scattered pebbles on open ground.
                Tile::Floor if h > 0.9 => {
                    let off = V2::new(
                        (tile_hash(x * 5, y) - 0.5) * 1.4,
                        (tile_hash(x, y * 5) - 0.5) * 1.4,
                    );
                    let size = 0.12 + h * 0.12;
                    p.cube(
                        at(c + off, size * 0.5)
                            * Mat4::rotation_y(h * 6.0)
                            * s(size, size * 0.6, size),
                        [0.25, 0.24, 0.22],
                    );
                }
                Tile::Ore { material, .. } => {
                    let (id, light) = match material {
                        Material::Ferrite => (names::FERRITE_ORE, None),
                        Material::Lumite => (names::LUMITE_ORE, Some([0.3, 0.85, 1.0])),
                        Material::Voidstone => (names::VOIDSTONE_ORE, Some([0.65, 0.3, 1.0])),
                    };
                    // Crystals erupt from the ore block (the [-1, 1] cube).
                    p.model(id, at(c, TILE * 0.5) * s(1.02, 1.02, 1.02));
                    if let Some(color) = light {
                        glows.push((
                            c.distance_squared(run.player().pos),
                            [c[0], TILE + 0.8, c[1]],
                            color,
                        ));
                    }
                }
                _ => {}
            }
        }
    }
    glows.sort_by(|a, b| a.0.total_cmp(&b.0));
    // Clustered lighting makes many small lights cheap; keep the nearest.
    for (_, pos, color) in glows.into_iter().take(40) {
        let pulse = 1.0 + (p.time * 2.0 + pos[0]).sin() * 0.15;
        p.point(pos, color, 1.6 * pulse, 7.0);
    }
    // The block being drilled: an outline that heats up with progress.
    run.player().mining.map(|m| {
        let k = m.progress.clamp(0.0, 1.0);
        let jitter = (p.time * 50.0).sin() * 0.01 * (1.0 + k);
        voxels::block_box(
            m.tile.0,
            0,
            m.tile.1,
            0.02 + jitter.abs(),
            [1.0, 0.75 - 0.5 * k, 0.3 - 0.25 * k, 1.0],
        )
    })
}

fn pods(p: &mut Painter<'_, '_>, game: &Game, run: &Run) {
    let landing = run.landing;
    let flame = |p: &mut Painter<'_, '_>, pos: V2, y: f32, power: f32| {
        let flicker = 0.85 + 0.15 * (p.time * 37.0).sin();
        p.ghost(
            at(pos, y - 1.0 * power) * s(0.45, 1.2 * power * flicker, 0.45),
            [1.0, 0.55, 0.15],
            0.6,
        );
        p.glow(at(pos, y - 0.1) * s(0.35, 0.08, 0.35), [1.0, 0.8, 0.4]);
        p.point(
            [pos[0], y - 0.8, pos[1]],
            [1.0, 0.55, 0.2],
            4.0 * power,
            14.0,
        );
    };
    let pod = |p: &mut Painter<'_, '_>, pos: V2, y: f32, door: f32| {
        p.rig(names::DROP_POD, at(pos, y), 0.0, |n| {
            if n == "door" {
                Mat4::rotation_y(-door)
            } else {
                Mat4::IDENTITY
            }
        });
    };
    match game.phase {
        Phase::Descent { ticks } => {
            let h = descent_height(ticks);
            pod(p, landing, h, 0.0);
            flame(p, landing, h, 1.0);
            // Atmospheric streaks rushing past.
            for i in 0..30 {
                let k = tile_hash(i, 99);
                let off = V2::new((k - 0.5) * 8.0, (tile_hash(99, i) - 0.5) * 8.0);
                let y = h + ((p.time * 40.0 + k * 30.0) % 30.0) - 5.0;
                p.ghost(
                    at(landing + off, y) * s(0.04, 2.0, 0.04),
                    [1.0, 0.9, 0.8],
                    0.35,
                );
            }
            return;
        }
        Phase::Ascent { ticks } => {
            let h = ascent_height(ticks);
            pod(p, landing, h, 0.0);
            flame(p, landing, h, 1.2);
            return;
        }
        _ => {}
    }
    // The arrival pod sits open, then returns to orbit.
    if run.ticks < 360 {
        let lift = if run.ticks > 200 {
            ((run.ticks - 200) as f32 / 60.0).powi(2) * 4.0
        } else {
            0.0
        };
        let door =
            (run.ticks as f32 / 40.0).min(1.0) * 1.3 * if run.ticks > 170 { 0.0 } else { 1.0 };
        pod(p, landing, lift, door);
        if lift > 0.0 {
            flame(p, landing, lift, 1.0);
        }
    }
    match run.extraction {
        Extraction::Inbound { ticks, pos } => {
            let h = if ticks < 180 {
                55.0 * (ticks as f32 / 180.0).powi(2)
            } else {
                55.0
            };
            if ticks < 180 {
                pod(p, pos, h, 0.0);
                flame(p, pos, h, 1.0);
            }
            // Landing zone preview.
            zone_ring(
                p,
                pos,
                1.0 - ticks as f32 / extraction_inbound() as f32,
                [1.0, 0.55, 0.15],
            );
        }
        Extraction::Waiting { pos, hold } => {
            pod(p, pos, 0.0, 1.3);
            let progress = hold as f32 / extraction_hold() as f32;
            zone_ring(p, pos, progress, [0.4, 1.0, 0.6]);
            p.point([pos[0], 3.0, pos[1]], [0.5, 1.0, 0.6], 2.5, 10.0);
        }
        Extraction::Idle | Extraction::Departed { .. } => {}
    }
}

fn zone_ring(p: &mut Painter<'_, '_>, pos: V2, progress: f32, color: [f32; 3]) {
    let n = 28;
    for i in 0..n {
        let a = i as f32 / n as f32 * std::f32::consts::TAU + p.time * 0.4;
        let lit = (i as f32 / n as f32) < progress;
        let point = pos + V2::new(a.sin(), a.cos()) * extraction_radius();
        let c = if lit { color } else { math_scale3(color, 0.25) };
        p.glow(
            at(point, 0.08) * Mat4::rotation_y(a) * s(0.32, 0.05, 0.12),
            c,
        );
    }
}

fn caches(p: &mut Painter<'_, '_>, run: &Run) {
    for (i, c) in run.caches.iter().enumerate() {
        let yaw = i as f32 * 1.3;
        if c.opened {
            if c.open_ticks < 40 {
                let k = 1.0 - c.open_ticks as f32 / 40.0;
                p.model(
                    names::CRATE_MEDIUM,
                    at(c.pos, 0.0) * Mat4::rotation_y(yaw) * s(1.8 * k, 1.8 * k, 1.8 * k),
                );
                p.point([c.pos[0], 2.0, c.pos[1]], [1.0, 0.8, 0.3], 4.0 * k, 9.0);
            }
            continue;
        }
        let bob = (p.time * 2.0 + i as f32).sin() * 0.05;
        p.model(
            names::CRATE_MEDIUM,
            at(c.pos, bob) * Mat4::rotation_y(yaw) * s(1.8, 1.8, 1.8),
        );
        p.ghost(at(c.pos, 4.0) * s(0.05, 4.0, 0.05), [1.0, 0.7, 0.25], 0.3);
        p.point([c.pos[0], 2.2, c.pos[1]], [1.0, 0.7, 0.3], 2.2, 8.0);
    }
}

fn pickups(p: &mut Painter<'_, '_>, run: &Run) {
    for (i, pk) in run.pickups.iter().enumerate() {
        let spin = Mat4::rotation_y(p.time * 3.0 + i as f32);
        let bob = (p.time * 4.0 + i as f32 * 0.7).sin() * 0.08;
        let (id, scale) = match pk.kind {
            planetfall_sim::run::PickupKind::Xp(v) => {
                (names::XP_SHARD, 0.8 + (v as f32).sqrt() * 0.2)
            }
            planetfall_sim::run::PickupKind::Material(Material::Ferrite, _) => {
                (names::FERRITE_CHUNK, 1.3)
            }
            planetfall_sim::run::PickupKind::Material(Material::Lumite, _) => {
                (names::LUMITE_CHUNK, 1.4)
            }
            planetfall_sim::run::PickupKind::Material(Material::Voidstone, _) => {
                (names::VOIDSTONE_CHUNK, 1.5)
            }
        };
        p.model(
            id,
            at(pk.pos, pk.height + bob) * spin * s(scale, scale, scale),
        );
    }
}

fn objective_markers(p: &mut Painter<'_, '_>, game: &Game, run: &Run) {
    let Some(target) = game.tutorial_prompt().and_then(|pr| pr.target) else {
        return;
    };
    let bounce = (p.time * 3.0).sin().abs() * 0.5;
    let _ = run;
    p.ghost(at(target, 2.0) * s(0.05, 2.0, 0.05), [1.0, 0.85, 0.3], 0.4);
    p.glow(
        at(target, 4.3 + bounce)
            * Mat4::rotation_y(p.time * 2.0)
            * Mat4::rotation_x(0.785)
            * rot_z(0.785)
            * s(0.22, 0.22, 0.22),
        [1.0, 0.8, 0.3],
    );
}
