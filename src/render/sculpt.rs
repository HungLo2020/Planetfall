//! Sculpted planet rock: a block mesher for cave terrain. Blocks stay true
//! cubes on the grid, but every exposed edge is chamfered and every corner
//! chipped by its own amount, so walls read as stacked, hand-cut stone
//! rather than machined boxes. Faces between neighbouring blocks are never
//! emitted; the ground and liquids stay flat.
//!
//! Quasar's culled extractor emits plain cubes or fixed models per block ID;
//! bevels that depend on which neighbours are open need the neighbourhood,
//! so this mesher walks the chunk snapshot itself and emits the same
//! `Quad`s (see QUASAR-ISSUES.md).

use quasar_math::{IVec3, Vec2, Vec3};
use quasar_voxel::{AIR, BlockCatalog, Face, Layer, Snapshot};
use quasar_voxel_mesh::{MeshBuffers, Quad, cube_face};

/// Chunk edge in voxels.
const EDGE: i32 = 16;

fn hash(a: i32, b: i32, c: i32, salt: u32) -> f32 {
    let mut h = (a as u32).wrapping_mul(0x9e37_79b1)
        ^ (b as u32).wrapping_mul(0x85eb_ca77)
        ^ (c as u32).wrapping_mul(0xc2b2_ae3d)
        ^ salt.wrapping_mul(0x27d4_eb2f);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2c1b_3c6d);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297a_2d39);
    h ^= h >> 15;
    (h & 0xffff) as f32 / 65535.0
}

/// How the mesher treats each block ID.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Carve {
    /// Flat cube faces (ground, liquids).
    Flat,
    /// Chamfered and chipped stone, with its mining wear (0 = intact,
    /// 1 to 3 = cracking): worn blocks chip deeper and darken.
    Stone(u8),
}

pub struct Sculptor<'a> {
    pub catalog: &'a BlockCatalog,
    /// Per block ID.
    pub carve: &'a [Carve],
}

/// Unit offsets of the eight cube corners.
fn corner_of(p: Vec3) -> IVec3 {
    IVec3::new(
        i32::from(p.x > 0.5),
        i32::from(p.y > 0.5),
        i32::from(p.z > 0.5),
    )
}

/// The face on `axis` at a corner (positive when the corner sits at 1).
fn side(axis: usize, corner: IVec3) -> Face {
    let positive = corner[axis] == 1;
    match (axis, positive) {
        (0, false) => Face::NegX,
        (0, true) => Face::PosX,
        (1, false) => Face::NegY,
        (1, true) => Face::PosY,
        (2, false) => Face::NegZ,
        _ => Face::PosZ,
    }
}

impl Sculptor<'_> {
    pub fn mesh(&self, blocks: &Snapshot<u16>, chunk: IVec3) -> MeshBuffers {
        let mut out = MeshBuffers::new();
        let origin = chunk * EDGE;
        let solid = |p: IVec3| blocks.at(p.x, p.y, p.z) != AIR;
        let carve_of = |b: u16| self.carve.get(b as usize).copied().unwrap_or(Carve::Flat);
        for y in 0..EDGE {
            for z in 0..EDGE {
                for x in 0..EDGE {
                    let cell = IVec3::new(x, y, z);
                    let block = blocks.at(x, y, z);
                    if block == AIR {
                        continue;
                    }
                    let mut open = [false; 6];
                    for face in Face::ALL {
                        open[face.index()] = !solid(cell + face.normal());
                    }
                    if !open.contains(&true) {
                        continue;
                    }
                    let Some(def) = self.catalog.get(block) else {
                        continue;
                    };
                    let carve = self
                        .carve
                        .get(block as usize)
                        .copied()
                        .unwrap_or(Carve::Flat);
                    let world = origin + cell;
                    // A corner with two open faces may only be cut when the
                    // block across the closed side is cut the same way
                    // (its matching faces are open too); otherwise the cut
                    // would expose that block's unmeshed face.
                    let mut pinned = 0u8;
                    for k in 0..8 {
                        let c = IVec3::new(k & 1, k >> 1 & 1, k >> 2 & 1);
                        let faces = [side(0, c), side(1, c), side(2, c)];
                        let closed: Vec<Face> =
                            faces.into_iter().filter(|f| !open[f.index()]).collect();
                        if closed.len() != 1 {
                            continue;
                        }
                        let across = cell + closed[0].normal();
                        let matches = faces
                            .into_iter()
                            .filter(|f| *f != closed[0])
                            .all(|f| !solid(across + f.normal()))
                            && matches!(
                                carve_of(blocks.at(across.x, across.y, across.z)),
                                Carve::Stone(_)
                            );
                        if !matches {
                            pinned |= 1 << k;
                        }
                    }
                    let block_info = BlockInfo {
                        pinned,
                        cell,
                        world,
                        open,
                        materials: def.materials,
                        layer: def.layer,
                        stone: matches!(carve, Carve::Stone(_)),
                        wear: match carve {
                            Carve::Stone(w) => w,
                            Carve::Flat => 0,
                        },
                        // Every block touching a lattice point chips it by
                        // the worst wear among them, so neighbours' cuts
                        // still meet.
                        corner_wear: std::array::from_fn(|k| {
                            let k = k as i32;
                            let c = cell + IVec3::new(k & 1, k >> 1 & 1, k >> 2 & 1);
                            (0..8)
                                .map(|o| {
                                    let t = c - IVec3::new(o & 1, o >> 1 & 1, o >> 2 & 1);
                                    match carve_of(blocks.at(t.x, t.y, t.z)) {
                                        Carve::Stone(w) => w,
                                        Carve::Flat => 0,
                                    }
                                })
                                .max()
                                .unwrap_or(0)
                        }),
                    };
                    block_info.emit(blocks, &mut out);
                }
            }
        }
        out
    }
}

struct BlockInfo {
    /// Corners (bit `x | y << 1 | z << 2`) that must stay sharp because a
    /// cut there would open a hole into a neighbour.
    pinned: u8,
    cell: IVec3,
    world: IVec3,
    open: [bool; 6],
    materials: [u32; 6],
    layer: Layer,
    stone: bool,
    /// This block's mining wear (0..=3).
    wear: u8,
    /// The worst wear around each corner (bit order as `pinned`).
    corner_wear: [u8; 8],
}

impl BlockInfo {
    /// Chip depth at a corner: most corners are lightly chamfered, some
    /// knocked off hard. Flat blocks are not cut.
    fn chip(&self, corner: IVec3) -> f32 {
        if !self.stone || self.pinned & (1 << (corner.x | corner.y << 1 | corner.z << 2)) != 0 {
            return 0.0;
        }
        // Keyed on the lattice point, so blocks sharing a corner cut it the
        // same way and their chamfers meet without gaps.
        let w = self.world + corner;
        let n = hash(w.x, w.y, w.z, 17);
        let base = if n > 0.88 {
            0.3 + (n - 0.88) * 1.2
        } else {
            0.07 + n * 0.13
        };
        // Mining wear knocks the corners off further, stage by stage.
        let wear = self.corner_wear[(corner.x | corner.y << 1 | corner.z << 2) as usize];
        let extra = [0.0, 0.05, 0.12, 0.2][usize::from(wear.min(3))];
        (base + extra * (0.6 + 0.8 * hash(w.x, w.y, w.z, 23))).min(0.46)
    }

    fn is_open(&self, face: Face) -> bool {
        self.open[face.index()]
    }

    /// Where face `face`'s vertex at `corner` lands: pulled in along every
    /// other axis whose face at that corner is also open.
    fn vertex(&self, face: Face, corner: IVec3) -> Vec3 {
        let mut p = corner.as_vec3();
        let r = self.chip(corner);
        for axis in 0..3 {
            if axis == face.axis() {
                continue;
            }
            if self.is_open(side(axis, corner)) {
                p[axis] += if corner[axis] == 1 { -r } else { r };
            }
        }
        p
    }

    /// A slightly lighter or darker stone per block.
    fn tint(&self) -> u32 {
        let w = self.world;
        let tone = if self.stone {
            (0.82 + 0.18 * hash(w.x, w.y, w.z, 3)) * (1.0 - 0.08 * f32::from(self.wear))
        } else {
            0.93 + 0.07 * hash(w.x, w.y, w.z, 5)
        };
        let g = (tone * 255.0).round() as u32;
        0xff00_0000 | g << 16 | g << 8 | g
    }

    fn emit(&self, blocks: &Snapshot<u16>, out: &mut MeshBuffers) {
        let tint = self.tint();
        let layer = out.layer_mut(self.layer);
        let base = self.cell.as_vec3();
        let opaque = |p: IVec3| blocks.at(p.x, p.y, p.z) != AIR;
        // Faces.
        for face in Face::ALL {
            if !self.is_open(face) {
                continue;
            }
            let corners = cube_face(face);
            let positions = corners.map(|c| self.vertex(face, corner_of(c)));
            // Texture coordinates follow the unit face, cropped by the chips.
            let e_u = corners[1] - corners[0];
            let e_v = corners[3] - corners[0];
            let uv = positions.map(|p| {
                let d = p - corners[0];
                Vec2::new(d.dot(e_u), 1.0 - d.dot(e_v))
            });
            // Vertex ambient occlusion from the cells in front of the face.
            let n = face.normal();
            let front = self.cell + n;
            let (ua, va) = match face.axis() {
                0 => (1, 2),
                1 => (0, 2),
                _ => (0, 1),
            };
            let ao = corners.map(|c| {
                let mut su = IVec3::ZERO;
                su[ua] = if c[ua] > 0.5 { 1 } else { -1 };
                let mut sv = IVec3::ZERO;
                sv[va] = if c[va] > 0.5 { 1 } else { -1 };
                let (s1, s2) = (opaque(front + su), opaque(front + sv));
                let corner = opaque(front + su + sv);
                if s1 && s2 {
                    0
                } else {
                    3 - (u8::from(s1) + u8::from(s2) + u8::from(corner))
                }
            });
            let flush = positions
                .iter()
                .zip(corners.iter())
                .all(|(p, c)| (*p - *c).length_squared() < 1e-6);
            layer.quads.push(Quad {
                positions: positions.map(|p| base + p),
                normal: n.as_vec3(),
                face: flush.then_some(face),
                uv,
                material: self.materials[face.index()],
                ao,
                light: [0; 4],
                tint,
                flipped: u16::from(ao[0]) + u16::from(ao[2]) < u16::from(ao[1]) + u16::from(ao[3]),
                size: [1, 1],
            });
        }
        if !self.stone {
            return;
        }
        // Chamfers along edges where both faces are open, and a facet on
        // each corner where three are.
        for corner in 0..8 {
            let c = IVec3::new(corner & 1, corner >> 1 & 1, corner >> 2 & 1);
            let faces = [side(0, c), side(1, c), side(2, c)];
            let open: Vec<Face> = faces.into_iter().filter(|f| self.is_open(*f)).collect();
            if open.len() == 3 {
                let points = open.iter().map(|f| self.vertex(*f, c)).collect::<Vec<_>>();
                let outward = open.iter().map(|f| f.normal().as_vec3()).sum::<Vec3>();
                self.facet(
                    layer,
                    base,
                    [points[0], points[1], points[2], points[2]],
                    outward,
                    self.lip_material(&open),
                    tint,
                );
            }
        }
        for a in Face::ALL {
            for b in Face::ALL {
                if a.index() >= b.index() || a.axis() == b.axis() {
                    continue;
                }
                if !self.is_open(a) || !self.is_open(b) {
                    continue;
                }
                // The edge's two corners: fixed on a's and b's axes, free on
                // the third.
                let third = 3 - a.axis() - b.axis();
                let mut ends = [IVec3::ZERO; 2];
                for (i, end) in ends.iter_mut().enumerate() {
                    end[a.axis()] = i32::from(a.is_positive());
                    end[b.axis()] = i32::from(b.is_positive());
                    end[third] = i as i32;
                }
                let quad = [
                    self.vertex(a, ends[0]),
                    self.vertex(a, ends[1]),
                    self.vertex(b, ends[1]),
                    self.vertex(b, ends[0]),
                ];
                let outward = (a.normal() + b.normal()).as_vec3();
                self.facet(layer, base, quad, outward, self.lip_material(&[a, b]), tint);
            }
        }
    }

    /// Chamfers show the side texture's top band (the moss or frost lip);
    /// purely vertical edges show the plain side.
    fn lip_material(&self, faces: &[Face]) -> u32 {
        let side = faces
            .iter()
            .find(|f| f.axis() != 1)
            .copied()
            .unwrap_or(Face::PosY);
        self.materials[side.index()]
    }

    fn facet(
        &self,
        layer: &mut quasar_voxel_mesh::LayerBuffers,
        base: Vec3,
        mut points: [Vec3; 4],
        outward: Vec3,
        material: u32,
        tint: u32,
    ) {
        let normal = (points[2] - points[0])
            .cross(points[3] - points[1])
            .normalize_or_zero();
        let normal = if normal == Vec3::ZERO {
            (points[1] - points[0])
                .cross(points[2] - points[0])
                .normalize_or_zero()
        } else {
            normal
        };
        if normal == Vec3::ZERO || (points[0] - points[1]).length_squared() < 1e-8 {
            return;
        }
        let normal = if normal.dot(outward) < 0.0 {
            points.reverse();
            -normal
        } else {
            normal
        };
        // Map the facet onto the upper strip of the texture.
        let along = (points[1] - points[0]).normalize_or_zero();
        let uv = points.map(|p| {
            let d = p - points[0];
            Vec2::new(
                d.dot(along).abs().fract(),
                (p.y - points[0].y).abs() * 0.5 + 0.02,
            )
        });
        layer.quads.push(Quad {
            positions: points.map(|p| base + p),
            normal,
            face: None,
            uv,
            material,
            ao: [3; 4],
            light: [0; 4],
            tint,
            flipped: false,
            size: [1, 1],
        });
    }
}
