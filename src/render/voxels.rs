//! Planet terrain and ship hull as voxels: block catalog and procedural
//! texture layers, chunk meshing (`quasar-voxel-mesh`) and drawing
//! (`quasar-render-voxel`), lit by the same lights and shadow map as meshes.
//!
//! The simulation owns the block volumes (`planetfall_sim::voxel`). Each
//! frame [`VoxelWorld::sync`] re-meshes chunks whose revision changed (mined
//! rock, placed ship blocks) and swaps the whole grid when the scene changes
//! between the ship and a planet.

use crate::render::sculpt::Carve;
use planetfall_sim::content::{Palette, PlanetId};
use planetfall_sim::voxel::{self, AIR, Blocks};
use quasar_frame::{Gpu, GraphicsResult, RenderTarget, error};
use quasar_math::{DVec3, IVec3, Mat4, Vec3};
use quasar_render_3d::{Lighting, ShadingModel, ShadowBinding};
use quasar_render_voxel::{
    ChunkKey, ChunkPlacement, Fog, MaterialDef, MaterialFilter, OverlayBox, PreparedVoxels,
    VoxelCamera, VoxelMaterials, VoxelPassOptions, VoxelRenderer, VoxelRendererConfig,
    VoxelShading,
};
use quasar_voxel::{BlockCatalog, BlockDef, GridId, GridTransform, Layer, Snapshot};
use quasar_voxel_mesh::{
    CatalogFaces, CatalogLayers, CatalogModels, CulledInput, CulledModelExtractor, MeshBuffers,
    ModelLibrary, VertexShade, opaque_table,
};
use std::collections::BTreeMap;
use vulkanic_gal::TextureView;

pub const GRID: GridId = GridId(0);
/// Chunk edge in voxels (`planetfall_sim::voxel::Layout` is `Cube16`).
const EDGE: i32 = 16;
/// Texture tile edge in texels.
const TEXELS: u32 = 32;

/// Texture layers, one per face look.
mod tex {
    pub const GROUND: u32 = 0;
    pub const GROUND_ALT: u32 = 1;
    pub const ROCK: u32 = 2;
    pub const ROCK_TOP: u32 = 3;
    pub const FERRITE: u32 = 4;
    pub const LUMITE: u32 = 5;
    pub const VOIDSTONE: u32 = 6;
    pub const BEDROCK: u32 = 7;
    pub const DECK: u32 = 8;
    pub const HULL: u32 = 9;
    pub const HULL_TOP: u32 = 10;
    pub const WINDOW: u32 = 11;
    pub const LAMP: u32 = 12;
    pub const KEEL: u32 = 13;
    pub const PATCH: u32 = 14;
    pub const LAVA: u32 = 15;
    pub const WATER: u32 = 16;
    pub const COUNT: u32 = 17;
}

/// Renderer-only block IDs after the simulation's (ship detail pieces).
const HULL_LOW: u16 = voxel::BLOCK_COUNT;
const HULL_CAP: u16 = voxel::BLOCK_COUNT + 1;
const WINDOW_LOW: u16 = voxel::BLOCK_COUNT + 2;
const WINDOW_CAP: u16 = voxel::BLOCK_COUNT + 3;
/// Four lamp quarters, `x | z << 1`.
const LAMP_QUARTER: u16 = voxel::BLOCK_COUNT + 4;

/// The ship is laid out (and built) in 2 m tiles but drawn with the same
/// 1 m blocks as the planets: every ship voxel becomes 2×2×2 fine ones.
fn refine_ship(coarse: &Blocks) -> Blocks {
    let mut fine = Blocks::default();
    for chunk in coarse.volume.chunk_coords() {
        for y in 0..EDGE {
            for z in 0..EDGE {
                for x in 0..EDGE {
                    let v = chunk * EDGE + IVec3::new(x, y, z);
                    let block = coarse.get(v);
                    if block == AIR {
                        continue;
                    }
                    for k in 0..8 {
                        let d = IVec3::new(k & 1, k >> 1 & 1, k >> 2 & 1);
                        let part = match (block, d.y) {
                            (voxel::HULL, 0) => HULL_LOW,
                            (voxel::HULL, _) => HULL_CAP,
                            (voxel::WINDOW, 0) => WINDOW_LOW,
                            (voxel::WINDOW, _) => WINDOW_CAP,
                            (voxel::LAMP, 0) => LAMP_QUARTER + (d.x | d.z << 1) as u16,
                            (voxel::LAMP, _) => AIR,
                            (b, _) => b,
                        };
                        if part != AIR {
                            fine.set(v * 2 + d, part);
                        }
                    }
                }
            }
        }
    }
    fine
}

/// Which world the grid currently holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorldKey {
    Ship,
    /// A planet surface; `drop` distinguishes successive runs.
    Planet {
        planet: PlanetId,
        drop: u32,
    },
}

impl WorldKey {
    /// Meters per drawn voxel (the ship is refined to planet-sized blocks).
    fn scale(self) -> f64 {
        match self {
            Self::Ship => f64::from(planetfall_sim::ship::TILE) / 2.0,
            Self::Planet { .. } => f64::from(planetfall_sim::terrain::TILE),
        }
    }
}

/// Block definitions and the per-block tables meshing needs.
struct Content {
    catalog: BlockCatalog,
    /// How planet blocks are sculpted, per block ID.
    carve: Vec<Carve>,
    library: ModelLibrary,
    opaque: Vec<bool>,
}

impl Content {
    fn new() -> Self {
        let mut catalog = BlockCatalog::new();
        let library = ModelLibrary::new();
        let sides = |side: u32, top: u32| [side, side, side, top, side, side];
        // Cut-away hull: walls stop at 0.6 of a block so the top-down camera
        // sees into rooms. Quasar derives the face shapes from the height,
        // so equal walls cull each other and nothing else.
        const WALL_HEIGHT: f32 = 0.6;
        let defs = [
            (voxel::GROUND, BlockDef::cube("ground", tex::GROUND)),
            (
                voxel::GROUND_ALT,
                BlockDef::cube("ground-alt", tex::GROUND_ALT),
            ),
            (
                voxel::ROCK,
                BlockDef::cube("rock", tex::ROCK).with_materials(sides(tex::ROCK, tex::ROCK_TOP)),
            ),
            (
                voxel::FERRITE_ORE,
                BlockDef::cube("ferrite", tex::FERRITE)
                    .with_materials(sides(tex::FERRITE, tex::ROCK_TOP)),
            ),
            (
                voxel::LUMITE_ORE,
                BlockDef::cube("lumite", tex::LUMITE)
                    .with_materials(sides(tex::LUMITE, tex::ROCK_TOP)),
            ),
            (
                voxel::VOIDSTONE_ORE,
                BlockDef::cube("voidstone", tex::VOIDSTONE)
                    .with_materials(sides(tex::VOIDSTONE, tex::ROCK_TOP)),
            ),
            (voxel::BEDROCK, BlockDef::cube("bedrock", tex::BEDROCK)),
            (voxel::DECK, BlockDef::cube("deck", tex::DECK)),
            (
                voxel::HULL,
                BlockDef::cube("hull", tex::HULL)
                    .with_materials(sides(tex::HULL, tex::HULL_TOP))
                    .with_height(WALL_HEIGHT),
            ),
            (
                voxel::WINDOW,
                BlockDef::cube("window", tex::WINDOW)
                    .with_height(WALL_HEIGHT)
                    .with_layer(Layer::Translucent),
            ),
            (
                voxel::LAMP,
                BlockDef::partial("lamp", tex::LAMP, [0.3, 0.0, 0.3], [0.7, 0.45, 0.7])
                    .with_emission(14),
            ),
            (voxel::KEEL, BlockDef::cube("keel", tex::KEEL)),
            (
                voxel::GROUND_PATCH,
                BlockDef::cube("ground-patch", tex::PATCH),
            ),
            (
                voxel::LAVA,
                BlockDef::cube("lava", tex::LAVA).with_emission(13),
            ),
            (
                voxel::WATER,
                BlockDef::cube("water", tex::WATER).with_emission(4),
            ),
        ];
        for (id, def) in defs {
            let added = catalog.add(def);
            debug_assert_eq!(added, id, "catalog order matches planetfall_sim::voxel");
        }
        debug_assert_eq!(catalog.defs().len(), voxel::BLOCK_COUNT as usize);
        // Renderer-only pieces for drawing 2 m ship blocks out of 1 m ones
        // (see `refine_ship`): a full-height wall course under a short cap
        // (walls stay 1.2 m), and lamp quarters around the tile centre.
        let ship_parts = [
            (
                HULL_LOW,
                BlockDef::cube("hull-low", tex::HULL)
                    .with_materials(sides(tex::HULL, tex::HULL_TOP)),
            ),
            (
                HULL_CAP,
                BlockDef::cube("hull-cap", tex::HULL)
                    .with_materials(sides(tex::HULL, tex::HULL_TOP))
                    .with_height(2.0 * WALL_HEIGHT - 1.0),
            ),
            (
                WINDOW_LOW,
                BlockDef::cube("window-low", tex::WINDOW).with_layer(Layer::Translucent),
            ),
            (
                WINDOW_CAP,
                BlockDef::cube("window-cap", tex::WINDOW)
                    .with_height(2.0 * WALL_HEIGHT - 1.0)
                    .with_layer(Layer::Translucent),
            ),
        ];
        for (id, def) in ship_parts {
            let added = catalog.add(def);
            debug_assert_eq!(added, id);
        }
        for q in 0..4u16 {
            let (qx, qz) = (f32::from(q & 1), f32::from(q >> 1));
            // The coarse lamp spans 0.3..0.7 of the tile: 0.6..1 of the low
            // fine cell and 0..0.4 of the high one, 0.9 m tall.
            let lo = |high: f32| if high > 0.5 { 0.0 } else { 0.6 };
            let hi = |high: f32| if high > 0.5 { 0.4 } else { 1.0 };
            let added = catalog.add(
                BlockDef::partial(
                    "lamp-quarter",
                    tex::LAMP,
                    [lo(qx), 0.0, lo(qz)],
                    [hi(qx), 0.9, hi(qz)],
                )
                .with_emission(14),
            );
            debug_assert_eq!(added, LAMP_QUARTER + q);
        }
        let opaque = opaque_table(&catalog);
        let carve = (0..catalog.defs().len() as u16)
            .map(|id| match id {
                voxel::ROCK
                | voxel::FERRITE_ORE
                | voxel::LUMITE_ORE
                | voxel::VOIDSTONE_ORE
                | voxel::BEDROCK => Carve::Stone,
                _ => Carve::Flat,
            })
            .collect();
        Self {
            catalog,
            carve,
            library,
            opaque,
        }
    }

    fn mesh(&self, blocks: &Blocks, chunk: IVec3, sculpted: bool) -> MeshBuffers {
        let mut snapshot = Snapshot::new();
        blocks
            .volume
            .snapshot(blocks.block, chunk, 1, &mut snapshot);
        if sculpted {
            return crate::render::sculpt::Sculptor {
                catalog: &self.catalog,
                carve: &self.carve,
            }
            .mesh(&snapshot, chunk);
        }
        let mut out = MeshBuffers::new();
        CulledModelExtractor::default().extract(
            CulledInput {
                blocks: &snapshot,
                orientation: None,
                block_light: None,
                sky_light: None,
                opaque: &self.opaque,
            },
            &CatalogModels {
                catalog: &self.catalog,
                library: &self.library,
            },
            &CatalogFaces(&self.catalog),
            &VertexShade::default(),
            &CatalogLayers(&self.catalog),
            |_| true,
            &mut out,
        );
        out
    }
}

fn hash(x: u32, y: u32, salt: u32) -> f32 {
    let mut h =
        x.wrapping_mul(0x9e37_79b1) ^ y.wrapping_mul(0x85eb_ca77) ^ salt.wrapping_mul(0xc2b2_ae3d);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2c1b_3c6d);
    h ^= h >> 12;
    (h & 0xffff) as f32 / 65535.0
}

fn encode(v: f32) -> u8 {
    let v = v.clamp(0.0, 1.0);
    let s = if v <= 0.003_130_8 {
        v * 12.92
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    };
    (s * 255.0).round() as u8
}

/// Procedural sRGB RGBA8 layers in `tex` order. Planet layers follow the
/// planet's palette; ship layers are fixed.
fn texture_layers(palette: &Palette) -> Vec<Vec<u8>> {
    let mix = |a: [f32; 3], b: [f32; 3], t: f32| -> [f32; 3] {
        std::array::from_fn(|i| a[i] + (b[i] - a[i]) * t)
    };
    (0..tex::COUNT)
        .map(|layer| {
            let mut pixels = Vec::with_capacity((TEXELS * TEXELS * 4) as usize);
            for y in 0..TEXELS {
                for x in 0..TEXELS {
                    let n = hash(x, y, layer);
                    let grain = hash(x / 4, y / 4, layer + 31);
                    let edge = x == 0 || y == 0 || x == TEXELS - 1 || y == TEXELS - 1;
                    let shade = |c: [f32; 3], amount: f32| {
                        c.map(|v| v * (1.0 - amount + 2.0 * amount * (0.6 * n + 0.4 * grain)))
                    };
                    // Stone sides: sediment strata, hairline cracks and a
                    // ragged lip of the top growth spilling over the edge.
                    let strata = |c: [f32; 3]| {
                        let band =
                            hash(0, y / 3, layer + 41) * 0.6 + hash(0, y / 7, layer + 43) * 0.4;
                        let wave = ((x as f32 * 0.35 + band * 6.0).sin() * 0.5 + 0.5) * 0.04;
                        c.map(|v| v * (0.84 + 0.24 * band + wave))
                    };
                    let crack = (0..3u32).any(|k| {
                        let x0 = hash(k, 1, layer + 51) * TEXELS as f32;
                        let y0 = hash(k, 2, layer + 51) * TEXELS as f32 * 0.6;
                        let len = 8.0 + hash(k, 3, layer + 51) * 14.0;
                        let fy = y as f32;
                        let drift = (hash(k, y / 4, layer + 53) - 0.5) * 5.0
                            + (hash(k, y / 4 + 1, layer + 53) - 0.5) * 5.0 * ((y % 4) as f32 / 4.0);
                        fy >= y0 && fy <= y0 + len && (x as f32 - x0 - drift).abs() < 0.7
                    });
                    let lip = y < 3 + (hash(x / 2, 0, layer + 61) * 4.0) as u32;
                    let side = |c: [f32; 3]| {
                        if lip {
                            shade(palette.rock_top, 0.14)
                        } else if crack {
                            strata(c).map(|v| v * 0.55)
                        } else {
                            strata(shade(c, 0.12))
                        }
                    };
                    let ore = |vein: [f32; 3]| {
                        // Crystal clusters with a bright facet on each.
                        let cluster = hash(x / 5, y / 5, layer + 7);
                        let inside = cluster > 0.55 && hash(x / 2, y / 2, layer + 9) > 0.35;
                        if lip {
                            shade(palette.rock_top, 0.14)
                        } else if inside && (x + y) % 5 == 0 {
                            vein.map(|v| (v * 1.35).min(1.0))
                        } else if inside {
                            shade(vein, 0.18)
                        } else {
                            side(palette.rock)
                        }
                    };
                    // Tops: the planet's growth in clumps over bare stone,
                    // with pebbles.
                    let top = || {
                        let clump = hash(x / 4, y / 4, layer + 71) * 0.6
                            + hash(x / 2, y / 2, layer + 73) * 0.4;
                        if clump < 0.22 {
                            shade(palette.rock, 0.16)
                        } else if n > 0.93 {
                            palette.rock_top.map(|v| v * 0.7)
                        } else {
                            shade(palette.rock_top, 0.16)
                        }
                    };
                    let (rgb, alpha) = match layer {
                        tex::GROUND => (shade(palette.floor, 0.12), 1.0),
                        tex::GROUND_ALT => (shade(palette.floor_alt, 0.12), 1.0),
                        tex::ROCK => (side(palette.rock), 1.0),
                        tex::ROCK_TOP => (top(), 1.0),
                        tex::FERRITE => (ore([0.72, 0.42, 0.22]), 1.0),
                        tex::LUMITE => (ore([0.35, 0.95, 1.0]), 1.0),
                        tex::VOIDSTONE => (ore([0.55, 0.25, 0.85]), 1.0),
                        tex::BEDROCK => (strata(shade(palette.bedrock, 0.2)), 1.0),
                        tex::LAVA => {
                            // Glowing cracks between dark crusted plates.
                            let crack = hash(x / 2, y / 2, layer + 3) > 0.55 || n > 0.8;
                            if crack {
                                (mix([1.0, 0.35, 0.05], [1.0, 0.75, 0.2], n), 1.0)
                            } else {
                                (shade([0.25, 0.06, 0.02], 0.3), 1.0)
                            }
                        }
                        tex::WATER => {
                            // Deep blue with bright ripple lines.
                            let ripple = (x + y * 3) % 8 < 1 && grain > 0.5;
                            if ripple {
                                ([0.14, 0.3, 0.5], 1.0)
                            } else {
                                (mix([0.03, 0.08, 0.22], [0.06, 0.16, 0.36], n), 1.0)
                            }
                        }
                        tex::PATCH => {
                            // Clumped growth over the floor colour.
                            let clump = hash(x / 2, y / 2, layer + 11);
                            if clump > 0.3 {
                                (shade(palette.patch, 0.22), 1.0)
                            } else {
                                (shade(palette.floor, 0.12), 1.0)
                            }
                        }
                        tex::DECK => {
                            let seam = edge;
                            let rivet = (x == 2 || x == TEXELS - 3) && (y == 2 || y == TEXELS - 3);
                            let base = [0.20, 0.22, 0.25];
                            if seam {
                                ([0.08, 0.09, 0.1], 1.0)
                            } else if rivet {
                                ([0.4, 0.42, 0.45], 1.0)
                            } else {
                                (shade(base, 0.06), 1.0)
                            }
                        }
                        tex::HULL => {
                            if (4..6).contains(&y) {
                                ([0.66, 0.33, 0.08], 1.0)
                            } else {
                                (shade([0.30, 0.32, 0.36], 0.08), 1.0)
                            }
                        }
                        tex::HULL_TOP => ([0.14, 0.15, 0.17], 1.0),
                        tex::WINDOW => {
                            if edge {
                                ([0.3, 0.32, 0.36], 1.0)
                            } else {
                                (mix([0.25, 0.5, 0.75], [0.6, 0.85, 1.0], n * 0.3), 0.3)
                            }
                        }
                        tex::LAMP => {
                            if edge {
                                ([0.3, 0.3, 0.32], 1.0)
                            } else {
                                ([1.0, 0.86, 0.6], 1.0)
                            }
                        }
                        _ => (shade([0.18, 0.19, 0.22], 0.1), 1.0),
                    };
                    // Rock blocks get chipped, darkened rims so each one
                    // reads as a block even where the tops run together.
                    let rocky = matches!(
                        layer,
                        tex::ROCK
                            | tex::ROCK_TOP
                            | tex::FERRITE
                            | tex::LUMITE
                            | tex::VOIDSTONE
                            | tex::BEDROCK
                    );
                    let rim = x.min(y).min(TEXELS - 1 - x).min(TEXELS - 1 - y);
                    let chip = hash(x, y, layer + 91);
                    let rgb = if rocky && (rim == 0 && chip > 0.1 || rim == 1 && chip > 0.75) {
                        rgb.map(|v| v * if rim == 0 { 0.78 } else { 0.9 })
                    } else if rocky && rim == 1 && (x == 1 || y == 1) {
                        // A faint lit lip on the upper-left edges.
                        rgb.map(|v| (v * 1.12).min(1.0))
                    } else {
                        rgb
                    };
                    pixels.extend([
                        encode(rgb[0]),
                        encode(rgb[1]),
                        encode(rgb[2]),
                        (alpha * 255.0) as u8,
                    ]);
                }
            }
            pixels
        })
        .collect()
}

fn materials(gal: &Gpu, palette: &Palette) -> GraphicsResult<VoxelMaterials> {
    let defs: Vec<MaterialDef> = (0..tex::COUNT)
        .map(|layer| match layer {
            tex::LUMITE => MaterialDef::layer(layer).with_emissive([0.05, 0.2, 0.25]),
            tex::VOIDSTONE => MaterialDef::layer(layer).with_emissive([0.12, 0.04, 0.2]),
            tex::LAMP => MaterialDef::layer(layer).with_emissive([2.4, 2.0, 1.4]),
            _ => MaterialDef::layer(layer),
        })
        .collect();
    VoxelMaterials::new(
        gal,
        TEXELS,
        &texture_layers(palette),
        &defs,
        MaterialFilter::Nearest,
    )
}

/// The voxel renderer plus what is uploaded to it.
pub struct VoxelWorld {
    renderer: Option<VoxelRenderer>,
    content: Content,
    /// The world on the grid and the palette its materials were built with.
    key: Option<WorldKey>,
    palette: Option<PlanetId>,
    /// Revision each uploaded chunk was meshed at.
    uploaded: BTreeMap<(i32, i32, i32), u64>,
    /// The ship in 1 m blocks and the ship revision it was refined at.
    ship: Option<(u64, Blocks)>,
    pending_submissions: Vec<u64>,
}

/// What the painter wants from the voxel pass this frame.
pub struct VoxelScene {
    pub key: WorldKey,
    /// The planet whose palette textures the blocks (`None` keeps the
    /// current one; the ship's own blocks never change).
    pub palette: Option<(PlanetId, Palette)>,
    pub overlays: Vec<OverlayBox>,
    pub fog: Option<Fog>,
}

/// Overlay boxes and pass flags for one voxel draw.
pub struct VoxelPass<'a> {
    pub camera: &'a VoxelCamera,
    pub lighting: Lighting<'a>,
    pub shadow: Option<&'a ShadowBinding>,
    pub fog: Option<Fog>,
    pub clear: Option<[f32; 4]>,
    pub overlays: Vec<OverlayBox>,
}

impl VoxelWorld {
    pub fn new() -> Self {
        Self {
            renderer: None,
            content: Content::new(),
            key: None,
            palette: None,
            uploaded: BTreeMap::new(),
            ship: None,
            pending_submissions: Vec::new(),
        }
    }

    /// Forget the uploaded world. Call when the game is replaced (new game or
    /// a loaded slot): a fresh ship or terrain restarts its chunk revisions,
    /// so stale meshes with matching revisions would otherwise survive.
    pub fn invalidate(&mut self) {
        self.key = None;
        self.ship = None;
    }

    /// Point the grid at `key`'s blocks and re-mesh changed chunks.
    pub fn sync(
        &mut self,
        gal: &Gpu,
        key: WorldKey,
        palette: Option<(PlanetId, Palette)>,
        blocks: &Blocks,
        revisions: impl Iterator<Item = (IVec3, u64)>,
    ) -> GraphicsResult<()> {
        // Planet palettes are baked into the material layers.
        let wanted = match palette {
            Some(p) => Some(p),
            None if self.renderer.is_none() => {
                Some((PlanetId::first(), PlanetId::first().info().palette))
            }
            None => None,
        };
        if let Some((planet, palette)) = wanted
            && (self.renderer.is_none() || self.palette != Some(planet))
        {
            if let Some(mut old) = self.renderer.take() {
                old.wait_and_retire(gal)?;
            }
            let mut renderer = VoxelRenderer::new(
                gal,
                materials(gal, &palette)?,
                VoxelRendererConfig {
                    max_lights: crate::render::MAX_LIGHTS,
                    ..VoxelRendererConfig::default()
                },
            )?;
            renderer.set_grid(GRID, GridTransform::default(), key.scale());
            self.renderer = Some(renderer);
            self.palette = Some(planet);
            self.key = None;
        }
        let renderer = self.renderer.as_mut().expect("created above");
        renderer.retire_completed(gal);
        if self.key != Some(key) {
            renderer.remove_grid(GRID);
            renderer.set_grid(GRID, GridTransform::default(), key.scale());
            self.uploaded.clear();
            self.key = Some(key);
        }
        if key == WorldKey::Ship {
            // Ship edits are rare: refine and re-mesh the whole hull.
            let revision = revisions.map(|(_, r)| r).max().unwrap_or(0);
            if self.ship.as_ref().map(|(r, _)| *r) != Some(revision) || self.uploaded.is_empty() {
                let fine = refine_ship(blocks);
                for (x, y, z) in std::mem::take(&mut self.uploaded).into_keys() {
                    renderer.remove(ChunkKey::new(GRID, IVec3::new(x, y, z)));
                }
                for chunk in fine.volume.chunk_coords() {
                    let mesh = self.content.mesh(&fine, chunk, false);
                    let key = ChunkKey::new(GRID, chunk);
                    renderer.upload(gal, key, ChunkPlacement::new(chunk * EDGE), &mesh)?;
                    self.uploaded.insert((chunk.x, chunk.y, chunk.z), revision);
                }
                self.ship = Some((revision, fine));
            }
            return Ok(());
        }
        for (chunk, revision) in revisions {
            let id = (chunk.x, chunk.y, chunk.z);
            if self.uploaded.get(&id) == Some(&revision) {
                continue;
            }
            let mesh = self
                .content
                .mesh(blocks, chunk, matches!(key, WorldKey::Planet { .. }));
            let key = ChunkKey::new(GRID, chunk);
            renderer.remove(key);
            renderer.upload(gal, key, ChunkPlacement::new(chunk * EDGE), &mesh)?;
            self.uploaded.insert(id, revision);
        }
        Ok(())
    }

    /// Opaque and cutout blocks (`translucent: false`) or windows only.
    pub fn prepare(
        &mut self,
        gal: &Gpu,
        target: &RenderTarget,
        pass: VoxelPass<'_>,
        translucent: bool,
    ) -> GraphicsResult<Option<PreparedVoxels>> {
        let Some(renderer) = self.renderer.as_mut() else {
            return Ok(None);
        };
        let shading = VoxelShading {
            ao_strength: 0.55,
            voxel_light: false,
            sky_brightness: 1.0,
            min_light: 0.0,
            model: ShadingModel::BlinnPhong,
            specular: [0.08; 3],
            shininess: 16.0,
            fog: pass.fog,
        };
        let prepared = renderer.prepare(
            gal,
            target,
            pass.camera,
            pass.lighting,
            pass.shadow,
            &shading,
            &VoxelPassOptions {
                clear_color: pass.clear,
                clear_depth: pass.clear.is_some(),
                opaque: !translucent,
                cutout: !translucent,
                translucent,
                overlays: pass.overlays,
            },
        )?;
        Ok(Some(prepared))
    }

    /// Voxel shadow casters into the shared shadow map (after mesh casters).
    pub fn prepare_depth(
        &mut self,
        gal: &Gpu,
        depth: &TextureView,
        view_projection: Mat4,
    ) -> GraphicsResult<Option<PreparedVoxels>> {
        let Some(renderer) = self.renderer.as_mut() else {
            return Ok(None);
        };
        let prepared =
            renderer.prepare_depth(gal, depth, view_projection.as_dmat4(), DVec3::ZERO, false)?;
        Ok(Some(prepared))
    }

    /// The voxel submissions of the frame being built (`HybridFrame`
    /// collects them); reported to the renderer by [`Self::submitted`].
    pub fn pending(&mut self, submissions: Vec<u64>) {
        self.pending_submissions = submissions;
    }

    /// Call after the frame's commands were submitted.
    pub fn submitted(&mut self, gal: &Gpu) {
        if let Some(renderer) = self.renderer.as_mut() {
            for submission in self.pending_submissions.drain(..) {
                renderer.submitted(gal, submission);
            }
        }
    }

    pub fn release(&mut self, gal: &Gpu) -> GraphicsResult<()> {
        self.pending_submissions.clear();
        if let Some(mut renderer) = self.renderer.take() {
            renderer.wait_and_retire(gal)?;
        }
        self.uploaded.clear();
        self.key = None;
        Ok(())
    }
}

/// A voxel camera matching the mesh camera.
pub fn camera(eye: Vec3, target: Vec3, projection: Mat4) -> GraphicsResult<VoxelCamera> {
    VoxelCamera::looking(
        eye.as_dvec3(),
        (target - eye).normalize_or_zero(),
        Vec3::Y,
        projection,
    )
    .ok_or_else(|| error("invalid voxel camera"))
}

/// Overlay box around one block (`grow` pads it outward).
/// The overlay box around a 2 m ship tile at ship voxel `(x, y, z)`.
pub fn ship_box(x: i32, y: i32, z: i32, grow: f32, color: [f32; 4]) -> OverlayBox {
    let mut b = block_box(x * 2, y * 2, z * 2, grow, color);
    b.max += Vec3::splat(1.0);
    b
}

pub fn block_box(x: i32, y: i32, z: i32, grow: f32, color: [f32; 4]) -> OverlayBox {
    let min = Vec3::new(x as f32, y as f32, z as f32) - Vec3::splat(grow);
    OverlayBox {
        grid: GRID,
        min,
        max: min + Vec3::splat(1.0 + 2.0 * grow),
        color,
    }
}
