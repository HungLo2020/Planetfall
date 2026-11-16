//! Planet surface: a block grid of floor and minable walls, cave generation,
//! circle collision, mining, and a horde flow field built on `quasar-nav`.

use crate::content::{Material, PlanetId};
use crate::math::{V2, distance};
use crate::rng::Rng;
use quasar_nav::{Grid, GridCell, Neighbor};

/// World size of one block, in meters.
pub const TILE: f32 = 2.0;
pub const MAP_SIZE: u32 = 52;

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
            Self::Rock { .. } => Self::rock().max_hp_inner(),
            Self::Ore { material, .. } => Self::ore(material).max_hp_inner(),
            _ => 0.0,
        }
    }

    fn max_hp_inner(self) -> f32 {
        match self {
            Self::Rock { hp } | Self::Ore { hp, .. } => hp,
            _ => 0.0,
        }
    }
}

/// What breaking a block produced.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Broken {
    pub tile: (i32, i32),
    pub was: Tile,
}

pub struct Terrain {
    pub width: u32,
    pub height: u32,
    tiles: Vec<Tile>,
    nav: Grid<2>,
    /// Incremented whenever a block is broken.
    pub revision: u64,
}

/// Layout facts the run needs from generation.
pub struct Layout {
    pub landing: V2,
    pub caches: Vec<V2>,
}

impl Terrain {
    pub fn tile(&self, x: i32, y: i32) -> Tile {
        if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
            return Tile::Bedrock;
        }
        self.tiles[(y as u32 * self.width + x as u32) as usize]
    }

    fn set(&mut self, x: i32, y: i32, tile: Tile) {
        let index = (y as u32 * self.width + x as u32) as usize;
        self.tiles[index] = tile;
        let _ = self.nav.set_blocked(GridCell::new2(x, y), tile.solid());
    }

    pub fn tiles(&self) -> impl Iterator<Item = ((i32, i32), Tile)> + '_ {
        self.tiles.iter().enumerate().map(|(i, &t)| {
            (
                (
                    (i as u32 % self.width) as i32,
                    (i as u32 / self.width) as i32,
                ),
                t,
            )
        })
    }

    pub fn world_size(&self) -> V2 {
        [self.width as f32 * TILE, self.height as f32 * TILE]
    }

    pub fn tile_of(p: V2) -> (i32, i32) {
        ((p[0] / TILE).floor() as i32, (p[1] / TILE).floor() as i32)
    }

    pub fn center_of(x: i32, y: i32) -> V2 {
        [(x as f32 + 0.5) * TILE, (y as f32 + 0.5) * TILE]
    }

    pub fn solid_at(&self, p: V2) -> bool {
        let (x, y) = Self::tile_of(p);
        self.tile(x, y).solid()
    }

    /// Damage a minable block. Returns the broken tile when it gives way.
    pub fn mine(&mut self, x: i32, y: i32, amount: f32) -> Option<Broken> {
        let tile = self.tile(x, y);
        let next = match tile {
            Tile::Rock { hp } => Tile::Rock { hp: hp - amount },
            Tile::Ore { material, hp } => Tile::Ore {
                material,
                hp: hp - amount,
            },
            _ => return None,
        };
        let remaining = match next {
            Tile::Rock { hp } | Tile::Ore { hp, .. } => hp,
            _ => unreachable!(),
        };
        if remaining <= 0.0 {
            self.set(x, y, Tile::Floor);
            self.revision += 1;
            Some(Broken {
                tile: (x, y),
                was: tile,
            })
        } else {
            let index = (y as u32 * self.width + x as u32) as usize;
            self.tiles[index] = next;
            None
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
                    let min = [x as f32 * TILE, y as f32 * TILE];
                    let closest = [
                        p[0].clamp(min[0], min[0] + TILE),
                        p[1].clamp(min[1], min[1] + TILE),
                    ];
                    let d = [p[0] - closest[0], p[1] - closest[1]];
                    let dist = (d[0] * d[0] + d[1] * d[1]).sqrt();
                    if dist >= radius {
                        continue;
                    }
                    let depth = radius - dist;
                    if dist > 1e-5 {
                        p[0] += d[0] / dist * depth;
                        p[1] += d[1] / dist * depth;
                    } else {
                        // Center inside the block: push toward the tile we came from.
                        let c = Self::center_of(x, y);
                        let away = crate::math::normalize(crate::math::sub(p, c));
                        let away = if away == [0.0, 0.0] { [0.0, 1.0] } else { away };
                        p[0] += away[0] * (radius + TILE * 0.5);
                        p[1] += away[1] * (radius + TILE * 0.5);
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

    /// Whether a straight segment stays out of solid blocks (sampled).
    pub fn line_clear(&self, a: V2, b: V2) -> bool {
        let steps = (distance(a, b) / (TILE * 0.25)).ceil().max(1.0) as u32;
        (0..=steps).all(|i| {
            let t = i as f32 / steps as f32;
            !self.solid_at([a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t])
        })
    }

    pub fn nav(&self) -> &Grid<2> {
        &self.nav
    }

    pub fn count_ore(&self, material: Material) -> usize {
        self.tiles
            .iter()
            .filter(|t| matches!(t, Tile::Ore { material: m, .. } if *m == material))
            .count()
    }

    /// Generate a cave for `planet`. Deterministic for a given seed.
    pub fn generate(planet: PlanetId, seed: u64, tutorial: bool) -> (Self, Layout) {
        let info = planet.info();
        let mut rng = Rng::new(seed);
        let (w, h) = (MAP_SIZE as i32, MAP_SIZE as i32);
        let mut solid = vec![false; (w * h) as usize];
        let idx = |x: i32, y: i32| (y * w + x) as usize;
        for y in 0..h {
            for x in 0..w {
                let border = x < 2 || y < 2 || x >= w - 2 || y >= h - 2;
                solid[idx(x, y)] = border || rng.chance(0.47);
            }
        }
        for _ in 0..5 {
            let mut next = solid.clone();
            for y in 2..h - 2 {
                for x in 2..w - 2 {
                    let mut walls = 0;
                    for dy in -1..=1 {
                        for dx in -1..=1 {
                            if (dx != 0 || dy != 0) && solid[idx(x + dx, y + dy)] {
                                walls += 1;
                            }
                        }
                    }
                    next[idx(x, y)] = walls > 4 || (walls == 4 && solid[idx(x, y)]);
                }
            }
            solid = next;
        }
        let (lx, ly) = (w / 2, h / 2);
        let clear = |solid: &mut Vec<bool>, cx: i32, cy: i32, r: i32| {
            for y in cy - r..=cy + r {
                for x in cx - r..=cx + r {
                    if x > 1
                        && y > 1
                        && x < w - 2
                        && y < h - 2
                        && (x - cx).pow(2) + (y - cy).pow(2) <= r * r + 1
                    {
                        solid[idx(x, y)] = false;
                    }
                }
            }
        };
        clear(&mut solid, lx, ly, 4);
        // Carve a few tunnels from the landing site so the cave is explorable.
        for _ in 0..5 {
            let angle = rng.range(0.0, std::f32::consts::TAU);
            let (mut x, mut y) = (lx as f32, ly as f32);
            let len = rng.range(10.0, 20.0) as i32;
            let mut a = angle;
            for _ in 0..len {
                a += rng.range(-0.5, 0.5);
                x = (x + a.cos()).clamp(3.0, (w - 4) as f32);
                y = (y + a.sin()).clamp(3.0, (h - 4) as f32);
                clear(&mut solid, x as i32, y as i32, 1);
            }
        }
        // Keep only the floor region connected to the landing site.
        let mut reachable = vec![false; solid.len()];
        let mut stack = vec![(lx, ly)];
        while let Some((x, y)) = stack.pop() {
            if x < 0 || y < 0 || x >= w || y >= h || solid[idx(x, y)] || reachable[idx(x, y)] {
                continue;
            }
            reachable[idx(x, y)] = true;
            stack.extend([(x + 1, y), (x - 1, y), (x, y + 1), (x, y - 1)]);
        }
        let mut tiles = vec![Tile::rock(); solid.len()];
        for y in 0..h {
            for x in 0..w {
                let border = x < 1 || y < 1 || x >= w - 1 || y >= h - 1;
                tiles[idx(x, y)] = if border {
                    Tile::Bedrock
                } else if reachable[idx(x, y)] {
                    Tile::Floor
                } else {
                    Tile::rock()
                };
            }
        }
        // Ore veins on walls that touch open floor, seeded and grown into clusters.
        let exposed = |tiles: &Vec<Tile>, x: i32, y: i32| {
            [(1, 0), (-1, 0), (0, 1), (0, -1)].iter().any(|(dx, dy)| {
                let (nx, ny) = (x + dx, y + dy);
                nx >= 0 && ny >= 0 && nx < w && ny < h && tiles[idx(nx, ny)] == Tile::Floor
            })
        };
        let mut seeds = Vec::new();
        for y in 1..h - 1 {
            for x in 1..w - 1 {
                if !matches!(tiles[idx(x, y)], Tile::Rock { .. }) || !exposed(&tiles, x, y) {
                    continue;
                }
                let roll = rng.f32();
                let mut acc = 0.0;
                for (i, material) in Material::ALL.iter().enumerate() {
                    acc += info.ore[i] * 0.45;
                    if roll < acc {
                        seeds.push((x, y, *material));
                        break;
                    }
                }
            }
        }
        for (x, y, material) in seeds {
            tiles[idx(x, y)] = Tile::ore(material);
            for _ in 0..rng.below(3) {
                let (dx, dy) = [(1, 0), (-1, 0), (0, 1), (0, -1)][rng.below(4)];
                let (nx, ny) = (x + dx, y + dy);
                if nx > 0
                    && ny > 0
                    && nx < w - 1
                    && ny < h - 1
                    && matches!(tiles[idx(nx, ny)], Tile::Rock { .. })
                {
                    tiles[idx(nx, ny)] = Tile::ore(material);
                }
            }
        }
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
                        cx + rng.below((2 * r + 1) as usize) as i32 - r,
                        cy + rng.below((2 * r + 1) as usize) as i32 - r,
                    )
                } else {
                    (
                        1 + rng.below((w - 2) as usize) as i32,
                        1 + rng.below((h - 2) as usize) as i32,
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
        let objective = if tutorial { 4 } else { info.objective as usize };
        ensure(&mut tiles, &mut rng, Material::Lumite, 3, Some((lx, ly, 7)));
        ensure(
            &mut tiles,
            &mut rng,
            Material::Ferrite,
            4,
            Some((lx, ly, 7)),
        );
        ensure(&mut tiles, &mut rng, Material::Lumite, objective + 8, None);

        let mut nav = Grid::<2>::from_blocked(
            [MAP_SIZE, MAP_SIZE],
            tiles.iter().map(|t| t.solid()).collect(),
        )
        .expect("map dimensions are valid");
        nav.set_diagonal(true);
        nav.set_corner_cutting(false);

        let landing = Self::center_of(lx, ly);
        // Weapon caches: one close by (always in the tutorial), the rest far apart.
        let mut caches: Vec<V2> = Vec::new();
        let mut candidates: Vec<V2> = floor.iter().map(|&(x, y)| Self::center_of(x, y)).collect();
        // Deterministic shuffle.
        for i in (1..candidates.len()).rev() {
            let j = rng.below(i + 1);
            candidates.swap(i, j);
        }
        if let Some(near) = candidates
            .iter()
            .copied()
            .filter(|&c| (8.0..14.0).contains(&distance(c, landing)))
            .min_by(|a, b| distance(*a, landing).total_cmp(&distance(*b, landing)))
        {
            caches.push(near);
        }
        let wanted = if tutorial { 3 } else { 4 };
        for c in candidates {
            if caches.len() >= wanted {
                break;
            }
            if distance(c, landing) > 22.0 && caches.iter().all(|&o| distance(o, c) > 20.0) {
                caches.push(c);
            }
        }
        (
            Self {
                width: MAP_SIZE,
                height: MAP_SIZE,
                tiles,
                nav,
                revision: 0,
            },
            Layout { landing, caches },
        )
    }
}

/// Distance-to-player field over the nav grid. Hordes walk downhill.
pub struct FlowField {
    distance: Vec<f32>,
    width: i32,
    height: i32,
    origin: (i32, i32),
    revision: u64,
    ready: bool,
}

impl FlowField {
    pub fn new(terrain: &Terrain) -> Self {
        Self {
            distance: vec![f32::INFINITY; (terrain.width * terrain.height) as usize],
            width: terrain.width as i32,
            height: terrain.height as i32,
            origin: (-1, -1),
            revision: u64::MAX,
            ready: false,
        }
    }

    /// Recompute when the target moved to a new cell or the terrain changed.
    pub fn update(&mut self, terrain: &Terrain, target: V2) {
        let origin = Terrain::tile_of(target);
        if self.ready && origin == self.origin && terrain.revision == self.revision {
            return;
        }
        self.origin = origin;
        self.revision = terrain.revision;
        self.ready = true;
        self.distance.fill(f32::INFINITY);
        let grid = terrain.nav();
        let start = GridCell::new2(origin.0, origin.1);
        let Ok(start_index) = grid.index(start) else {
            return;
        };
        // Dijkstra over quasar-nav's neighbor rules (diagonals without corner cutting).
        let mut heap = std::collections::BinaryHeap::new();
        self.distance[start_index] = 0.0;
        heap.push(Visit(0.0, start));
        let mut neighbors: Vec<Neighbor<GridCell<2>>> = Vec::with_capacity(8);
        while let Some(Visit(d, cell)) = heap.pop() {
            let Ok(i) = grid.index(cell) else { continue };
            if d > self.distance[i] {
                continue;
            }
            neighbors.clear();
            grid.neighbors_into(cell, &mut neighbors);
            for n in &neighbors {
                let Ok(j) = grid.index(n.node) else { continue };
                let nd = d + n.cost;
                if nd < self.distance[j] {
                    self.distance[j] = nd;
                    heap.push(Visit(nd, n.node));
                }
            }
        }
    }

    pub fn distance_at(&self, x: i32, y: i32) -> f32 {
        if x < 0 || y < 0 || x >= self.width || y >= self.height {
            return f32::INFINITY;
        }
        self.distance[(y * self.width + x) as usize]
    }

    /// The world point an agent at `p` should walk toward.
    pub fn next_waypoint(&self, terrain: &Terrain, p: V2) -> Option<V2> {
        let (x, y) = Terrain::tile_of(p);
        let here = self.distance_at(x, y);
        let mut best: Option<((i32, i32), f32)> = None;
        for dy in -1..=1 {
            for dx in -1..=1 {
                if dx == 0 && dy == 0 {
                    continue;
                }
                let (nx, ny) = (x + dx, y + dy);
                if dx != 0
                    && dy != 0
                    && (terrain.tile(x + dx, y).solid() || terrain.tile(x, y + dy).solid())
                {
                    continue;
                }
                let d = self.distance_at(nx, ny);
                if d.is_finite() && d < here && best.is_none_or(|(_, b)| d < b) {
                    best = Some(((nx, ny), d));
                }
            }
        }
        best.map(|((nx, ny), _)| Terrain::center_of(nx, ny))
    }
}

#[derive(PartialEq)]
struct Visit(f32, GridCell<2>);
impl Eq for Visit {}
impl PartialOrd for Visit {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Visit {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        other
            .0
            .total_cmp(&self.0)
            .then_with(|| other.1.cmp(&self.1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generation_is_deterministic_and_playable() {
        for planet in PlanetId::ALL {
            let (a, layout) = Terrain::generate(planet, 42, false);
            let (b, _) = Terrain::generate(planet, 42, false);
            assert!(a.tiles().eq(b.tiles()));
            assert!(!a.solid_at(layout.landing));
            assert!(a.count_ore(Material::Lumite) >= planet.info().objective as usize);
            assert!(!layout.caches.is_empty());
            for cache in &layout.caches {
                assert!(!a.solid_at(*cache));
            }
        }
    }

    #[test]
    fn mining_opens_navigation() {
        let (mut t, layout) = Terrain::generate(PlanetId::Verdant, 3, true);
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
        assert!(!t.nav().is_blocked(GridCell::new2(x, y)));
        assert_eq!(t.revision, 1);
        let mut field = FlowField::new(&t);
        field.update(&t, layout.landing);
        let (lx, ly) = Terrain::tile_of(layout.landing);
        assert_eq!(field.distance_at(lx, ly), 0.0);
    }

    #[test]
    fn circles_are_pushed_out_of_walls() {
        let (t, _) = Terrain::generate(PlanetId::Verdant, 9, false);
        let ((x, y), _) = t.tiles().find(|(_, tile)| *tile == Tile::Bedrock).unwrap();
        let inside = Terrain::center_of(x, y);
        let probe = [inside[0] + TILE * 0.5 + 0.1, inside[1]];
        let (resolved, hit) = t.resolve_circle(probe, 0.4);
        assert!(hit.is_some() || resolved != probe || !t.tile(x + 1, y).solid());
    }
}
