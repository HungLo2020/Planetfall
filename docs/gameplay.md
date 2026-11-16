# Gameplay

## The loop

1. **Ship.** Walk the ship. The **Navigation Console** plots FTL jumps, the
   **Fabricator** builds and upgrades modules, the **Drop Pod** deploys to the
   planet you are orbiting.
2. **FTL.** A short warp cinematic; the ship arrives in orbit.
3. **Drop.** The pod falls to a landing site in a generated cave.
4. **Surface.** Weapons fire automatically at the nearest hostile. Walk into
   rock to drill through it. Collect XP shards to level up (pick 1 of 3 field
   perks; the run pauses while you choose). Walk over **weapon caches** for new
   weapons. Mine **lumite** to complete the objective.
5. **Extract.** Press F / (Y) once the objective is done. The pod lands at the
   drop site after 12 s and the horde intensifies. Stand inside the ring for 6 s
   (leaving drains progress) to lift off.
6. **Debrief.** Carried materials are banked in the hold. Death or abandoning
   loses all cargo.
7. **Upgrade.** Spend ferrite, lumite and voidstone at the Fabricator, or
   build onto the hull yourself in build mode.

Persistent: materials, ship modules and levels, player-built ship blocks,
stats, tutorial completion, current orbit. Per-run only: weapons, weapon
levels, perks, XP level.

## Tutorial

On a new profile a ship AI walks through: moving → navigation console → plot
Verdant-7 → FTL → drop pod → survive (the horde is held back until now) →
mine lumite → level up → open a weapon cache → finish the objective → call
extraction → hold the zone → debrief → build a first module. The tutorial
drop spawns half as many enemies, guarantees lumite and ferrite near the pod
and places a weapon cache close by. After the tutorial extraction the ship AI
adds hull salvage to the hold if needed, so the cheapest module is always
affordable (the debrief shows it). Dying restarts from the drop bay. Prompts
show keyboard or controller labels depending on the last device used.

## Planets

| Planet | Unlock | Hazard | Objective | Character |
|---|---|---|---|---|
| Verdant-7 | start | x1.0 | 6 lumite | Mossy caves, ferrite-rich |
| Cinder Reach | Reactor 1 | x1.45 | 9 lumite | Volcanic, dense lumite, skitter swarms |
| Hollow Frost | Reactor 2 | x2.0 | 12 lumite | Icy, voidstone seams, brutes |

Hazard scales enemy health and damage. Spawn pressure grows with time on the
surface and nearly doubles during extraction.

## Weapons (levels 1–5, max 4 carried)

| Weapon | Behaviour |
|---|---|
| Pulse Blaster | Starter. Fast bolts at the nearest target; twin bolts at 4, pierce at 5 |
| Scattergun | Short-range pellet fan; more pellets per level |
| Arc Caster | Chain lightning jumping between nearby enemies |
| Plasma Mortar | Lobbed shells that burst in an area |
| Orbit Drones | Blades circling the player, hitting anything they touch |

Weapons only fire with line of sight (the mortar arcs over walls).

## Enemies

| Enemy | Notes |
|---|---|
| Husk | Infected colonist. Slow, steady melee |
| Skitter | Fast, fragile, comes in packs and lunges |
| Bloater | Slow; detonates on contact or death, also hurting other enemies |
| Brute | Late-game tank; drops voidstone and ferrite |

Enemies follow a flow field toward the player, separate from each other and
climb out of the ground when they spawn. Weapons and enemies check line of
sight with voxel raycasts, and mining a block repairs the flow field locally.

## Field perks (on level-up)

New weapon, weapon +1, Reinforced Suit (+20 max health and heal), Servo Boots
(+10% speed), Overcharge (+15% damage), Rapid Cycling (10% faster weapons),
Magnetic Field (+1.5 m pickup range), Hardened Drill (+40% mining), Nanite
Repair (+1 HP/s).

## Ship modules (permanent)

| Module | Effect per level | Max |
|---|---|---|
| Med Bay | +25 max health | 3 |
| Armory | +12% weapon damage | 3 |
| Drill Workshop | +35% mining speed | 3 |
| Tractor Array | +1.25 m pickup range | 3 |
| Reactor & Nav Core | Unlocks the next planet | 2 |

The first purchase of each constructs a new room on the ship: its blocks
drop into place over a second and a half, and the room joins the walkable
hull when complete. Costs are in `crates/planetfall-sim/src/content.rs`.

## Building the ship

After the tutorial, press B / (X) aboard the ship for build mode. The block
in front of you is outlined (green: placeable, red: not, with the reason in
the hint). E / (A) places the selected block, Space / (B) removes the one in
front, F / (Y) cycles blocks, and B / (X) leaves build mode.

| Block | Cost | Notes |
|---|---|---|
| Deck plating | 2 ferrite | Extends the hull; must touch existing deck |
| Hull wall | 2 ferrite | Cut-away height like the original walls |
| Window | 1 ferrite, 1 lumite | Translucent glass panel |
| Lamp | 1 ferrite, 1 lumite | Emissive fixture that lights the deck |

Rules: building stays within a margin around the hull; module rooms (built
or not) are reserved; nothing may be placed next to a station, under you, or
where it would wall any station off from you; deck can only be removed if the
hull stays in one piece; the original room floors stay. Blocks you placed are
refunded when removed; original hull walls can be knocked out (for free, no
refund) to open rooms or build outward. Edits are saved with the profile.

## Tuning

All numbers live in `content.rs` (planets, weapons, enemies, upgrades),
`run.rs` (spawn pacing, dash, extraction timings) and `terrain.rs` (cave
generation, ore toughness). `cargo run --release -- --headless --autoplay
--ticks 90000` is a quick balance smoke test: the autopilot should extract
far more often than it dies.
