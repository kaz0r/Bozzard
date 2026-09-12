# Post processing

Bozzard's editor and player share a configurable HDR post-processing stack. Open `examples/demo/scenes/bonfire-lab.json` to see filmic color, horizontal bloom streaks, contact occlusion, heat shimmer, grain, a vignette, and drifting volumetric firelight together. **Effects → Live preview** animates atmosphere while editing; **Play** runs the full simulation. See [atmosphere and motion effects](atmosphere-effects.md) for smoke, ash, sparks, TAA, motion blur and reflections.

In **Effects**, choose **Apply preset**: Neutral (the legacy look), Cinematic, Bonfire, Neon, or Noir. Presets replace the global display settings; individual controls remain editable. Sliders, presets, reset, and volume edits use normal Undo/Redo, scene saving, and Play isolation. The bonfire scene keeps its original exposure and bloom intensity, with the new effects tuned around them.

## Controls

| Effect | Controls and behavior |
| --- | --- |
| Tone mapping | Reinhard remains the default for existing scenes. Filmic uses a soft toe and highlight shoulder, maps luminance, and compresses highlights into the output gamut. The tone-mapping switch bypasses only the curve. |
| Temporal AA | Eight-sample camera/object reprojection with depth rejection and reactive particles. Replaces FXAA when enabled. |
| Motion blur | Shutter angle, bounded radius and samples; camera/object motion with silhouette spreading and foreground depth protection. |
| Reflections | Material normals/roughness, bounded screen-space depth tracing and environment fallback. |
| Depth of field | Focus distance, focal length, aperture, and maximum blur; separate foreground/background bokeh. See [camera effects](camera-effects.md). |
| Auto exposure | GPU histogram metering, EV limits, target brightness, center weighting, and separate adaptation rates. Manual EV remains additive compensation. |
| Color grading | Warmth and green/magenta tint, saturation, contrast, and per-channel lift/gamma/gain. Neutral values preserve the original grading. This is an analytic grade, without external LUT import. |
| Bloom | Existing HDR threshold, intensity, and spread, plus horizontal anamorphic stretch. The threshold is measured before exposure. |
| Ambient occlusion | Enabled, strength (0–3), world radius (0.01–10), and surface bias (0–0.5). Uses opaque scene depth to reinforce contact and crevice shading. |
| Heat shimmer | Enabled, maximum displacement (0–30 pixels at 1080 pixels high), HDR source threshold, speed, and plume height (fraction of viewport height). Bright source pixels drive an upward shimmer. |
| Volumetric fog | Shadowed light scattering, height density, drifting noise, wind, phase anisotropy, distance, and quality. See [volumetric fog and light shafts](volumetrics.md). |
| Film grain | Intensity (0–0.25) and grain size (1–4 pixels). Deterministic for a given simulation time, with a 24 Hz pattern and reduced noise in black/white regions. |
| Vignette | Intensity (0–1), roundness, and feather. Accounts for viewport aspect ratio. |

Exposure retains its -16…16 EV range. New effects are neutral or disabled when absent in JSON, so older scenes retain their appearance. Every control is validated for finite values and a bounded range in both the scene and renderer APIs. `display.tone_mapper` accepts `reinhard` or `filmic`; new nested settings are `color_grading`, `ambient_occlusion`, `heat_distortion`, `grain`, `vignette`, `volumetric_fog`, `depth_of_field`, `auto_exposure`, `temporal_aa`, `motion_blur`, and `reflections`. Bloom adds `anamorphic`.

## Effect volumes

**Effects → Effect volume at camera** (or **Post-process Volumes → Add effect volume**) creates a world-space axis-aligned box. Edit its center, half size, blend distance, weight, priority, and its own complete display settings or preset. The camera receives full weight inside the box, with a smooth fade over the blend distance outside. Higher priorities apply last; equal priorities retain document order. Up to 32 volumes are supported and saved in `post_process_volumes`.

Continuous controls interpolate, and disabled effects blend from zero intensity. The tone-mapping switch, mapper selection, and TAA enablement change at the midpoint; use the same mapper in adjacent volumes for a seamless transition. Volumes follow the editor's fly camera while editing and the active scene camera during Play or in the player. They have numeric bounds controls; viewport handles are not provided.

## Blueprint animation

The Blueprint node menu includes **Set Exposure (EV)**, **Set Bloom Intensity**, **Set Saturation**, **Set Heat Strength**, **Set Grain Intensity**, **Set Vignette Intensity**, **Set Volumetric Fog Density**, **Set Volumetric Light Intensity**, **Set Focus Distance**, and **Set Aperture (f-stop)**. Each accepts an execution input and a numeric Value. Connect Time/Sine/Multiply/Add for pulses, or trigger these actions from gameplay events. Bloom, heat, focus-distance, and fog-density setters also enable their effects.

Actions write transient global overrides after volume blending. Graph execution order determines the last writer, as with other Blueprint setters. Invalid values fail the action before writing its override. Capture/Save preserves authored settings and volumes; restarting Play clears overrides and resets the visual clock. The fixed-step demo integration advances `SceneInstance::advance_display`; other scene hosts should call it once per simulation tick.

## Renderer order and limits

1. Geometry writes HDR, depth, surface normals/roughness, motion/reactivity, and Fresnel/occlusion. Lit soft particles composite into HDR.
2. Enabled reflections trace opaque depth and blend with existing environment specular lighting.
3. SSAO uses a half-resolution 24-tap spiral. A full-resolution pass combines occlusion and heat distortion with depth protection.
4. Volumetric fog integrates shadowed light at half resolution and composites against opaque depth.
5. TAA resolves subpixel history, then camera/object motion blur gathers along bounded velocity.
6. Auto exposure meters HDR luminance; depth of field gathers near/far bokeh.
7. Bloom reconstructs HDR brightness; exposure, white balance, tone mapping, and color grading map it for display.
8. FXAA is used when TAA is disabled; grain, vignette, and output encoding finish the frame.

SSAO approximates occlusion from visible depth. It cannot see offscreen geometry, uses geometric rather than normal-map detail, and modulates composited scene color rather than only indirect light. Transparent surfaces use the opaque depth behind them. Heat is an HDR brightness proxy, not a temperature simulation or authored heat-volume system, so sufficiently bright non-fire surfaces can also shimmer. Anamorphic bloom is a stretched pyramid filter rather than an optical lens simulation.

Both depth effects skip all their passes and release intermediate targets when disabled or zero-strength. AO uses half-resolution work, while the heat/composite pass is full resolution; enabling both shares that composite. Volumetric fog adds a half-resolution trace and full-resolution composite; thin silhouettes can trace additional rays. Bloom remains conditional. Targets resize with the viewport; input bindings update when effect sources change. Auto exposure retains a small GPU history state. TAA adds ping-pong HDR/surface history and motion blur uses velocity tiles; bokeh and bloom reuse their targets when temporal inputs alternate.

2D scene extraction disables all these effects but retains normal display encoding and FXAA. `draw_linear` bypasses every post effect and remains the numeric diagnostic path. UI and editor overlays are drawn afterward. Live preview advances an isolated effects clock in edit mode. Pausing preview or simulation freezes animation and eye adaptation. Independent stills at time zero meter exposure immediately.

## Verification and captures

```sh
cargo test --workspace --locked --offline
cargo clippy --workspace --all-targets --locked --offline -- -D warnings
python3 tools/check_headless.py
# Explicit native GPU tests with optional diagnostic images:
BOZZARD_POST_CAPTURE_DIR="$PWD/work/post-processing-tests" cargo test -p bozzard-render --test post_processing --locked --offline -- --nocapture
cargo run -p bozzard-player --locked --offline -- --smoke --backend metal --scene examples/demo/scenes/bonfire-lab.json --output work/post-processing-bonfire
cargo run -p bozzard-editor-app --locked --offline -- --scene examples/demo/scenes/bonfire-lab.json
```

Use `vulkan` or `dx12` on the corresponding native backend. GPU tests require a working graphics adapter and fail explicitly when it is unavailable. Coverage includes legacy JSON, all presets, invalid parameters, volume blending/order, Blueprint overrides, history and Play isolation, neutral bypass, planar/contact occlusion, horizontal bloom spread, localized animated heat, deterministic grain, sRGB parity, alpha preservation, raw diagnostics, and resize/toggle cycles down to one-pixel dimensions.

Player smoke writes `loaded-3d-before-post.ppm` with the scene's original exposure/bloom and Reinhard mapping, `loaded-3d-before-optics.ppm` with lens and eye adaptation disabled, `loaded-3d-before-volumetrics.ppm` with only volumetric fog disabled, `loaded-3d.ppm` with the configured effects, and `loaded-3d-animated.ppm` after simulation. Save/reload images freeze the same runtime display overrides and visual clock; authored display settings are verified separately because runtime appearance is not serialized.
