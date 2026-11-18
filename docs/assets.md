# Assets

Everything the game loads lives under `assets/` and ships as `app/assets/`
in releases. No asset is downloaded at runtime.

**The files under `assets/` are the source of truth.** Any model, sound or
icon can be hand-authored or replaced with a better one; the scripts in
`DevUtils/assets/` are tools that draft assets, not a build step. They only
write files that do not exist yet: name an asset to remake it, or pass
`--force` to remake everything (which discards hand-made work). The four
class suits (`suit-*.glb`) are hand-finished models.

| Directory | Contents | Origin | License |
|---|---|---|---|
| `data/` | Content and tuning TOML (weapons, enemies, planets, perks, modules, tuning, model aliases) | Hand-written | Original to this project |
| `models/` | 53 low-poly GLB models: skinned characters (astronaut and the four class suits; husk, brute, skitter, bloater; the role enemies spitter, disruptor, burrower and gnat; the bosses matriarch, magma tyrant, glacier colossus, bog leviathan and deep wyrm) and props (drop pod, the twelve weapons, drone, mines, ore decorations, pickups, beacon, vents, oxygen vents, ship stations and modules, crew lockers, glowcaps, cave decoration (fungus, obsidian crystals, ice spires), trade station, ship exterior, planet) | Drafted by `DevUtils/assets/generate_models.py`; the four class suits are hand-finished | Original to this project |
| `sounds/` | 93 effects with 187 variant files (every weapon, enemy role, boss, hazard, combo, pickup, loot rarity, footsteps, ship and UI action), 6 ambience loops (ship hum and one per planet) and 4 music tracks (ship, surface, extraction, boss); short effects are WAV, long ones and loops Ogg Vorbis | Drafted by `DevUtils/assets/generate_sounds.py` | Original to this project |
| `ui/` | `icons/` (115 PNG icons, one per name: weapons, gear bases, materials, interface glyphs, objective types, class emblems, abilities, planets, ship modules, and controller and mouse button glyphs for each device family; the game packs them into one atlas at start-up) and `logo.png` (the PLANETFALL wordmark) | Drafted by `DevUtils/assets/generate_icons.py` | Original to this project |
| `kenney/blaster-kit/` | `blaster-d.glb`, `blaster-f.glb`, `crate-medium.glb`, `crate-wide.glb` + colormap | [Kenney Blaster Kit](https://kenney.nl/assets/blaster-kit), via Quasar's Nightwatch example | CC0 1.0 (`License.txt`) |
| `kenney/survival-kit/` | `barrel.glb`, `workbench.glb`, `metal-panel.glb` + colormap | [Kenney Survival Kit](https://kenney.nl/assets/survival-kit), via Quasar's Nightwatch example | CC0 1.0 (`License.txt`) |
| `fonts/` | `DejaVuSans.ttf`, `DejaVuSans-Bold.ttf` | DejaVu fonts, via Quasar's Starfall example | Bitstream Vera license; DejaVu changes public domain (`LICENSE-DejaVu.txt`) |
| `basalt/` | `cover.png` (900x1200) and `icon.svg` | Cover rendered by the game (`--cover`); icon hand-written SVG | Original to this project |

## Models

Hand-made characters must keep the rig of their kind (the joint names and
clip names in the table below) so the game can animate them; props are
free-form. The drafting script is described next.


`generate_models.py` builds models from convex solids (boxes, prisms, cones,
bipyramid crystals, icospheres) with flat normals. Output is byte-for-byte
deterministic.

**Characters** are skinned glTF: one joint per body part (rigid weights), a
lit body primitive and an unlit glow primitive (eyes, visors, lamps) that
sample an embedded 16x16 colour-palette texture (lit colours in the first
half, glow colours in the second), and animation clips sampled into
quaternion keyframes. Variants share a rig and its clips and differ in
parts and materials (`recolor` swaps a body's materials):

| Model | Joints | Clips |
|---|---|---|
| astronaut, suit-striker, suit-prospector, suit-warden, suit-engineer | `root`, `torso`, `head`, `arm_l/r`, `leg_l/r` | `idle`, `walk`, `aim` (right arm only, layered) |
| husk, brute, spitter, disruptor, magma-tyrant, glacier-colossus | same humanoid rig | `idle`, `walk`, `attack` (blended by lunge) |
| skitter, burrower, gnat, matriarch, deep-wyrm | `root`, `body`, `head`, `leg_l0..2`, `leg_r0..2` | `idle`, `walk`, `attack` |
| bloater, bog-leviathan | `root`, `body`, `leg_l/r` | `walk`, `pulse` (body scale, layered) |

Class suits give each class a silhouette (Striker: helmet fin and shoulder
thrusters; Prospector: headlamp and drill pack; Warden: pauldrons, chest
plate and forearm shield; Engineer: tool pack, antenna dish and tool arm),
tinted in the class colour. Each class names its suit in `classes.toml`
(`model`).

Walk clips span one gait loop in walk-distance units, so the game plays them
by how far a character moved. The renderer samples and blends clips with
`quasar-animation` and poses them on the GPU (`SkinningPass`).

**Props** keep one named node per moving part; its translation is the pivot:

| Model | Animated nodes |
|---|---|
| drop-pod | `body`, `door`, `thruster` |
| drone | `core`, `rotor` |
| nav-console / reactor-core / drill-rig / beacon | `holo` / `core` / `bit` / `light` |

Characters face +Z with feet at y = 0. Ore decorations span the [-1, 1] cube
of a wall block.

Models are referred to by name: each `models/<name>.glb` loads as `<name>`
and `data/models.toml` names the Kenney files (`blaster`, `scattergun`,
`barrel`, ...). Sounds load by name too: `name.wav` or `name.ogg`, plus
variants `name.v2`, `name.v3`... that the game picks between at random.

**Voxel textures** for terrain and hull blocks are not files: `src/render/voxels.rs`
generates 32x32 layers per planet palette at runtime (rock with strata,
cracks, chipped rims and a lip of the planet's growth, ore crystals, plated
decks). Planet rock is meshed by `src/render/sculpt.rs`, which chamfers and
chips every exposed block edge so the 1 m cubes read as cut stone.

## Icons

Each icon is `assets/ui/icons/<name>.png`, any size (square works best); a
replacement only needs the same file name.


`generate_icons.py` draws every item icon with PIL in one flat style: chunky
dark outlines, two-tone cel shading with a top-left highlight, a soft glow
behind energy parts and a drop shadow, drawn at 4x and downsampled. Icon
names are the weapon and gear ids in `assets/data`, so new content needs a
matching icon (a missing icon leaves an empty frame). Interface
icons follow fixed names: `obj-<objective kind>`, `class-<id>`,
`ability-<kind>`, `planet-<id>`, `module-<id>`, and button glyphs
`xbox-a`, `ps-cross`, `nin-b`, `pad-lb`, `pad-ls`, `mouse-left` and so on,
which `src/bindings.rs` maps from binding names. The wordmark uses the
bundled DejaVu Sans Bold.

## Sounds

Any `name.wav`/`name.ogg` (and `name.vN` variants) can be replaced with a
hand-made sound.


`generate_sounds.py` (numpy and scipy, plus ffmpeg with libvorbis for the
Ogg files) designs every sound in layers: a transient, a body and a tail.
Its toolkit has band-limited oscillators, FM, coloured noise and crackle,
resonant and swept filters, saturation, ping-pong delay and convolution
reverb with generated rooms. Frequent sounds get two to six variants, and
effects are loudness-matched and trimmed. Music is stereo at 32 kHz: drums,
basses, pads, plucks, bells and leads sequenced per track and rendered three
times through the effects, keeping the middle pass, so tails wrap across the
loop point. Ambience loops are crossfaded into themselves. Every random
choice is seeded from the sound's name, so output is reproducible.

## Drafting new assets

```sh
python3 DevUtils/assets/generate_models.py            # add models that do not exist yet
python3 DevUtils/assets/generate_models.py husk       # remake one (replaces it)
python3 DevUtils/assets/generate_sounds.py            # add missing sounds
python3 DevUtils/assets/generate_icons.py             # add missing icons
cargo run --release -- --cover assets/basalt/cover.png
cargo test   # models/sounds decode and every name in assets/data exists
```

`--force` remakes everything a script knows and discards hand-made files.
