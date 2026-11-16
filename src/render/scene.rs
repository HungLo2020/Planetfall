//! Turns game state into lit surface draws and lights. Pure presentation:
//! reads the simulation, never changes it.

use super::geometry::{Textures3D, tile_hash};
use super::models::{ModelId, Models};
use crate::render::camera::{ascent_height, descent_height};
use planetfall_sim::content::{EnemyKind, Material, Palette, PlanetId, ShipUpgrade, WeaponKind};
use planetfall_sim::game::{FTL_TICKS, LOST_TICKS};
use planetfall_sim::math::{self, V2};
use planetfall_sim::run::{
    BlastKind, EXTRACTION_HOLD, EXTRACTION_INBOUND, EXTRACTION_RADIUS, Extraction, ProjectileKind,
    Run, SparkKind,
};
use planetfall_sim::ship::{Room, ShipTile, StationKind};
use planetfall_sim::terrain::{TILE, Terrain, Tile};
use planetfall_sim::{Game, Phase};
use quasar_render_3d::{
    Attenuation, Light, LitMaterial, Mat4, Mesh, NormalMap, ShadingModel, SurfaceDraw3D,
    SurfaceMaterial,
};

/// Everything the 3D passes need for one frame.
pub struct SceneOutput<'a> {
    pub draws: Vec<SurfaceDraw3D<'a>>,
    pub lights: Vec<Light>,
    pub ambient: [f32; 3],
    pub clear: [f32; 4],
    /// Center of the directional shadow volume.
    pub shadow_focus: [f32; 3],
}

#[derive(Clone, Copy)]
enum Surface {
    Plain,
    Rock,
    Metal,
}

pub struct Painter<'a> {
    cube: &'a Mesh,
    tex: &'a Textures3D,
    models: &'a Models,
    pub draws: Vec<SurfaceDraw3D<'a>>,
    pub lights: Vec<Light>,
    pub time: f32,
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

impl<'a> Painter<'a> {
    pub fn new(cube: &'a Mesh, tex: &'a Textures3D, models: &'a Models, time: f32) -> Self {
        Self {
            cube,
            tex,
            models,
            draws: Vec::with_capacity(2048),
            lights: Vec::with_capacity(24),
            time,
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
            Surface::Rock => Some(NormalMap {
                uv_scale: [uv; 2],
                strength: 1.0,
                ..NormalMap::new(&self.tex.rock_normal)
            }),
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

    /// Unlit emissive cube (projectiles, sparks, lights).
    fn glow(&mut self, model: Mat4, color: [f32; 3]) {
        let mut lit = LitMaterial::textured(&self.tex.white);
        lit.base.tint = [0.0, 0.0, 0.0, 1.0];
        lit.emissive = color;
        lit.shading = ShadingModel::Unlit;
        let mut m = SurfaceMaterial::from(lit);
        m.receives_shadows = false;
        self.draws.push(SurfaceDraw3D {
            mesh: self.cube,
            model,
            material: m,
        });
    }

    /// Soft holographic glow: an emissive volume dimmed by `alpha`. (The lit
    /// pass encodes sRGB in-shader, so effects are opaque emissive geometry.)
    fn ghost(&mut self, model: Mat4, color: [f32; 3], alpha: f32) {
        self.glow(model, math_scale3(color, 0.35 + alpha.clamp(0.0, 1.0)));
    }

    fn model(&mut self, id: ModelId, root: Mat4) {
        let draws = self.models.get(id).draws(root);
        self.draws.extend(draws);
    }

    fn model_tinted(&mut self, id: ModelId, root: Mat4, tint: [f32; 3], emissive: [f32; 3]) {
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

    /// Draw a rigged model: `pose(node)` rotates each named part about its pivot.
    fn rig(&mut self, id: ModelId, root: Mat4, flash: f32, pose: impl Fn(&str) -> Mat4) {
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
        if self.lights.len() >= 24 {
            return;
        }
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
        self.glow(
            t(mid[0], mid[1], mid[2])
                * Mat4::rotation_y(yaw)
                * Mat4::rotation_x(pitch)
                * s(width, width, len * 0.5),
            color,
        );
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "pose inputs read straight from game state"
    )]
    fn astronaut(
        &mut self,
        pos: V2,
        y: f32,
        yaw: f32,
        walk: f32,
        moving: bool,
        flash: f32,
        weapon: Option<WeaponKind>,
    ) {
        let time = self.time;
        let swing = if moving {
            (walk * 2.2).sin() * 0.7
        } else {
            0.0
        };
        let bob = if moving {
            (walk * 4.4).sin().abs() * 0.06
        } else {
            (self.time * 2.0).sin() * 0.01
        };
        let root = at(pos, y + bob) * Mat4::rotation_y(yaw);
        self.rig(ModelId::Astronaut, root, flash, |name| match name {
            "leg_l" => Mat4::rotation_x(swing),
            "leg_r" => Mat4::rotation_x(-swing),
            "arm_l" => Mat4::rotation_x(-swing * 0.8),
            "arm_r" => Mat4::rotation_x(if weapon.is_some() { -1.2 } else { swing * 0.8 }),
            "head" => Mat4::rotation_y((time * 0.7).sin() * 0.15),
            _ => Mat4::IDENTITY,
        });
        if let Some(weapon) = weapon {
            let (id, scale) = match weapon {
                WeaponKind::PulseBlaster | WeaponKind::OrbitDrones => (ModelId::Blaster, 0.7),
                WeaponKind::Scattergun => (ModelId::Scattergun, 0.7),
                WeaponKind::ArcCaster => (ModelId::ArcCaster, 1.0),
                WeaponKind::PlasmaMortar => (ModelId::PlasmaMortar, 1.0),
            };
            // Held out in front of the right arm (pivot at the shoulder, arm raised).
            let gun = root
                * t(0.37, 1.34, 0.0)
                * Mat4::rotation_x(-1.2)
                * t(0.0, -0.62, 0.05)
                * Mat4::rotation_x(1.2)
                * s(scale, scale, scale);
            let flip = if matches!(id, ModelId::Blaster | ModelId::Scattergun) {
                Mat4::rotation_y(std::f32::consts::PI)
            } else {
                Mat4::IDENTITY
            };
            self.model(id, gun * flip);
        }
        // Suit lamp so the player always reads in dark caves.
        self.point([pos[0], y + 2.6, pos[1] + 0.4], [0.85, 0.92, 1.0], 1.3, 9.0);
    }
}

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

// ---------------------------------------------------------------------------
// Ship
// ---------------------------------------------------------------------------

pub fn ship<'a>(p: &mut Painter<'a>, game: &Game) -> SceneOutput<'a> {
    let ship = &game.ship;
    let time = p.time;
    p.lights.push(sun(SUN_DIRECTION, [1.0, 0.95, 0.88], 0.55));
    let hull = [0.30, 0.32, 0.36];
    let deck = [0.20, 0.22, 0.25];
    let stripe = [0.95, 0.47, 0.12];
    let built = |room: Room| match room {
        Room::Module(u) => ship.build_progress(u),
        _ => 1.0,
    };
    let room_of = |x: i32, y: i32| {
        ship.layout.rooms.iter().copied().find(|r| {
            let (x0, y0, x1, y1) = r.rect();
            (x0..=x1).contains(&x) && (y0..=y1).contains(&y)
        })
    };
    for ((x, y), tile) in ship.layout.tiles() {
        let c = [(x as f32 + 0.5) * TILE, (y as f32 + 0.5) * TILE];
        // Rooms under construction assemble block by block, dropping into place.
        let nearest_room = room_of(x, y).or_else(|| {
            [
                (1, 0),
                (-1, 0),
                (0, 1),
                (0, -1),
                (1, 1),
                (-1, -1),
                (1, -1),
                (-1, 1),
            ]
            .iter()
            .find_map(|(dx, dy)| room_of(x + dx, y + dy))
        });
        let progress = nearest_room.map_or(1.0, built);
        let delay = tile_hash(x, y) * 0.6;
        let local = ((progress - delay) / 0.4).clamp(0.0, 1.0);
        if local <= 0.0 {
            continue;
        }
        let drop = (1.0 - local) * 6.0;
        let shade = 0.92 + tile_hash(x * 3, y * 7) * 0.12;
        match tile {
            ShipTile::Space => {}
            ShipTile::Floor => {
                p.block(
                    at(c, -0.35 + drop) * s(0.99, 0.35, 0.99),
                    math_scale3(deck, shade),
                    Surface::Metal,
                    1.0,
                );
                if local < 1.0 {
                    p.glow(at(c, 0.05 + drop) * s(1.0, 0.02, 1.0), [1.0, 0.6, 0.2]);
                }
            }
            // Cut-away walls: low enough that the top-down camera sees into rooms.
            ShipTile::Wall => {
                p.block(
                    at(c, 0.6 + drop) * s(1.0, 0.6, 1.0),
                    math_scale3(hull, shade),
                    Surface::Metal,
                    1.0,
                );
                p.cube(at(c, 1.22 + drop) * s(1.0, 0.02, 1.0), [0.14, 0.15, 0.17]);
                p.cube(
                    at(c, 0.95 + drop) * s(1.01, 0.04, 1.01),
                    math_scale3(stripe, 0.7),
                );
            }
            ShipTile::Window => {
                p.block(
                    at(c, 0.3 + drop) * s(1.0, 0.3, 1.0),
                    hull,
                    Surface::Metal,
                    1.0,
                );
                p.ghost(
                    at(c, 0.85 + drop) * s(0.98, 0.25, 0.1),
                    [0.1, 0.25, 0.4],
                    0.0,
                );
                p.cube(at(c, 1.15 + drop) * s(1.0, 0.04, 1.0), hull);
            }
        }
        // Underside hull for depth against space.
        if tile != ShipTile::Space {
            p.cube(at(c, -1.4 + drop) * s(1.0, 0.7, 1.0), [0.18, 0.19, 0.22]);
        }
    }
    for room in &ship.layout.rooms {
        let c = room.center();
        let (warm, glow) = match room {
            Room::Bridge => ([0.6, 0.85, 1.0], 1.1),
            Room::DropBay => ([1.0, 0.75, 0.5], 1.2),
            Room::Module(ShipUpgrade::NavComputer) => ([0.45, 0.6, 1.0], 2.2),
            Room::Module(ShipUpgrade::HullPlating) => ([0.5, 1.0, 0.6], 1.4),
            _ => ([1.0, 0.92, 0.8], 1.3),
        };
        if built(*room) > 0.5 {
            p.point([c[0], 4.0, c[1]], warm, glow, 11.0);
        }
    }
    for station in &ship.layout.stations {
        let root = at(station.pos, 0.0) * Mat4::rotation_y(station.yaw);
        match station.kind {
            StationKind::NavConsole => {
                p.rig(ModelId::NavConsole, root, 0.0, |n| {
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
                    ModelId::Planet,
                    root * t(0.0, 1.9, 0.0) * s(0.22, 0.22, 0.22),
                    tint,
                    math_scale3(tint, 0.5),
                );
            }
            StationKind::Fabricator => {
                p.model(ModelId::Workbench, root * s(2.2, 2.2, 2.2));
                p.model(
                    ModelId::MetalPanel,
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
            StationKind::DropPod => {
                p.model(ModelId::PodCradle, root);
                let ready = game.profile.orbiting.is_some();
                p.rig(ModelId::DropPod, root * t(0.0, 0.25, 0.0), 0.0, |n| {
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
                match upgrade {
                    ShipUpgrade::HullPlating => p.model(ModelId::MedBay, root),
                    ShipUpgrade::WeaponCalibration => {
                        p.model(ModelId::WeaponRack, root * t(0.0, 0.0, -0.4))
                    }
                    ShipUpgrade::DrillRig => p.rig(ModelId::DrillRig, root, 0.0, |n| {
                        if n == "bit" {
                            rot_z(time * 6.0)
                        } else {
                            Mat4::IDENTITY
                        }
                    }),
                    ShipUpgrade::TractorArray => {
                        p.rig(ModelId::Beacon, root * s(2.0, 2.0, 2.0), 0.0, |n| {
                            if n == "light" {
                                Mat4::rotation_y(time * 2.0)
                            } else {
                                Mat4::IDENTITY
                            }
                        });
                    }
                    ShipUpgrade::NavComputer => p.rig(ModelId::ReactorCore, root, 0.0, |n| {
                        if n == "core" {
                            Mat4::rotation_y(time * 1.5)
                        } else {
                            Mat4::IDENTITY
                        }
                    }),
                }
                // Clutter that grows with the module level.
                for i in 0..game.profile.level(upgrade) {
                    let off = [1.6 - f32::from(i) * 1.1, 1.8];
                    let prop = if i % 2 == 0 {
                        ModelId::Barrel
                    } else {
                        ModelId::CrateMedium
                    };
                    p.model(
                        prop,
                        at([station.pos[0] + off[0], station.pos[1] + off[1]], 0.0)
                            * s(2.0, 2.0, 2.0),
                    );
                }
            }
        }
    }
    // Decor in the base rooms.
    p.model(
        ModelId::CrateWide,
        at([5.6 * TILE, 14.6 * TILE], 0.0) * s(2.2, 2.2, 2.2),
    );
    p.model(
        ModelId::Barrel,
        at([9.4 * TILE, 14.7 * TILE], 0.0) * s(2.2, 2.2, 2.2),
    );
    p.model(
        ModelId::Barrel,
        at([9.0 * TILE, 15.2 * TILE], 0.0) * s(2.2, 2.2, 2.2),
    );
    p.model(
        ModelId::CrateMedium,
        at([5.5 * TILE, 1.6 * TILE], 0.0) * Mat4::rotation_y(0.4) * s(2.0, 2.0, 2.0),
    );
    let pl = &ship.player;
    if !matches!(game.phase, Phase::Descent { .. }) {
        p.astronaut(pl.pos, 0.0, pl.yaw, pl.walk, pl.moving, 0.0, None);
    }
    ship_markers(p, game);
    space_backdrop(p, game.profile.orbiting, [7.5 * TILE, 8.5 * TILE]);
    SceneOutput {
        draws: std::mem::take(&mut p.draws),
        lights: std::mem::take(&mut p.lights),
        ambient: [0.08, 0.085, 0.1],
        clear: [0.004, 0.006, 0.014, 1.0],
        shadow_focus: [pl.pos[0], 0.0, pl.pos[1]],
    }
}

fn space_backdrop(p: &mut Painter, orbiting: Option<PlanetId>, center: V2) {
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
    if let Some(planet) = orbiting {
        let tint = planet.info().palette.planet;
        let spin = Mat4::rotation_y(p.time * 0.02);
        p.model_tinted(
            ModelId::Planet,
            t(center[0] - 55.0, -120.0, center[1] - 25.0) * spin * s(85.0, 85.0, 85.0),
            tint,
            math_scale3(tint, 0.08),
        );
    }
}

// ---------------------------------------------------------------------------
// FTL travel
// ---------------------------------------------------------------------------

pub fn ftl<'a>(p: &mut Painter<'a>, ticks: u32, to: PlanetId) -> SceneOutput<'a> {
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
    p.rig(ModelId::ShipExterior, t(shake, 0.0, 0.0), 0.0, |_| {
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
            ModelId::Planet,
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
    }
}

// ---------------------------------------------------------------------------
// Planet surface (also used for descent, ascent and death)
// ---------------------------------------------------------------------------

pub fn surface<'a>(
    p: &mut Painter<'a>,
    game: &Game,
    run: &Run,
    view_center: V2,
) -> SceneOutput<'a> {
    let palette: Palette = run.planet.info().palette;
    let time = p.time;
    p.lights.push(sun(SUN_DIRECTION, palette.sun, 0.85));
    let wide = !matches!(game.phase, Phase::Surface | Phase::Lost { .. });
    terrain(p, run, &palette, view_center, if wide { 6 } else { 0 });
    let hide_player = matches!(game.phase, Phase::Descent { .. } | Phase::Ascent { .. });
    pods(p, game, run);
    caches(p, run);
    pickups(p, run);
    enemies(p, run);
    if !hide_player {
        let pl = &run.player;
        let dying = if let Phase::Lost { ticks } = game.phase {
            1.0 - ticks as f32 / LOST_TICKS as f32
        } else {
            0.0
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
            p.rig(ModelId::Astronaut, root, 0.0, |_| Mat4::IDENTITY);
        } else {
            let held = run
                .weapons
                .iter()
                .find(|w| w.kind != WeaponKind::OrbitDrones)
                .map(|w| w.kind);
            // Face the most recent shot while standing still.
            let yaw = if pl.moving {
                pl.yaw
            } else {
                run.weapons
                    .iter()
                    .filter(|w| w.since_fired < 30)
                    .map(|w| w.aim)
                    .next()
                    .unwrap_or(pl.yaw)
            };
            p.astronaut(pl.pos, 0.0, yaw, pl.walk, pl.moving, flash, held);
            if pl.dash_ticks > 0 {
                let back = math::from_yaw(pl.yaw);
                for i in 1..4 {
                    let q = math::sub(pl.pos, math::scale(back, i as f32 * 0.5));
                    let k = 0.12 - i as f32 * 0.025;
                    p.glow(at(q, 0.9) * s(k, k, k), [0.5, 0.85, 1.0]);
                }
            }
        }
        for (i, d) in run.drone_positions().into_iter().enumerate() {
            let bob = (p.time * 3.0 + i as f32).sin() * 0.15;
            p.rig(ModelId::Drone, at(d, 1.3 + bob), 0.0, |n| {
                if n == "rotor" {
                    Mat4::rotation_y(time * 14.0)
                } else {
                    Mat4::IDENTITY
                }
            });
        }
    }
    projectiles(p, run);
    effects(p, run);
    objective_markers(p, game, run);
    SceneOutput {
        draws: std::mem::take(&mut p.draws),
        lights: std::mem::take(&mut p.lights),
        ambient: palette.ambient,
        clear: [palette.sky[0], palette.sky[1], palette.sky[2], 1.0],
        shadow_focus: [view_center[0], 0.0, view_center[1]],
    }
}

fn terrain(p: &mut Painter, run: &Run, palette: &Palette, center: V2, extra: i32) {
    let (cx, cy) = Terrain::tile_of(center);
    let mining = run.player.mining;
    let mut glows: Vec<(f32, [f32; 3], [f32; 3])> = Vec::new();
    for y in cy - 10 - extra..=cy + 8 + extra {
        for x in cx - 12 - extra..=cx + 12 + extra {
            let tile = run.terrain.tile(x, y);
            let c = Terrain::center_of(x, y);
            let h = tile_hash(x, y);
            match tile {
                Tile::Floor => {
                    let base = if (x + y) % 2 == 0 {
                        palette.floor
                    } else {
                        palette.floor_alt
                    };
                    p.block(
                        at(c, -0.2) * s(1.0, 0.2, 1.0),
                        math_scale3(base, 0.9 + h * 0.2),
                        Surface::Rock,
                        1.0,
                    );
                    // Scattered pebbles and tufts.
                    if h > 0.9 {
                        let off = [
                            (tile_hash(x * 5, y) - 0.5) * 1.4,
                            (tile_hash(x, y * 5) - 0.5) * 1.4,
                        ];
                        let size = 0.12 + h * 0.12;
                        p.cube(
                            at(math::add(c, off), size * 0.5)
                                * Mat4::rotation_y(h * 6.0)
                                * s(size, size * 0.6, size),
                            math_scale3(mix3(palette.rock, palette.rock_top, h), 0.6),
                        );
                    }
                }
                Tile::Bedrock => {
                    p.block(
                        at(c, 1.6) * s(1.0, 1.6, 1.0),
                        palette.bedrock,
                        Surface::Rock,
                        1.0,
                    );
                }
                Tile::Rock { .. } | Tile::Ore { .. } => {
                    let (shake, crack) = match mining {
                        Some(m) if m.tile == (x, y) => (
                            (p.time * 50.0).sin() * 0.03 * (1.0 + m.progress),
                            m.progress,
                        ),
                        _ => (0.0, 0.0),
                    };
                    let tone = 0.85 + h * 0.25 - crack * 0.35;
                    let height = 1.1 + (h - 0.5) * 0.25;
                    let body = at([c[0] + shake, c[1]], height) * s(1.0, height, 1.0);
                    p.block(body, math_scale3(palette.rock, tone), Surface::Rock, 1.0);
                    p.cube(
                        at([c[0] + shake, c[1]], height * 2.0 + 0.06) * s(1.0, 0.07, 1.0),
                        math_scale3(palette.rock_top, 0.9 + h * 0.2),
                    );
                    if let Tile::Ore { material, .. } = tile {
                        let (id, light) = match material {
                            Material::Ferrite => (ModelId::FerriteOre, None),
                            Material::Lumite => (ModelId::LumiteOre, Some([0.3, 0.85, 1.0])),
                            Material::Voidstone => (ModelId::VoidstoneOre, Some([0.65, 0.3, 1.0])),
                        };
                        p.model(id, body);
                        if let Some(color) = light {
                            glows.push((
                                math::distance_sq(c, run.player.pos),
                                [c[0], height * 2.0 + 0.8, c[1]],
                                color,
                            ));
                        }
                    }
                }
            }
        }
    }
    glows.sort_by(|a, b| a.0.total_cmp(&b.0));
    for (_, pos, color) in glows.into_iter().take(7) {
        let pulse = 1.0 + (p.time * 2.0 + pos[0]).sin() * 0.15;
        p.point(pos, color, 1.6 * pulse, 7.0);
    }
}

fn pods(p: &mut Painter, game: &Game, run: &Run) {
    let landing = run.landing;
    let flame = |p: &mut Painter, pos: V2, y: f32, power: f32| {
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
    let pod = |p: &mut Painter, pos: V2, y: f32, door: f32| {
        p.rig(ModelId::DropPod, at(pos, y), 0.0, |n| {
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
                let off = [(k - 0.5) * 8.0, (tile_hash(99, i) - 0.5) * 8.0];
                let y = h + ((p.time * 40.0 + k * 30.0) % 30.0) - 5.0;
                p.ghost(
                    at(math::add(landing, off), y) * s(0.04, 2.0, 0.04),
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
                1.0 - ticks as f32 / EXTRACTION_INBOUND as f32,
                [1.0, 0.55, 0.15],
            );
        }
        Extraction::Waiting { pos, hold } => {
            pod(p, pos, 0.0, 1.3);
            let progress = hold as f32 / EXTRACTION_HOLD as f32;
            zone_ring(p, pos, progress, [0.4, 1.0, 0.6]);
            p.point([pos[0], 3.0, pos[1]], [0.5, 1.0, 0.6], 2.5, 10.0);
        }
        Extraction::Idle | Extraction::Departed { .. } => {}
    }
}

fn zone_ring(p: &mut Painter, pos: V2, progress: f32, color: [f32; 3]) {
    let n = 28;
    for i in 0..n {
        let a = i as f32 / n as f32 * std::f32::consts::TAU + p.time * 0.4;
        let lit = (i as f32 / n as f32) < progress;
        let point = [
            pos[0] + a.sin() * EXTRACTION_RADIUS,
            pos[1] + a.cos() * EXTRACTION_RADIUS,
        ];
        let c = if lit { color } else { math_scale3(color, 0.25) };
        p.glow(
            at(point, 0.08) * Mat4::rotation_y(a) * s(0.32, 0.05, 0.12),
            c,
        );
    }
}

fn caches(p: &mut Painter, run: &Run) {
    for (i, c) in run.caches.iter().enumerate() {
        let yaw = i as f32 * 1.3;
        if c.opened {
            if c.open_ticks < 40 {
                let k = 1.0 - c.open_ticks as f32 / 40.0;
                p.model(
                    ModelId::CrateMedium,
                    at(c.pos, 0.0) * Mat4::rotation_y(yaw) * s(1.8 * k, 1.8 * k, 1.8 * k),
                );
                p.point([c.pos[0], 2.0, c.pos[1]], [1.0, 0.8, 0.3], 4.0 * k, 9.0);
            }
            continue;
        }
        let bob = (p.time * 2.0 + i as f32).sin() * 0.05;
        p.model(
            ModelId::CrateMedium,
            at(c.pos, bob) * Mat4::rotation_y(yaw) * s(1.8, 1.8, 1.8),
        );
        p.ghost(at(c.pos, 4.0) * s(0.05, 4.0, 0.05), [1.0, 0.7, 0.25], 0.3);
        p.point([c.pos[0], 2.2, c.pos[1]], [1.0, 0.7, 0.3], 2.2, 8.0);
    }
}

fn pickups(p: &mut Painter, run: &Run) {
    for (i, pk) in run.pickups.iter().enumerate() {
        let spin = Mat4::rotation_y(p.time * 3.0 + i as f32);
        let bob = (p.time * 4.0 + i as f32 * 0.7).sin() * 0.08;
        let (id, scale) = match pk.kind {
            planetfall_sim::run::PickupKind::Xp(v) => {
                (ModelId::XpShard, 0.8 + (v as f32).sqrt() * 0.2)
            }
            planetfall_sim::run::PickupKind::Material(Material::Ferrite, _) => {
                (ModelId::FerriteChunk, 1.3)
            }
            planetfall_sim::run::PickupKind::Material(Material::Lumite, _) => {
                (ModelId::LumiteChunk, 1.4)
            }
            planetfall_sim::run::PickupKind::Material(Material::Voidstone, _) => {
                (ModelId::VoidstoneChunk, 1.5)
            }
        };
        p.model(
            id,
            at(pk.pos, pk.height + bob) * spin * s(scale, scale, scale),
        );
    }
}

fn enemies(p: &mut Painter, run: &Run) {
    for e in &run.enemies {
        let rise = if e.spawn_ticks > 0 {
            -(e.spawn_ticks as f32 / 24.0) * 2.2
        } else {
            0.0
        };
        let root = at(e.pos, rise) * Mat4::rotation_y(e.yaw);
        let flash = if e.hit_ticks > 0 {
            e.hit_ticks as f32 / 6.0 * 0.9
        } else {
            0.0
        };
        let swing = (e.walk * 2.0).sin() * 0.6;
        let lunge = e.attack_ticks as f32 / 14.0;
        match e.kind {
            EnemyKind::Husk | EnemyKind::Brute => {
                let id = if e.kind == EnemyKind::Husk {
                    ModelId::Husk
                } else {
                    ModelId::Brute
                };
                let sway = (e.walk * 1.0).sin() * 0.08;
                p.rig(id, root * rot_z(sway), flash, |n| match n {
                    "leg_l" => Mat4::rotation_x(swing),
                    "leg_r" => Mat4::rotation_x(-swing),
                    "arm_l" => Mat4::rotation_x(-1.35 - lunge * 0.6 + swing * 0.15),
                    "arm_r" => Mat4::rotation_x(-1.35 - lunge * 0.6 - swing * 0.15),
                    "head" => rot_z(sway * 2.0),
                    "torso" => Mat4::rotation_x(0.12 + lunge * 0.25),
                    _ => Mat4::IDENTITY,
                });
            }
            EnemyKind::Skitter => {
                let gait = e.walk * 3.5;
                p.rig(
                    ModelId::Skitter,
                    root * t(0.0, (gait * 2.0).sin().abs() * 0.05, 0.0),
                    flash,
                    |n| {
                        if let Some(rest) = n.strip_prefix("leg_") {
                            let side = if rest.starts_with('l') { 1.0 } else { -1.0 };
                            let idx: f32 = rest[1..].parse().unwrap_or(0.0);
                            let phase = gait
                                + idx * 2.1
                                + if side > 0.0 {
                                    0.0
                                } else {
                                    std::f32::consts::PI
                                };
                            Mat4::rotation_y(phase.sin() * 0.35)
                                * rot_z(side * phase.cos().max(0.0) * 0.25)
                        } else if n == "head" {
                            Mat4::rotation_x(-lunge * 0.5)
                        } else {
                            Mat4::IDENTITY
                        }
                    },
                );
            }
            EnemyKind::Bloater => {
                let pulse = 1.0 + (p.time * 6.0 + e.id as f32).sin() * 0.05;
                let danger = (1.0 - (math::distance(e.pos, run.player.pos) / 5.0)).clamp(0.0, 1.0);
                p.rig(
                    ModelId::Bloater,
                    root,
                    flash + danger * 0.4 * ((p.time * 14.0).sin() * 0.5 + 0.5),
                    |n| match n {
                        "body" => s(pulse, pulse, pulse),
                        "leg_l" => Mat4::rotation_x(swing),
                        "leg_r" => Mat4::rotation_x(-swing),
                        _ => Mat4::IDENTITY,
                    },
                );
            }
        }
    }
    for c in &run.corpses {
        let k = (c.ticks as f32 / 14.0).min(1.0);
        let sink = ((c.ticks as f32 - 30.0) / 20.0).clamp(0.0, 1.0) * 1.2;
        let root = at(c.pos, -sink) * Mat4::rotation_y(c.yaw) * Mat4::rotation_x(k * 1.5);
        let id = match c.kind {
            EnemyKind::Husk => ModelId::Husk,
            EnemyKind::Brute => ModelId::Brute,
            EnemyKind::Skitter => ModelId::Skitter,
            EnemyKind::Bloater => continue,
        };
        p.rig(id, root, 0.0, |_| Mat4::IDENTITY);
    }
}

fn projectiles(p: &mut Painter, run: &Run) {
    let mut lights = 0;
    for pr in &run.projectiles {
        match pr.kind {
            ProjectileKind::Bolt => {
                let d = pr.direction();
                let yaw = math::yaw_of(d);
                p.glow(
                    at(pr.pos, pr.height) * Mat4::rotation_y(yaw) * s(0.07, 0.07, 0.42),
                    [0.5, 0.95, 1.0],
                );
                if lights < 3 {
                    p.point(
                        [pr.pos[0], pr.height, pr.pos[1]],
                        [0.4, 0.85, 1.0],
                        0.8,
                        4.0,
                    );
                    lights += 1;
                }
            }
            ProjectileKind::Pellet => {
                let yaw = math::yaw_of(pr.direction());
                p.glow(
                    at(pr.pos, pr.height) * Mat4::rotation_y(yaw) * s(0.06, 0.06, 0.18),
                    [1.0, 0.7, 0.25],
                );
            }
            ProjectileKind::Mortar { .. } => {
                let k = 0.28 + (p.time * 20.0).sin() * 0.04;
                p.glow(at(pr.pos, pr.height) * s(k, k, k), [0.55, 1.0, 0.45]);
                p.point([pr.pos[0], pr.height, pr.pos[1]], [0.5, 1.0, 0.4], 1.5, 6.0);
            }
        }
    }
    for b in &run.beams {
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
                    math_scale3([0.6, 0.75, 1.0], 1.5 * fade + 0.5),
                );
                prev = next;
            }
        }
        if let Some(last) = b.points.last() {
            p.point([last[0], 1.2, last[1]], [0.5, 0.65, 1.0], 2.5 * fade, 6.0);
        }
    }
}

fn effects(p: &mut Painter, run: &Run) {
    for b in &run.blasts {
        let t01 = 1.0 - b.ttl as f32 / b.total as f32;
        let (color, alpha) = match b.kind {
            BlastKind::Plasma => ([0.55, 1.0, 0.45], 0.45),
            BlastKind::Bloater => ([0.85, 0.95, 0.3], 0.55),
            BlastKind::PodLanding => ([1.0, 0.75, 0.4], 0.4),
        };
        let _ = alpha;
        let r = b.radius * (0.3 + 0.7 * t01.sqrt());
        let fade = 1.0 - t01;
        // Expanding shard ring plus a collapsing core.
        let n = 18;
        for i in 0..n {
            let a = i as f32 / n as f32 * std::f32::consts::TAU + b.pos[0];
            let q = [b.pos[0] + a.sin() * r, b.pos[1] + a.cos() * r];
            let k = 0.18 * fade + 0.04;
            p.glow(
                at(q, 0.4 + t01 * 0.8) * Mat4::rotation_y(a) * s(k, k, k * 2.5),
                math_scale3(color, 0.6 + fade),
            );
        }
        let core = b.radius * 0.45 * fade;
        if core > 0.02 {
            let shell = p.models.get(ModelId::Planet);
            for mut d in shell.draws(at(b.pos, 0.6) * s(core, core * 0.8, core)) {
                d.material.lit.base.tint = [0.0, 0.0, 0.0, 1.0];
                d.material.lit.emissive = math_scale3(color, 1.2);
                d.material.lit.shading = ShadingModel::Unlit;
                d.material.receives_shadows = false;
                p.draws.push(d);
            }
        }
        p.point([b.pos[0], 1.5, b.pos[1]], color, 6.0 * fade, b.radius * 3.0);
    }
    for sp in &run.sparks {
        let color = match sp.kind {
            SparkKind::Rock => [0.55, 0.5, 0.45],
            SparkKind::Ore(Material::Ferrite) => [1.0, 0.6, 0.3],
            SparkKind::Ore(Material::Lumite) => [0.4, 0.95, 1.0],
            SparkKind::Ore(Material::Voidstone) => [0.75, 0.4, 1.0],
            SparkKind::Blood => [0.35, 0.65, 0.2],
            SparkKind::Energy => [0.6, 0.9, 1.0],
        };
        let k = 0.05 + (sp.ttl as f32 / 45.0) * 0.05;
        if matches!(sp.kind, SparkKind::Rock | SparkKind::Blood) {
            p.cube(t(sp.pos[0], sp.pos[1], sp.pos[2]) * s(k, k, k), color);
        } else {
            p.glow(t(sp.pos[0], sp.pos[1], sp.pos[2]) * s(k, k, k), color);
        }
    }
}

fn objective_markers(p: &mut Painter, game: &Game, run: &Run) {
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

/// Markers for ship tutorial steps (drawn in ship space).
pub fn ship_markers(p: &mut Painter, game: &Game) {
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
