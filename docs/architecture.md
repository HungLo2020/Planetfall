# Architecture

Planetfall is an ordinary Cargo workspace. There is no engine-owned project
format, editor or runner requirement: `cargo run` plays, `cargo test` tests.
Quasar crates are pinned by git tag (`v0.2.25`) in the root `Cargo.toml`.

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
│   ├── audio.rs             quasar-audio buses, spatial SFX with variants, footsteps, crossfaded music
│   ├── render/              frame pipeline, voxels, skinning, scene/ building
│   └── ui/                  quasar-ui HUD, menus, terminals, 2D overlay; kit.rs is the
│                            shared look (windows, buttons, chips, bars, key caps)
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
| `ship` | Voxel hull from built modules plus the player's edits, walking/collision, stations (nav console, fabricator, locker room, drop pod, modules), module assembly, free-form building rules |
| `run` | One drop (no cosmetic state: particles are presentation). Players are a list (`players[0]` is local; `update_players` takes one input each) so co-op can follow. Split by system: `player` (movement, dash, mining), `abilities` (class abilities, barrier posts, turrets), `weapons`, `arsenal` (flamer heat, ricochet, mines, melee, homing, gravity), `hazards` (lava and water ground, spore clouds, vents, oxygen, quakes), `enemies`, `spawner` (threat level, surges, elites, weighted spawn tables), `loot` (pickups, caches, rarity rolls, upgrade and weapon-pick offers), `weapons` also resolves hits: element statuses, shock arcs and legendary specials, `extraction`, `mission` (objective progress and set pieces: boss arena, survivor, drill rig, wreck, glowcaps, relays; contract modifiers); `tests` |
| `cavegen` | Cave layout: chamber graph, winding tunnels, noise alcoves and smoothing, set-piece arena, connectivity repair, ore seams and floor patches (value noise is local; see QUASAR-ISSUES) |
| `terrain` | The cave committed to a voxel volume (ground blocks for patches, lava and water; quake rubble saved in order with mined blocks), mining edits, circle collision, voxel raycast line of sight, horde flow field |
| `voxel` | Block IDs and the shared volume shape (one block per voxel: planet blocks are `terrain::TILE` = 1 m, ship blocks `ship::TILE` = 2 m; ground at `y = -1`, walls from `y = 0`) |
| `profile` | Persistent progress (ship edits, the active class and a `ClassRecord` per class: class XP, starting weapon, found weapon blueprints, equipped gear; the stash, credits, docking and the station shelf), purchase and trade rules, module bonuses and versioned migration |
| `gear` | Equipment items: rolling bases, rarities and affixes, stat scaling, names, prices, reroll and fuse |
| `save` | `SaveGame`: a profile plus the resumable `Session` (tick, RNG, tutorial, ship position, a suspended drop) and slot metadata |
| `mission` | Contracts (primary, secondary, site, mutator), the mission board generator, hazard levels |
| `tutorial` | Step definitions and prompt text |
| `content` | The content registry: definitions parsed and validated from `assets/data/*.toml`, string-keyed IDs (`WeaponId`, `EnemyId`, `PlanetId`, `PerkId`, `UpgradeId`, `ClassId`, `ModId`), class levels, `Offer` (an `Upgrade` at a `Rarity`) |
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
`assets/data/` (see [Gameplay](gameplay.md#tuning-and-adding-content)). The
binary reads the files at startup and passes their text to
`content::install`. The record tables go through
`quasar_config::content::ContentRegistry`: each definition implements
`ContentRecord` (its own field checks) and planets declare references to
enemy ids, so a bad edit reports every problem at once as
`table[record].field: message`. Checks across a table or against the tuning
(a starter weapon, levels matching `max_level`, a planet at nav level 0)
follow, and the result is stored as one immutable `Content` in a global
`OnceLock`. The renderer's tests check that named models and sounds exist. `content()` falls back to the
copy compiled into the sim crate (`include_str!`), which tests, the headless
CLI and a missing data folder use.

Behaviour stays in code as small archetype enums: `WeaponBehavior` (bolt,
scatter, arc, mortar, orbit), `EnemyBehavior` (walker, lunger, exploder),
`AbilityKind` (bore, barrier, blink, turret), `PerkEffect` (shared by perks
and class passives), `ModStat`, `Element` and `Special` (legendary
behaviours) and `UpgradeEffect`. A new weapon that fits an archetype is a
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
| quasar-input | Named actions (keyboard axes, keys, D-pad button-pair axes), `GamepadHub` + gilrs, `most_recent_device` across window and hub for prompts, device families and `format_prompt` button names |
| quasar-frame / vulkanic-gal | Device, swapchain, `render_offscreen` screenshots |
| quasar-render-3d | Lit surfaces (Blinn-Phong, clustered point + directional lights, normal maps, fog), shadow maps, `SceneTarget` (linear HDR), sorted `SurfaceTransparentDraw3D`, `PostChain` (bloom, ACES, grade, vignette), `SkinningPass` + `SkinnedCrowd`, `ParticleSystem`, `DecalRenderer`, `OutlineRenderer`, mesh primitives |
| quasar-render-voxel / quasar-voxel-mesh | Voxel terrain and hull: culled meshing with partial-height walls, paged GPU arenas, GPU culling, translucent windows, fog, overlays, voxel shadow casters, `HybridFrame` pass ordering |
| quasar-assets + quasar-animation | glTF decoding (a static loader for props and a named animated configuration for characters); skeletons, clip sampling, pose blending and joint palettes |
| quasar-math | Camera `look_at`/`perspective`/`project`, damping, glam types |
| quasar-render-2d | HUD canvas: markers, bars, fades |
| quasar-ui + quasar-text | Retained menus/terminals/HUD with anchored layout, `NodeStyle` cards and wrapped labels; `ControlsScreen` rebinding with `save_bindings`/`load_bindings`; shaped DejaVu fonts |
| quasar-audio | WAV and Ogg Vorbis clips, music/effects buses, spatial panning, per-play pitch/gain `Variation`, looping music and ambience (crossfaded with voice volumes) |
| quasar-config | Common CLI flags (`--resolution`, `--headless`, `--no-audio`, ...); content tables (`ContentRegistry`, validation, references); settings read and written with `load_file_typed`/`save_file_typed` |
| quasar-storage | Basalt `GamePaths` discovery and atomic JSON writes for save slots |
| quasar-diagnostics | `--log-level` structured logging (startup paths, phases, save errors) |

### Rendering

One `Graphics` owns every render state on a single caller-owned GAL device.
Each frame:

1. `render/camera.rs` eases a top-down follow camera (cinematic framings for
   FTL, descent, ascent and death).
2. `render/scene/` (`ship`, `space`, `surface`, `characters`, `effects`)
   turns game state into mesh draws, transparent draws, lights (up to 128,
   clustered by Quasar past a dozen), skinned-character requests with an
   optional outline style, a `VoxelScene` (which world, overlays, fog) and
   per-phase post settings (damage reddens and darkens the edges; losing the
   signal drains colour).
3. `render/skins.rs` samples each character's clip layers
   (`quasar-animation`) and poses every instance of a character with one
   `SkinnedCrowd` per primitive: one dispatch per crowd, and consecutive
   slots draw as one instanced draw through the ordinary surface and shadow
   passes.
4. `render/effects.rs` turns the frame's `GameEvent`s into GPU particles
   (blood, rock dust, ore glints, wall impacts, explosions, landing dust),
   keeps scorch and blood decals for the current drop, and holds the outline
   styles that mark the player and enemies hidden behind rock.
5. `render/voxels.rs` swaps the grid between ship and planet and re-meshes
   chunks whose revision changed (mined rock, placed blocks). Planet rock is
   meshed by `render/sculpt.rs`, which chamfers and chips every exposed edge
   (keeping seams with neighbours closed); ground and liquids stay flat. The
   ship is laid out in 2 m tiles but drawn in the same 1 m blocks: each ship
   voxel is refined into 2×2×2 renderer-only pieces (a full wall course under
   a 0.2 m cap, so walls stay 1.2 m; lamp quarters) and re-meshed whole when
   the ship changes.
6. `quasar_render_voxel::HybridFrame` orders the passes and collects the
   voxel submissions: compute (crowd skinning); shadows (mesh casters, then
   voxel casters); opaque (voxels clear the HDR `SceneTarget`, meshes share
   their depth, both fogged alike); decals; translucent (windows, then sorted
   blended surfaces on the same `SurfaceRenderState3D`); effects (particles,
   outlines); post (bloom, tonemap, grade) and the 2D overlay and UI.

Characters are skinned; props with moving parts (pod door, drone rotor,
holograms) turn named glTF nodes. Models and sounds are looked up by name:
every `assets/models/*.glb` and `assets/sounds/*.{wav,ogg}` loads under its file
stem (variants `name.v2`, `name.v3`... join their base name), plus the aliases in `assets/data/models.toml`, and content data refers
to those names. The astronaut and every enemy model load through a named
animated loader configuration (skins and clips); props use the static loader.

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
  level-up menu with a headless GPU; `--menu main|saves|settings|controls|pause`
  renders that menu instead (with sample slots; nothing is read from disk).
- Headless runs always use a fresh in-memory profile; `--slot N` picks the
  save slot a windowed session opens.
- `--cover FILE` stages a fight on Cinder Reach and renders the 900x1200
  Basalt cover with the title.
