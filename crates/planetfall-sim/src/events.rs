//! Per-tick facts the presentation layer reacts to (sounds, effects, saves).
//! The simulation never depends on whether anyone listens.

use crate::content::{EnemyId, Material, PlanetId, ShipBlock, UpgradeId, WeaponId};
use crate::math::V2;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GameEvent {
    // Weapons and combat.
    Fired {
        weapon: WeaponId,
        at: V2,
    },
    EnemyHit {
        at: V2,
    },
    EnemyKilled {
        kind: EnemyId,
        at: V2,
    },
    EnemyAlerted {
        kind: EnemyId,
        at: V2,
    },
    Explosion {
        at: V2,
        radius: f32,
    },
    PlayerHurt {
        amount: f32,
    },
    PlayerDied,
    Dashed,
    // Mining and loot.
    MineHit {
        at: V2,
    },
    BlockBroken {
        at: V2,
        material: Option<Material>,
    },
    PickedUpXp,
    PickedUpMaterial {
        material: Material,
        amount: u32,
    },
    LevelUp {
        level: u32,
    },
    WeaponFound {
        weapon: WeaponId,
    },
    ObjectiveComplete,
    // Extraction.
    ExtractionCalled,
    ExtractionDenied,
    PodLanded {
        at: V2,
    },
    Liftoff,
    // Ship and travel.
    TerminalOpened,
    TerminalClosed,
    Denied,
    UpgradeBuilt {
        upgrade: UpgradeId,
        level: u8,
    },
    FtlStarted {
        to: PlanetId,
    },
    FtlArrived {
        at: PlanetId,
    },
    DescentStarted,
    Landed,
    RunEnded {
        extracted: bool,
    },
    TutorialStep,
    TutorialComplete,
    /// Persistent profile changed and should be saved.
    ProfileChanged,
    /// Ship build mode switched on or off.
    BuildMode {
        on: bool,
    },
    BlockPlaced {
        block: ShipBlock,
    },
    BlockRemoved,
}
