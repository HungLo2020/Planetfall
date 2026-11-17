# Planetfall

A top-down sci-fi roguelite in a 3D block world. Your home is a barebones
salvage ship that grows module by module. Plot an FTL jump, ride a drop pod to
a hostile planet, mine ore while hordes close in, and get back to the pod
before they overwhelm you. Whatever you carry home pays for permanent ship
upgrades; field weapons and perks stay on the planet.

> **Core loop:** build aboard the ship → navigation console → FTL → drop pod →
> survive, mine, loot → extract → fabricate upgrades → repeat.

Planetfall is written in Rust on the [Quasar](https://github.com/HungLo2020/Quasar)
engine crates and is packaged as a [Basalt](https://github.com/HungLo2020/Basalt)
game. See [IDEA.md](IDEA.md) for the original concept.

Coding standards: [CODING-STANDARDS.md](https://github.com/HungLo2020/LinuxScripts/blob/master/CODING-STANDARDS.md)

## Play

```sh
cargo run --release
```

| Action | Keyboard / mouse | Controller |
|---|---|---|
| Move | WASD / arrows | Left stick / D-pad |
| Use station / deploy | E / Enter | A (South) |
| Dash (brief invulnerability) | Space / Shift | B (East) / RB |
| Class ability | Q | LB / RT |
| Call extraction pod | F | Y (North) |
| Ship build mode (after the tutorial) | B | X (West) |
| Build: place / remove / next block | E / Space / F | A / B / Y |
| Pause / back | Esc | Start / B |
| Menus | Arrows, Enter, mouse | D-pad, A, B |
| Fullscreen | F11 | — |

Weapons aim and fire automatically. Walk into rock to drill through it.
Rebind keys and buttons under Settings → Controls.

### What is in this first pass

- **Main menu and five save slots**: continue, new game, load or delete a
  slot, settings (volumes, fullscreen, shadows, bloom, screen shake) and a
  controls screen that rebinds keyboard and controller buttons.
  Progress autosaves, and quitting mid-drop resumes the drop later.
- **Four classes**: Striker, Prospector, Warden and Engineer, each with a
  signature weapon, familiar weapons, an active ability (blink, bore,
  barrier, turret) and a passive. Switch in the ship's locker room; every
  class keeps its own level, unlocks and starting weapon.
- **Tutorial**: a ship AI walks a new commander from the barebones ship through
  the nav console, the first FTL jump and drop, mining, level-ups, a weapon
  cache, extraction and the first module build. It only runs once.
- **Ship hub**: a walkable voxel ship with a navigation console, fabricator
  and drop bay. Five modules (Med Bay, Armory, Drill Workshop, Tractor Array,
  Reactor & Nav Core) are *constructed* as new rooms whose blocks drop into
  place when you first buy them, then upgrade further.
- **Free-form building**: in build mode, place and remove deck plating, hull
  walls, windows and lamps around the ship, paid from the hold and refunded
  on removal; the hull can never be split or a station walled off.
- **Five planets**: Verdant-7, Cinder Reach, Hollow Frost, Tidewell Marsh and
  Kharon's Rest, each with its own palette, ore mix, hazards and boss pool;
  later worlds unlock with the Reactor.
- **Hazards**: lava channels, shallow water, spore clouds that fire
  ignites, heat and cold vents, thin air with an oxygen meter (only on
  oxygen-poor worlds and sites) and quakes that bring rubble down.
- **Contracts**: a mission board of random contracts per planet: lumite
  quotas, boss kills, rescues, drill defence, salvage and elite hunts, with
  optional secondary objectives, site conditions, mutators and hazard levels
  1 to 5 that unlock as you succeed.
- **Surface runs**: organic voxel caves of minable blocks (winding tunnels,
  looping chambers, a pillared set-piece arena, ore seams, per-planet floor
  patches and props); ferrite,
  lumite and voidstone ore; eight enemy types by role (husks, skitters,
  bloaters, brutes, ranged spitters, jamming disruptors, burrowers and
  flyers) driven by a flow field, elites with five affixes and five bosses;
  twelve auto-firing weapons across eleven archetypes (bolts, scatter, arcs,
  mortars, drones, flamer with heat, ricochet, mines, melee, homing and
  gravity) in four slots (new picks at levels 5, 10 and 15); rarity-rolled
  upgrades with elements (fire, acid, cold, shock) that combine (shatter,
  thermal shock, conductive water, acid fuel, spore ignition) and legendary
  behaviour changes; supply caches; a threat
  level that climbs all drop long, with elites and warned surge waves, so
  greed decides when to leave; a timed extraction where
  the pod lands at your drop site and you must hold the zone.
- **Equipment and trade**: four gear slots per class (helmet, suit, boots,
  gadget) filled with randomised items (rarity, base stat and up to four
  affixes) found in caches and on elites and bosses, kept even on death and
  equipped in the locker room; familiar weapon blueprints; the Kestrel
  Exchange trade station to sell, buy, reroll, fuse and trade materials for
  credits.
- **Persistence**: each slot (`data/save/slot-N.json`) holds materials,
  modules, ship edits, stats, tutorial state and any drop in progress;
  settings are TOML in `data/config/`. Dying or abandoning a drop loses
  everything carried.
- **Data-driven content**: weapons, enemies, planets, perks, ship modules and
  tuning are TOML files in `assets/data/` (see [Gameplay](docs/gameplay.md#tuning-and-adding-content)).
- **Presentation**: original low-poly GLB models (plus CC0 Kenney props) with
  GPU-skinned crowds and authored clips, a suit per class and a silhouette
  per enemy role and boss, per-planet weather (spores, ash, snow, fireflies,
  dust), embers over lava, splashes, frost and drips on frozen and wet
  enemies, element combo bursts, voxel terrain and hull with fog on
  blocks and meshes alike, directional shadows over meshes and voxels,
  clustered point lights, GPU particles (blood, rock dust, ore glints,
  explosions), scorch and blood decals, outlines on characters hidden behind
  rock, HDR bloom, filmic tonemapping and see-through effects, FTL and drop-pod
  cinematics, synthesized sound effects and music, and a shaped-font HUD and
  menus with device-aware button prompts.

## Command line

Every feature runs without a window, which is how CI and agents drive the game.

```sh
cargo run --release -- --help
# Simulate 10 minutes with the autopilot and print a summary line.
cargo run --release -- --headless --autoplay --ticks 36000
# Render the state after N ticks offscreen (GPU, no window).
cargo run --release -- --headless --autoplay --ticks 2400 --screenshot shot.png
# Regenerate the Basalt cover art.
cargo run --release -- --cover assets/basalt/cover.png
# Simulation-only build with no graphics, window or audio dependencies.
cargo run --no-default-features -- --headless --autoplay
```

Other flags: `--seed N`, `--fresh` (new unsaved game, skipping the menu),
`--slot N` (load slot N, skipping the menu), `--no-save`,
`--trace` (print the tick of every phase and menu change), `--menu main|saves|settings|controls|pause|gear|trade` (screenshot a menu; gear and trade open the locker's gear tab or the station market on the ship), `--frames N`, `--asset-root DIR`, and Quasar's common `--fullscreen`,
`--windowed`, `--resolution WxH`, `--no-audio`, `--audio-device`, `--log-level`.

## Build, verify and package

Requires Rust 1.94+, a Vulkan 1.3 driver to play, and on Debian/Ubuntu:
`build-essential pkg-config libudev-dev libasound2-dev libxkbcommon-dev libwayland-dev libvulkan-dev`.

```sh
python3 DevUtils/verify.py          # fmt, clippy, tests, headless autopilot soak
python3 DevUtils/verify.py --gpu    # plus offscreen screenshots in dist/screenshots
python3 DevUtils/package.py         # dist/Planetfall-<version>-linux-x64.zip
```

Releases are published by the manual **Release** workflow, which replaces the
single rolling `Latest` GitHub Release that Basalt installs from (see
[Basalt integration](docs/basalt.md)).

## Documentation

- [Design](docs/design.md): the vision, the three loops, classes, missions,
  equipment and the roadmap for the next stages.
- [Architecture](docs/architecture.md): crates, the simulation/presentation split
  and how Quasar is used.
- [Gameplay](docs/gameplay.md): the loop, tutorial, planets, weapons, enemies,
  upgrades and tuning.
- [Assets](docs/assets.md): generated models and sounds, third-party licenses,
  regeneration.
- [Basalt integration](docs/basalt.md): data layout, packaging, catalog entry
  ([planetfall.json](planetfall.json)) and the compliance checklist.
- [DevUtils](DevUtils/README.md): development scripts.

## AGENTS

Instructions for AI agents working in this repository. `AGENTS.md` is a symlink
to this README.

- Follow the [coding standards](https://github.com/HungLo2020/LinuxScripts/blob/master/CODING-STANDARDS.md)
  and the [Basalt Game Standard](https://github.com/HungLo2020/Basalt/blob/main/STANDARDS.md).
- Only change this repository. Quasar and Basalt are upstream dependencies:
  Quasar is pinned by git tag in the root `Cargo.toml`; the Basalt catalog entry
  lives here as `planetfall.json` and is copied into Basalt by its maintainer.
- Prefer Quasar over hand-rolled code: math (`quasar-math`), RNG
  (`quasar-core`), replay (`quasar-sim`), voxels (`quasar-voxel*`), paths
  (`quasar-nav`), animation (`quasar-animation`), UI layout and prompts
  (`quasar-ui`, `quasar-input`). Record gaps and workarounds in
  [QUASAR-ISSUES.md](QUASAR-ISSUES.md).
- Game rules live in `crates/planetfall-sim` and must stay free of window, GPU,
  audio and file I/O. Presentation (rendering, UI, audio, input, storage) lives
  in the root `planetfall` crate. New gameplay must be drivable through
  `GameInput`/`Command` and exercised by the `Autopilot` or a test.
- Never write into `app/` at runtime or outside the install root; persistence
  goes through `src/storage.rs` (`quasar-storage`, atomic writes). Bump
  `PROFILE_VERSION` and extend `Profile::migrate` when the save format changes.
- Before finishing a change run `python3 DevUtils/verify.py` (add `--gpu` for
  visual changes and inspect the screenshots). Keep docs in `docs/` accurate.
- Models and sounds are generated: edit `DevUtils/assets/*.py` and regenerate
  rather than hand-editing files in `assets/models` or `assets/sounds`.
- Project skills live in `.agents/skills/` (`.claude/skills` links there):
  `package-release`, `regenerate-assets`, and `commit-and-release` (commit,
  push, wait for CI, dispatch Release, verify the published zip). Run
  `commit-and-release` only when explicitly asked to commit, push or release.
