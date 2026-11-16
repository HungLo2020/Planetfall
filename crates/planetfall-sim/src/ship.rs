//! The player's ship: a block-built hub that grows as modules are constructed.
//! Walk between stations; the nav console plots FTL jumps, the fabricator
//! builds modules, and the drop bay launches the pod.

use crate::content::{DT, ShipUpgrade};
use crate::math::{self, V2};
use crate::profile::Profile;
use crate::terrain::TILE;

pub const SHIP_W: i32 = 15;
pub const SHIP_H: i32 = 17;
const INTERACT_RANGE: f32 = 2.4;
const WALK_SPEED: f32 = 5.0;
const PLAYER_RADIUS: f32 = 0.45;
/// Ticks a newly built module takes to assemble visually.
pub const BUILD_TICKS: u32 = 90;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShipTile {
    Space,
    Floor,
    Wall,
    Window,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Room {
    Bridge,
    Corridor,
    DropBay,
    Module(ShipUpgrade),
}

impl Room {
    /// Tile rectangle `(x0, y0, x1, y1)`, inclusive of floor tiles.
    pub fn rect(self) -> (i32, i32, i32, i32) {
        match self {
            Self::Bridge => (5, 1, 9, 4),
            Self::Corridor => (6, 5, 8, 10),
            Self::DropBay => (4, 11, 10, 15),
            Self::Module(ShipUpgrade::HullPlating) => (1, 5, 5, 7),
            Self::Module(ShipUpgrade::WeaponCalibration) => (9, 5, 13, 7),
            Self::Module(ShipUpgrade::DrillRig) => (1, 8, 5, 10),
            Self::Module(ShipUpgrade::TractorArray) => (9, 8, 13, 10),
            Self::Module(ShipUpgrade::NavComputer) => (11, 11, 13, 15),
        }
    }

    pub fn center(self) -> V2 {
        let (x0, y0, x1, y1) = self.rect();
        [
            (x0 + x1 + 1) as f32 * 0.5 * TILE,
            (y0 + y1 + 1) as f32 * 0.5 * TILE,
        ]
    }

    fn contains(self, x: i32, y: i32) -> bool {
        let (x0, y0, x1, y1) = self.rect();
        (x0..=x1).contains(&x) && (y0..=y1).contains(&y)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum StationKind {
    NavConsole,
    Fabricator,
    DropPod,
    /// Decorative module equipment; interacting explains the module.
    Module(ShipUpgrade),
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

/// Static layout derived from which modules are built.
#[derive(Clone, Debug, PartialEq)]
pub struct ShipLayout {
    pub rooms: Vec<Room>,
    tiles: Vec<ShipTile>,
    pub stations: Vec<Station>,
}

impl ShipLayout {
    pub fn build(profile: &Profile) -> Self {
        let mut rooms = vec![Room::Bridge, Room::Corridor, Room::DropBay];
        for upgrade in ShipUpgrade::ALL {
            if profile.level(upgrade) > 0 {
                rooms.push(Room::Module(upgrade));
            }
        }
        let mut tiles = vec![ShipTile::Space; (SHIP_W * SHIP_H) as usize];
        let floor = |x: i32, y: i32| rooms.iter().any(|r| r.contains(x, y));
        for y in 0..SHIP_H {
            for x in 0..SHIP_W {
                let i = (y * SHIP_W + x) as usize;
                if floor(x, y) {
                    tiles[i] = ShipTile::Floor;
                } else if (-1..=1).any(|dy| (-1..=1).any(|dx| floor(x + dx, y + dy))) {
                    tiles[i] = ShipTile::Wall;
                }
            }
        }
        // Bridge viewport windows along the hull's nose.
        for x in 5..=9 {
            let i = x as usize;
            if tiles[i] == ShipTile::Wall {
                tiles[i] = ShipTile::Window;
            }
        }
        let t = |x: f32, y: f32| [x * TILE, y * TILE];
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
            tiles,
            stations,
        }
    }

    pub fn tile(&self, x: i32, y: i32) -> ShipTile {
        if x < 0 || y < 0 || x >= SHIP_W || y >= SHIP_H {
            return ShipTile::Space;
        }
        self.tiles[(y * SHIP_W + x) as usize]
    }

    pub fn tiles(&self) -> impl Iterator<Item = ((i32, i32), ShipTile)> + '_ {
        self.tiles
            .iter()
            .enumerate()
            .map(|(i, &t)| (((i as i32) % SHIP_W, (i as i32) / SHIP_W), t))
    }

    fn walkable(&self, p: V2) -> bool {
        let (x, y) = ((p[0] / TILE).floor() as i32, (p[1] / TILE).floor() as i32);
        self.tile(x, y) == ShipTile::Floor
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
            math::distance(p, s.pos) < r + PLAYER_RADIUS
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ShipPlayer {
    pub pos: V2,
    pub yaw: f32,
    pub walk: f32,
    pub moving: bool,
}

pub struct Ship {
    pub layout: ShipLayout,
    pub player: ShipPlayer,
    /// Recently built modules still assembling, with ticks remaining.
    pub construction: Vec<(ShipUpgrade, u32)>,
    pub ticks: u32,
}

impl Ship {
    pub fn new(profile: &Profile) -> Self {
        let layout = ShipLayout::build(profile);
        let spawn = [7.5 * TILE, 9.5 * TILE];
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
        }
    }

    /// Put the player beside the drop bay after a run.
    pub fn place_at_drop_bay(&mut self) {
        self.player.pos = [7.5 * TILE, 11.6 * TILE];
        self.player.yaw = std::f32::consts::PI;
    }

    pub fn rebuild(&mut self, profile: &Profile) {
        self.layout = ShipLayout::build(profile);
    }

    pub fn start_construction(&mut self, upgrade: ShipUpgrade) {
        self.construction.retain(|(u, _)| *u != upgrade);
        self.construction.push((upgrade, BUILD_TICKS));
    }

    /// 0..1 assembly progress of a module room (1 when complete).
    pub fn build_progress(&self, upgrade: ShipUpgrade) -> f32 {
        self.construction
            .iter()
            .find(|(u, _)| *u == upgrade)
            .map_or(1.0, |(_, t)| 1.0 - *t as f32 / BUILD_TICKS as f32)
    }

    pub fn update(&mut self, movement: V2) {
        self.ticks += 1;
        for (_, t) in &mut self.construction {
            *t = t.saturating_sub(1);
        }
        self.construction.retain(|(_, t)| *t > 0);
        let movement = math::clamp_unit(movement);
        let moving = math::length(movement) > 0.15;
        self.player.moving = moving;
        if !moving {
            return;
        }
        let step = math::scale(movement, WALK_SPEED * DT);
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
    }

    /// The station the player can use right now, if any.
    pub fn focused_station(&self) -> Option<Station> {
        self.layout
            .stations
            .iter()
            .filter(|s| {
                math::distance(s.pos, self.player.pos)
                    < INTERACT_RANGE
                        + if s.kind == StationKind::DropPod {
                            0.8
                        } else {
                            0.0
                        }
            })
            .min_by(|a, b| {
                math::distance(a.pos, self.player.pos)
                    .total_cmp(&math::distance(b.pos, self.player.pos))
            })
            .copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        profile.upgrades.drill_rig = 1;
        let layout = ShipLayout::build(&profile);
        assert!(layout.rooms.contains(&Room::Module(ShipUpgrade::DrillRig)));
        let (x0, y0, ..) = Room::Module(ShipUpgrade::DrillRig).rect();
        assert_eq!(layout.tile(x0, y0), ShipTile::Floor);
    }

    #[test]
    fn walls_stop_the_player() {
        let mut ship = Ship::new(&Profile::default());
        for _ in 0..600 {
            ship.update([1.0, 0.0]);
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
                let to = math::sub(station.pos, s.player.pos);
                if s.focused_station().is_some_and(|f| f.kind == station.kind) {
                    break;
                }
                // Walk the corridor spine first, then toward the station.
                let waypoint = if (s.player.pos[1] - station.pos[1]).abs() > 1.0 {
                    [7.5 * TILE, station.pos[1]]
                } else {
                    station.pos
                };
                let dir = math::normalize(math::sub(waypoint, s.player.pos));
                s.update(if math::length(to) > 0.1 {
                    dir
                } else {
                    [0.0, 0.0]
                });
            }
            assert_eq!(s.focused_station().map(|f| f.kind), Some(station.kind));
        }
    }
}
