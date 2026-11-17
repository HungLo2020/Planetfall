//! Planet surface: a `quasar-voxel` block volume of ground, minable rock and
//! ore, cave generation, circle collision, mining, line of sight (voxel
//! raycasts) and a horde flow field over the voxel nav grid (`quasar-nav`).

use crate::cavegen::{self, Cell};
use crate::content::{Material, PlanetId};
use crate::math::V2;
use crate::voxel::{self, AIR, Blocks};
use quasar_core::Rng;
use quasar_math::{IVec3, Vec3};
use quasar_nav::voxel::{VoxelNavBounds, sync_changes, walkable_grid};
use quasar_nav::{DistanceField, FieldOptions, Grid, GridCell};
use quasar_voxel::{EditBatch, EditOptions, TieBreak};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// World size of one block, in meters.
pub const TILE: f32 = 2.0;
pub const MAP_SIZE: u32 = 60;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Tile {
    Floor,
    Rock { hp: f32 },
    Ore { material: Material, hp: f32 },
    Bedrock,
}

impl Tile {
    pub fn solid(self) -> bool {
        !matches!(self, Self::Floor)
    }

    pub fn minable(self) -> bool {
        matches!(self, Self::Rock { .. } | Self::Ore { .. })
    }

    fn rock() -> Self {
        Self::Rock { hp: 2.4 }
    }

    fn ore(material: Material) -> Self {
        let hp = match material {
            Material::Ferrite => 3.2,
            Material::Lumite => 3.8,
            Material::Voidstone => 5.5,
        };
        Self::Ore { material, hp }
    }

    pub fn max_hp(self) -> f32 {
        match self {
            Self::Rock { .. } => 2.4,
            Self::Ore { material, .. } => Self::ore(material).hp(),
            _ => 0.0,
        }
    }

    fn hp(self) -> f32 {
        match self {
            Self::Rock { hp } | Self::Ore { hp, .. } => hp,
            _ => 0.0,
        }
    }

    /// The wall-layer block for this tile.
    fn block(self) -> u16 {
        match self {
            Self::Floor => AIR,
            Self::Rock { .. } => voxel::ROCK,
            Self::Ore {
                material: Material::Ferrite,
                ..
            } => voxel::FERRITE_ORE,
            Self::Ore {
                material: Material::Lumite,
                ..
            } => voxel::LUMITE_ORE,
            Self::Ore {
                material: Material::Voidstone,
                ..
            } => voxel::VOIDSTONE_ORE,
            Self::Bedrock => voxel::BEDROCK,
        }
    }

    /// Undamaged tile for a wall-layer block.
    fn from_block(block: u16) -> Self {
        match block {
            voxel::ROCK => Self::rock(),
            voxel::FERRITE_ORE => Self::ore(Material::Ferrite),
            voxel::LUMITE_ORE => Self::ore(Material::Lumite),
            voxel::VOIDSTONE_ORE => Self::ore(Material::Voidstone),
            AIR => Self::Floor,
            _ => Self::Bedrock,
        }
    }
}

/// What covers an open floor tile.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ground {
    Plain,
    /// Burns whoever stands in it.
    Lava,
    /// Shallow water: slows and soaks.
    Water,
}

/// What breaking a block produced.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Broken {
    pub tile: (i32, i32),
    pub was: Tile,
}

const NAV_BOUNDS: VoxelNavBounds = VoxelNavBounds {
    min: IVec3::new(0, 0, 0),
    max: IVec3::new(MAP_SIZE as i32 - 1, 0, MAP_SIZE as i32 - 1),
};

fn solid_block(block: u16) -> bool {
    block != AIR
}

/// Terrain saves as its generation inputs plus what was mined, and is
/// regenerated on load (the voxel volume and nav grid are derived).
#[derive(Clone, Serialize, Deserialize)]
#[serde(into = "TerrainSave", try_from = "TerrainSave")]
pub struct Terrain {
    pub width: u32,
    pub height: u32,
    planet: PlanetId,
    seed: u64,
    tutorial: bool,
    /// Blocks broken, in order.
    broken: Vec<(i32, i32)>,
    /// Rubble brought down by quakes, with how many blocks had been broken
    /// before (so a reload replays edits in order).
    collapsed: Vec<(usize, (i32, i32))>,
    blocks: Blocks,
    /// Damage dealt to partially mined blocks.
    damage: BTreeMap<(i32, i32), f32>,
    /// Standable cells, one layer deep (`quasar_nav::voxel::walkable_grid`).
    nav: Grid<3>,
    /// Incremented whenever a block is broken.
    pub revision: u64,
    /// Revision at which each voxel chunk last changed (for re-meshing).
    chunk_revisions: BTreeMap<(i32, i32, i32), u64>,
    /// Nav cells whose walkability changed since the flow field last saw them.
    nav_dirty: Vec<GridCell<3>>,
}

/// Layout facts the run needs from generation.
/// Lumite veins every non-tutorial layout guarantees (ore quotas reach 14
/// at hazard 5).
pub const MIN_LUMITE: usize = 16;

/// Mission sites every layout provides.
pub const SITES: usize = 4;

pub struct Layout {
    pub landing: V2,
    pub caches: Vec<V2>,
    /// Far, spread-out chambers for mission set pieces (boss arena,
    /// survivor, drill rig, wreck, relays), farthest first.
    pub sites: Vec<V2>,
    /// Open floor spots away from the pod, shuffled (glowcaps, parts).
    pub spots: Vec<V2>,
}

impl Terrain {
    pub fn tile(&self, x: i32, y: i32) -> Tile {
        if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
            return Tile::Bedrock;
        }
        let tile = Tile::from_block(self.blocks.get(voxel::voxel(x, y, 0)));
        match (tile, self.damage.get(&(x, y))) {
            (Tile::Rock { hp }, Some(d)) => Tile::Rock { hp: hp - d },
            (Tile::Ore { material, hp }, Some(d)) => Tile::Ore {
                material,
                hp: hp - d,
            },
            (tile, _) => tile,
        }
    }

    pub fn tiles(&self) -> impl Iterator<Item = ((i32, i32), Tile)> + '_ {
        let w = self.width as i32;
        (0..(self.width * self.height) as i32).map(move |i| {
            let (x, y) = (i % w, i / w);
            ((x, y), self.tile(x, y))
        })
    }

    /// The block volume (read-only; renderers mesh it).
    pub fn blocks(&self) -> &Blocks {
        &self.blocks
    }

    /// Voxel chunks and the revision they last changed at.
    pub fn chunk_revisions(&self) -> impl Iterator<Item = (IVec3, u64)> + '_ {
        self.chunk_revisions
            .iter()
            .map(|(&(x, y, z), &r)| (IVec3::new(x, y, z), r))
    }

    pub fn world_size(&self) -> V2 {
        V2::new(self.width as f32 * TILE, self.height as f32 * TILE)
    }

    pub fn tile_of(p: V2) -> (i32, i32) {
        ((p[0] / TILE).floor() as i32, (p[1] / TILE).floor() as i32)
    }

    pub fn center_of(x: i32, y: i32) -> V2 {
        V2::new((x as f32 + 0.5) * TILE, (y as f32 + 0.5) * TILE)
    }

    pub fn solid_at(&self, p: V2) -> bool {
        let (x, y) = Self::tile_of(p);
        self.tile(x, y).solid()
    }

    /// Damage a minable block. Returns the broken tile when it gives way.
    pub fn mine(&mut self, x: i32, y: i32, amount: f32) -> Option<Broken> {
        let tile = self.tile(x, y);
        if !tile.minable() {
            return None;
        }
        if tile.hp() - amount > 0.0 {
            *self.damage.entry((x, y)).or_default() += amount;
            return None;
        }
        self.damage.remove(&(x, y));
        self.broken.push((x, y));
        self.break_block(x, y);
        Some(Broken {
            tile: (x, y),
            was: tile,
        })
    }

    /// What covers the floor at a tile.
    pub fn ground(&self, x: i32, y: i32) -> Ground {
        match self.blocks.get(voxel::voxel(x, y, -1)) {
            voxel::LAVA => Ground::Lava,
            voxel::WATER => Ground::Water,
            _ => Ground::Plain,
        }
    }

    pub fn ground_at(&self, p: V2) -> Ground {
        let (x, y) = Self::tile_of(p);
        self.ground(x, y)
    }

    /// Bring rubble down on an open floor tile (quakes). Returns whether it
    /// fell.
    pub fn collapse(&mut self, x: i32, y: i32) -> bool {
        if self.tile(x, y) != Tile::Floor || x < 1 || y < 1 {
            return false;
        }
        self.collapsed.push((self.broken.len(), (x, y)));
        self.set_wall(x, y, voxel::ROCK);
        true
    }

    /// Clear a wall block and propagate the edit to nav and re-meshing.
    fn break_block(&mut self, x: i32, y: i32) {
        self.set_wall(x, y, AIR);
    }

    fn set_wall(&mut self, x: i32, y: i32, block: u16) {
        let mut batch = EditBatch::new();
        batch.set(self.blocks.block, voxel::voxel(x, y, 0), block);
        let result = self.blocks.volume.apply(&batch, EditOptions::default());
        self.revision += 1;
        for chunk in &result.dirty {
            self.chunk_revisions
                .insert((chunk.x, chunk.y, chunk.z), self.revision);
        }
        if let Ok(cells) = sync_changes(
            &mut self.nav,
            &self.blocks.volume,
            self.blocks.block,
            NAV_BOUNDS,
            1,
            &result.changes,
            solid_block,
        ) {
            self.nav_dirty.extend(cells);
        }
    }

    /// Push a circle out of solid blocks. Returns the resolved position and the
    /// block it collided with most directly (used for auto-mining).
    pub fn resolve_circle(&self, mut p: V2, radius: f32) -> (V2, Option<(i32, i32)>) {
        let mut hit = None;
        let mut deepest = 0.0f32;
        for _ in 0..2 {
            let (cx, cy) = Self::tile_of(p);
            for y in cy - 1..=cy + 1 {
                for x in cx - 1..=cx + 1 {
                    if !self.tile(x, y).solid() {
                        continue;
                    }
                    let min = V2::new(x as f32, y as f32) * TILE;
                    let closest = p.clamp(min, min + V2::splat(TILE));
                    let d = p - closest;
                    let dist = d.length();
                    if dist >= radius {
                        continue;
                    }
                    let depth = radius - dist;
                    if dist > 1e-5 {
                        p += d / dist * depth;
                    } else {
                        // Center inside the block: push toward the tile we came from.
                        let away = (p - Self::center_of(x, y)).normalize_or_zero();
                        let away = if away == V2::ZERO { V2::Y } else { away };
                        p += away * (radius + TILE * 0.5);
                    }
                    if depth > deepest {
                        deepest = depth;
                        hit = Some((x, y));
                    }
                }
            }
        }
        (p, hit)
    }

    /// Whether a straight segment at waist height stays out of solid blocks
    /// (a voxel DDA raycast).
    pub fn line_clear(&self, a: V2, b: V2) -> bool {
        let delta = (b - a) / TILE;
        let length = delta.length();
        if length < 1e-4 {
            return !self.solid_at(a);
        }
        let origin = Vec3::new(a.x / TILE, 0.5, a.y / TILE);
        let direction = Vec3::new(delta.x, 0.0, delta.y) / length;
        self.blocks
            .volume
            .raycast(
                self.blocks.block,
                origin,
                direction,
                length,
                TieBreak::default(),
                |_, block| block != AIR,
            )
            .is_none()
    }

    pub fn count_ore(&self, material: Material) -> usize {
        self.tiles()
            .filter(|(_, t)| matches!(t, Tile::Ore { material: m, .. } if *m == material))
            .count()
    }

    /// Generate a cave for `planet` (see `cavegen`). Deterministic for a
    /// given seed.
    pub fn generate(planet: PlanetId, seed: u64, tutorial: bool) -> (Self, Layout) {
        let mut rng = Rng::seed_from_u64(seed);
        let (w, h) = (MAP_SIZE as i32, MAP_SIZE as i32);
        let cave = cavegen::generate(planet, w, &mut rng, seed, SITES, tutorial);
        let idx = |x: i32, y: i32| (y * w + x) as usize;
        let (lx, ly) = cave.landing;
        let mut tiles: Vec<Tile> = cave
            .cells
            .iter()
            .map(|c| match c {
                Cell::Floor => Tile::Floor,
                Cell::Rock => Tile::rock(),
                Cell::Ore(m) => Tile::ore(*m),
                Cell::Bedrock => Tile::Bedrock,
            })
            .collect();
        let sites = cave.sites.clone();
        let exposed = |tiles: &Vec<Tile>, x: i32, y: i32| {
            [(1, 0), (-1, 0), (0, 1), (0, -1)].iter().any(|(dx, dy)| {
                let (nx, ny) = (x + dx, y + dy);
                nx >= 0 && ny >= 0 && nx < w && ny < h && tiles[idx(nx, ny)] == Tile::Floor
            })
        };
        let floor: Vec<(i32, i32)> = (0..h)
            .flat_map(|y| (0..w).map(move |x| (x, y)))
            .filter(|&(x, y)| tiles[idx(x, y)] == Tile::Floor)
            .collect();
        // Guarantee enough lumite for the objective, with a starter vein near the pod.
        let ensure = |tiles: &mut Vec<Tile>,
                      rng: &mut Rng,
                      material: Material,
                      count: usize,
                      near: Option<(i32, i32, i32)>| {
            let have = tiles
                .iter()
                .filter(|t| matches!(t, Tile::Ore { material: m, .. } if *m == material))
                .count();
            let mut needed = count.saturating_sub(have);
            let mut guard = 0;
            while needed > 0 && guard < 20_000 {
                guard += 1;
                let (x, y) = if let Some((cx, cy, r)) = near {
                    (
                        cx + rng.range_usize(0..(2 * r + 1) as usize) as i32 - r,
                        cy + rng.range_usize(0..(2 * r + 1) as usize) as i32 - r,
                    )
                } else {
                    (
                        1 + rng.range_usize(0..(w - 2) as usize) as i32,
                        1 + rng.range_usize(0..(h - 2) as usize) as i32,
                    )
                };
                if x > 0
                    && y > 0
                    && x < w - 1
                    && y < h - 1
                    && matches!(tiles[idx(x, y)], Tile::Rock { .. })
                    && exposed(tiles, x, y)
                {
                    tiles[idx(x, y)] = Tile::ore(material);
                    needed -= 1;
                }
            }
        };
        // Enough lumite for the largest ore quota, with room to spare.
        let objective = if tutorial { 4 } else { MIN_LUMITE };
        ensure(&mut tiles, &mut rng, Material::Lumite, 3, Some((lx, ly, 7)));
        ensure(
            &mut tiles,
            &mut rng,
            Material::Ferrite,
            4,
            Some((lx, ly, 7)),
        );
        ensure(&mut tiles, &mut rng, Material::Lumite, objective + 8, None);
        ensure(&mut tiles, &mut rng, Material::Ferrite, 14, None);

        // Commit the tiles to voxels: ground under everything, walls above,
        // and a taller bedrock rim.
        let mut blocks = Blocks::default();
        for y in 0..h {
            for x in 0..w {
                let tile = tiles[idx(x, y)];
                let ground = if cave.lava[idx(x, y)] {
                    voxel::LAVA
                } else if cave.water[idx(x, y)] {
                    voxel::WATER
                } else if cave.patches[idx(x, y)] {
                    voxel::GROUND_PATCH
                } else if (x + y) % 2 == 0 {
                    voxel::GROUND
                } else {
                    voxel::GROUND_ALT
                };
                blocks.set(voxel::voxel(x, y, -1), ground);
                blocks.set(voxel::voxel(x, y, 0), tile.block());
                if tile == Tile::Bedrock {
                    blocks.set(voxel::voxel(x, y, 1), voxel::BEDROCK);
                }
            }
        }
        let mut nav = walkable_grid(&blocks.volume, blocks.block, NAV_BOUNDS, 1, solid_block)
            .expect("map dimensions are valid");
        nav.set_diagonal(true);
        nav.set_corner_cutting(false);
        let chunk_revisions = blocks
            .volume
            .chunk_coords()
            .into_iter()
            .map(|c| ((c.x, c.y, c.z), 0))
            .collect();

        let landing = Self::center_of(lx, ly);
        // Weapon caches: one close by (always in the tutorial), the rest far apart.
        let mut caches: Vec<V2> = Vec::new();
        let mut candidates: Vec<V2> = floor.iter().map(|&(x, y)| Self::center_of(x, y)).collect();
        // Deterministic shuffle.
        for i in (1..candidates.len()).rev() {
            let j = rng.range_usize(0..i + 1);
            candidates.swap(i, j);
        }
        let spots: Vec<V2> = candidates
            .iter()
            .copied()
            .filter(|c| c.distance(landing) > 8.0)
            .take(120)
            .collect();
        if let Some(near) = candidates
            .iter()
            .copied()
            .filter(|&c| (8.0..14.0).contains(&c.distance(landing)))
            .min_by(|a, b| (*a).distance(landing).total_cmp(&(*b).distance(landing)))
        {
            caches.push(near);
        }
        let wanted = if tutorial { 3 } else { 4 };
        for c in candidates {
            if caches.len() >= wanted {
                break;
            }
            if c.distance(landing) > 22.0 && caches.iter().all(|&o| o.distance(c) > 20.0) {
                caches.push(c);
            }
        }
        (
            Self {
                width: MAP_SIZE,
                height: MAP_SIZE,
                planet,
                seed,
                tutorial,
                broken: Vec::new(),
                collapsed: Vec::new(),
                blocks,
                damage: BTreeMap::new(),
                nav,
                revision: 0,
                chunk_revisions,
                nav_dirty: Vec::new(),
            },
            Layout {
                landing,
                caches,
                sites: sites
                    .into_iter()
                    .map(|(x, y)| Self::center_of(x, y))
                    .collect(),
                spots,
            },
        )
    }
}

#[derive(Serialize, Deserialize)]
struct TerrainSave {
    planet: PlanetId,
    seed: u64,
    tutorial: bool,
    broken: Vec<(i32, i32)>,
    damage: Vec<((i32, i32), f32)>,
    #[serde(default)]
    collapsed: Vec<(usize, (i32, i32))>,
}

impl From<Terrain> for TerrainSave {
    fn from(t: Terrain) -> Self {
        Self {
            planet: t.planet,
            seed: t.seed,
            tutorial: t.tutorial,
            broken: t.broken,
            damage: t.damage.into_iter().collect(),
            collapsed: t.collapsed,
        }
    }
}

impl TryFrom<TerrainSave> for Terrain {
    type Error = String;

    fn try_from(save: TerrainSave) -> Result<Self, String> {
        let (mut terrain, _) = Terrain::generate(save.planet, save.seed, save.tutorial);
        let mut rubble = save.collapsed.into_iter().peekable();
        for (n, (x, y)) in save.broken.into_iter().enumerate() {
            while let Some((_, (rx, ry))) = rubble.next_if(|(at, _)| *at <= n) {
                terrain.collapse(rx, ry);
            }
            if !terrain.tile(x, y).minable() {
                return Err(format!("saved terrain breaks unminable block ({x}, {y})"));
            }
            terrain.broken.push((x, y));
            terrain.break_block(x, y);
        }
        for (_, (rx, ry)) in rubble {
            terrain.collapse(rx, ry);
        }
        terrain.damage = save.damage.into_iter().collect();
        // The flow field is rebuilt from scratch after loading.
        terrain.nav_dirty.clear();
        Ok(terrain)
    }
}

/// Distance-to-player field over the voxel nav grid
/// (`quasar_nav::DistanceField`). Hordes walk downhill. Moving the goal
/// rebuilds it; mined blocks repair only the routes they affect.
#[derive(Clone)]
pub struct FlowField {
    field: DistanceField<3>,
    goal: (i32, i32),
}

fn cell(x: i32, y: i32) -> GridCell<3> {
    GridCell::new3(x, 0, y)
}

impl FlowField {
    pub fn new(terrain: &Terrain, target: V2) -> Self {
        let goal = Terrain::tile_of(target);
        let field = terrain
            .nav
            .distance_field(&[cell(goal.0, goal.1)], &FieldOptions::default())
            .expect("finite default options");
        Self { field, goal }
    }

    /// Follow the target to its cell and repair routes through mined blocks.
    pub fn update(&mut self, terrain: &mut Terrain, target: V2) {
        let goal = Terrain::tile_of(target);
        let dirty = std::mem::take(&mut terrain.nav_dirty);
        if goal != self.goal {
            self.goal = goal;
            // Goals outside the grid leave the previous field in place.
            let _ = self.field.set_goals(&terrain.nav, &[cell(goal.0, goal.1)]);
        } else if !dirty.is_empty() {
            let _ = self.field.update_region(&terrain.nav, &dirty);
        }
    }

    pub fn distance_at(&self, x: i32, y: i32) -> f32 {
        self.field.distance(cell(x, y)).unwrap_or(f32::INFINITY)
    }

    /// The world point an agent at `p` should walk toward.
    pub fn next_waypoint(&self, terrain: &Terrain, p: V2) -> Option<V2> {
        let (x, y) = Terrain::tile_of(p);
        self.field
            .next_cell(&terrain.nav, cell(x, y))
            .map(|c| Terrain::center_of(c.coords[0], c.coords[2]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generation_is_deterministic_and_playable() {
        for planet in PlanetId::all() {
            let (a, layout) = Terrain::generate(planet, 42, false);
            let (b, _) = Terrain::generate(planet, 42, false);
            assert!(a.tiles().eq(b.tiles()));
            assert!(!a.solid_at(layout.landing));
            assert!(a.count_ore(Material::Lumite) >= MIN_LUMITE);
            assert!(!layout.caches.is_empty());
            for cache in &layout.caches {
                assert!(!a.solid_at(*cache));
            }
        }
    }

    #[test]
    fn caves_are_connected_organic_and_stocked() {
        for planet in PlanetId::all() {
            for seed in 0..25 {
                for tutorial in [false, true] {
                    let (t, layout) = Terrain::generate(planet, seed, tutorial);
                    // Everything open is reachable from the pod without mining.
                    let field = FlowField::new(&t, layout.landing);
                    for ((x, y), tile) in t.tiles() {
                        if tile == Tile::Floor {
                            assert!(
                                field.distance_at(x, y).is_finite(),
                                "{planet:?} seed {seed}: ({x}, {y}) cut off"
                            );
                        }
                    }
                    assert_eq!(layout.sites.len(), SITES);
                    for p in layout
                        .sites
                        .iter()
                        .chain(&layout.caches)
                        .chain(&layout.spots)
                    {
                        assert!(!t.solid_at(*p), "{planet:?} seed {seed}: {p:?} in rock");
                    }
                    // The arena is a big open room far from the pod.
                    let arena = layout.sites[0];
                    assert!(arena.distance(layout.landing) > 30.0);
                    let (ax, ay) = Terrain::tile_of(arena);
                    let open = (-3..=3)
                        .flat_map(|dy| (-3..=3).map(move |dx| (dx, dy)))
                        .filter(|&(dx, dy)| t.tile(ax + dx, ay + dy) == Tile::Floor)
                        .count();
                    assert!(open >= 40, "{planet:?} seed {seed}: arena has {open} floor");
                    let floor = t.tiles().filter(|(_, t)| *t == Tile::Floor).count();
                    assert!(
                        (500..1800).contains(&floor),
                        "{planet:?} seed {seed} tutorial {tutorial}: {floor} floor"
                    );
                    if !tutorial {
                        assert!(t.count_ore(Material::Lumite) >= MIN_LUMITE);
                        assert!(t.count_ore(Material::Ferrite) >= 10);
                    }
                }
            }
        }
    }

    #[test]
    fn mining_opens_navigation() {
        let (mut t, layout) = Terrain::generate(PlanetId::first(), 3, true);
        let (x, y) = t
            .tiles()
            .find(|(_, tile)| matches!(tile, Tile::Rock { .. }))
            .map(|(c, _)| c)
            .unwrap();
        let mut broken = None;
        for _ in 0..100 {
            if let Some(b) = t.mine(x, y, 0.5) {
                broken = Some(b);
                break;
            }
        }
        assert!(broken.is_some());
        assert_eq!(t.tile(x, y), Tile::Floor);
        assert!(!t.nav.is_blocked(cell(x, y)));
        assert_eq!(t.blocks().get(voxel::voxel(x, y, 0)), AIR);
        assert_eq!(t.revision, 1);
        let mut field = FlowField::new(&t, layout.landing);
        field.update(&mut t, layout.landing);
        let (lx, ly) = Terrain::tile_of(layout.landing);
        assert_eq!(field.distance_at(lx, ly), 0.0);
        // The broken block is now on the field (reachable or walled off by rock).
        assert!(field.distance_at(x, y) >= 0.0);
    }

    #[test]
    fn circles_are_pushed_out_of_walls() {
        let (t, _) = Terrain::generate(PlanetId::first(), 9, false);
        let ((x, y), _) = t.tiles().find(|(_, tile)| *tile == Tile::Bedrock).unwrap();
        let inside = Terrain::center_of(x, y);
        let probe = V2::new(inside[0] + TILE * 0.5 + 0.1, inside[1]);
        let (resolved, hit) = t.resolve_circle(probe, 0.4);
        assert!(hit.is_some() || resolved != probe || !t.tile(x + 1, y).solid());
    }

    #[test]
    fn line_of_sight_uses_voxel_raycasts() {
        let (t, layout) = Terrain::generate(PlanetId::first(), 5, false);
        // Through open floor next to the landing site.
        let a = layout.landing;
        assert!(t.line_clear(a, a + V2::new(1.0, 0.0)));
        // Into the bedrock rim from inside the map.
        let edge = Terrain::center_of(0, 10);
        assert!(!t.line_clear(Terrain::center_of(3, 10), edge));
    }

    #[test]
    fn mining_repairs_the_flow_field_like_a_rebuild() {
        let (mut t, layout) = Terrain::generate(PlanetId::from_key("cinder").unwrap(), 11, false);
        let mut field = FlowField::new(&t, layout.landing);
        // Break every rock touching open floor near the landing site.
        let (lx, ly) = Terrain::tile_of(layout.landing);
        for y in ly - 6..=ly + 6 {
            for x in lx - 6..=lx + 6 {
                if matches!(t.tile(x, y), Tile::Rock { .. }) {
                    while t.mine(x, y, 10.0).is_none() {}
                }
            }
        }
        field.update(&mut t, layout.landing);
        let fresh = FlowField::new(&t, layout.landing);
        for ((x, y), _) in t.tiles() {
            assert_eq!(
                field.distance_at(x, y),
                fresh.distance_at(x, y),
                "({x}, {y})"
            );
        }
    }
}
