# Gameplay

## Menus and saves

The game opens on the main menu with the ship behind it. **Continue** resumes
the last slot played, **New game** starts a commander in any of five slots
(asking before overwriting one), **Load game** lists the slots with their
location, progress and play time (and can delete one), and **Settings** holds
volumes, fullscreen, shadows, bloom and screen shake. **Settings → Controls**
rebinds the keyboard/mouse and controller buttons (select an action, press
the new button; conflicts are flagged and *Reset to defaults* restores them);
the in-game button prompts follow the new bindings. Esc / Start pauses;
from the pause menu you can open settings, abandon a drop, or save and quit to
the main menu. A drop in progress is saved too and resumes where you left it.

## The loop

1. **Ship.** Walk the ship. The **Navigation Console** plots FTL jumps, the
   **Fabricator** builds and upgrades modules, the **Locker Room** switches
   class and starting weapon, the **Drop Pod** deploys to the planet you are
   orbiting.
2. **FTL.** A short warp cinematic; the ship arrives in orbit.
3. **Drop.** The pod falls to a landing site in a generated cave.
4. **Surface.** Weapons fire automatically at the nearest hostile. Walk into
   rock to drill through it. Press Q / (LB) for the class ability. Collect XP shards to level up (pick 1 of 3 field
   upgrades; the run pauses while you choose). Walk over **supply caches** for
   a rare-or-better upgrade. Mine **lumite** to complete the objective.
5. **Extract.** Press F / (Y) any time after the objective is done. The pod
   lands at the drop site after 12 s; the horde intensifies and the threat
   climbs twice as fast. Stand inside the ring for 6 s
   (leaving drains progress) to lift off.
6. **Debrief.** Carried materials are banked in the hold and the class earns
   class XP. Death or abandoning loses all cargo.
7. **Upgrade.** Spend ferrite, lumite and voidstone at the Fabricator, or
   build onto the hull yourself in build mode.

Persistent: materials, ship modules and levels, player-built ship blocks,
stats, tutorial completion, current orbit, the active class and every
class's level, unlocks and starting weapon. Per-run only: weapons, weapon
levels, perks, XP level.

## Classes and the locker room

Four crew classes, switched any time at the **Locker Room** in the corridor.
Every class keeps its own record per save: switching Warden → Engineer →
Warden loses nothing.

| Class | Signature weapon | Ability (Q / LB) | Passive |
|---|---|---|---|
| Striker | Pulse Blaster | **Blink**: teleport ahead through enemies, damaging everything passed | **Hunter**: +25% damage to heavy enemies (100+ health, and elites), faster dash, slightly faster |
| Prospector | Seam Cutter | **Bore**: drill a straight tunnel ahead, breaking rock (with drops) and shredding enemies in it | **Deep Seams**: +50% mining, 25% chance of extra ore per vein, -15 max health |
| Warden | Scattergun | **Barrier**: a row of shock posts the horde cannot pass | **Hold the Line**: +25 max health, extraction zone fills 30% faster |
| Engineer | Orbit Drones | **Sentry Turret**: a turret that fires at the nearest hostile | **Field Repairs**: regeneration, faster ability recharge, wider pickup range |

- **Familiar weapons** (two per class) are offered three times as often in
  the weapon picks at dive levels 5, 10 and 15. Each can be chosen as the starting weapon once
  unlocked (class levels 2 and 5, or found on a dive in a later stage); the
  signature weapon then joins the pool.
- **Class XP**: everything collected on a dive plus a bonus for extracting
  (half of the collected XP, no bonus, after a failed drop). Levels grow the
  ability (tiers at class levels 4 and 8) and passive (6 and 10), unlock
  familiar starting weapons and add class perks to level-up offers.
- **Class perks** (two per class, one from class level 3) appear only for
  their class: e.g. Momentum and Executioner for the Striker, Rich Veins and
  Overclocked Drill for the Prospector.
- New commanders start as the Striker; the tutorial is unchanged.

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

Hazard scales enemy health, damage and spawn pressure; the threat level
(below) does the rest.

## Escalation: why you leave

Nothing stops you staying after the objective, except that the planet only
gets worse:

- **Threat level** rises all drop long (0.6 levels a minute, twice that
  while the pod is inbound or waiting) and shows on the HUD with the time to
  the next level. Each level adds spawn pressure (+32%) and makes new
  spawns tougher and harder-hitting (health +15%, damage +6% per level,
  growing with threat^1.3, so the late game bites). Bloaters join from
  threat 1, brutes from threat 3.
- **Surges** come every 3 threat levels: the HUD warns 8 seconds ahead,
  then a wave arrives from several sides and spawning runs hot for a few
  seconds.
- **Elites** join from threat 1 (3% of spawns per level, up to 35%): taller,
  gold-outlined, with an affix: Armored (triple health), Swift (faster),
  Frenzied (hits much harder). They drop four times the XP plus a little
  ore, and count as heavy (the Striker's bonus applies).
- **Diminishing growth**: the XP needed per level grows with the square of
  the level, so upgrades slow down while the threat keeps climbing.

Measured with the autopilot (it never extracts in that probe), a build
typical for each planet survives roughly 6 to 10 minutes: the soft cap a
drop is tuned around. The debrief shows the threat reached.

## Weapons (4 slots)

| Weapon | Behaviour | Element |
|---|---|---|
| Pulse Blaster | Striker signature. Fast bolts at the nearest target | — |
| Seam Cutter | Prospector signature. Short-range piercing cutting bolts | — |
| Scattergun | Warden signature. Short-range pellet fan | — |
| Arc Caster | Chain lightning jumping between nearby enemies | Shock |
| Plasma Mortar | Lobbed shells that burst in an area (arcs over walls) | Fire |
| Orbit Drones | Engineer signature. Blades circling the player | — |

A dive starts with the class's starting weapon. Dive levels **5, 10 and 15**
offer a choice of three new weapons (familiar ones three times as likely)
until all four slots are full. Weapons only fire with line of sight (the
mortar arcs over walls).

## Upgrades, rarity and elements

Every other level-up offers three upgrades, each rolled at a **rarity**:
common, uncommon, rare or legendary (base odds 60/27/10/3; planet hazard and
the Luck perk shift them upward). Rarity sets the amount (e.g. Hotter Rounds
+20/32/48/70% damage) and unlocks more:

- **Weapon mods** (`mods.toml`): damage, fire rate, extra projectiles,
  range, area, pierce. Every upgrade on a weapon also adds 6% damage (its
  level, shown as Lv on the HUD).
- **Elements** (rare and up): Incendiary (burn damage over time), Corrosive
  (damage over time and +25% damage taken), Cryo (chill slows, then
  freezes), Capacitor (hits may arc to a neighbour). The Arc Caster and
  Plasma Mortar have shock and fire built in. Elements do not interact yet.
- **Legendary behaviour changes**: Twin Barrel (bolts also fire at a second
  target), Double Blast (a second fan behind you), Overload (lightning
  forks), Cluster Shells (shells split into bomblets), Second Ring (a second
  drone ring), and for abilities Mobile Sentry (the turret follows you),
  Echo Step (two blink charges), Molten Bore (wider, burning bore) and Arc
  Fence (barrier posts zap further, with shock).
- **Ability mods**: Quick Charge, Amplifier, Extended.
- **Field and class perks**, scaled by rarity.

**Supply caches** offer a choice of three rare-or-better upgrades.

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

## Field perks

Reinforced Suit (max health and heal), Servo Boots (speed), Overcharge
(damage), Rapid Cycling (fire rate), Magnetic Field (pickup range), Hardened
Drill (mining), Nanite Repair (regeneration), Lucky Charm (rarer upgrades),
plus two class perks per class. Amounts scale with the rolled rarity.

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
hull when complete. Costs are in `assets/data/upgrades.toml`.

## Building the ship

After the tutorial, press B / (X) aboard the ship for build mode. The block
in front of you is outlined (green: placeable, red: not, with the reason in
the hint). E / (A) places the selected block, Space / (B) removes the one in
front, F / (Y) cycles blocks, and B / (X) leaves build mode.

| Block | Cost (`tuning.toml` `[blocks]`) | Notes |
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

## Tuning and adding content

Content and balance are TOML in `assets/data/` (shipped as
`app/assets/data/`); each file starts with a comment describing its fields.

| File | Holds |
|---|---|
| `weapons.toml` | Behaviour archetype, base stats, optional base element, held model, sound, colour |
| `enemies.toml` | Behaviour archetype, stats, spawn rules (weight, group size, first minute, late or tutorial), drops, model, blood colour, sounds |
| `planets.toml` | Unlock level, hazard, objective, ore chances, voxel palette, per-enemy spawn weights |
| `perks.toml` | Field perks: effect, common amount (scaled by rarity), weight, optional cap, optional class and class level |
| `mods.toml` | Weapon and ability upgrades: stat, amounts per rarity, minimum rarity, element or legendary special, which behaviours or abilities, max stacks |
| `classes.toml` | Classes: signature and familiar weapons, ability archetype and tiers, passive effects, unlock levels, suit colour |
| `upgrades.toml` | Ship modules: effect, per-level amount, costs, room rectangle, station model and light |
| `tuning.toml` | Player, weapon slots and pick levels, rarity odds, XP, horde, class XP, extraction, tutorial and build-block numbers |
| `models.toml` | Model names for files outside `assets/models` (Kenney props) |

To add content that fits an existing archetype, add an entry with a new `id`
and refer to models and sounds by name (generate new ones with the
`regenerate-assets` skill). New behaviour needs a new archetype variant in
`crates/planetfall-sim/src/content/defs.rs` and its system in `run/`.
Content is validated on load, so a bad file stops the game with a message
naming it. Terrain generation and ore toughness remain in `terrain.rs`.

`cargo test` checks the data (every model and sound it names exists, every
enemy can spawn on every planet), and `cargo run --release -- --headless
--autoplay --ticks 90000` is a quick balance smoke test: the autopilot should
extract far more often than it dies.
