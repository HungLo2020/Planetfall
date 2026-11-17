//! Cave layout generation for planet surfaces: one flat floor level of
//! organic chambers joined by winding tunnels, with set-piece rooms for the
//! mission, ore seams that follow the rock and floor patches.
//!
//! 1. Nodes: the landing chamber in the middle, the mission sites (spread
//!    around it, the first one far out) and a few random chambers.
//! 2. Edges: a minimum spanning tree over the nodes plus a few extra links
//!    that close loops, so the horde (and the player) can circle around.
//! 3. Carving: each chamber is a blob with a wobbly radius; each edge is a
//!    tunnel that wanders toward its target and swells and narrows.
//! 4. Detail: value noise opens alcoves along the walls, then a cellular
//!    automaton smooths the result without closing chamber and tunnel cores.
//! 5. Set pieces: the main site becomes a round arena with rock pillars for
//!    cover; the landing stays clear.
//! 6. Connectivity: every open pocket is either linked to the landing by a
//!    tunnel or filled in, so the pod, the objectives and extraction are
//!    always reachable without mining.
//! 7. Ore seams: rock near the floor along noise ridges, the material chosen
//!    by region and depth (voidstone deeper), at the planet's ore density.
//!
//! The style knobs are per planet (`[planet.terrain]` in `planets.toml`).

use crate::content::{Material, PlanetId, TerrainStyle};
use crate::math::V2;
use quasar_core::Rng;
use std::f32::consts::TAU;

/// What each cell of the generated map holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cell {
    Floor,
    Rock,
    Ore(Material),
    Bedrock,
}

/// A generated map (row-major, `width * height` cells).
pub struct Cave {
    pub width: i32,
    pub height: i32,
    pub cells: Vec<Cell>,
    /// Floor cells covered by a planet patch (moss, ash, frost).
    pub patches: Vec<bool>,
    /// Floor cells of lava channels and shallow water (planet hazards).
    pub lava: Vec<bool>,
    pub water: Vec<bool>,
    pub landing: (i32, i32),
    /// Mission sites, the set-piece arena first.
    pub sites: Vec<(i32, i32)>,
}

/// Radius of the main set-piece room in cells.
pub const ARENA_RADIUS: f32 = 5.5;
/// Radius kept clear around the landing site.
const LANDING_RADIUS: f32 = 4.5;
/// Cells kept as rock around the map edge (plus the bedrock rim).
const MARGIN: i32 = 3;

/// Seeded 2D value noise with fractal octaves, in [0, 1].
#[derive(Clone, Copy)]
pub struct Noise {
    seed: u32,
}

impl Noise {
    pub fn new(seed: u64, salt: u32) -> Self {
        Self {
            seed: (seed as u32)
                ^ ((seed >> 32) as u32).rotate_left(7)
                ^ salt.wrapping_mul(0x9e37_79b9),
        }
    }

    pub fn hash(&self, x: i32, y: i32) -> f32 {
        let mut h =
            (x as u32).wrapping_mul(0x85eb_ca77) ^ (y as u32).wrapping_mul(0xc2b2_ae3d) ^ self.seed;
        h ^= h >> 15;
        h = h.wrapping_mul(0x2c1b_3c6d);
        h ^= h >> 12;
        h = h.wrapping_mul(0x297a_2d39);
        h ^= h >> 15;
        (h & 0x00ff_ffff) as f32 / 16_777_215.0
    }

    fn value(&self, x: f32, y: f32) -> f32 {
        let (x0, y0) = (x.floor(), y.floor());
        let (fx, fy) = (x - x0, y - y0);
        let (sx, sy) = (fx * fx * (3.0 - 2.0 * fx), fy * fy * (3.0 - 2.0 * fy));
        let (ix, iy) = (x0 as i32, y0 as i32);
        let a = self.hash(ix, iy);
        let b = self.hash(ix + 1, iy);
        let c = self.hash(ix, iy + 1);
        let d = self.hash(ix + 1, iy + 1);
        let top = a + (b - a) * sx;
        let bottom = c + (d - c) * sx;
        top + (bottom - top) * sy
    }

    /// Fractal noise at `frequency` (cycles per cell) with `octaves`.
    pub fn fbm(&self, x: f32, y: f32, frequency: f32, octaves: u32) -> f32 {
        let (mut sum, mut amp, mut norm, mut f) = (0.0, 1.0, 0.0, frequency);
        for o in 0..octaves {
            let shift = o as f32 * 17.31;
            sum += self.value(x * f + shift, y * f - shift) * amp;
            norm += amp;
            amp *= 0.5;
            f *= 2.0;
        }
        sum / norm
    }
}

struct Grid {
    w: i32,
    h: i32,
    open: Vec<bool>,
    /// Cells smoothing must never close (chamber and tunnel cores).
    keep: Vec<bool>,
}

impl Grid {
    fn idx(&self, x: i32, y: i32) -> usize {
        (y * self.w + x) as usize
    }

    fn inside(&self, x: i32, y: i32) -> bool {
        x >= MARGIN && y >= MARGIN && x < self.w - MARGIN && y < self.h - MARGIN
    }

    fn open_at(&self, x: i32, y: i32) -> bool {
        x >= 0 && y >= 0 && x < self.w && y < self.h && self.open[self.idx(x, y)]
    }

    fn set(&mut self, x: i32, y: i32, open: bool, keep: bool) {
        if self.inside(x, y) {
            let i = self.idx(x, y);
            self.open[i] = open;
            self.keep[i] |= keep;
        }
    }

    fn disc(&mut self, c: V2, r: f32, keep_r: f32) {
        let (x0, x1) = ((c[0] - r).floor() as i32, (c[0] + r).ceil() as i32);
        let (y0, y1) = ((c[1] - r).floor() as i32, (c[1] + r).ceil() as i32);
        for y in y0..=y1 {
            for x in x0..=x1 {
                let d = V2::new(x as f32, y as f32).distance(c);
                if d <= r + 0.15 {
                    self.set(x, y, true, d <= keep_r);
                }
            }
        }
    }

    /// An organic chamber: a disc whose radius wobbles with the angle.
    fn blob(&mut self, c: V2, r: f32, roughness: f32, rng: &mut Rng) {
        let phases: [f32; 4] = std::array::from_fn(|_| rng.range_f32(0.0..TAU));
        let reach = r * (1.0 + roughness) + 1.0;
        let (x0, x1) = ((c[0] - reach).floor() as i32, (c[0] + reach).ceil() as i32);
        let (y0, y1) = ((c[1] - reach).floor() as i32, (c[1] + reach).ceil() as i32);
        for y in y0..=y1 {
            for x in x0..=x1 {
                let d = V2::new(x as f32, y as f32) - c;
                let a = d[1].atan2(d[0]);
                let wobble = 0.45 * (2.0 * a + phases[0]).sin()
                    + 0.3 * (3.0 * a + phases[1]).sin()
                    + 0.15 * (5.0 * a + phases[2]).sin()
                    + 0.1 * (7.0 * a + phases[3]).sin();
                let edge = r * (1.0 + roughness * wobble);
                let len = d.length();
                if len <= edge {
                    self.set(x, y, true, len <= edge * 0.55);
                }
            }
        }
    }

    /// A tunnel wandering from `a` toward `b`.
    fn tunnel(&mut self, a: V2, b: V2, width: [f32; 2], rng: &mut Rng) {
        let base = rng.range_f32(width[0]..width[1].max(width[0] + 0.01));
        let phase = rng.range_f32(0.0..TAU);
        let lo = MARGIN as f32 + 1.0;
        let (hx, hy) = ((self.w - MARGIN - 2) as f32, (self.h - MARGIN - 2) as f32);
        let mut p = a;
        let to = b - a;
        let mut heading = to[1].atan2(to[0]);
        for step in 0..600 {
            let to = b - p;
            if to.length() < 1.0 {
                break;
            }
            let desired = to[1].atan2(to[0]);
            let mut turn = (desired - heading).rem_euclid(TAU);
            if turn > std::f32::consts::PI {
                turn -= TAU;
            }
            // Wander more when far away, home in when close.
            let pull = if to.length() < 6.0 { 0.6 } else { 0.22 };
            heading += turn * pull + rng.range_f32(-0.5..0.5);
            p += V2::new(heading.cos(), heading.sin()) * 0.75;
            p = V2::new(p[0].clamp(lo, hx), p[1].clamp(lo, hy));
            let r = base * (1.0 + 0.35 * (step as f32 * 0.27 + phase).sin());
            self.disc(p, r, 0.5);
        }
        // Finish straight if the walk ran out of steps.
        let gap = b - p;
        let steps = (gap.length() / 0.5).ceil() as i32;
        for i in 0..=steps {
            let q = p + gap * (i as f32 / steps.max(1) as f32);
            self.disc(q, base, 0.5);
        }
    }

    fn open_neighbours(&self, x: i32, y: i32) -> u32 {
        let mut n = 0;
        for dy in -1..=1 {
            for dx in -1..=1 {
                if (dx != 0 || dy != 0) && self.open_at(x + dx, y + dy) {
                    n += 1;
                }
            }
        }
        n
    }

    /// Cellular smoothing: fill lonely open cells and open cells walled in
    /// by floor, never closing kept cores.
    fn smooth(&mut self, passes: u32) {
        for _ in 0..passes {
            let mut next = self.open.clone();
            for y in MARGIN..self.h - MARGIN {
                for x in MARGIN..self.w - MARGIN {
                    let i = self.idx(x, y);
                    let n = self.open_neighbours(x, y);
                    if self.open[i] && n <= 2 && !self.keep[i] {
                        next[i] = false;
                    } else if !self.open[i] && n >= 6 {
                        next[i] = true;
                    }
                }
            }
            self.open = next;
        }
    }

    /// Flood fill over open cells; returns a label per cell (0 = rock).
    fn components(&self) -> (Vec<u32>, u32) {
        let mut label = vec![0u32; self.open.len()];
        let mut next = 0;
        for start in 0..self.open.len() {
            if !self.open[start] || label[start] != 0 {
                continue;
            }
            next += 1;
            let mut stack = vec![start];
            label[start] = next;
            while let Some(i) = stack.pop() {
                let (x, y) = (i as i32 % self.w, i as i32 / self.w);
                for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                    let (nx, ny) = (x + dx, y + dy);
                    if self.open_at(nx, ny) {
                        let j = self.idx(nx, ny);
                        if label[j] == 0 {
                            label[j] = next;
                            stack.push(j);
                        }
                    }
                }
            }
        }
        (label, next)
    }

    /// Link every pocket worth keeping to the landing's region and fill the
    /// rest, until one region remains.
    fn connect(&mut self, landing: (i32, i32), rng: &mut Rng, width: [f32; 2]) {
        for _ in 0..64 {
            let (label, count) = self.components();
            let main = label[self.idx(landing.0, landing.1)];
            if count <= 1 {
                return;
            }
            let mut sizes = vec![0usize; count as usize + 1];
            let mut kept = vec![false; count as usize + 1];
            for (i, &l) in label.iter().enumerate() {
                sizes[l as usize] += 1;
                kept[l as usize] |= self.keep[i];
            }
            let cells: Vec<(i32, i32, u32)> = label
                .iter()
                .enumerate()
                .filter(|(_, l)| **l != 0)
                .map(|(i, &l)| (i as i32 % self.w, i as i32 / self.w, l))
                .collect();
            // The first stray pocket: link it if it matters, else fill it.
            let Some(&(_, _, stray)) = cells.iter().find(|c| c.2 != main) else {
                return;
            };
            if sizes[stray as usize] < 10 && !kept[stray as usize] {
                for &(x, y, l) in &cells {
                    if l == stray {
                        let i = self.idx(x, y);
                        self.open[i] = false;
                    }
                }
                continue;
            }
            // Closest pair of cells between the pocket and the main region.
            let mut best = (i32::MAX, (0, 0), (0, 0));
            for &(x, y, l) in &cells {
                if l != stray {
                    continue;
                }
                for &(mx, my, ml) in &cells {
                    if ml == main {
                        let d = (mx - x).pow(2) + (my - y).pow(2);
                        if d < best.0 {
                            best = (d, (x, y), (mx, my));
                        }
                    }
                }
            }
            let (a, b) = (best.1, best.2);
            self.tunnel(
                V2::new(a.0 as f32, a.1 as f32),
                V2::new(b.0 as f32, b.1 as f32),
                [width[0].min(1.0), width[0].max(1.0)],
                rng,
            );
        }
    }
}

/// Minimum spanning tree (Prim) over node positions, plus extra edges that
/// close loops with chance `loops`.
fn links(nodes: &[(V2, f32)], loops: f32, rng: &mut Rng) -> Vec<(usize, usize)> {
    let n = nodes.len();
    let mut edges = Vec::new();
    let mut joined = vec![false; n];
    joined[0] = true;
    for _ in 1..n {
        let mut best = (f32::MAX, 0, 0);
        for i in (0..n).filter(|&i| joined[i]) {
            for j in (0..n).filter(|&j| !joined[j]) {
                let d = nodes[i].0.distance(nodes[j].0);
                if d < best.0 {
                    best = (d, i, j);
                }
            }
        }
        joined[best.2] = true;
        edges.push((best.1, best.2));
    }
    for i in 0..n {
        if !rng.chance(f64::from(loops)) {
            continue;
        }
        let linked = |j: usize, edges: &Vec<(usize, usize)>| {
            edges
                .iter()
                .any(|&(a, b)| (a, b) == (i, j) || (a, b) == (j, i))
        };
        let nearest = (0..n)
            .filter(|&j| j != i && !linked(j, &edges))
            .min_by(|&a, &b| {
                nodes[i]
                    .0
                    .distance(nodes[a].0)
                    .total_cmp(&nodes[i].0.distance(nodes[b].0))
            });
        if let Some(j) = nearest {
            edges.push((i, j));
        }
    }
    edges
}

/// Generate the cave for `planet`. `sites` mission sites are placed; the
/// tutorial map is smaller in spirit (fewer chambers, no loops).
pub fn generate(
    planet: PlanetId,
    size: i32,
    rng: &mut Rng,
    seed: u64,
    sites: usize,
    tutorial: bool,
) -> Cave {
    let info = planet.info();
    let style: &TerrainStyle = &info.terrain;
    let (w, h) = (size, size);
    let mut grid = Grid {
        w,
        h,
        open: vec![false; (w * h) as usize],
        keep: vec![false; (w * h) as usize],
    };
    let landing = (w / 2, h / 2);
    let center = V2::new(landing.0 as f32, landing.1 as f32);
    let half = (size / 2) as f32;

    // 1. Nodes: landing, mission sites, extra chambers.
    let mut nodes: Vec<(V2, f32)> = vec![(center, LANDING_RADIUS)];
    let first = rng.range_f32(0.0..TAU);
    let mut site_cells = Vec::new();
    for i in 0..sites {
        let a = first + i as f32 * TAU / sites as f32 + rng.range_f32(-0.45..0.45);
        let (dist, r) = if i == 0 {
            (half * 0.74, ARENA_RADIUS)
        } else {
            (half * rng.range_f32(0.5..0.72), rng.range_f32(3.2..4.4))
        };
        let lim = MARGIN as f32 + r + 1.5;
        let p = center + V2::new(a.cos(), a.sin()) * dist;
        let p = V2::new(
            p[0].clamp(lim, w as f32 - 1.0 - lim),
            p[1].clamp(lim, h as f32 - 1.0 - lim),
        );
        site_cells.push((p[0].round() as i32, p[1].round() as i32));
        nodes.push((V2::new(p[0].round(), p[1].round()), r));
    }
    let [lo, hi] = style.chambers;
    let wanted = if tutorial {
        lo.min(4)
    } else {
        lo + rng.range_u32(0..hi.saturating_sub(lo) + 1)
    };
    let mut tries = 0;
    let mut extra = 0;
    while extra < wanted && tries < 600 {
        tries += 1;
        let r = rng.range_f32(
            style.chamber_radius[0]..style.chamber_radius[1].max(style.chamber_radius[0] + 0.01),
        );
        let lim = MARGIN as f32 + r + 1.0;
        let p = V2::new(
            rng.range_f32(lim..w as f32 - 1.0 - lim),
            rng.range_f32(lim..h as f32 - 1.0 - lim),
        );
        if nodes
            .iter()
            .all(|(q, qr)| q.distance(p) > (r + qr) * 0.8 + 1.0)
        {
            nodes.push((p, r));
            extra += 1;
        }
    }

    // 2-3. Link and carve.
    let loops = if tutorial { 0.0 } else { style.loops };
    for (a, b) in links(&nodes, loops, rng) {
        grid.tunnel(nodes[a].0, nodes[b].0, style.tunnel_width, rng);
    }
    for (i, &(p, r)) in nodes.iter().enumerate() {
        let roughness = if i == 0 {
            style.roughness * 0.5
        } else {
            style.roughness
        };
        grid.blob(p, r, roughness, rng);
    }

    // 4. Alcoves along walls, then smoothing.
    let noise = Noise::new(seed, 0xa1c0);
    if style.alcoves > 0.0 {
        let threshold = 1.0 - style.alcoves * 0.45;
        let mut next = grid.open.clone();
        for y in MARGIN..h - MARGIN {
            for x in MARGIN..w - MARGIN {
                let near = grid.open_neighbours(x, y) > 0;
                if near && noise.fbm(x as f32, y as f32, 0.13, 3) > threshold {
                    next[grid.idx(x, y)] = true;
                }
            }
        }
        grid.open = next;
    }
    grid.smooth(2);

    // 5. Set pieces: a clean round arena with four rock pillars, a clear
    // landing, and a clear core at every other site.
    grid.disc(center, LANDING_RADIUS - 0.5, LANDING_RADIUS - 0.5);
    for (i, &(sx, sy)) in site_cells.iter().enumerate() {
        let c = V2::new(sx as f32, sy as f32);
        if i == 0 {
            grid.disc(c, ARENA_RADIUS, ARENA_RADIUS);
        } else {
            grid.disc(c, 2.6, 2.6);
        }
    }
    if let Some(&(sx, sy)) = site_cells.first() {
        let turn = rng.range_f32(0.0..TAU);
        for k in 0..4 {
            let a = turn + k as f32 * TAU / 4.0;
            let px = sx + (a.cos() * 3.4).round() as i32;
            let py = sy + (a.sin() * 3.4).round() as i32;
            let i = grid.idx(px, py);
            grid.open[i] = false;
            grid.keep[i] = false;
        }
    }

    // 6. One connected floor.
    grid.connect(landing, rng, style.tunnel_width);

    // 7. Ore seams.
    let mut cells: Vec<Cell> = (0..w * h)
        .map(|i| {
            let (x, y) = (i % w, i / w);
            if x < 1 || y < 1 || x >= w - 1 || y >= h - 1 {
                Cell::Bedrock
            } else if grid.open[i as usize] {
                Cell::Floor
            } else {
                Cell::Rock
            }
        })
        .collect();
    let idx = |x: i32, y: i32| (y * w + x) as usize;
    // Distance to the floor (1 = touching), up to 2.
    let depth = |cells: &Vec<Cell>, x: i32, y: i32| -> Option<u32> {
        for d in 1..=2 {
            for dy in -d..=d {
                for dx in -d..=d {
                    let (nx, ny) = (x + dx, y + dy);
                    if (dx.abs() == d || dy.abs() == d)
                        && nx >= 0
                        && ny >= 0
                        && nx < w
                        && ny < h
                        && cells[idx(nx, ny)] == Cell::Floor
                        && (d > 1 || dx == 0 || dy == 0)
                    {
                        return Some(d as u32);
                    }
                }
            }
        }
        None
    };
    let seam = Noise::new(seed, 0x5ea3);
    let warp = Noise::new(seed, 0x3a7b);
    let region = Noise::new(seed, 0x7e61);
    let mut candidates: Vec<(f32, i32, i32)> = Vec::new();
    let mut exposed = 0usize;
    for y in 1..h - 1 {
        for x in 1..w - 1 {
            if cells[idx(x, y)] != Cell::Rock {
                continue;
            }
            let Some(d) = depth(&cells, x, y) else {
                continue;
            };
            if d == 1 {
                exposed += 1;
            }
            let v = seam.fbm(x as f32, y as f32, 0.09, 3);
            let ridge = 1.0 - (2.0 * v - 1.0).abs();
            candidates.push((ridge - 0.1 * (d as f32 - 1.0), x, y));
        }
    }
    candidates.sort_by(|a, b| b.0.total_cmp(&a.0).then((a.1, a.2).cmp(&(b.1, b.2))));
    let density = info.ore.ferrite + info.ore.lumite + info.ore.voidstone;
    let count = ((exposed as f32) * density * 0.75 * style.seams) as usize;
    for &(_, x, y) in candidates.iter().take(count) {
        // Material by warped region, voidstone more likely deeper out.
        let (wx, wy) = (
            x as f32 + 4.0 * (warp.fbm(x as f32, y as f32, 0.1, 2) - 0.5),
            y as f32 + 4.0 * (warp.fbm(y as f32, x as f32, 0.1, 2) - 0.5),
        );
        let roll = region.hash((wx / 6.0).floor() as i32, (wy / 6.0).floor() as i32);
        let far = (V2::new(x as f32, y as f32).distance(center) / half).min(1.4);
        let weights = [
            (Material::Ferrite, info.ore.ferrite * (1.25 - 0.5 * far)),
            (Material::Lumite, info.ore.lumite),
            (Material::Voidstone, info.ore.voidstone * (0.2 + 1.6 * far)),
        ];
        let total: f32 = weights.iter().map(|(_, w)| w.max(0.0)).sum();
        let mut acc = roll * total;
        let mut material = Material::Ferrite;
        for (m, wgt) in weights {
            if acc < wgt.max(0.0) {
                material = m;
                break;
            }
            acc -= wgt.max(0.0);
        }
        cells[idx(x, y)] = Cell::Ore(material);
    }

    // Floor patches (moss, ash, frost) where noise is high.
    let patch_noise = Noise::new(seed, 0x9a7c);
    let patches = (0..w * h)
        .map(|i| {
            let (x, y) = (i % w, i / w);
            cells[i as usize] == Cell::Floor
                && patch_noise.fbm(x as f32, y as f32, 0.11, 3) > 1.0 - style.patches * 0.5
        })
        .collect();

    // Hazard ground: lava runs in thin channels along noise ridges, water
    // pools in blobs; both stay clear of the landing and the mission sites.
    let hazards = &info.hazards;
    let clear_of = |x: i32, y: i32| {
        let p = V2::new(x as f32, y as f32);
        p.distance(center) > LANDING_RADIUS + 2.5
            && site_cells
                .iter()
                .all(|&(sx, sy)| p.distance(V2::new(sx as f32, sy as f32)) > 3.5)
    };
    let lava_noise = Noise::new(seed, 0x1a7a);
    let water_noise = Noise::new(seed, 0x3a7e);
    let mut lava = vec![false; (w * h) as usize];
    let mut water = vec![false; (w * h) as usize];
    if !tutorial {
        for y in 0..h {
            for x in 0..w {
                let i = (y * w + x) as usize;
                if cells[i] != Cell::Floor || !clear_of(x, y) {
                    continue;
                }
                if hazards.lava > 0.0 {
                    let v = lava_noise.fbm(x as f32, y as f32, 0.07, 2);
                    let ridge = 1.0 - (2.0 * v - 1.0).abs();
                    if ridge > 1.0 - hazards.lava * 0.45 {
                        lava[i] = true;
                        continue;
                    }
                }
                if hazards.water > 0.0
                    && water_noise.fbm(x as f32, y as f32, 0.09, 3) > 1.0 - hazards.water * 1.8
                {
                    water[i] = true;
                }
            }
        }
    }

    Cave {
        width: w,
        height: h,
        cells,
        patches,
        lava,
        water,
        landing,
        sites: site_cells,
    }
}
