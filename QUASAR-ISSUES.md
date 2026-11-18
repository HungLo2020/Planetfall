# Quasar issues found building Planetfall

Status as of Planetfall on Quasar `v0.2.25` (audited again on 2026-11-18,
with profiles from `--bench`). Section 1 lists problems in Quasar features
Planetfall uses, each with the workaround Planetfall carries. Section 2 lists
what is still missing or deliberately not adopted, and Section 3 lists
Planetfall code that would sit better in Quasar. Gaps that Quasar has closed
are removed once Planetfall adopts the fix.

Quasar 0.2.25 closed every problem the 0.2.24 audit listed, and Planetfall
now uses those fixes: wrapped labels in fixed-width cards, first-frame wrapped
glyphs (no warm-up pass), styled modals without a focus outline, slider
insets, the most recent device across the window and gamepad hub, normal-mapped
transparency on `SurfaceRenderState3D` (one lit state), mesh fog shared with
voxels, clustered lights (128 per frame instead of 24), `SkinnedCrowd`,
`HybridFrame` pass ordering, named loader configurations, partial-height
blocks, GPU particles, decals, outlines, `ControlsScreen` and content tables.

## 1. Problems and workarounds

1. **ui: `ControlsScreen` cannot rebind an analogue axis.** Capture treats any
   binding that is not a two-button `Binding::Axis` as a button, so rebinding
   a gamepad `move_x` row would replace the left stick with one button.
   Planetfall lists movement on the keyboard/mouse column only. Capturing a
   stick deflection (or keeping axis bindings out of a button capture) would
   fix it.
2. **ui: the controls screen's reset button is sized to its label**, unlike
   the game's full-width menu buttons beside it. A layout or style hook for
   the reset button would let it match the surrounding menu.
3. **config: references are checked only once every record parses.** A load
   with both a field error and a dangling reference reports the field error
   first and the reference on the next attempt. Listing both would save an
   edit-reload cycle.
4. **ui: a node style paints beneath its widget.** `set_style` on a
   `Widget::Button` draws the border first and the button's own background
   over it, so a coloured button border never shows. Planetfall wraps each
   upgrade offer in a styled panel instead. Painting the border after the
   widget (or a button style) would fix it.
5. **render-3d: particle systems have no `clear`.** Live particles survive a
   scene change (a new drop, a loaded save) until they expire. They are short
   enough not to matter here, but a reset would be tidier.
6. **core: no coherent noise or coordinate hash.** `quasar-core` has a
   seeded `Rng` but no value/Perlin/simplex noise and no stateless hash of
   integer coordinates, so `cavegen.rs` carries a small seeded value noise
   with fractal octaves (alcoves, ore seams, floor patches), and the renderer
   has three coordinate hashes for per-block variation (`geometry`,
   `sculpt`, `voxels`; each feeds existing visuals, so they stay distinct). A
   deterministic noise and hash module would replace them.
7. **ui: no per-label text colour.** Labels take their colour from the
   theme (`text`, `dim_text`), so rarity-coloured item names are drawn by
   small overlay `Ui` trees (one per colour) over placeholder spacers in
   `src/ui/mod.rs`. A colour override on `Widget::Label` (or `NodeStyle`)
   would remove the overlays.
8. **voxel-mesh: no neighbourhood-aware bevels.** `CulledModelExtractor`
   emits a fixed model per block ID, but chamfers that follow which faces
   are open need the neighbourhood, so `src/render/sculpt.rs` meshes planet
   rock itself into the same `Quad`s. A bevel or "rounded block" model
   policy in quasar-voxel-mesh would replace it.
9. **ui: intrinsic size ignores a child's fixed size.** Auto-sized panels
   measure children by content, so an empty panel, a spacer or an image
   holder with a fixed `LayoutItem` size counts as zero, and a button
   counts its text rather than its fixed width. `ui::kit` adds padded
   "strut" panels where an exact size must be measured, and sets the
   theme's `row_height` to the button height.
10. **ui: no per-widget styling of buttons and checkboxes.** Buttons fill
   with the theme colour over their `NodeStyle`, so the theme's button
   colour is transparent and every button carries its own style, with the
   hover and press tints drawn over it. A checkbox's box uses the theme's
   track colour, so checkbox rows get a lit background to show it.
11. **ui: no inline images in text.** Button prompts ("press [F]") are laid
   out by `kit::rich`: words and key caps in wrapped rows, with line breaks
   decided from estimated text widths.

12. **nav: moving a distance field's goal rebuilds it.**
   `DistanceField::set_goals` replaces the field with a fresh
   `distance_field` (new buffers, a full Dijkstra over the 120x120 grid).
   The horde field follows the player's cell, so each step into a new block
   costs one rebuild: these are the simulation's slowest ticks (p99 2.1 ms
   against a 0.27 ms mean at about 50 enemies in `--bench`). Reusing the
   buffers, or repairing outward from the old and new goals as
   `update_region` does for edits, would flatten them.
13. **render-3d: crowd palettes upload byte by byte.**
   `SkinningPass::record_crowd` builds its upload with
   `bytes(palette.iter().flat_map(..))` into a `Vec<u8>` and creates a new
   staging buffer every frame. It is the largest single CPU cost of
   Planetfall's frame (about a fifth of `Graphics::commands` at a surge,
   more than half of it the byte loop). A slice cast (`bytemuck`) and a
   reused staging ring would remove most of it. `quasar_frame::read_rgba`
   repacks rows the same way (about 40% of `--bench` CPU time, offscreen
   only).
14. **voxel: point reads go through the chunk hash map.** `Volume::get`
   looks the chunk up in an `FxHashMap` and unpacks a paletted value on
   every call. Circle collision, line of sight and tile queries do this
   thousands of times a tick, and full-map scans (finding the nearest ore)
   were 80% of headless CPU time until Planetfall indexed its ore blocks.
   `get_with(Cursor)` helps runs inside one chunk; a dense read view of a
   bounded layer would make small fixed-size maps cheap to query.
15. **frame: offscreen frames report no GPU time.** `OffscreenTarget::render`
   on the headless device leaves `gpu_frame_nanos` empty (no timestamp
   queries), so `--bench` reports CPU timings only.
16. **ui: `TextInput` editing needs a persistent tree, and its caret is
   estimated.** Planetfall rebuilds its layers every frame, which cannot
   keep a focused field's cursor or focus animation. The developer console
   therefore runs a one-field `Ui` per frame to apply Quasar's editing
   (cursor, Home/End, Delete, IME text) and draws the line and caret
   itself. The built-in caret is also placed with a fixed advance
   (`TEXT_ADVANCE`) rather than the text measure, so it drifts on
   proportional fonts. A standalone line-editing helper (text, cursor and
   `UiInput` in, edited text out) and a measured caret would fix both.
17. **core: weighted choice takes only `f64` weights.** Content weights are
   integers, and `planetfall_sim::random::weighted` picks with one
   `range_u32` draw. `Rng::weighted_index` draws differently, so switching
   would change every seeded run and replay. An integer-weight variant would
   let Planetfall drop its copy.

18. **render-voxel, render-3d: no emissive textures.** A voxel
   `MaterialDef` has one emissive colour for the whole layer, and model
   import rejects glTF `emissiveTexture` maps for classic materials, so
   nothing can glow per texel. Ore blocks therefore keep only a faint even
   tint while their crystal props and lights carry the glow; lava cracks and
   ore veins are bright albedo rather than emission. An emissive mask layer
   in the voxel texture array (and emissive maps on models) would let veins,
   cracks and eyes glow on their own.

19. **render-3d: no toon shading or edge-detection post effect.**
   `ShadingModel` offers unlit, diffuse and Blinn-Phong, and `PostChain`
   has bloom, tonemap, grade and vignette, so the Borderlands-style look is a
   Planetfall pass (`src/render/ink.rs`): it copies `SceneTarget`'s colour to
   a scratch texture each frame and draws ink and cel bands from colour and
   depth. A banded/ramp shading model and a depth (and normal) edge pass in
   the post chain would replace it, and normals would let creases follow
   real surface folds instead of depth slope.

## 2. Still missing or not adopted

**Deliberately not adopted**
- **Content hot reload.** `ContentRegistry` can swap tables at runtime, but
  Planetfall's deterministic simulation keys content by index and saves by
  id, so content stays one immutable snapshot per session. Planetfall uses
  the registry for parsing, field validation and cross-table references.
- **Quasar yaw for models.** `yaw_rotation_for(GLTF_FORWARD, yaw)` handles
  +Z-facing glTF models, but Planetfall's simulation already stores yaw in
  model-facing terms (`planetfall_sim::math`); converting would change saved
  state and replay hashes for no visible gain.
- **Per-vertex colours.** Characters keep the generated palette texture; the
  lit body and unlit glow are still separate primitives, so `COLOR_0` would
  not save a draw.
- **Localisation.** Planetfall is English only for now; `UiText::keyed` and
  string tables are ready when that changes.
- **Networking.** `quasar-net` provides lockstep and rollback sessions, but
  Basalt has no rule allowing network use yet.

**Adopted in the 2026-11-18 audit**
- `quasar-profile` (`FrameProfiler`, `Distribution`) and a reused
  `OffscreenTarget` drive `--bench`.
- `Rng::shuffle` replaces a hand-written Fisher-Yates loop in cave
  generation (it draws the same numbers, so seeds are unchanged).
- `TextInput` editing powers the developer console's line (see issue 16).

**For Planetfall's next features**
- **Voxel light propagation** (`Propagator`) for lamp-lit hulls and caves,
  and `quasar-jobs` with `RemeshQueue` for background meshing, are available
  but not yet adopted.
- **Chunk streaming** (`StreamingPlanner`, `RemeshQueue`) is unnecessary for
  120x120 maps; multi-layer planets will need it.

## 3. Planetfall code that belongs in Quasar

Generic pieces Planetfall carries for lack of an engine version. Each is
kept small and in one place so it can be swapped out when Quasar has it.

- **Neighbourhood-aware block sculpting** (`src/render/sculpt.rs`, issue 8):
  chamfered, chipped and worn rock meshed into Quasar's `Quad`s.
- **Coherent noise and coordinate hashes** (`cavegen::Noise`, the render
  hashes; issue 6).
- **Integer-weighted choice** (`planetfall_sim::random::weighted`; issue 17).
- **Line-of-sight steering over a flow field**: enemies, survivors and
  sentries each walk straight at a goal they can see and otherwise follow
  `DistanceField::next_cell`. A `quasar-nav` steering helper (goal, a
  visibility test, the field) would remove the three copies.
- **Ink and cel shading** (`src/render/ink.rs`, issue 19).
- **Hit-stop**: `desktop.rs` defers ticks for a beat on big hits. A
  time-scale or freeze-frame helper in `quasar-app` would standardise it.
- **Camera trauma shake**: `desktop.rs` adds trauma per event and lets it
  decay; `render::camera::CameraRig` turns it into a jitter that grows faster
  than the trauma. Both halves are engine-generic.
- **Command console**: `planetfall_sim::cheats` parses a command tree and
  suggests the next word from a syntax table, and `ui::console` draws the
  log, suggestions and line. The completion engine and the overlay are
  generic; only the commands are Planetfall's.
- **Render interpolation snapshots** (`src/interp.rs`): Quasar's fixed-step
  clock supplies the blend factor, and Planetfall records and restores
  positions around each frame. A small snapshot and blend helper in
  `quasar-sim` would standardise it; which fields to blend stays
  game-specific.
- **Text width estimates** (`ui::kit::text_width`, issues 9 and 11): the
  builder sizes struts and wraps rich text without a measure because
  intrinsic sizing ignores fixed child sizes.
