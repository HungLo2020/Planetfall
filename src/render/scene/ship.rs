//! The ship: hull assembly, stations, modules, build mode and markers.

use super::*;

// ---------------------------------------------------------------------------
// Ship
// ---------------------------------------------------------------------------

pub fn ship<'a>(p: &mut Painter<'a, '_>, game: &Game) -> SceneOutput<'a> {
    let ship = &game.ship;
    let time = p.time;
    p.lights.push(sun(SUN_DIRECTION, [1.0, 0.95, 0.88], 0.55));
    let built = |room: Room| match room {
        Room::Module(u) => ship.build_progress(u),
        // Rooms still assembling are absent from the hull until complete.
        _ => 1.0,
    };
    // Module rooms assemble block by block, dropping into place, then join
    // the voxel hull when construction completes.
    for &(upgrade, _) in &ship.construction {
        let progress = ship.build_progress(upgrade);
        for (v, id) in ShipLayout::room_blocks(Room::Module(upgrade)) {
            let delay = tile_hash(v.x, v.z) * 0.6 + (v.y + 2) as f32 * 0.05;
            let local = ((progress - delay) / 0.35).clamp(0.0, 1.0);
            if local <= 0.0 {
                continue;
            }
            let drop = (1.0 - local) * 6.0;
            let height = if id == voxel::HULL || id == voxel::WINDOW {
                0.6
            } else {
                1.0
            };
            let base = V2::new(
                (v.x as f32 + 0.5) * SHIP_TILE,
                (v.z as f32 + 0.5) * SHIP_TILE,
            );
            let y = v.y as f32 * SHIP_TILE + height + drop;
            let color = match id {
                voxel::DECK => [0.20, 0.22, 0.25],
                voxel::WINDOW => [0.35, 0.6, 0.85],
                voxel::KEEL => [0.18, 0.19, 0.22],
                _ => [0.30, 0.32, 0.36],
            };
            p.block(
                at(base, y) * s(1.0, height, 1.0),
                color,
                Surface::Metal,
                1.0,
            );
            if local < 1.0 {
                // Welding seam along the top while the block settles.
                p.ghost(
                    at(base, y + height + 0.02) * s(1.0, 0.02, 1.0),
                    [1.0, 0.6, 0.2],
                    0.35 * (1.0 - local),
                );
            }
        }
    }
    // Player-built lamps light the deck.
    let mut overlays = Vec::new();
    for z in BUILD_AREA.1..=BUILD_AREA.3 {
        for x in BUILD_AREA.0..=BUILD_AREA.2 {
            if ship.layout.wall(x, z) == voxel::LAMP {
                let c = V2::new((x as f32 + 0.5) * SHIP_TILE, (z as f32 + 0.5) * SHIP_TILE);
                p.point([c.x, 1.6, c.y], [1.0, 0.85, 0.6], 1.4, 9.0);
            }
        }
    }
    // Build mode: cursor box and a ghost of the selected block.
    if let Some(block) = ship.build {
        let (x, z) = ship.build_cursor();
        let ok = ship.can_place(&game.profile).is_ok();
        let color = if ok {
            [0.4, 1.0, 0.55]
        } else {
            [1.0, 0.35, 0.3]
        };
        let layer = block.layer();
        overlays.push(voxels::ship_box(
            x,
            layer,
            z,
            0.02,
            [color[0], color[1], color[2], 1.0],
        ));
        let c = V2::new((x as f32 + 0.5) * SHIP_TILE, (z as f32 + 0.5) * SHIP_TILE);
        let (y, height) = match block {
            ShipBlock::Deck => (-1.0, 1.0),
            ShipBlock::Lamp => (0.45, 0.45),
            _ => (0.6, 0.6),
        };
        let pulse = 0.25 + 0.1 * (time * 5.0).sin();
        p.glass(at(c, y) * s(1.02, height, 1.02), color, pulse);
    }
    for room in &ship.layout.rooms {
        let c = room.center();
        let (warm, glow) = match room {
            Room::Bridge => ([0.6, 0.85, 1.0], 1.1),
            Room::DropBay => ([1.0, 0.75, 0.5], 1.2),
            Room::Module(u) => (u.info().light, u.info().light_intensity),
            Room::Corridor => ([1.0, 0.92, 0.8], 1.3),
        };
        if built(*room) > 0.5 {
            p.point([c[0], 4.0, c[1]], warm, glow, 11.0);
        }
    }
    for station in &ship.layout.stations {
        let root = at(station.pos, 0.0) * Mat4::rotation_y(station.yaw);
        match station.kind {
            StationKind::NavConsole => {
                p.rig(names::NAV_CONSOLE, root, 0.0, |n| {
                    if n == "holo" {
                        Mat4::rotation_y(time * 0.8)
                    } else {
                        Mat4::IDENTITY
                    }
                });
                // Holographic star map of the destination.
                let tint = game
                    .profile
                    .orbiting
                    .map_or([0.4, 0.8, 1.0], |pl| pl.info().palette.planet);
                p.model_tinted(
                    names::PLANET,
                    root * t(0.0, 1.9, 0.0) * s(0.22, 0.22, 0.22),
                    tint,
                    math_scale3(tint, 0.5),
                );
            }
            StationKind::Fabricator => {
                p.model(names::WORKBENCH, root * s(2.2, 2.2, 2.2));
                p.model(
                    names::METAL_PANEL,
                    root * t(0.0, 0.0, -0.9) * s(2.0, 2.0, 2.0),
                );
                let arm = (p.time * 2.0).sin() * 0.3;
                p.glow(
                    root * t(arm, 2.0, 0.0) * s(0.07, 0.07, 0.07),
                    [1.0, 0.55, 0.15],
                );
                p.point(
                    [station.pos[0], 2.5, station.pos[1]],
                    [1.0, 0.6, 0.3],
                    1.2,
                    6.0,
                );
            }
            StationKind::Locker => {
                p.model(names::LOCKER, root * t(0.0, 0.0, -0.2));
                // Lit in the active class's colour.
                let color = game.profile.class.info().color;
                p.point([station.pos[0] + 0.6, 2.2, station.pos[1]], color, 0.9, 5.0);
            }
            StationKind::DropPod => {
                p.model(names::POD_CRADLE, root);
                let ready = game.profile.orbiting.is_some();
                p.rig(names::DROP_POD, root * t(0.0, 0.25, 0.0), 0.0, |n| {
                    if n == "door" && ready {
                        Mat4::rotation_y(-1.2)
                    } else {
                        Mat4::IDENTITY
                    }
                });
                if ready {
                    p.point(
                        [station.pos[0], 1.5, station.pos[1] + 1.5],
                        [1.0, 0.6, 0.25],
                        1.5,
                        7.0,
                    );
                }
            }
            StationKind::Module(upgrade) => {
                let progress = ship.build_progress(upgrade);
                if progress < 0.7 {
                    continue;
                }
                let pop = ((progress - 0.7) / 0.3).clamp(0.0, 1.0);
                let root = root * s(pop, pop, pop);
                // The module's station model, with an optional spinning part.
                let info = upgrade.info();
                let k = info.station_scale;
                let [ox, oy, oz] = info.station_offset;
                let root = root * t(ox, oy, oz) * s(k, k, k);
                p.rig(&info.station, root, 0.0, |node| match &info.spin {
                    Some(spin) if spin.node == node => {
                        let a = time * spin.speed;
                        match spin.axis {
                            Axis::X => Mat4::rotation_x(a),
                            Axis::Y => Mat4::rotation_y(a),
                            Axis::Z => rot_z(a),
                        }
                    }
                    _ => Mat4::IDENTITY,
                });
                // Clutter that grows with the module level.
                for i in 0..game.profile.level(upgrade) {
                    let off = [1.6 - f32::from(i) * 1.1, 1.8];
                    let prop = if i % 2 == 0 {
                        names::BARREL
                    } else {
                        names::CRATE_MEDIUM
                    };
                    p.model(
                        prop,
                        at(
                            V2::new(station.pos[0] + off[0], station.pos[1] + off[1]),
                            0.0,
                        ) * s(2.0, 2.0, 2.0),
                    );
                }
            }
        }
    }
    // Decor in the base rooms.
    p.model(
        names::CRATE_WIDE,
        at(V2::new(5.6 * SHIP_TILE, 14.6 * SHIP_TILE), 0.0) * s(2.2, 2.2, 2.2),
    );
    p.model(
        names::BARREL,
        at(V2::new(9.4 * SHIP_TILE, 14.7 * SHIP_TILE), 0.0) * s(2.2, 2.2, 2.2),
    );
    p.model(
        names::BARREL,
        at(V2::new(9.0 * SHIP_TILE, 15.2 * SHIP_TILE), 0.0) * s(2.2, 2.2, 2.2),
    );
    p.model(
        names::CRATE_MEDIUM,
        at(V2::new(5.5 * SHIP_TILE, 1.6 * SHIP_TILE), 0.0)
            * Mat4::rotation_y(0.4)
            * s(2.0, 2.0, 2.0),
    );
    let pl = &ship.player;
    if !matches!(game.phase, Phase::Descent { .. }) {
        p.astronaut(
            game.profile.class,
            pl.pos,
            0.0,
            pl.yaw,
            pl.walk,
            pl.moving,
            0.0,
            None,
        );
    }
    ship_markers(p, game);
    space_backdrop(
        p,
        game.profile.orbiting,
        game.profile.docked,
        V2::new(7.5 * SHIP_TILE, 8.5 * SHIP_TILE),
    );
    SceneOutput {
        draws: std::mem::take(&mut p.draws),
        lights: std::mem::take(&mut p.lights),
        ambient: [0.08, 0.085, 0.1],
        clear: [0.004, 0.006, 0.014, 1.0],
        shadow_focus: [pl.pos[0], 0.0, pl.pos[1]],
        transparent: std::mem::take(&mut p.transparent),
        skinned: std::mem::take(&mut p.skinned),
        post: post_settings(game, p.time),
        voxels: Some(VoxelScene {
            key: WorldKey::Ship,
            palette: None,
            overlays,
            fog: None,
        }),
    }
}

fn space_backdrop(p: &mut Painter<'_, '_>, orbiting: Option<PlanetId>, docked: bool, center: V2) {
    for i in 0..260 {
        let h = |k: i32| tile_hash(i * 7 + k, i * 13 - k);
        let x = center[0] + (h(1) - 0.5) * 260.0;
        let z = center[1] + (h(2) - 0.5) * 200.0;
        let y = -30.0 - h(3) * 50.0;
        let size = 0.06 + h(4) * 0.14;
        let twinkle = 0.6 + 0.4 * (p.time * (1.0 + h(5) * 3.0) + h(6) * 9.0).sin();
        let color = mix3([0.7, 0.8, 1.0], [1.0, 0.85, 0.7], h(7));
        p.glow(
            t(x, y, z) * s(size, size, size),
            math_scale3(color, twinkle * 1.6),
        );
    }
    if docked {
        // Moored beside the trade station, lit by its windows.
        let spin = Mat4::rotation_y(p.time * 0.03);
        p.model(
            names::TRADE_STATION,
            t(center[0] - 45.0, -40.0, center[1] - 10.0)
                * Mat4::rotation_x(0.35)
                * spin
                * s(26.0, 26.0, 26.0),
        );
        p.point(
            [center[0] - 30.0, -20.0, center[1]],
            [1.0, 0.85, 0.55],
            3.0,
            60.0,
        );
    }
    if let Some(planet) = orbiting {
        let tint = planet.info().palette.planet;
        let spin = Mat4::rotation_y(p.time * 0.02);
        p.model_tinted(
            names::PLANET,
            t(center[0] - 55.0, -120.0, center[1] - 25.0) * spin * s(85.0, 85.0, 85.0),
            tint,
            math_scale3(tint, 0.08),
        );
    }
}

/// Markers for ship tutorial steps (drawn in ship space).
fn ship_markers(p: &mut Painter<'_, '_>, game: &Game) {
    let Some(target) = game.tutorial_prompt().and_then(|pr| pr.target) else {
        return;
    };
    let bounce = (p.time * 3.0).sin().abs() * 0.4;
    p.ghost(at(target, 3.5) * s(0.04, 3.5, 0.04), [1.0, 0.85, 0.3], 0.4);
    p.glow(
        at(target, 3.6 + bounce)
            * Mat4::rotation_y(p.time * 2.0)
            * Mat4::rotation_x(0.785)
            * rot_z(0.785)
            * s(0.25, 0.25, 0.25),
        [1.0, 0.8, 0.3],
    );
}
