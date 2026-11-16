# Architecture

Planetfall is an ordinary Cargo workspace. There is no engine-owned project
format, editor or runner requirement: `cargo run` plays, `cargo test` tests.
Quasar crates are pinned by git tag (`v0.2.24`) in the root `Cargo.toml`.

```
Planetfall/
├── crates/planetfall-sim/   game rules (no window, GPU, audio or file I/O)
├── src/                     the `planetfall` binary: presentation + platform
│   ├── main.rs              entry: desktop, headless sim, offscreen renders
│   ├── settings.rs          CLI (quasar-config CommonArgs + game flags)
│   ├── storage.rs           Basalt data layout: save slots, TOML settings
│   ├── desktop.rs           quasar-app runner: window, ticks, menus, saving
│   ├── bindings.rs          action bindings + device-aware button prompts
│   ├── controls.rs          keyboard/mouse + gilrs controllers → GameInput
│   ├── audio.rs             quasar-audio buses, spatial SFX, music loops
│   ├── render/              frame pipeline, voxels, skinning, scene/ building
│   └── ui/                  quasar-ui HUD, menus, terminals, 2D overlay
├── assets/data/             content and tuning as TOML (weapons, enemies, ...)
├── assets/                  models, sounds, fonts, Basalt artwork
├── DevUtils/                generators, verification and packaging scripts
└── planetfall.json          Basalt catalog entry
```

## Simulation (`planetfall-sim`)

All gameplay is plain Rust data advanced by fixed 60 Hz ticks of a
device-independent `GameInput` (movement vector, edge-triggered interact /
dash / call-extraction / build, and an optional `Command` such as `TravelTo`,
`Purchase` or `ChoosePerk`). The crate depends only on `serde` and Quasar's
headless crates (`quasar-config` parses the content TOML from strings).

| Module | Responsibility |
|---|---|
| `game` | Top-level state machine: `Ship → Ftl → Ship (orbit) → Descent → Surface → Ascent/Lost → Ship (debrief)`; terminals; build mode; tutorial driver; banking cargo; `quasar_sim::Simulation` impl |
| `ship` | Voxel hull from built modules plus the player's edits, walking/collision, stations, module assembly, free-form building rules |
| `run` | One drop, split by system: `player` (movement, dash, mining), `weapons`, `enemies`, `spawner` (weighted spawn tables), `loot` (pickups, caches, perks), `extraction`; `tests` |
| `terrain` | Cave generation into a voxel volume, mining edits, circle collision, voxel raycast line of sight, horde flow field |
| `voxel` | Block IDs and the shared volume shape (one `TILE` per voxel; ground at `y = -1`, walls at `y = 0`) |
| `profile` | Persistent progress (including ship edits), purchase rules, module bonuses and versioned migration |
| `save` | `SaveGame`: a profile plus the resumable `Session` (tick, RNG, tutorial, ship position, a suspended drop) and slot metadata |
| `tutorial` | Step definitions and prompt text |
| `content` | The content registry: definitions parsed and validated from `assets/data/*.toml`, string-keyed IDs (`WeaponId`, `EnemyId`, `PlanetId`, `PerkId`, `UpgradeId`) |
| `autoplay` | An autopilot that plays the entire loop, including building (headless runs, soak tests, CI) |
| `math` | Ground-plane conventions (`V2` = glam `Vec2`, +Z-facing yaw) over `quasar-math` |
| `events` | `GameEvent`s emitted per tick for sound, effects and saving |

### Quasar in the simulation

- **quasar-core** `Rng`: every random choice comes from seeded, splittable
  xoshiro256** generators.
- **quasar-math**: glam vectors and angle helpers (`turn_towards`,
  `angle_delta`). Planetfall's models face +Z, so `math` keeps its yaw
  helpers (Quasar yaws face −Z).
- **quasar-sim**: `Game` implements `Simulation`; `state_hash` uses
  `StateHasher` (including RNG state). A test records an autopilot session
  with `Recorder`, replays it with `Replay` and resumes from a mid-run
  snapshot, requiring identical hashes.
- **quasar-voxel**: the planet surface and the ship hull are `Volume<Cube16>`
  block volumes. Mining applies an `EditBatch`; its change list drives nav
  updates and its dirty chunks drive re-meshing. Line of sight is a voxel
  DDA `raycast`. Ship deck removal uses `split_check` so the hull never
  splits.
- **quasar-nav** (`voxel` feature): `walkable_grid` builds the horde's
  `Grid<3>` from the terrain volume; `sync_changes` turns mining edits into
  dirty cells; a `DistanceField` gives every enemy its next cell and is
  repaired with `update_region` rather than rebuilt.
- **quasar-spatial**: a `SpatialGrid<2, u32>` indexes live enemies each tick for
  nearest-target selection, projectile hits, arc chaining, drone contact and
  separation steering.

The simulation never reads the clock, filesystem or devices.

### Content

Weapons, enemies, planets, perks, ship modules and global tuning are data in
`assets/data/` (see [Gameplay](gameplay.md#adding-content)). The binary reads
the files at startup and passes their text to `content::install`, which
parses, validates (unique IDs, five weapon levels, known models are checked by
the renderer's tests, every planet has a spawn table) and stores one
immutable `Content` in a global `OnceLock`. `content()` falls back to the
copy compiled into the sim crate (`include_str!`), which tests, the headless
CLI and a missing data folder use.

Behaviour stays in code as small archetype enums: `WeaponBehavior` (bolt,
scatter, arc, mortar, orbit), `EnemyBehavior` (walker, lunger, exploder),
`PerkEffect` and `UpgradeEffect`. A new weapon that fits an archetype is a
TOML entry; a new kind of behaviour is a new variant plus its system code.

IDs are `u16` indices but serialize as their string keys, so saves survive
reordering, and unknown keys (removed content) are dropped on load.

### Saves

`SaveGame::capture` snapshots a `Game` into a slot: the profile plus a
`Session`. Transitional phases (FTL, descent, ascent, a lost signal) are
settled on a clone first, so a save always resumes on the ship or on the
surface. A drop in progress is saved whole: the run serializes everything
except derived state (the flow field and spatial index are rebuilt), and the
terrain saves only its planet, seed and the blocks broken or damaged since
generation (`TerrainSave`), regenerating the rest on load. A test checks a
resumed drop hashes and plays identically to one never interrupted.

## Presentation (`planetfall` binary)

`desktop.rs` implements `quasar_app::Application`: `frame_update` handles
menus, pause and input latching once per frame; `tick` advances the game at a
fixed 60 Hz (deferred while a menu pauses play); `render` submits one frame.
The game opens on the main menu (continue, new game, load, settings) with the
ship idling behind it; the active slot autosaves every minute of play, when
the profile changes, and on pause-quit, suspend and exit.

| Quasar crate | Use |
|---|---|
| quasar-app / quasar-window / quasar-core | Event loop, window, fixed-step timing, fullscreen, focus/suspend |
| quasar-input | Named actions (keyboard axes, keys, D-pad button-pair axes), `GamepadHub` + gilrs, device families and `format_prompt` button names |
| quasar-frame / vulkanic-gal | Device, swapchain, `render_offscreen` screenshots |
| quasar-render-3d | Lit surfaces (Blinn-Phong, point + directional lights, normal maps), shadow maps, `SceneTarget` (linear HDR), sorted `TransparentDraw3D`, `PostChain` (bloom, ACES, grade, vignette), `SkinningPass`, mesh primitives |
| quasar-render-voxel / quasar-voxel-mesh | Voxel terrain and hull: culled-model meshing (cut-away wall models), paged GPU arenas, GPU culling, translucent windows, fog, overlays, voxel shadow casters |
| quasar-assets + quasar-animation | glTF decoding with skins and clips; skeletons, clip sampling, pose blending and joint palettes |
| quasar-math | Camera `look_at`/`perspective`/`project`, damping, glam types |
| quasar-render-2d | HUD canvas: markers, bars, fades |
| quasar-ui + quasar-text | Retained menus/terminals/HUD with anchored layout, `NodeStyle` cards and wrapped labels; shaped DejaVu fonts |
| quasar-audio | WAV clips, music/effects buses, spatial panning, per-play pitch/gain `Variation`, looping music and ambience |
| quasar-config | Common CLI flags (`--resolution`, `--headless`, `--no-audio`, ...); TOML content parsing; settings read and written with `load_file_typed`/`save_file_typed` |
| quasar-storage | Basalt `GamePaths` discovery and atomic JSON writes for save slots |
| quasar-diagnostics | `--log-level` structured logging (startup paths, phases, save errors) |

### Rendering

One `Graphics` owns every render state on a single caller-owned GAL device.
Each frame:

1. `render/camera.rs` eases a top-down follow camera (cinematic framings for
   FTL, descent, ascent and death).
2. `render/scene/` (`ship`, `space`, `surface`, `characters`, `effects`) turns game state into mesh draws, transparent draws,
   lights, skinned-character requests, a `VoxelScene` (which world, overlays,
   fog) and per-phase post settings (damage reddens and darkens the edges;
   losing the signal drains colour).
3. `render/skins.rs` samples each character's clip layers
   (`quasar-animation`), poses pooled `SkinnedMesh`es with one compute
   dispatch each and returns ordinary surface draws.
4. `render/voxels.rs` swaps the grid between ship and planet and re-meshes
   chunks whose revision changed (mined rock, placed blocks).
5. Passes, in submission order: skinning; one directional shadow map with mesh
   casters then voxel casters; opaque voxels into the HDR `SceneTarget`;
   opaque meshes on the same depth; translucent voxels (windows); sorted
   transparent draws (beams, flames, blasts, holograms, build previews); the
   post chain onto the output; the 2D overlay and UI layers.

Characters are skinned; props with moving parts (pod door, drone rotor,
holograms) turn named glTF nodes. Models and sounds are looked up by name:
every `assets/models/*.glb` and `assets/sounds/*.wav` loads under its file
stem, plus the aliases in `assets/data/models.toml`, and content data refers
to those names. Any model with a skin is a character.

### UI

`ui::Builder` creates `quasar-ui` trees. Cards are panels with a `NodeStyle`
(background, border, radius); HUD cards are pinned with `Anchor`s; tutorial
prompts and hints stack upward from the bottom edge as word-wrapped labels.
The HUD is rebuilt every frame (small tree); menus rebuild only when a
signature of what they display changes, preserving keyboard/gamepad focus.
Prompts such as `{interact}` are formatted by `bindings::Prompts` for the
device family the player last used.

## Headless and offscreen modes

- `--headless [--autoplay] --ticks N` runs only the simulation and prints a
  summary (with `--no-default-features` the binary has no graphics code).
- `--screenshot FILE` renders the final state, HUD, open terminal or
  level-up menu with a headless GPU; `--menu main|saves|settings|pause`
  renders that menu instead (with sample slots; nothing is read from disk).
- Headless runs always use a fresh in-memory profile; `--slot N` picks the
  save slot a windowed session opens.
- `--cover FILE` stages a fight on Cinder Reach and renders the 900x1200
  Basalt cover with the title.
