# Quasar issues found building Planetfall

Status as of Planetfall on Quasar `v0.2.25`. Section 1 lists problems in
Quasar features Planetfall uses, each with the workaround Planetfall carries.
Section 2 lists what is still missing or deliberately not adopted. Gaps that
Quasar has closed are removed once Planetfall adopts the fix.

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
6. **core: no coherent noise.** `quasar-core` has a seeded `Rng` but no
   value/Perlin/simplex noise, so `cavegen.rs` carries a small seeded value
   noise with fractal octaves for alcoves, ore seams and floor patches. A
   deterministic 2D/3D noise module would replace it.
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

**For Planetfall's next features**
- **Voxel light propagation** (`Propagator`) for lamp-lit hulls and caves,
  and `quasar-jobs` with `RemeshQueue` for background meshing, are available
  but not yet adopted.
- **Chunk streaming** (`StreamingPlanner`, `RemeshQueue`) is unnecessary for
  120x120 maps; multi-layer planets will need it.
