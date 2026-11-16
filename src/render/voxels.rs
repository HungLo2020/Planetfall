//! Planet terrain and ship hull as voxels: block catalog and procedural
//! texture layers, chunk meshing (`quasar-voxel-mesh`) and drawing
//! (`quasar-render-voxel`), lit by the same lights and shadow map as meshes.
//!
//! The simulation owns the block volumes (`planetfall_sim::voxel`). Each
//! frame [`VoxelWorld::sync`] re-meshes chunks whose revision changed (mined
//! rock, placed ship blocks) and swaps the whole grid when the scene changes
//! between the ship and a planet.

use planetfall_sim::content::{Palette, PlanetId};
use planetfall_sim::terrain::TILE;
use planetfall_sim::voxel::{self, Blocks};
use quasar_frame::{Gpu, GraphicsResult, RenderTarget, error};
use quasar_math::{DVec3, IVec3, Mat4, Vec3};
use quasar_render_3d::{Lighting, ShadingModel, ShadowBinding};
use quasar_render_voxel::{
    ChunkKey, ChunkPlacement, Fog, MaterialDef, MaterialFilter, OverlayBox, PreparedVoxels,
    VoxelCamera, VoxelMaterials, VoxelPassOptions, VoxelRenderer, VoxelRendererConfig,
    VoxelShading,
};
use quasar_voxel::{BlockCatalog, BlockDef, FaceShape, GridId, GridTransform, Layer, Snapshot};
use quasar_voxel_mesh::{
    CatalogFaces, CatalogLayers, CatalogModels, CulledInput, CulledModelExtractor, MeshBuffers,
    ModelLibrary, VertexShade, box_quads, opaque_table,
};
use std::collections::BTreeMap;
use vulkanic_gal::TextureView;

pub const GRID: GridId = GridId(0);
/// Chunk edge in voxels (`planetfall_sim::voxel::Layout` is `Cube16`).
const EDGE: i32 = 16;
/// Texture tile edge in texels.
const TEXELS: u32 = 16;

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
    pub const COUNT: u32 = 14;
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

/// Block definitions and the per-block tables meshing needs.
struct Content {
    catalog: BlockCatalog,
    library: ModelLibrary,
    opaque: Vec<bool>,
}

impl Content {
    fn new() -> Self {
        let mut catalog = BlockCatalog::new();
        let mut library = ModelLibrary::new();
        let sides = |side: u32, top: u32| [side, side, side, top, side, side];
        // Cut-away hull: walls stop at 0.6 of a block so the top-down camera
        // sees into rooms; they only cull against each other.
        let low = FaceShape::Shape(1);
        let wall_faces = [low, low, FaceShape::Full, FaceShape::None, low, low];
        let wall = |library: &mut ModelLibrary, side: u32, top: u32| {
            library.add(box_quads(
                Vec3::ZERO,
                Vec3::new(1.0, 0.6, 1.0),
                sides(side, top),
            ))
        };
        let hull_model = wall(&mut library, tex::HULL, tex::HULL_TOP);
        let window_model = wall(&mut library, tex::WINDOW, tex::WINDOW);
        let lamp_model = library.add(box_quads(
            Vec3::new(0.3, 0.0, 0.3),
            Vec3::new(0.7, 0.45, 0.7),
            [tex::LAMP; 6],
        ));
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
                    .with_model(hull_model)
                    .with_faces(wall_faces),
            ),
            (
                voxel::WINDOW,
                BlockDef::cube("window", tex::WINDOW)
                    .with_model(window_model)
                    .with_faces(wall_faces)
                    .with_layer(Layer::Translucent),
            ),
            (
                voxel::LAMP,
                BlockDef::cube("lamp", tex::LAMP)
                    .with_model(lamp_model)
                    .with_faces([FaceShape::None; 6])
                    .with_emission(14),
            ),
            (voxel::KEEL, BlockDef::cube("keel", tex::KEEL)),
        ];
        for (id, def) in defs {
            let added = catalog.add(def);
            debug_assert_eq!(added, id, "catalog order matches planetfall_sim::voxel");
        }
        debug_assert_eq!(catalog.defs().len(), voxel::BLOCK_COUNT as usize);
        let opaque = opaque_table(&catalog);
        Self {
            catalog,
            library,
            opaque,
        }
    }

    fn mesh(&self, blocks: &Blocks, chunk: IVec3) -> MeshBuffers {
        let mut snapshot = Snapshot::new();
        blocks
            .volume
            .snapshot(blocks.block, chunk, 1, &mut snapshot);
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
                    let ore = |vein: [f32; 3]| {
                        let crystal = hash(x / 3, y / 3, layer + 7) > 0.72;
                        if crystal {
                            shade(vein, 0.15)
                        } else {
                            shade(palette.rock, 0.18)
                        }
                    };
                    let (rgb, alpha) = match layer {
                        tex::GROUND => (shade(palette.floor, 0.12), 1.0),
                        tex::GROUND_ALT => (shade(palette.floor_alt, 0.12), 1.0),
                        tex::ROCK => (shade(palette.rock, 0.2), 1.0),
                        tex::ROCK_TOP => (shade(palette.rock_top, 0.12), 1.0),
                        tex::FERRITE => (ore([0.72, 0.42, 0.22]), 1.0),
                        tex::LUMITE => (ore([0.35, 0.95, 1.0]), 1.0),
                        tex::VOIDSTONE => (ore([0.55, 0.25, 0.85]), 1.0),
                        tex::BEDROCK => (shade(palette.bedrock, 0.25), 1.0),
                        tex::DECK => {
                            let seam = edge || x == TEXELS / 2;
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
            pending_submissions: Vec::new(),
        }
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
                Some((PlanetId::Verdant, PlanetId::Verdant.info().palette))
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
                VoxelRendererConfig::default(),
            )?;
            renderer.set_grid(GRID, GridTransform::default(), f64::from(TILE));
            self.renderer = Some(renderer);
            self.palette = Some(planet);
            self.key = None;
        }
        let renderer = self.renderer.as_mut().expect("created above");
        renderer.retire_completed(gal);
        if self.key != Some(key) {
            renderer.remove_grid(GRID);
            renderer.set_grid(GRID, GridTransform::default(), f64::from(TILE));
            self.uploaded.clear();
            self.key = Some(key);
        }
        for (chunk, revision) in revisions {
            let id = (chunk.x, chunk.y, chunk.z);
            if self.uploaded.get(&id) == Some(&revision) {
                continue;
            }
            let mesh = self.content.mesh(blocks, chunk);
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
        self.pending_submissions.push(prepared.submission);
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
        self.pending_submissions.push(prepared.submission);
        Ok(Some(prepared))
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
pub fn block_box(x: i32, y: i32, z: i32, grow: f32, color: [f32; 4]) -> OverlayBox {
    let min = Vec3::new(x as f32, y as f32, z as f32) - Vec3::splat(grow);
    OverlayBox {
        grid: GRID,
        min,
        max: min + Vec3::splat(1.0 + 2.0 * grow),
        color,
    }
}
