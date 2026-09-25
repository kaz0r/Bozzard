# Text Rendering

Open `examples/demo/scenes/text-lab.json` in the editor. The **3D** view demonstrates signs, rotated text and an opaque object occluding lettering; **2D** demonstrates centered wrapping, monospace text and opacity. Both work in the standalone player too.

```sh
cargo run -p bozzard-editor-app -- --scene examples/demo/scenes/text-lab.json
```

## Authoring

1. Create an **Empty** entity (or select an existing one).
2. Choose **Properties → Add Component → Text Rendering**.
3. Edit the multiline text, choose **Sans**, **Monospace** or a **Custom** font, and set font size, color, opacity, alignment and optional word-wrap width. A custom font needs a font asset first: import a `.ttf`/`.otf` file, then set the component's **Custom font** field to it (or drag-assign the font onto the selected object). The font is part of the scene, so Undo/Redo and prefab copies carry it like any other asset reference.
4. Choose **2D** or **3D** in the component. The matching scene camera/view must exist.
5. Use the ordinary Transform and gizmos to move, rotate, scale or parent it. Select text by clicking its layout rectangle; **F** frames it. The component's **×** removes only text; Undo/Redo restores edits and removal.

No Mesh Renderer or Material is required. Text and mesh components can coexist independently. Removing a mesh does not remove its text.

## Space and appearance

- Text is flat geometry in the entity's **local XY plane**, facing +Z. Rows advance toward -Y. It inherits parent transforms, including rotated/nonuniform/mirrored transforms.
- The origin anchors the **top** of the block. Left, center and right alignment place the corresponding horizontal edge/center at the origin, and align multiline rows accordingly.
- Font size is an **em size in local units**, not pixels. Wrapping width is also in local units; `null` disables wrapping. Explicit newlines always create rows.
- **3D** uses the perspective scene camera; **2D** uses the existing orthographic scene layer. Enable **Screen HUD** for screen-anchored text instead. HUD text draws over the finished image, unaffected by world depth, camera motion, fog or post-processing.
- Text is unlit and alpha-blended, depth-tested against geometry, and does not write depth or cast shadows. It participates in the existing 3D fog/display processing. Transparent objects use the renderer's existing block-center sorting, not per-glyph order-independent transparency.
- Color is linear RGB with straight alpha. Text's Enabled flag and layer are independent of any mesh on the entity.

## Runtime and persistence

`TextRendering` is a normal, optional ECS component. Scene/prefab JSON preserves its settings; prefab instances have independent component values. Capture reads live text, while editor Stop discards Play changes and restores authored values. Destroyed/removed text releases its cached geometry; the atlas is released when a rendered view contains no text.

Existing Blueprint Transform actions affect text. **Set Visible** hides both text and mesh on the target, not descendants. **Set Color** updates text RGB while preserving opacity, and also updates a mesh/Material if attached. **Text**, **Number to Text**, **Join Text**, **Get Text**, and **Set Text** nodes support dynamic labels. Number to Text takes a decimal count from 0 to 6; text values and computed results are bounded to 4096 UTF-8 bytes. Invalid output reports a simulation error without replacing the target label. Variables remain numeric.

```json
"text_rendering": {
  "enabled": true,
  "layer": "3d",
  "text": "Hello\nworld",
  "font": {"custom": "title-font"},
  "font_size": 0.5,
  "max_width": 4.0,
  "alignment": "center",
  "color": [1.0, 0.8, 0.2, 1.0]
}
```

The entire component is optional. Inside it, omitted fields use defaults: enabled, 3D, `"Text"`, Sans, size 0.5, no wrapping, left alignment and white. `font` is `"sans"`, `"monospace"` or `{"custom": "<font asset id>"}`; the referenced asset must exist in the scene's catalog with kind `font`. Text laid out with a custom font keeps its own bounds cache per font snapshot, and reimporting the font file invalidates it on the next layout.

## Variable fonts and fallback chains

After choosing a custom font, the Inspector exposes its named variable axes with
font-provided limits and a Reset control. Axis tags (for example `wght` and `wdth`)
and design-space values are saved in `font_axes`. Unknown tags and out-of-range
values fail data-dependent validation after import. The headless scene schema
checks finite values, valid four-character tags and bounded counts independently.
Changing the primary font clears its previous axes and fallback settings.

**Add fallback font** appends an imported font. Move entries up to change priority
or remove them with ×. Each missing glyph uses the first face that contains it;
fallback faces use their default variation instance. **Use bundled fonts for
remaining glyphs** appends the built-in Sans/emoji chain. These are component
settings and support Undo/Redo, prefab remapping, save/load and exported games.

```json
"font": {"custom": "title-font"},
"font_axes": {"wght": 700, "wdth": 85},
"font_fallbacks": ["symbols", "localized-text"],
"builtin_font_fallback": true
```

Bounds, picking and GPU text use the same resolved style. Exact cache keys include
source revisions, normalized axis coordinates, ordered fallback identities and the
bundled-fallback choice. Extraction shares imported font bytes; setting an explicit
default axis reuses the default style. Style changes rebuild affected atlas/geometry
state, while unchanged styles retain it. Eight styles share the bounded CPU metrics
atlas; GPU geometry is pruned with the current view. This supports authored styles,
not per-frame animated font-axis effects.

## Initial limits

- Plain text only: no rich-text tags, outlines, extrusion, or TextMesh Pro API compatibility. Custom fonts are single TTF/OTF files up to 4 MiB (`.ttf`, `.otf`), up to one primary and four ordered fallback font assets per Text Rendering component. Font files may expose up to 16 variable axes.
- **Sans uses bundled Roboto**, with epaint's Sans/emoji fallback chain. It is embedded, so no system font installation is needed. The font and license are in `crates/bozzard-text/assets`. Shaping, kerning and wrapping use epaint. Custom fonts search the authored fallback chain when a glyph is missing, then optionally the bundled fonts. With neither fallback configured, missing glyphs retain the original `.notdef` behavior. Invalid or truncated font files are rejected at import, before they can reach the renderer.
- Screen HUD and UI widgets rasterize antialiased glyphs at their **physical display size**, including the editor/player's display scale, with linear sampling for fractional positions. This preserves small strokes when the viewport resizes. Oversized glyphs are bounded to 256 pixels/em to protect the atlas. World text retains a **64-pixel/em raster atlas**, so very large world text softens and very small/distant world text can alias. These are coverage atlases, not SDF/MSDF; mipmaps are avoided because the atlas has only one pixel of glyph padding.
- At most 4096 UTF-8 bytes per component and 65536 per rendered view. Font size: `0.001..1000`; wrap width: `0.001..10000`; finite RGBA in `0..1`.
- Requires support for a 4096-pixel texture dimension. Atlas overflow reports an error rather than drawing stale glyphs. Meshes are cached by layout, shared for identical settings, and pruned when unused. Atlas resizing/recycling invalidates affected geometry and bindings.
- CPU font layout also supports headless UI metrics; the server remains free of GPU, window and asset-import dependencies.

## Checks

```sh
cargo test -p bozzard-editor --test text_rendering
cargo test -p bozzard-editor --test fonts
cargo test -p bozzard-assets --test fonts
cargo test -p bozzard-text --test styles
cargo test -p bozzard-project --test export
cargo test -p bozzard-render layout_wraps
cargo test -p bozzard-render text_gpu_depth -- --ignored --nocapture
python3 tools/check_headless.py
```

The hardware regression checks visible pixels, opaque occlusion, opacity, atlas-growth stability, repeated edits, bounded GPU resources and cleanup. Editor tests cover component workflow, parented bounds/picking, layers, validation, serialization, history, prefab isolation, Blueprint visibility/color and Play restoration. Font tests cover TTF import validation, load-failure/reload isolation, round-tripping a custom-font scene through the editor, custom-vs-built-in bounds differing, and a missing font asset failing the open instead of falling back.

## Screen HUD

Open `examples/demo/scenes/hud-lab.json`, press Play, focus the viewport, and press Space to increment the counter. Resize the viewport: the counter stays top-left, the heading top-right, and the instructions bottom-center.

In Text Rendering, enable **Screen HUD** and set Anchor X/Y (0 to 1) and Offset X/Y. `(0,0)` is top-left; `(1,1)` is bottom-right. Offsets, font size, and wrap width use logical pixels, scaled for the editor/player display. Positive Y offsets move down. Horizontal text alignment defines which edge is attached; the top of the text block is the vertical origin. For bottom alignment, use a negative Y offset large enough for the text. Entity and parent transforms are ignored for HUD placement.

HUD belongs to its chosen 2D/3D view. It is selectable in the viewport or hierarchy; edit its anchors in the Inspector. World gizmos and world bounds do not move/include HUD labels. Overlapping HUD text draws in object-ID order, with later IDs on top. This foundation provides text labels, not interactive buttons, automatic panels, or rich text.

```json
"screen": { "anchor": [1, 0], "offset": [-24, 24] }
```

The optional `screen` field lives inside `text_rendering`. Omitting it preserves existing world text. HUD settings and Blueprint text literals survive scenes, prefabs, and game exports; Play-time text resets when Play stops or the game reloads.
