---
name: regenerate-assets
description: Add or change Planetfall's 3D models (GLB), sounds (WAV/Ogg) or UI icons, hand-authored or drafted by the DevUtils scripts. Use when a request involves new enemies, props, weapons, character parts, icons, or sound effects/music.
compatibility: Requires cargo and python3; sounds also need numpy, scipy and ffmpeg (libvorbis), icons need Pillow.
metadata:
  author: Planetfall
  version: "2.0"
---

# Regenerate assets

The files in `assets/` are the source of truth: models, sounds and icons may
be hand-authored or replaced (the class suits are). The scripts in
`DevUtils/assets/` draft assets. They write only files that do not exist
yet; naming an asset remakes it and `--force` remakes everything, which
throws away hand-made work, so never do either for an asset the user did not
ask to replace. To install a hand-made asset, copy it over the file of the
same name.

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
2. Run `python3 DevUtils/assets/generate_models.py <name>` for a new or
   explicitly replaced model (no names: only missing models are written).
   A hand-made character must keep its kind's joints and clip names.
3. Every `assets/models/<name>.glb` loads automatically as `<name>`; models
   kept elsewhere get a name in `assets/data/models.toml`. Refer to the name
   from content data (`model = "..."` in `weapons.toml`, `enemies.toml`,
   `upgrades.toml`). The astronaut and every model an enemy names load as
   characters (skins and clips); everything else loads through the static
   loader, which rejects a skin. Enemies play `walk`, plus `attack` or
   `pulse` if the model has them.
4. `cargo test` checks that every model decodes, that characters expose
   their joints and clips, and that every model named in the data exists
   (`all_models_decode_and_content_models_exist`,
   `rigs_expose_their_joints_and_clips`).

## Sounds

1. Edit `DevUtils/assets/generate_sounds.py` (oscillators, filtered noise and
   envelopes; loops must be whole cycles or crossfaded with `seamless`).
2. Run `python3 DevUtils/assets/generate_sounds.py <name>` for a new or
   explicitly replaced sound (no names: only missing sounds are made).
3. Every WAV or Ogg loads under its file stem (`name.vN` are variants). Weapon and enemy sounds are named in
   the data (`sound`, `death_sound`, `alert_sound`); other effects are mapped
   from a `GameEvent` in `src/audio.rs`.
   `all_clips_decode_and_content_sounds_exist` verifies every clip and name.

## Icons

`assets/ui/icons/<name>.png`, any size; `DevUtils/assets/generate_icons.py
<name>` drafts one in the house style. Names are content ids and the glyph
names in `docs/assets.md`.

Third-party assets (Kenney CC0 models, DejaVu fonts) are documented in
`docs/assets.md`; keep their license files beside them.
