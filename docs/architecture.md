# Architecture

Planetfall is an ordinary Cargo workspace. There is no engine-owned project
format, editor or runner requirement: `cargo run` plays, `cargo test` tests.

```
Planetfall/
├── crates/planetfall-sim/   game rules (no window, GPU, audio or file I/O)
├── src/                     the `planetfall` binary: presentation + platform
│   ├── main.rs              entry: desktop, headless sim, offscreen renders
│   ├── settings.rs          CLI (quasar-config CommonArgs + game flags)
│   ├── storage.rs           Basalt data layout + atomic JSON (quasar-storage)
│   ├── desktop.rs           quasar-app runner: window, ticks, menus, saving
│   ├── controls.rs          keyboard/mouse + gilrs controllers → GameInput
│   ├── audio.rs             quasar-audio buses, spatial SFX, music loops
│   ├── render/              quasar-render-3d world + shadows, scene building
│   └── ui/                  quasar-ui HUD, menus, terminals, 2D overlay
├── assets/                  models, sounds, fonts, Basalt artwork
├── DevUtils/                generators, verification and packaging scripts
└── planetfall.json          Basalt catalog entry
```

## Simulation (`planetfall-sim`)

All gameplay is plain Rust data advanced by fixed 60 Hz ticks of a
device-independent `GameInput` (movement vector, edge-triggered interact /
dash / call-extraction, and an optional `Command` such as `TravelTo`,
`Purchase` or `ChoosePerk`). The crate depends only on `serde`,
`quasar-nav` and `quasar-spatial`.

| Module | Responsibility |
|---|---|
| `game` | Top-level state machine: `Ship → Ftl → Ship (orbit) → Descent → Surface → Ascent/Lost → Ship (debrief)`; terminals; tutorial driver; banking cargo |
| `ship` | Block layout built from purchased modules, walking/collision, stations, construction animation timers |
| `run` | One drop: player, auto-firing weapons, horde AI, mining, pickups, caches, level-up perks, extraction |
| `terrain` | Cave generation, minable tiles, circle collision, and a Dijkstra flow field over a `quasar_nav::Grid<2>` |
| `profile` | Persistent save data, purchase rules and versioned migration |
| `tutorial` | Step definitions and prompt text |
| `content` | Static tuning tables: planets, weapons, enemies, upgrades, perks |
| `autoplay` | An autopilot that plays the entire loop (headless runs, soak tests, CI) |
| `events` | `GameEvent`s emitted per tick for sound, effects and saving |

Determinism: every random choice comes from a seeded xorshift `Rng`; runs
have `state_hash()` for repeatability checks. The simulation never reads the
clock, filesystem or devices.

### Quasar in the simulation

- **quasar-nav**: the planet's walkability lives in a `Grid<2>` (diagonals
  without corner cutting). Mining a block unblocks its cell. The horde uses a
  Dijkstra distance field computed with `Grid::neighbors_into`, recomputed only
  when the player changes cell or the grid changes.
- **quasar-spatial**: a `SpatialGrid<2, u32>` indexes live enemies each tick for
  nearest-target selection, projectile hits, arc chaining, drone contact and
  separation steering.

## Presentation (`planetfall` binary)

`desktop.rs` implements `quasar_app::Application`: `frame_update` handles
menus, pause and input latching once per frame; `tick` advances the game at a
fixed 60 Hz (deferred while a menu pauses play); `render` submits one frame.
Edge inputs are latched per frame and consumed by the next tick, so short taps
are never lost at high frame rates.

| Quasar crate | Use |
|---|---|
| quasar-app / quasar-window / quasar-core | Event loop, window, fixed-step timing, fullscreen, focus/suspend |
| quasar-input | Named actions (keyboard axes, keys, mouse), `GamepadHub` + gilrs controllers |
| quasar-frame / vulkanic-gal | Device, swapchain, offscreen targets, headless GPU screenshots |
| quasar-render-3d | Lit surface pass (Blinn-Phong, point + directional lights, normal maps), directional shadow maps, instancing, frustum culling, glTF model upload |
| quasar-assets | Mounted asset root, glTF/GLB decoding (Kenney textures included) |
| quasar-render-2d | HUD canvas: bars, vignette, markers, fades; atlas text pages |
| quasar-ui + quasar-text | Retained menus/terminals with focus and gamepad navigation; shaped DejaVu fonts |
| quasar-audio | WAV clips, music/effects buses, spatial panning, looping music and ambience |
| quasar-config | Common CLI flags (`--resolution`, `--headless`, `--no-audio`, ...) |
| quasar-storage | Basalt `GamePaths` discovery and atomic JSON writes |
| quasar-diagnostics | `--log-level` structured logging (startup paths, phases, save errors) |

### Rendering

One `Graphics` owns a `SurfaceRenderState3D`, a `ShadowRenderState3D` and a
`RenderState2D` on a single caller-owned GAL device. Each frame:

1. `render/camera.rs` eases a top-down follow camera (cinematic framings for
   FTL, descent, ascent and death).
2. `render/scene.rs` turns game state into `SurfaceDraw3D`s and lights. Only
   tiles near the camera are emitted. Animated characters are drawn per glTF
   node: each named part (`leg_l`, `arm_r`, `door`, ...) is rotated about its
   node translation.
3. Opaque lit draws become shadow casters for one directional map centred on
   the view.
4. The 2D pass draws the overlay (vignette, mining bar, off-screen arrows,
   fades), then each UI layer's panel backgrounds and widgets.

The lit pass encodes sRGB in its shader, which Quasar forbids for blended
draws, so all effects (beams, flames, blasts, holograms) are opaque emissive
geometry.

### UI

`ui::Builder` creates `quasar-ui` trees. Panel backgrounds are painted by the
renderer from recorded `UiBox`es so the HUD's full-screen root stays
transparent. The HUD is rebuilt every frame (small tree); menus rebuild only
when a signature of what they display changes, preserving keyboard/gamepad
focus. Labels are wrapped with the real shaped-font metrics.

## Headless and offscreen modes

- `--headless [--autoplay] --ticks N` runs only the simulation and prints a
  summary (with `--no-default-features` the binary has no graphics code).
- `--screenshot FILE` renders the final state, HUD, open terminal or
  level-up menu with a headless GPU.
- `--cover FILE` stages a fight on Cinder Reach and renders the 900x1200
  Basalt cover with the title.
