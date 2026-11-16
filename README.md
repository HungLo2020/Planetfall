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
| Move | WASD / arrows | Left stick |
| Use station / deploy | E / Enter | A (South) |
| Dash (brief invulnerability) | Space / Shift | B (East) / RB |
| Call extraction pod | F | Y (North) |
| Pause / back | Esc | Start / B |
| Menus | Arrows, Enter, mouse | D-pad, A, B |
| Fullscreen | F11 | — |

Weapons aim and fire automatically. Walk into rock to drill through it.

### What is in this first pass

- **Tutorial**: a ship AI walks a new commander from the barebones ship through
  the nav console, the first FTL jump and drop, mining, level-ups, a weapon
  cache, extraction and the first module build. It only runs once.
- **Ship hub**: a walkable block-built ship with a navigation console,
  fabricator and drop bay. Five modules (Med Bay, Armory, Drill Workshop,
  Tractor Array, Reactor & Nav Core) are *constructed* as new rooms that
  assemble block by block when you first buy them, then upgrade further.
- **Three planets**: Verdant-7, Cinder Reach and Hollow Frost, each with its own
  palette, ore mix, hazard level and objective; later worlds unlock with the
  Reactor.
- **Surface runs**: procedurally generated caves of minable blocks; ferrite,
  lumite and voidstone ore; four enemy types (husks, skitters, bloaters, brutes)
  driven by a flow field; five auto-firing weapons with five levels each;
  weapon caches; XP shards and level-up perk choices; a timed extraction where
  the pod lands at your drop site and you must hold the zone.
- **Persistence**: materials, modules, stats and tutorial state save to
  `data/save/profile.json`. Dying or abandoning a drop loses everything carried.
- **Presentation**: original low-poly GLB models (plus CC0 Kenney props),
  rigid-part animation, directional shadows, dynamic point lights, FTL and
  drop-pod cinematics, synthesized sound effects and music, and a shaped-font
  HUD and menus.

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

Other flags: `--seed N`, `--fresh` (new in-memory profile), `--no-save`,
`--frames N`, `--asset-root DIR`, and Quasar's common `--fullscreen`,
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
