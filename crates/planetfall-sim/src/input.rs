//! Device-independent player intent for one simulation tick.

use crate::content::{PlanetId, ShipUpgrade};
use crate::math::V2;
use serde::{Deserialize, Serialize};

/// A discrete choice made in a terminal or menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Command {
    /// Navigation console: FTL to a planet.
    TravelTo(PlanetId),
    /// Fabricator: build or upgrade a ship module.
    Purchase(ShipUpgrade),
    /// Level-up screen: pick one of the offered perks.
    ChoosePerk(usize),
    /// Close the open terminal (or acknowledge the debrief).
    Close,
    /// Give up on the current drop. Carried materials are lost.
    Abandon,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct GameInput {
    /// Movement on the ground plane: +x is screen-right, +z is screen-down.
    pub movement: V2,
    /// Edge-triggered: use the station / pod in front of the player (in ship
    /// build mode: place the selected block).
    pub interact: bool,
    /// Edge-triggered: short invulnerable burst of speed (in ship build mode:
    /// remove the block in front).
    pub dash: bool,
    /// Edge-triggered: call the extraction pod (in ship build mode: next block).
    pub call_extraction: bool,
    /// Edge-triggered: toggle ship build mode.
    pub build: bool,
    pub command: Option<Command>,
}
