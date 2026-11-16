//! Turns game state into lit surface draws and lights. Pure presentation:
//! reads the simulation, never changes it.

use super::geometry::{Textures3D, tile_hash};
use super::models::{ModelId, Models};
use super::skins::{Layer, Posed, SkinRequest, Skins};
use super::voxels::{self, VoxelScene, WorldKey};
use crate::render::camera::{ascent_height, descent_height};
use planetfall_sim::content::ShipBlock;
use planetfall_sim::content::{EnemyKind, Material, Palette, PlanetId, ShipUpgrade, WeaponKind};
use planetfall_sim::game::{FTL_TICKS, LOST_TICKS};
use planetfall_sim::math::{self, V2};
use planetfall_sim::run::{
    BlastKind, EXTRACTION_HOLD, EXTRACTION_INBOUND, EXTRACTION_RADIUS, Extraction, ProjectileKind,
    Run, SparkKind,
};
use planetfall_sim::ship::{BUILD_AREA, Room, ShipLayout, StationKind};
use planetfall_sim::terrain::{TILE, Terrain, Tile};
use planetfall_sim::voxel;
use planetfall_sim::{Game, Phase};
use quasar_render_3d::{
    Attenuation, BlendMode, BloomSettings, ColorGrade, Light, LitMaterial, Mat4, Mesh, NormalMap,
    PostSettings, ShadingModel, SurfaceDraw3D, SurfaceMaterial, TransparentDraw3D,
    VignetteSettings,
};
use quasar_render_voxel::{Fog, OverlayBox};

/// Everything the 3D passes need for one frame.
pub struct SceneOutput<'a> {
    pub draws: Vec<SurfaceDraw3D<'a>>,
    /// Blended draws, sorted back to front by the renderer.
    pub transparent: Vec<TransparentDraw3D<'a>>,
    pub lights: Vec<Light>,
    pub ambient: [f32; 3],
    pub clear: [f32; 4],
    /// Center of the directional shadow volume.
    pub shadow_focus: [f32; 3],
    pub post: PostSettings,
    /// Characters to pose on the GPU (drawn after skinning).
    pub skinned: Vec<SkinRequest>,
    /// Terrain or hull blocks to draw, if the scene has any.
    pub voxels: Option<VoxelScene>,
}

#[derive(Clone, Copy)]
enum Surface {
    Plain,
    Metal,
}

pub struct Painter<'a, 's> {
    cube: &'a Mesh,
    sphere: &'a Mesh,
    tex: &'a Textures3D,
    models: &'a Models,
    skins: &'s Skins,
    pub draws: Vec<SurfaceDraw3D<'a>>,
    pub transparent: Vec<TransparentDraw3D<'a>>,
    /// Characters to pose on the GPU this frame.
    pub skinned: Vec<SkinRequest>,
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

impl<'a, 's> Painter<'a, 's> {
    pub fn new(
        cube: &'a Mesh,
        sphere: &'a Mesh,
        tex: &'a Textures3D,
        models: &'a Models,
        skins: &'s Skins,
        time: f32,
    ) -> Self {
        Self {
            cube,
            sphere,
            tex,
            models,
            skins,
            skinned: Vec::with_capacity(128),
            draws: Vec::with_capacity(2048),
            transparent: Vec::with_capacity(512),
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

    /// Unlit emissive cube (projectiles, sparks, lights). Emission above 1.0
    /// blooms in the post chain.
    fn glow(&mut self, model: Mat4, color: [f32; 3]) {
        let mut lit = LitMaterial::textured(&self.tex.white);
        lit.base.tint = [0.0, 0.0, 0.0, 1.0];
        lit.emissive = math_scale3(color, GLOW);
        lit.shading = ShadingModel::Unlit;
        let mut m = SurfaceMaterial::from(lit);
        m.receives_shadows = false;
        self.draws.push(SurfaceDraw3D {
            mesh: self.cube,
            model,
            material: m,
        });
    }

    /// Additive light volume: flames, beams, holograms, light shafts.
    fn ghost(&mut self, model: Mat4, color: [f32; 3], alpha: f32) {
        self.blend(self.cube, model, color, alpha, BlendMode::Additive);
    }

    /// Additive sphere: blast cores and glow halos.
    fn halo(&mut self, model: Mat4, color: [f32; 3], alpha: f32) {
        self.blend(self.sphere, model, color, alpha, BlendMode::Additive);
    }

    /// See-through tinted surface (glass), alpha-blended over the scene.
    fn glass(&mut self, model: Mat4, color: [f32; 3], alpha: f32) {
        let mut lit = LitMaterial::textured(&self.tex.white);
        lit.base.tint = [color[0], color[1], color[2], alpha];
        lit.specular = [0.6; 3];
        lit.shininess = 96.0;
        self.transparent.push(TransparentDraw3D::new(
            self.cube,
            model,
            lit,
            BlendMode::Alpha,
        ));
    }

    fn blend(&mut self, mesh: &'a Mesh, model: Mat4, color: [f32; 3], alpha: f32, mode: BlendMode) {
        let mut lit = LitMaterial::textured(&self.tex.white);
        lit.base.tint = [0.0, 0.0, 0.0, alpha.clamp(0.0, 1.0)];
        lit.emissive = math_scale3(color, GLOW);
        lit.shading = ShadingModel::Unlit;
        self.transparent
            .push(TransparentDraw3D::new(mesh, model, lit, mode));
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

    /// Pose a skinned character from clip layers and queue it for GPU
    /// skinning. Returns the pose for attachments.
    fn character(&mut self, id: ModelId, root: Mat4, layers: &[Layer], flash: f32) -> Posed {
        let posed = self.skins.character(id).pose(layers);
        self.skinned.push(SkinRequest {
            id,
            palette: posed.palette.clone(),
            root,
            flash,
        });
        posed
    }

    /// Draw a prop with named parts: `pose(node)` rotates each part about its pivot.
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
        let model = t(mid[0], mid[1], mid[2]) * Mat4::rotation_y(yaw) * Mat4::rotation_x(pitch);
        self.glow(model * s(width * 0.45, width * 0.45, len * 0.5), color);
        self.ghost(model * s(width * 2.2, width * 2.2, len * 0.5), color, 0.35);
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
        let root = at(pos, y) * Mat4::rotation_y(yaw);
        let mut layers = vec![if moving {
            Layer::full("walk", walk)
        } else {
            Layer::full("idle", self.time)
        }];
        if weapon.is_some() {
            layers.push(Layer::full("aim", 0.0));
        }
        let posed = self.character(ModelId::Astronaut, root, &layers, flash);
        if let Some(weapon) = weapon {
            let (id, scale) = match weapon {
                WeaponKind::PulseBlaster | WeaponKind::OrbitDrones => (ModelId::Blaster, 0.7),
                WeaponKind::Scattergun => (ModelId::Scattergun, 0.7),
                WeaponKind::ArcCaster => (ModelId::ArcCaster, 1.0),
                WeaponKind::PlasmaMortar => (ModelId::PlasmaMortar, 1.0),
            };
            // Held in the right hand: follow the arm joint, then level the gun.
            let arm = posed.bone("arm_r").unwrap_or(Mat4::IDENTITY);
            let gun =
                root * arm * t(0.0, -0.62, 0.05) * Mat4::rotation_x(1.2) * s(scale, scale, scale);
            let flip = if matches!(id, ModelId::Blaster | ModelId::Scattergun) {
                Mat4::rotation_y(std::f32::consts::PI)
            } else {
                Mat4::IDENTITY
            };
            self.model(id, gun * flip);
        }
        // Suit lamp so the player always reads in dark caves.
        self.point(
            [pos[0], y + 3.6, pos[1] + 0.6],
            [0.85, 0.92, 1.0],
            1.15,
            10.0,
        );
    }
}

/// HDR boost for emissive effects so the post chain blooms them.
const GLOW: f32 = 2.2;

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

/// Bloom on emissive effects, filmic tonemapping and a soft vignette.
fn base_post() -> PostSettings {
    PostSettings {
        exposure: 0.85,
        bloom: BloomSettings {
            threshold: 1.25,
            knee: 0.4,
            intensity: 0.4,
            levels: 5,
            ..BloomSettings::default()
        },
        vignette: VignetteSettings {
            strength: 0.28,
            ..VignetteSettings::default()
        },
        grade: ColorGrade {
            saturation: 1.08,
            contrast: 1.06,
            ..ColorGrade::default()
        },
        ..PostSettings::default()
    }
}

/// Phase-dependent grading: danger reddens and darkens the edges; losing the
/// signal drains the colour.
fn post_settings(game: &Game, time: f32) -> PostSettings {
    let mut post = base_post();
    let Some(run) = &game.run else {
        return post;
    };
    match game.phase {
        Phase::Surface => {
            let p = &run.player;
            let danger = (1.0 - p.health / p.max_health).max(0.0);
            let hurt = p.hurt_ticks as f32 / 18.0;
            let pulse = 0.8 + 0.2 * (time * 6.0).sin();
            let edge = ((danger * danger * 0.5 + hurt * 0.4) * pulse).min(0.75);
            post.vignette.strength = (post.vignette.strength + edge).min(1.0);
            post.grade.gain = [1.0 + edge * 0.6, 1.0 - edge * 0.45, 1.0 - edge * 0.45];
        }
        Phase::Lost { ticks } => {
            let t = 1.0 - ticks as f32 / LOST_TICKS as f32;
            post.grade.saturation = 1.0 - t * 0.8;
            post.vignette.strength = 0.5 + t * 0.4;
        }
        _ => {}
    }
    post
}

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
            let base = V2::new((v.x as f32 + 0.5) * TILE, (v.z as f32 + 0.5) * TILE);
            let y = v.y as f32 * TILE + height + drop;
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
                let c = V2::new((x as f32 + 0.5) * TILE, (z as f32 + 0.5) * TILE);
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
        overlays.push(voxels::block_box(
            x,
            layer,
            z,
            0.02,
            [color[0], color[1], color[2], 1.0],
        ));
        let c = V2::new((x as f32 + 0.5) * TILE, (z as f32 + 0.5) * TILE);
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
        ModelId::CrateWide,
        at(V2::new(5.6 * TILE, 14.6 * TILE), 0.0) * s(2.2, 2.2, 2.2),
    );
    p.model(
        ModelId::Barrel,
        at(V2::new(9.4 * TILE, 14.7 * TILE), 0.0) * s(2.2, 2.2, 2.2),
    );
    p.model(
        ModelId::Barrel,
        at(V2::new(9.0 * TILE, 15.2 * TILE), 0.0) * s(2.2, 2.2, 2.2),
    );
    p.model(
        ModelId::CrateMedium,
        at(V2::new(5.5 * TILE, 1.6 * TILE), 0.0) * Mat4::rotation_y(0.4) * s(2.0, 2.0, 2.0),
    );
    let pl = &ship.player;
    if !matches!(game.phase, Phase::Descent { .. }) {
        p.astronaut(pl.pos, 0.0, pl.yaw, pl.walk, pl.moving, 0.0, None);
    }
    ship_markers(p, game);
    space_backdrop(p, game.profile.orbiting, V2::new(7.5 * TILE, 8.5 * TILE));
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

fn space_backdrop(p: &mut Painter<'_, '_>, orbiting: Option<PlanetId>, center: V2) {
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
            p.character(ModelId::Astronaut, root, &[Layer::full("idle", 0.0)], 0.0);
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
                    let q = pl.pos - (back * i as f32 * 0.5);
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
                color: palette.sky.map(super::srgb_to_linear),
                start: 38.0,
                end: 95.0,
            }),
        }),
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
                        Material::Ferrite => (ModelId::FerriteOre, None),
                        Material::Lumite => (ModelId::LumiteOre, Some([0.3, 0.85, 1.0])),
                        Material::Voidstone => (ModelId::VoidstoneOre, Some([0.65, 0.3, 1.0])),
                    };
                    // Crystals erupt from the ore block (the [-1, 1] cube).
                    p.model(id, at(c, TILE * 0.5) * s(1.02, 1.02, 1.02));
                    if let Some(color) = light {
                        glows.push((
                            c.distance_squared(run.player.pos),
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
    for (_, pos, color) in glows.into_iter().take(7) {
        let pulse = 1.0 + (p.time * 2.0 + pos[0]).sin() * 0.15;
        p.point(pos, color, 1.6 * pulse, 7.0);
    }
    // The block being drilled: an outline that heats up with progress.
    run.player.mining.map(|m| {
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

fn zone_ring(p: &mut Painter<'_, '_>, pos: V2, progress: f32, color: [f32; 3]) {
    let n = 28;
    for i in 0..n {
        let a = i as f32 / n as f32 * std::f32::consts::TAU + p.time * 0.4;
        let lit = (i as f32 / n as f32) < progress;
        let point = pos + V2::new(a.sin(), a.cos()) * EXTRACTION_RADIUS;
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

fn pickups(p: &mut Painter<'_, '_>, run: &Run) {
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

fn enemies(p: &mut Painter<'_, '_>, run: &Run) {
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
        let lunge = e.attack_ticks as f32 / 14.0;
        let mut layers = vec![Layer::full("walk", e.walk)];
        let id = match e.kind {
            EnemyKind::Husk => ModelId::Husk,
            EnemyKind::Brute => ModelId::Brute,
            EnemyKind::Skitter => ModelId::Skitter,
            EnemyKind::Bloater => ModelId::Bloater,
        };
        let mut glow = flash;
        if e.kind == EnemyKind::Bloater {
            layers.push(Layer::full("pulse", p.time + e.id as f32 * 0.37));
            let danger = (1.0 - (e.pos.distance(run.player.pos) / 5.0)).clamp(0.0, 1.0);
            glow += danger * 0.4 * ((p.time * 14.0).sin() * 0.5 + 0.5);
        } else if lunge > 0.0 {
            layers.push(Layer {
                clip: "attack",
                time: 0.0,
                weight: lunge,
            });
        }
        p.character(id, root, &layers, glow);
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
        p.character(id, root, &[Layer::full("idle", 0.0)], 0.0);
    }
}

fn projectiles(p: &mut Painter<'_, '_>, run: &Run) {
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

fn effects(p: &mut Painter<'_, '_>, run: &Run) {
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

/// Markers for ship tutorial steps (drawn in ship space).
pub fn ship_markers(p: &mut Painter<'_, '_>, game: &Game) {
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
