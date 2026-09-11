# Scene fog

Add this optional top-level object to a version-1 scene. The [material and neon galleries](showcases.md) include tuned examples:

```json
"fog": {
  "enabled": true,
  "color": [0.5, 0.6, 0.7],
  "distance_density": 0.02,
  "start_distance": 0.0,
  "height_density": 0.04,
  "base_height": 0.0,
  "height_falloff": 1.0
}
```

Omitting `fog` disables it. Each field is optional; the defaults are as above except `enabled: false` and `height_density: 0.0`. Unknown fields are rejected. All numbers must be finite. Color is scene-linear RGB in 0..1; densities and falloff accept 0..1000; start distance accepts 0..100000; base height accepts -100000..100000.

Distance density is constant extinction per world unit. Height density adds extinction `height_density * exp(-height_falloff * max(y - base_height, 0))`: constant below the base, decreasing exponentially above it. Zero falloff gives uniform height density. Zero densities have no effect. Both components begin after `start_distance`, measured along the pixel's world-space ray from the camera **near plane**, so editor navigation, perspective and orthographic views share the same implementation. This deliberately excludes the invisible eye-to-near-plane segment.

The shader analytically integrates height density along the remaining segment and mixes geometry's linear RGB toward the fog color with `1 - exp(-optical_depth)`. Basic/PBR, unlit, checker and toon geometry receive fog before bloom/exposure/tone mapping. Alpha and alpha discard are unchanged. The sky/background, normal diagnostics, raw `draw_linear` captures, 2D extraction and editor overlays are unaffected. This is analytic colored extinction, not volumetric lighting: no noise, light shafts, scattering shadows or fogged sky.

**Scene lighting → Fog (3D)** exposes these fields and Reset fog. Edits use normal gesture history, Undo/Redo, save/reopen and isolated Play state.

Regression checks (GPU test requires a native adapter and fails rather than skipping):

```sh
cargo test -p bozzard-scene -p bozzard-editor -p bozzard-render --test fog --offline
cargo test -p bozzard-player fog --offline
cargo check -p bozzard-player -p bozzard-editor-app --tests --offline
```
