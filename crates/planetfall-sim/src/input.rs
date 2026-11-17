//! Device-independent player intent for one simulation tick.

use crate::content::{ClassId, GearSlot, Material, PlanetId, Rarity, UpgradeId, WeaponId};
use crate::math::V2;
use serde::{Deserialize, Serialize};

/// A discrete choice made in a terminal or menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Command {
    /// Navigation console: FTL to a planet.
    TravelTo(PlanetId),
    /// Navigation console: FTL to the trade station and dock.
    Dock,
    /// Fabricator: build or upgrade a ship module.
    Purchase(UpgradeId),
    /// Level-up screen: pick one of the offered perks.
    ChoosePerk(usize),
    /// Close the open terminal (or acknowledge the debrief).
    Close,
    /// Give up on the current drop. Carried materials are lost.
    Abandon,
    /// Mission board: take contract `contract` at hazard level `hazard`.
    Deploy { contract: usize, hazard: u8 },
    /// Locker room: switch the active class.
    SelectClass(ClassId),
    /// Locker room: the active class's starting weapon (its signature or an
    /// unlocked familiar weapon).
    SetStartWeapon(WeaponId),
    /// Locker room: wear a stash item on the active class.
    Equip(u32),
    /// Locker room: take off the active class's item in a slot.
    Unequip(GearSlot),
    /// Trade station: sell a stash item for credits.
    Sell(u32),
    /// Trade station: buy an item off the shelf.
    Buy(usize),
    /// Trade station: reroll an item's affixes.
    Reroll(u32),
    /// Trade station: fuse unworn items of a rarity into one of the next.
    Fuse(Rarity),
    /// Trade station: trade one lot of a material for credits (or back).
    TradeMaterial { material: Material, buy: bool },
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
    /// Edge-triggered: use the class ability.
    pub ability: bool,
    pub command: Option<Command>,
}
