//! The player's ship: a `quasar-voxel` block hull that grows as modules are
//! constructed and as the player builds. Walk between stations; the nav
//! console plots FTL jumps, the fabricator builds modules, and the drop bay
//! launches the pod. In build mode the player places and removes deck, hull,
//! window and lamp blocks, paid from (and refunded to) the hold.

use crate::content::{DT, Materials, ShipBlock, UpgradeId};
use crate::math::{self, V2};
use crate::profile::{Profile, ShipEdit};
use crate::terrain::TILE;
use crate::voxel::{self, AIR, Blocks};
use quasar_math::IVec3;
use quasar_voxel::{Face, split_check};

pub const SHIP_W: i32 = 15;
pub const SHIP_H: i32 = 17;
const INTERACT_RANGE: f32 = 2.4;
const WALK_SPEED: f32 = 5.0;
const PLAYER_RADIUS: f32 = 0.45;
/// Ticks a newly built module takes to assemble.
pub const BUILD_TICKS: u32 = 90;
/// Tiles (inclusive) where free-form building is allowed.
pub const BUILD_AREA: (i32, i32, i32, i32) = (-4, -3, SHIP_W + 3, SHIP_H + 3);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Room {
    Bridge,
    Corridor,
    DropBay,
    Module(UpgradeId),
}

impl Room {
    /// Tile rectangle `(x0, y0, x1, y1)`, inclusive of floor tiles.
    pub fn rect(self) -> (i32, i32, i32, i32) {
        match self {
            Self::Bridge => (5, 1, 9, 4),
            Self::Corridor => (6, 5, 8, 10),
            Self::DropBay => (4, 11, 10, 15),
            Self::Module(u) => {
                let [x0, y0, x1, y1] = u.info().room;
                (x0, y0, x1, y1)
            }
        }
    }

    pub fn center(self) -> V2 {
        let (x0, y0, x1, y1) = self.rect();
        V2::new(
            (x0 + x1 + 1) as f32 * 0.5 * TILE,
            (y0 + y1 + 1) as f32 * 0.5 * TILE,
        )
    }

    fn contains(self, x: i32, y: i32) -> bool {
        let (x0, y0, x1, y1) = self.rect();
        (x0..=x1).contains(&x) && (y0..=y1).contains(&y)
    }

    /// Floor plus the surrounding wall ring.
    fn footprint_contains(self, x: i32, y: i32) -> bool {
        let (x0, y0, x1, y1) = self.rect();
        (x0 - 1..=x1 + 1).contains(&x) && (y0 - 1..=y1 + 1).contains(&y)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum StationKind {
    NavConsole,
    Fabricator,
    DropPod,
    /// Decorative module equipment; interacting explains the module.
    Module(UpgradeId),
}

impl StationKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::NavConsole => "Navigation Console",
            Self::Fabricator => "Fabricator",
            Self::DropPod => "Drop Pod",
            Self::Module(u) => u.name(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Station {
    pub kind: StationKind,
    pub pos: V2,
    pub yaw: f32,
}

/// Why a build action was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuildError {
    OutOfArea,
    /// Reserved for a module room.
    Reserved,
    Occupied,
    /// Walls, windows and lamps need deck below.
    NoDeck,
    /// New deck must touch existing deck.
    NotAttached,
    /// Would wall off a station or the player.
    BlocksAccess,
    /// Removing it would cut the ship in two.
    WouldSplit,
    /// The original rooms keep their floors.
    BaseFloor,
    NothingThere,
    Unaffordable,
}

impl BuildError {
    pub fn message(self) -> &'static str {
        match self {
            Self::OutOfArea => "Too far from the hull",
            Self::Reserved => "Reserved for a module room",
            Self::Occupied => "Something is already there",
            Self::NoDeck => "Needs deck plating below",
            Self::NotAttached => "Deck must connect to the ship",
            Self::BlocksAccess => "That would block a station",
            Self::WouldSplit => "That would cut the ship in two",
            Self::BaseFloor => "The original decks stay",
            Self::NothingThere => "Nothing to remove",
            Self::Unaffordable => "Not enough materials in the hold",
        }
    }
}

/// Block layout derived from built modules and the player's edits.
#[derive(Clone)]
pub struct ShipLayout {
    pub rooms: Vec<Room>,
    /// The hull as voxels (deck at `y = -1`, walls at `y = 0`).
    pub blocks: Blocks,
    /// Layout before player edits (refunds and reverts compare against it).
    base: Blocks,
    pub stations: Vec<Station>,
}

fn stamp_rooms(blocks: &mut Blocks, rooms: &[Room]) {
    let floor = |x: i32, y: i32| rooms.iter().any(|r| r.contains(x, y));
    for y in -1..=SHIP_H {
        for x in -1..=SHIP_W {
            if floor(x, y) {
                blocks.set(voxel::voxel(x, y, -1), voxel::DECK);
                blocks.set(voxel::voxel(x, y, -2), voxel::KEEL);
            } else if (-1..=1).any(|dy| (-1..=1).any(|dx| floor(x + dx, y + dy))) {
                // Bridge viewport windows along the hull's nose.
                let wall = if y == 0 && (5..=9).contains(&x) {
                    voxel::WINDOW
                } else {
                    voxel::HULL
                };
                // Walls stand on deck plating, so knocking one out opens
                // walkable floor to build outward from.
                blocks.set(voxel::voxel(x, y, 0), wall);
                blocks.set(voxel::voxel(x, y, -1), voxel::DECK);
                blocks.set(voxel::voxel(x, y, -2), voxel::KEEL);
            }
        }
    }
}

fn reserved(x: i32, y: i32) -> bool {
    UpgradeId::all().any(|u| Room::Module(u).footprint_contains(x, y))
}

fn in_area(x: i32, y: i32) -> bool {
    let (x0, y0, x1, y1) = BUILD_AREA;
    (x0..=x1).contains(&x) && (y0..=y1).contains(&y)
}

fn block_id(block: ShipBlock) -> u16 {
    match block {
        ShipBlock::Deck => voxel::DECK,
        ShipBlock::Hull => voxel::HULL,
        ShipBlock::Window => voxel::WINDOW,
        ShipBlock::Lamp => voxel::LAMP,
    }
}

fn ship_block(id: u16) -> Option<ShipBlock> {
    ShipBlock::ALL.into_iter().find(|&b| block_id(b) == id)
}

fn apply_edit(blocks: &mut Blocks, edit: &ShipEdit) {
    if reserved(edit.x, edit.z) || !in_area(edit.x, edit.z) {
        return;
    }
    let id = edit.block.map_or(AIR, block_id);
    blocks.set(voxel::voxel(edit.x, edit.z, edit.layer), id);
    if edit.layer == -1 {
        // Deck sits on its own keel block.
        let keel = if id == AIR { AIR } else { voxel::KEEL };
        blocks.set(voxel::voxel(edit.x, edit.z, -2), keel);
    }
}

impl ShipLayout {
    /// Rooms for every built module except those still in `hidden`
    /// (assembling), then the player's edits.
    pub fn build(profile: &Profile, hidden: &[UpgradeId]) -> Self {
        let mut rooms = vec![Room::Bridge, Room::Corridor, Room::DropBay];
        for upgrade in UpgradeId::all() {
            if profile.level(upgrade) > 0 && !hidden.contains(&upgrade) {
                rooms.push(Room::Module(upgrade));
            }
        }
        let mut base = Blocks::default();
        stamp_rooms(&mut base, &rooms);
        let mut blocks = base.clone();
        for edit in &profile.ship_edits {
            apply_edit(&mut blocks, edit);
        }
        let t = |x: f32, y: f32| V2::new(x * TILE, y * TILE);
        let mut stations = vec![
            Station {
                kind: StationKind::NavConsole,
                pos: t(7.5, 2.0),
                yaw: std::f32::consts::PI,
            },
            Station {
                kind: StationKind::Fabricator,
                pos: t(8.45, 7.0),
                yaw: -std::f32::consts::FRAC_PI_2,
            },
            Station {
                kind: StationKind::DropPod,
                pos: t(7.5, 13.5),
                yaw: std::f32::consts::PI,
            },
        ];
        for room in &rooms {
            if let Room::Module(u) = *room {
                let c = room.center();
                stations.push(Station {
                    kind: StationKind::Module(u),
                    pos: c,
                    yaw: if c[0] < 7.5 * TILE {
                        std::f32::consts::FRAC_PI_2
                    } else {
                        -std::f32::consts::FRAC_PI_2
                    },
                });
            }
        }
        Self {
            rooms,
            blocks,
            base,
            stations,
        }
    }

    /// Blocks a module room adds when it is constructed, for assembly effects.
    pub fn room_blocks(room: Room) -> Vec<(IVec3, u16)> {
        let mut blocks = Blocks::default();
        stamp_rooms(&mut blocks, &[room]);
        let (x0, y0, x1, y1) = room.rect();
        let mut out = Vec::new();
        for z in y0 - 1..=y1 + 1 {
            for x in x0 - 1..=x1 + 1 {
                for layer in -2..=0 {
                    let v = voxel::voxel(x, z, layer);
                    let id = blocks.get(v);
                    if id != AIR {
                        out.push((v, id));
                    }
                }
            }
        }
        out
    }

    pub fn deck(&self, x: i32, y: i32) -> bool {
        self.blocks.get(voxel::voxel(x, y, -1)) == voxel::DECK
    }

    /// The wall-layer block at a tile (`AIR` when open).
    pub fn wall(&self, x: i32, y: i32) -> u16 {
        self.blocks.get(voxel::voxel(x, y, 0))
    }

    fn walkable_tile(&self, x: i32, y: i32) -> bool {
        self.deck(x, y) && self.wall(x, y) == AIR
    }

    fn walkable(&self, p: V2) -> bool {
        let (x, y) = ((p[0] / TILE).floor() as i32, (p[1] / TILE).floor() as i32);
        self.walkable_tile(x, y)
    }

    pub fn station(&self, kind: StationKind) -> Option<&Station> {
        self.stations.iter().find(|s| s.kind == kind)
    }

    /// Solid footprints of stations, so the player walks around them.
    fn blocked_by_station(&self, p: V2) -> bool {
        self.stations.iter().any(|s| {
            let r = match s.kind {
                StationKind::DropPod => 1.3,
                StationKind::NavConsole => 0.9,
                StationKind::Fabricator => 0.8,
                StationKind::Module(_) => 0.9,
            };
            p.distance(s.pos) < r + PLAYER_RADIUS
        })
    }

    /// Whether every station tile is reachable on foot from `from`.
    fn stations_reachable(&self, from: (i32, i32)) -> bool {
        let (x0, y0, x1, y1) = BUILD_AREA;
        let width = (x1 - x0 + 1) as usize;
        let index = |x: i32, y: i32| (y - y0) as usize * width + (x - x0) as usize;
        let mut seen = vec![false; width * (y1 - y0 + 1) as usize];
        let mut stack = vec![from];
        while let Some((x, y)) = stack.pop() {
            if !in_area(x, y) || seen[index(x, y)] || !self.walkable_tile(x, y) {
                continue;
            }
            seen[index(x, y)] = true;
            stack.extend([(x + 1, y), (x - 1, y), (x, y + 1), (x, y - 1)]);
        }
        self.stations.iter().all(|s| {
            let (sx, sy) = tile_of(s.pos);
            in_area(sx, sy) && seen[index(sx, sy)]
        })
    }

    /// Edit `cell` to `value` and record it in the profile (or drop the edit
    /// when it restores the base layout).
    fn commit(
        &mut self,
        profile: &mut Profile,
        x: i32,
        z: i32,
        layer: i32,
        value: Option<ShipBlock>,
    ) {
        let edit = ShipEdit {
            x,
            z,
            layer,
            block: value,
        };
        apply_edit(&mut self.blocks, &edit);
        profile
            .ship_edits
            .retain(|e| (e.x, e.z, e.layer) != (x, z, layer));
        let base = self.base.get(voxel::voxel(x, z, layer));
        if base != value.map_or(AIR, block_id) {
            profile.ship_edits.push(edit);
        }
    }
}

fn tile_of(p: V2) -> (i32, i32) {
    ((p[0] / TILE).floor() as i32, (p[1] / TILE).floor() as i32)
}

#[derive(Clone, Debug, PartialEq)]
pub struct ShipPlayer {
    pub pos: V2,
    pub yaw: f32,
    pub walk: f32,
    pub moving: bool,
}

#[derive(Clone)]
pub struct Ship {
    pub layout: ShipLayout,
    pub player: ShipPlayer,
    /// Recently built modules still assembling, with ticks remaining.
    pub construction: Vec<(UpgradeId, u32)>,
    pub ticks: u32,
    /// The selected block while build mode is on.
    pub build: Option<ShipBlock>,
    /// Bumped whenever the hull's blocks change (for re-meshing).
    pub revision: u64,
}

impl Ship {
    pub fn new(profile: &Profile) -> Self {
        let layout = ShipLayout::build(profile, &[]);
        let spawn = V2::new(7.5 * TILE, 9.5 * TILE);
        Self {
            layout,
            player: ShipPlayer {
                pos: spawn,
                yaw: std::f32::consts::PI,
                walk: 0.0,
                moving: false,
            },
            construction: Vec::new(),
            ticks: 0,
            build: None,
            revision: 0,
        }
    }

    /// Put the player beside the drop bay after a run.
    pub fn place_at_drop_bay(&mut self) {
        self.player.pos = V2::new(7.5 * TILE, 11.6 * TILE);
        self.player.yaw = std::f32::consts::PI;
    }

    /// Re-derive the hull; rooms still assembling stay hidden until done.
    pub fn rebuild(&mut self, profile: &Profile) {
        let hidden: Vec<UpgradeId> = self.construction.iter().map(|(u, _)| *u).collect();
        self.layout = ShipLayout::build(profile, &hidden);
        self.revision += 1;
    }

    /// Begin assembling a module room; it joins the hull when complete.
    pub fn start_construction(&mut self, upgrade: UpgradeId, profile: &Profile) {
        self.construction.retain(|(u, _)| *u != upgrade);
        self.construction.push((upgrade, BUILD_TICKS));
        self.rebuild(profile);
    }

    /// 0..1 assembly progress of a module room (1 when complete).
    pub fn build_progress(&self, upgrade: UpgradeId) -> f32 {
        self.construction
            .iter()
            .find(|(u, _)| *u == upgrade)
            .map_or(1.0, |(_, t)| 1.0 - *t as f32 / BUILD_TICKS as f32)
    }

    /// Advance one tick. Returns true when a module finished assembling and
    /// the hull should be rebuilt from the profile.
    pub fn update(&mut self, movement: V2) -> bool {
        self.ticks += 1;
        for (_, t) in &mut self.construction {
            *t = t.saturating_sub(1);
        }
        let before = self.construction.len();
        self.construction.retain(|(_, t)| *t > 0);
        let finished = self.construction.len() != before;
        let movement = movement.clamp_length_max(1.0);
        let moving = movement.length() > 0.15;
        self.player.moving = moving;
        if !moving {
            return finished;
        }
        let step = movement * WALK_SPEED * DT;
        // Axis-separated sliding against walls and station footprints.
        for axis in 0..2 {
            let mut next = self.player.pos;
            next[axis] += step[axis];
            let mut probe = next;
            probe[axis] += step[axis].signum() * PLAYER_RADIUS;
            if self.layout.walkable(probe) && !self.layout.blocked_by_station(next) {
                self.player.pos = next;
            }
        }
        self.player.yaw = math::turn_toward(self.player.yaw, math::yaw_of(movement), 0.35);
        self.player.walk += WALK_SPEED * DT * 1.6;
        finished
    }

    /// The tile in front of the player (facing direction, nearest axis).
    pub fn build_cursor(&self) -> (i32, i32) {
        let (x, y) = tile_of(self.player.pos);
        let facing = math::from_yaw(self.player.yaw);
        if facing.x.abs() > facing.y.abs() {
            (x + facing.x.signum() as i32, y)
        } else {
            (x, y + facing.y.signum() as i32)
        }
    }

    /// Whether the selected block can go at the cursor (for the preview).
    pub fn can_place(&self, profile: &Profile) -> Result<(), BuildError> {
        let block = self.build.ok_or(BuildError::NothingThere)?;
        let (x, y) = self.build_cursor();
        self.check_place(block, x, y)?;
        if !profile.materials.covers(block.cost()) {
            return Err(BuildError::Unaffordable);
        }
        Ok(())
    }

    fn check_place(&self, block: ShipBlock, x: i32, y: i32) -> Result<(), BuildError> {
        if !in_area(x, y) {
            return Err(BuildError::OutOfArea);
        }
        if reserved(x, y) {
            return Err(BuildError::Reserved);
        }
        let layout = &self.layout;
        if block == ShipBlock::Deck {
            if layout.blocks.get(voxel::voxel(x, y, -1)) != AIR || layout.wall(x, y) != AIR {
                return Err(BuildError::Occupied);
            }
            let attached = [(1, 0), (-1, 0), (0, 1), (0, -1)]
                .iter()
                .any(|(dx, dy)| layout.deck(x + dx, y + dy));
            return if attached {
                Ok(())
            } else {
                Err(BuildError::NotAttached)
            };
        }
        if !layout.deck(x, y) {
            return Err(BuildError::NoDeck);
        }
        if layout.wall(x, y) != AIR {
            return Err(BuildError::Occupied);
        }
        let near_station = layout.stations.iter().any(|s| {
            let (sx, sy) = tile_of(s.pos);
            (x - sx).abs() <= 1 && (y - sy).abs() <= 1
        });
        let player = self.player.pos;
        let under_player = [V2::ZERO, V2::X, V2::NEG_X, V2::Y, V2::NEG_Y]
            .iter()
            .any(|d| tile_of(player + *d * PLAYER_RADIUS) == (x, y));
        if near_station || under_player {
            return Err(BuildError::BlocksAccess);
        }
        let mut trial = layout.clone();
        trial.blocks.set(voxel::voxel(x, y, 0), block_id(block));
        if !trial.stations_reachable(tile_of(player)) {
            return Err(BuildError::BlocksAccess);
        }
        Ok(())
    }

    /// Place the selected block at the cursor, paying from the hold.
    pub fn place(&mut self, profile: &mut Profile) -> Result<ShipBlock, BuildError> {
        self.can_place(profile)?;
        let block = self.build.expect("checked by can_place");
        let (x, y) = self.build_cursor();
        profile.materials.spend(block.cost());
        self.layout
            .commit(profile, x, y, block.layer(), Some(block));
        self.revision += 1;
        Ok(block)
    }

    /// Remove the block at the cursor (wall layer first, then deck). Blocks
    /// the player placed are refunded; original hull is salvaged for free.
    pub fn remove(&mut self, profile: &mut Profile) -> Result<Option<ShipBlock>, BuildError> {
        let (x, y) = self.build_cursor();
        if !in_area(x, y) {
            return Err(BuildError::OutOfArea);
        }
        if reserved(x, y) {
            return Err(BuildError::Reserved);
        }
        let layout = &self.layout;
        let (layer, id) = if layout.wall(x, y) != AIR {
            (0, layout.wall(x, y))
        } else if layout.deck(x, y) {
            (-1, voxel::DECK)
        } else {
            return Err(BuildError::NothingThere);
        };
        let Some(block) = ship_block(id) else {
            return Err(BuildError::NothingThere);
        };
        let placed = layout.base.get(voxel::voxel(x, y, layer)) != id;
        if layer == -1 {
            if !placed {
                return Err(BuildError::BaseFloor);
            }
            let player = tile_of(self.player.pos);
            let on_station = layout.stations.iter().any(|s| tile_of(s.pos) == (x, y));
            if player == (x, y) || on_station {
                return Err(BuildError::BlocksAccess);
            }
            let mut trial = layout.blocks.clone();
            let cell = voxel::voxel(x, y, -1);
            trial.set(cell, AIR);
            let deck = |v: &quasar_voxel::Volume<voxel::Layout>, c: IVec3| {
                c.y == -1 && v.get(trial.block, c) == voxel::DECK
            };
            let flat = |_: &quasar_voxel::Volume<voxel::Layout>, _: IVec3, face: Face| {
                !matches!(face, Face::NegY | Face::PosY)
            };
            if split_check(&trial.volume, &[cell], 4096, deck, flat).is_some() {
                return Err(BuildError::WouldSplit);
            }
        }
        if placed {
            profile.materials.add_all(block.cost());
        }
        self.layout.commit(profile, x, y, layer, None);
        self.revision += 1;
        Ok(placed.then_some(block))
    }
}

/// Total refund owed for every player-placed block (for tests and UI).
pub fn placed_value(profile: &Profile) -> Materials {
    let mut total = Materials::default();
    for edit in &profile.ship_edits {
        if let Some(block) = edit.block {
            total.add_all(block.cost());
        }
    }
    total
}

impl Ship {
    /// The station the player can use right now, if any.
    pub fn focused_station(&self) -> Option<Station> {
        self.layout
            .stations
            .iter()
            .filter(|s| {
                s.pos.distance(self.player.pos)
                    < INTERACT_RANGE
                        + if s.kind == StationKind::DropPod {
                            0.8
                        } else {
                            0.0
                        }
            })
            .min_by(|a, b| {
                a.pos
                    .distance(self.player.pos)
                    .total_cmp(&b.pos.distance(self.player.pos))
            })
            .copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drill() -> UpgradeId {
        UpgradeId::from_key("drill-workshop").unwrap()
    }

    #[test]
    fn base_ship_is_barebones() {
        let ship = Ship::new(&Profile::default());
        assert_eq!(ship.layout.rooms.len(), 3);
        assert!(ship.layout.station(StationKind::NavConsole).is_some());
        assert!(ship.layout.station(StationKind::DropPod).is_some());
        assert!(ship.layout.walkable(ship.player.pos));
    }

    #[test]
    fn modules_add_rooms() {
        let mut profile = Profile::default();
        profile.upgrades.insert(drill(), 1);
        let layout = ShipLayout::build(&profile, &[]);
        assert!(layout.rooms.contains(&Room::Module(drill())));
        let (x0, y0, ..) = Room::Module(drill()).rect();
        assert!(layout.deck(x0, y0));
        // A room still assembling is not walkable yet.
        let hidden = ShipLayout::build(&profile, &[drill()]);
        assert!(!hidden.deck(x0, y0));
    }

    #[test]
    fn walls_stop_the_player() {
        let mut ship = Ship::new(&Profile::default());
        for _ in 0..600 {
            ship.update(V2::new(1.0, 0.0));
        }
        assert!(ship.layout.walkable(ship.player.pos));
        assert!(ship.player.pos[0] < 10.0 * TILE);
    }

    #[test]
    fn can_reach_every_station() {
        let ship = Ship::new(&Profile::default());
        for station in &ship.layout.stations {
            let mut s = Ship::new(&Profile::default());
            for _ in 0..1200 {
                let to = station.pos - s.player.pos;
                if s.focused_station().is_some_and(|f| f.kind == station.kind) {
                    break;
                }
                // Walk the corridor spine first, then toward the station.
                let waypoint = if (s.player.pos[1] - station.pos[1]).abs() > 1.0 {
                    V2::new(7.5 * TILE, station.pos[1])
                } else {
                    station.pos
                };
                let dir = (waypoint - s.player.pos).normalize_or_zero();
                s.update(if to.length() > 0.1 {
                    dir
                } else {
                    V2::new(0.0, 0.0)
                });
            }
            assert_eq!(s.focused_station().map(|f| f.kind), Some(station.kind));
        }
    }

    /// A ship with a full hold, standing on the corridor facing east into
    /// the wall beside the fabricator corridor.
    fn builder() -> (Ship, Profile) {
        let profile = Profile {
            materials: Materials::new(100, 100, 0),
            tutorial_complete: true,
            ..Profile::default()
        };
        let mut ship = Ship::new(&profile);
        ship.build = Some(ShipBlock::Hull);
        (ship, profile)
    }

    fn face(ship: &mut Ship, tile: (i32, i32), yaw: f32) {
        ship.player.pos = V2::new((tile.0 as f32 + 0.5) * TILE, (tile.1 as f32 + 0.5) * TILE);
        ship.player.yaw = yaw;
    }

    #[test]
    fn building_charges_persists_and_refunds() {
        let (mut ship, mut profile) = builder();
        // Bridge floor (5..9, 1..4): face west from (6, 3) onto (5, 3).
        face(&mut ship, (6, 3), -std::f32::consts::FRAC_PI_2);
        assert_eq!(ship.build_cursor(), (5, 3));
        ship.build = Some(ShipBlock::Lamp);
        assert_eq!(ship.place(&mut profile), Ok(ShipBlock::Lamp));
        assert_eq!(profile.materials, Materials::new(99, 99, 0));
        assert_eq!(ship.layout.wall(5, 3), voxel::LAMP);
        assert_eq!(placed_value(&profile), ShipBlock::Lamp.cost());
        // The edit survives a rebuild from the profile alone.
        let reloaded = ShipLayout::build(&profile, &[]);
        assert_eq!(reloaded.wall(5, 3), voxel::LAMP);
        // Occupied now; removing refunds.
        assert_eq!(ship.place(&mut profile), Err(BuildError::Occupied));
        assert_eq!(ship.remove(&mut profile), Ok(Some(ShipBlock::Lamp)));
        assert_eq!(profile.materials, Materials::new(100, 100, 0));
        assert!(profile.ship_edits.is_empty());
    }

    #[test]
    fn deck_extends_the_hull_and_cannot_split_it() {
        let (mut ship, mut profile) = builder();
        let west = -std::f32::consts::FRAC_PI_2;
        // Knock out the bridge's west wall at (4, 3): original hull is
        // salvaged without a refund and leaves walkable deck.
        face(&mut ship, (5, 3), west);
        assert_eq!(ship.remove(&mut profile), Ok(None));
        assert!(ship.layout.walkable_tile(4, 3));
        // Build deck outward from there, two tiles.
        ship.build = Some(ShipBlock::Deck);
        face(&mut ship, (4, 3), west);
        assert_eq!(ship.place(&mut profile), Ok(ShipBlock::Deck));
        face(&mut ship, (3, 3), west);
        assert_eq!(ship.place(&mut profile), Ok(ShipBlock::Deck));
        assert!(ship.layout.deck(2, 3));
        assert_eq!(profile.materials, Materials::new(96, 100, 0));
        // Removing the middle tile would strand the outer one.
        face(&mut ship, (4, 3), west);
        assert_eq!(ship.remove(&mut profile), Err(BuildError::WouldSplit));
        // Outer first, then the middle: both refunded.
        face(&mut ship, (3, 3), west);
        assert_eq!(ship.remove(&mut profile), Ok(Some(ShipBlock::Deck)));
        face(&mut ship, (4, 3), west);
        assert_eq!(ship.remove(&mut profile), Ok(Some(ShipBlock::Deck)));
        assert_eq!(profile.materials, Materials::new(100, 100, 0));
        // New deck must touch existing deck; base floors stay.
        face(&mut ship, (3, 3), west);
        assert!(ship.place(&mut profile).is_err());
        face(&mut ship, (6, 3), west);
        assert_eq!(ship.remove(&mut profile), Err(BuildError::BaseFloor));
    }

    #[test]
    fn building_never_blocks_a_station_or_reserved_rooms() {
        let (mut ship, mut profile) = builder();
        // Next to the nav console.
        face(&mut ship, (6, 2), std::f32::consts::FRAC_PI_2);
        assert_eq!(ship.place(&mut profile), Err(BuildError::BlocksAccess));
        // Corridor (6..8, 5..10) is the only route aft: sealing it is refused.
        ship.build = Some(ShipBlock::Hull);
        let mut walled = 0;
        for x in 6..=8 {
            face(&mut ship, (x, 5), std::f32::consts::PI);
            if ship.place(&mut profile).is_ok() {
                walled += 1;
            }
        }
        assert!(walled < 3, "the corridor stays open");
        // Module rooms are reserved even before they are built.
        let (x0, y0, ..) = Room::Module(drill()).rect();
        face(&mut ship, (x0 + 1, y0), -std::f32::consts::FRAC_PI_2);
        assert_eq!(ship.place(&mut profile), Err(BuildError::Reserved));
        assert!(ship.layout.stations_reachable((7, 9)));
    }

    #[test]
    fn unaffordable_blocks_are_refused() {
        let (mut ship, mut profile) = builder();
        profile.materials = Materials::default();
        face(&mut ship, (6, 3), -std::f32::consts::FRAC_PI_2);
        assert_eq!(ship.place(&mut profile), Err(BuildError::Unaffordable));
    }
}
