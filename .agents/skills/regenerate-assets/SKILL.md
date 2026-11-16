---
name: regenerate-assets
description: Add or change Planetfall's generated 3D models (GLB) or synthesized sounds (WAV). Use when a request involves new enemies, props, weapons, character parts, or sound effects/music.
compatibility: Requires python3 (standard library only) and cargo.
metadata:
  author: Planetfall
  version: "1.0"
---

# Regenerate assets

Models and sounds are produced by deterministic scripts; never hand-edit the
outputs in `assets/models` or `assets/sounds`.

## Models

1. Edit `DevUtils/assets/generate_models.py`. Each model is a list of `Part`s.
   A part's pivot becomes its glTF node translation, which is the joint the
   game rotates (`leg_l`, `arm_r`, `door`, `rotor`, ...). Models face +Z with
   feet at y = 0. Ore decorations span the [-1, 1] wall cube.
2. Run `python3 DevUtils/assets/generate_models.py`.
3. Register new files in `src/render/models.rs` (`models!` list) and draw them
   in `src/render/scene.rs` (`Painter::rig` for animated parts).
4. `cargo test` checks that every model decodes and that rigs expose their
   joints (`all_models_decode`, `rigs_expose_their_joints`).

## Sounds

1. Edit `DevUtils/assets/generate_sounds.py` (oscillators, filtered noise and
   envelopes; loops must be whole cycles or crossfaded with `seamless`).
2. Run `python3 DevUtils/assets/generate_sounds.py`.
3. Add the name to `CLIPS` in `src/audio.rs` and map a `GameEvent` to it.
   `all_clips_decode` verifies every clip.

Third-party assets (Kenney CC0 models, DejaVu fonts) are documented in
`docs/assets.md`; keep their license files beside them.
