---
name: regenerate-assets
description: Add or change Planetfall's generated 3D models (GLB) or synthesized sounds (WAV). Use when a request involves new enemies, props, weapons, character parts, or sound effects/music.
compatibility: Requires python3 (standard library only) and cargo.
metadata:
  author: Planetfall
  version: "1.1"
---

# Regenerate assets

Models and sounds are produced by deterministic scripts; never hand-edit the
outputs in `assets/models` or `assets/sounds`.

## Models

1. Edit `DevUtils/assets/generate_models.py`. Each model is a list of `Part`s.
   Models face +Z with feet at y = 0. Ore decorations span the [-1, 1] wall
   cube.
   - **Characters** (`SKINNED`): every part becomes a joint (parent map such
     as `HUMANOID`), rigidly weighted, with a lit body and an unlit glow
     primitive sampling the shared palette texture. Clips are Python functions
     of `u` in `[0, 1)` returning joint rotations/offsets/scales. Walk clips
     span one gait loop in walk units; pose clips (`aim`, `attack`) hold one
     key and should animate only the joints they override.
   - **Props** (`MODELS`): a part's pivot becomes its glTF node translation,
     the joint the game turns (`door`, `rotor`, `holo`, ...).
2. Run `python3 DevUtils/assets/generate_models.py`.
3. Register new files in `src/render/models.rs` (`models!` list). Characters
   also go in `skins::CHARACTERS` and are drawn with `Painter::character`
   (clip layers); props with `Painter::rig`.
4. `cargo test` checks that every model decodes and that characters expose
   their joints and clips (`all_models_decode`,
   `rigs_expose_their_joints_and_clips`).

## Sounds

1. Edit `DevUtils/assets/generate_sounds.py` (oscillators, filtered noise and
   envelopes; loops must be whole cycles or crossfaded with `seamless`).
2. Run `python3 DevUtils/assets/generate_sounds.py`.
3. Add the name to `CLIPS` in `src/audio.rs` and map a `GameEvent` to it.
   `all_clips_decode` verifies every clip.

Third-party assets (Kenney CC0 models, DejaVu fonts) are documented in
`docs/assets.md`; keep their license files beside them.
