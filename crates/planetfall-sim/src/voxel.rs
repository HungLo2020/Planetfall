//! Block IDs and the voxel volume shape shared by the planet surface and the
//! ship (`quasar-voxel`). One voxel is one `TILE`-sized block: the ground
//! layer sits at `y = -1` (top surface at height 0), walls and furniture at
//! `y = 0`, and tall bedrock reaches `y = 1`. Simulation `(x, z)` tiles map
//! to voxel `(x, 0, z)`.

use quasar_math::IVec3;
use quasar_voxel::{ChannelId, Cube16, Schema, StorageKind, Volume};

pub use quasar_voxel::AIR;

/// Planet ground, checkerboard light tile.
pub const GROUND: u16 = 1;
/// Planet ground, checkerboard dark tile.
pub const GROUND_ALT: u16 = 2;
pub const ROCK: u16 = 3;
pub const FERRITE_ORE: u16 = 4;
pub const LUMITE_ORE: u16 = 5;
pub const VOIDSTONE_ORE: u16 = 6;
pub const BEDROCK: u16 = 7;
/// Ship deck plating (walkable floor).
pub const DECK: u16 = 8;
/// Ship hull wall (cut-away height so the camera sees into rooms).
pub const HULL: u16 = 9;
pub const WINDOW: u16 = 10;
pub const LAMP: u16 = 11;
/// Dark underside below decks and walls.
pub const KEEL: u16 = 12;
/// Number of block IDs (for catalog tables).
pub const BLOCK_COUNT: u16 = 13;

pub type Layout = Cube16;

/// A block volume with its single `block` channel.
#[derive(Clone)]
pub struct Blocks {
    pub volume: Volume<Layout>,
    pub block: ChannelId<u16>,
}

impl Default for Blocks {
    fn default() -> Self {
        let mut schema = Schema::new();
        let block = schema.channel("block", StorageKind::Paletted, AIR);
        Self {
            volume: Volume::new(schema),
            block,
        }
    }
}

impl Blocks {
    pub fn get(&self, voxel: IVec3) -> u16 {
        self.volume.get(self.block, voxel)
    }

    pub fn set(&mut self, voxel: IVec3, value: u16) {
        self.volume.set(self.block, voxel, value);
    }
}

/// Voxel of a ground-plane tile at `layer` (0 = wall height, -1 = ground).
pub fn voxel(x: i32, z: i32, layer: i32) -> IVec3 {
    IVec3::new(x, layer, z)
}
