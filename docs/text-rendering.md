# Text Rendering

Open `examples/demo/scenes/text-lab.json` in the editor. The **3D** view demonstrates signs, rotated text and an opaque object occluding lettering; **2D** demonstrates centered wrapping, monospace text and opacity. Both work in the standalone player too.

```sh
cargo run -p bozzard-editor-app -- --scene examples/demo/scenes/text-lab.json
```

## Authoring

1. Create an **Empty** entity (or select an existing one).
2. Choose **Properties → Add Component → Text Rendering**.
3. Edit the multiline text, choose **Sans** or **Monospace**, and set font size, color, opacity, alignment and optional word-wrap width.
4. Choose **2D** or **3D** in the component. The matching scene camera/view must exist.
5. Use the ordinary Transform and gizmos to move, rotate, scale or parent it. Select text by clicking its layout rectangle; **F** frames it. The component's **×** removes only text; Undo/Redo restores edits and removal.

No Mesh Renderer or Material is required. Text and mesh components can coexist independently. Removing a mesh does not remove its text.

## Space and appearance

- Text is flat geometry in the entity's **local XY plane**, facing +Z. Rows advance toward -Y. It inherits parent transforms, including rotated/nonuniform/mirrored transforms.
- The origin anchors the **top** of the block. Left, center and right alignment place the corresponding horizontal edge/center at the origin, and align multiline rows accordingly.
- Font size is an **em size in local units**, not pixels. Wrapping width is also in local units; `null` disables wrapping. Explicit newlines always create rows.
- **3D** uses the perspective scene camera; **2D** uses the existing orthographic scene layer. This is not a screen-space canvas/HUD or automatic camera-facing billboard. Text can be parented to a camera, but there is no pixel-anchor/layout system.
- Text is unlit and alpha-blended, depth-tested against geometry, and does not write depth or cast shadows. It participates in the existing 3D fog/display processing. Transparent objects use the renderer's existing block-center sorting, not per-glyph order-independent transparency.
- Color is linear RGB with straight alpha. Text's Enabled flag and layer are independent of any mesh on the entity.

## Runtime and persistence

`TextRendering` is a normal, optional ECS component. Scene/prefab JSON preserves its settings; prefab instances have independent component values. Capture reads live text, while editor Stop discards Play changes and restores authored values. Destroyed/removed text releases its cached geometry; the atlas is released when a rendered view contains no text.

Existing Blueprint Transform actions affect text. **Set Visible** hides both text and mesh on the target, not descendants. **Set Color** updates text RGB while preserving opacity, and also updates a mesh/Material if attached. This pass does not add string ports or a Set Text Blueprint node; runtime Rust code can change the component's `text` field.

```json
"text_rendering": {
  "enabled": true,
  "layer": "3d",
  "text": "Hello\nworld",
  "font": "sans",
  "font_size": 0.5,
  "max_width": 4.0,
  "alignment": "center",
  "color": [1.0, 0.8, 0.2, 1.0]
}
```

The entire component is optional. Inside it, omitted fields use defaults: enabled, 3D, `"Text"`, Sans, size 0.5, no wrapping, left alignment and white.

## Initial limits

- Plain text only: no rich-text tags, outlines, extrusion, custom font imports, or TextMesh Pro API compatibility.
- Reuses epaint's bundled fonts, Unicode shaping, kerning and wrapping. Glyph coverage depends on those fonts; unsupported characters use their fallback glyph.
- A shared **64-pixel/em raster atlas**, not SDF/MSDF. Ordinary scaling works, but very large text softens and very small/distant text can alias. Distance-field rendering is the next step if those cases matter. Mipmaps are deliberately avoided because the upstream atlas has only one pixel of glyph padding.
- At most 4096 UTF-8 bytes per component and 65536 per rendered view. Font size: `0.001..1000`; wrap width: `0.001..10000`; finite RGBA in `0..1`.
- Requires support for a 4096-pixel texture dimension. Atlas overflow reports an error rather than drawing stale glyphs. Meshes are cached by layout, shared for identical settings, and pruned when unused. Atlas resizing/recycling invalidates affected geometry and bindings.
- Font layout/rasterization lives only in the rendering side. Simulation/server remains graphics- and font-library-free.

## Checks

```sh
cargo test -p bozzard-editor --test text_rendering
cargo test -p bozzard-render layout_wraps
cargo test -p bozzard-render text_gpu_depth -- --ignored --nocapture
python3 tools/check_headless.py
```

The hardware regression checks visible pixels, opaque occlusion, opacity, atlas-growth stability, repeated edits, bounded GPU resources and cleanup. Editor tests cover component workflow, parented bounds/picking, layers, validation, serialization, history, prefab isolation, Blueprint visibility/color and Play restoration.
