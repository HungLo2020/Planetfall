# Quasar issues found building Planetfall

Status as of Planetfall on Quasar `v0.2.24`. Section 1 lists what the first
audit asked for and what Planetfall now uses instead of its own code.
Section 2 lists problems found while adopting 0.2.24, each with the
workaround Planetfall carries. Section 3 lists what is still missing, for this
game and others.

## 1. Resolved upstream and adopted

| First-audit item | Quasar feature now used | Planetfall code removed |
|---|---|---|
| Transparent/additive draws refused | `SceneTarget`, `TransparentDraw3D`, `PostChain` | Opaque fake glows; 2D damage vignette (now post grade) |
| Skins and clips rejected | `GltfLoader::animated`, `quasar-animation` `model`, `SkinningPass` | Rigid-part character rigs and hand-written poses |
| Labels never wrap | `Widget::wrapped_label` | Greedy word wrapper and pre-wrapped lines |
| Panels painted, root offsets ignored | `NodeStyle`, `Anchor`, honoured offsets/margins | `UiBox` painter, transparent-theme hack, manual HUD measuring |
| No flow fields | `DistanceField` + `update_region` | Hand-written Dijkstra field |
| No shared math | `quasar-math` (glam, `look_at`, `project`, `damp_factor`, angle helpers) | Own `V2` helpers, camera projection |
| No deterministic RNG | `quasar_core::Rng`, `StateHasher::write_rng` | Own xorshift RNG |
| Last-used device, button names, button axes | `DeviceFamily`, `Bindings::format_prompt`, `gamepad_button_axis` | Label table (`device_text`) |
| Mesh primitives | `MeshData::cuboid`/`sphere` | Hand-built cube |
| Offscreen boilerplate | `quasar_frame::render_offscreen` + `Image::write_png` | Target/readback/PNG code |
| Per-play sound variation | `PlaybackConfig::with_variation` | Own pitch jitter |
| (unused before) | `quasar-sim` `Recorder`/`Replay`/snapshots | Ad-hoc state hash; adds a replay test |
| Voxel world (2.1 / 2.2) | `quasar-voxel`, `quasar-voxel-mesh`, `quasar-render-voxel`, nav `voxel` adapter | Cube-per-tile terrain and ship drawing, tile grids, sampled line of sight |

**Correction to the first audit (old item 1.8):** quasar-input already
buffered keyboard edges between frames and ticks in 0.2.21. Planetfall
latches only controller edges, because it reads the gamepad hub's
`InputState`, which clears edges every frame.

## 2. Problems found adopting 0.2.24

1. **ui: wrapped labels ignore a fixed-width parent.** `intrinsic()` passes the
   parent's *available* width down, not a `Length::Fixed` panel's own width,
   so a wrapped label in a 300-unit card wraps at screen width. Workaround:
   pass `max_width` explicitly (`ui::Builder::paragraph`).
2. **ui: wrapped glyphs appear one frame late.** `TextRenderer::prepare`
   warms glyphs from the *unwrapped* layout, so glyphs at new positions on
   wrapped lines miss the atlas sync. A single-frame offscreen render drops
   every wrapped line after the first. Workaround: `Graphics::warm_text`
   draws the layers into a scratch canvas before offscreen renders.
3. **ui: modal panels always draw a focus outline,** styled or not, so a
   full-screen dimmer gets a frame around the screen.
4. **Two `TextWrap` types** (`quasar_ui::TextWrap`, `quasar_text::TextWrap`)
   with the same variants; using the wrong one is a confusing type error.
5. **input: no global last-used device.** `last_active_device` lives on each
   `InputState`, and the window and gamepad hub keep separate states, so a
   game cannot ask which device was used most recently. Planetfall compares
   per-frame activity itself (`controls.rs`).
6. **render-3d: transparency only on `LitRenderState3D`.** `SurfaceRenderState3D`
   (normal maps, the state most games draw with) has no
   `prepare_transparent`, so Planetfall keeps a second lit state for blended
   draws.
7. **render-3d: no per-vertex colour.** `Vertex` is position/normal/uv, so a
   multi-material character needs one `SkinnedMesh` (and one dispatch plus
   palette upload) per material, per instance. Planetfall bakes each
   character into a palette texture with two primitives (lit body and unlit
   glow) to keep it to two dispatches.
8. **render-3d: skinning cost scales per instance.** Every posed character
   needs its own `SkinnedMesh` and a staging-buffer palette upload each
   frame. Fine for about 100 enemies (Planetfall pools them, capped at 160
   per model); hordes in the hundreds need instanced skinning or baked
   animation textures.
9. **render-voxel and render-3d are composed by hand.** There is no reference
   for voxels plus meshes on one `SceneTarget`. Planetfall draws opaque voxels
   (clearing), then meshes with `PassOptions::load()`, then a second voxel
   pass for translucent blocks, then mesh transparency. Shadows go mesh
   casters first, then `prepare_depth` into `ShadowTarget::layer_view(0)`. An
   example or helper would save every game rediscovering this order. Callers
   must also remember `submitted()` after each submit.
10. **Fog exists only in the voxel renderer.** Meshes are not fogged, so a
    distant enemy stays crisp against a fogged wall. The fog needs to be in
    render-3d's lighting or the post chain (depth is exposed for this).
11. **assets: one `GltfLoader` configuration per `AssetServer`.** Static props
    and animated characters must share the `animated()` loader.
12. **Yaw convention vs glTF.** Quasar yaws face −Z; glTF (and Planetfall's
    models) face +Z. A `yaw_rotation_for(forward_axis)` helper or a
    documented recipe would prevent a π offset in every game.
13. **Voxel cells are uniform cubes.** Cut-away walls (0.6 of a block high)
    need custom box models and face-shape rules instead of a per-block
    height or non-uniform cell size.
14. **ui: sliders have no inner padding.** `Widget::Slider` draws its label
    at the rect's top edge and the track on its bottom edge, so the focus
    outline touches both (visible in Planetfall's settings menu). A theme
    inset for sliders would fix it.

## 3. Still missing

**For Planetfall's next features**
- **Particles:** no GPU particle system. Sparks, mining dust and death bursts
  are still CPU cubes; `TransparentDraw3D` makes billboards possible, but
  every game would rebuild emitters.
- **Decals** (scorch marks, mining scars) and **screen-space outlines** for
  enemies behind walls.
- **Clustered/forward+ lighting:** point lights are capped (24 here), which
  caves full of lamps and glowing ore exceed quickly.
- **Instanced skinning / animation textures** for larger hordes (see 2.8).
- **Chunk streaming** is available (`StreamingPlanner`, `RemeshQueue`) but
  Planetfall's 52x52 maps don't need it yet; multi-layer planets will.
- **Networking:** co-op needs lockstep or rollback input sync over
  `quasar-sim` journals plus a transport crate. Basalt would also need a rule
  allowing network use.

**For any game**
- **A complete controls screen:** quasar-ui has rebind capture
  (`begin_rebind`/`finish_rebind`) but no ready screen that lists `Bindings`,
  detects conflicts and persists them; each game builds its own.
- **Data-driven content:** Planetfall now keeps its content in TOML, parsed
  with `quasar_config::load_toml_typed` and validated by hand. Quasar has no
  schema or validation layer for content tables, and because the registry is
  a global `OnceLock` (game code reads it everywhere), Planetfall cannot use
  `quasar-assets` hot reload for it; a reload-aware registry pattern would
  help. Settings are written with `quasar_config::save_file_typed`, which
  covered everything needed.
- **Localisation** hooks for `UiText`.

**Available in Quasar, not yet adopted by Planetfall:** rebind capture in
quasar-ui, asset hot reload, `quasar-jobs` with `RemeshQueue` for background
meshing, and voxel light propagation (`Propagator`) for lamp-lit hulls and
caves.
