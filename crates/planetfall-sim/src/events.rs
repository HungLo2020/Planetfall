//! Per-tick facts the presentation layer reacts to (sounds, effects, saves).
//! The simulation never depends on whether anyone listens.

use crate::content::{
    AbilityKind, ClassId, Element, EnemyId, GearSlot, Material, PlanetId, Rarity, ShipBlock,
    UpgradeId, VentKind, WeaponId,
};
use crate::math::V2;

/// Element interactions (see `[combos]`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ComboKind {
    /// A heavy hit on a frozen enemy.
    Shatter,
    /// Fire on a frozen or chilled enemy.
    Thermal,
    /// Shock arcing through wet enemies.
    Conductive,
}

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
    /// Everyone is down: the drop is lost.
    PlayerDied,
    /// One player went down (others may fight on in co-op).
    PlayerDown {
        player: usize,
    },
    Dashed,
    /// A class ability fired from `from` towards `to`.
    AbilityUsed {
        player: usize,
        kind: AbilityKind,
        from: V2,
        to: V2,
    },
    /// The ability was pressed while recharging.
    AbilityNotReady,
    // Mining and loot.
    /// A drill bite on the block face (`material` is `None` for plain rock).
    MineHit {
        at: V2,
        material: Option<Material>,
    },
    /// A bolt or pellet struck a wall.
    Impact {
        weapon: WeaponId,
        at: V2,
        height: f32,
    },
    /// A block being mined cracked further: `stage` 1 to 3 of its wear
    /// (drawn as cracks that spread until it breaks).
    BlockCracked {
        at: V2,
        material: Option<Material>,
        stage: u8,
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
    /// Equipment found on a dive (it goes to the stash, even on death).
    GearFound {
        rarity: Rarity,
        slot: GearSlot,
    },
    /// A familiar weapon blueprint found: a new starting-weapon option.
    BlueprintFound {
        weapon: WeaponId,
    },
    /// A supply cache was opened (its upgrade choice follows).
    CacheOpened {
        at: V2,
    },
    /// A rare or legendary upgrade was taken.
    RareUpgrade {
        rarity: Rarity,
    },
    /// A mission objective moved forward (parts found, ...).
    ObjectiveProgress,
    /// The optional secondary objective is done.
    SecondaryComplete,
    BossAwakened {
        at: V2,
    },
    SurvivorFound,
    SurvivorDown,
    RigStarted {
        at: V2,
    },
    RigBroken,
    /// The threat level rose.
    ThreatRose {
        level: u32,
    },
    /// A surge arrives in `seconds`.
    SurgeWarning {
        seconds: u32,
    },
    SurgeStarted {
        level: u32,
    },
    /// An element status took hold on an enemy (for effects).
    Status {
        element: Element,
        at: V2,
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
    /// FTL toward a planet, or the trade station (`None`).
    FtlStarted {
        to: Option<PlanetId>,
    },
    FtlArrived {
        at: Option<PlanetId>,
    },
    /// Gear put on or taken off in the locker room.
    Equipped,
    /// Fire set off a spore cloud.
    SporeBlast {
        at: V2,
    },
    /// A heat or cold vent pulsed.
    VentPulse {
        at: V2,
        kind: VentKind,
    },
    /// Oxygen fell below a quarter.
    OxygenLow,
    /// A quake is coming (seconds of warning).
    QuakeWarning,
    /// A quake hit near a player.
    Quake {
        at: V2,
    },
    /// A block of rubble came down.
    Rubble {
        at: V2,
    },
    /// Two elements combined on an enemy.
    Combo {
        kind: ComboKind,
        at: V2,
    },
    /// A ranged enemy spat.
    Spat {
        at: V2,
    },
    /// A disruptor's pulse went off.
    Jammed {
        at: V2,
    },
    /// A burrower dug in or came up.
    Burrow {
        at: V2,
        surfaced: bool,
    },
    /// A boss used its ability.
    BossAbility {
        at: V2,
    },
    /// A splitting elite broke apart.
    Split {
        at: V2,
    },
    /// A trade went through at the station.
    Traded,
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
    /// The locker room switched the active class.
    ClassChanged {
        class: ClassId,
    },
    /// A class gained a level at the debrief.
    ClassLevelUp {
        class: ClassId,
        level: u32,
    },
}
