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
│   ├── storage.rs           Basalt data layout + atomic JSON (quasar-storage)
│   ├── desktop.rs           quasar-app runner: window, ticks, menus, saving
│   ├── bindings.rs          action bindings + device-aware button prompts
│   ├── controls.rs          keyboard/mouse + gilrs controllers → GameInput
│   ├── audio.rs             quasar-audio buses, spatial SFX, music loops
│   ├── render/              frame pipeline, voxels, skinning, scene building
│   └── ui/                  quasar-ui HUD, menus, terminals, 2D overlay
├── assets/                  models, sounds, fonts, Basalt artwork
├── DevUtils/                generators, verification and packaging scripts
└── planetfall.json          Basalt catalog entry
```

## Simulation (`planetfall-sim`)

All gameplay is plain Rust data advanced by fixed 60 Hz ticks of a
device-independent `GameInput` (movement vector, edge-triggered interact /
dash / call-extraction / build, and an optional `Command` such as `TravelTo`,
`Purchase` or `ChoosePerk`). The crate depends only on `serde` and Quasar's
headless crates.

| Module | Responsibility |
|---|---|
| `game` | Top-level state machine: `Ship → Ftl → Ship (orbit) → Descent → Surface → Ascent/Lost → Ship (debrief)`; terminals; build mode; tutorial driver; banking cargo; `quasar_sim::Simulation` impl |
| `ship` | Voxel hull from built modules plus the player's edits, walking/collision, stations, module assembly, free-form building rules |
| `run` | One drop: player, auto-firing weapons, horde AI, mining, pickups, caches, level-up perks, extraction |
| `terrain` | Cave generation into a voxel volume, mining edits, circle collision, voxel raycast line of sight, horde flow field |
| `voxel` | Block IDs and the shared volume shape (one `TILE` per voxel; ground at `y = -1`, walls at `y = 0`) |
| `profile` | Persistent save data (including ship edits), purchase rules and versioned migration |
| `tutorial` | Step definitions and prompt text |
| `content` | Static tuning tables: planets, weapons, enemies, upgrades, ship blocks, perks |
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

## Presentation (`planetfall` binary)

`desktop.rs` implements `quasar_app::Application`: `frame_update` handles
menus, pause and input latching once per frame; `tick` advances the game at a
fixed 60 Hz (deferred while a menu pauses play); `render` submits one frame.

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
| quasar-config | Common CLI flags (`--resolution`, `--headless`, `--no-audio`, ...) |
| quasar-storage | Basalt `GamePaths` discovery and atomic JSON writes |
| quasar-diagnostics | `--log-level` structured logging (startup paths, phases, save errors) |

### Rendering

One `Graphics` owns every render state on a single caller-owned GAL device.
Each frame:

1. `render/camera.rs` eases a top-down follow camera (cinematic framings for
   FTL, descent, ascent and death).
2. `render/scene.rs` turns game state into mesh draws, transparent draws,
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
holograms) turn named glTF nodes.

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
  level-up menu with a headless GPU.
- `--cover FILE` stages a fight on Cinder Reach and renders the 900x1200
  Basalt cover with the title.
