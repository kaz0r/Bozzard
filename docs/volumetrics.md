# Volumetric fog and light shafts

Open `examples/demo/scenes/light-shafts-lab.json` for warm sunlight passing through stone windows, or `bonfire-lab.json` for orange firelight in drifting haze. **Effects → Fine tuning → Post Processing → Volumetric fog & light shafts** controls the medium. Press **Play** to animate wind; pause freezes it.

```sh
cargo run -p bozzard-editor-app -- --scene examples/demo/scenes/light-shafts-lab.json
cargo run -p bozzard-player -- --scene examples/demo/scenes/bonfire-lab.json
```

The scene sun, point lights, spotlights, and object directional lights illuminate the fog. Enable a sun/point/spot light's shadows to carve beams around opaque geometry. Fog uses the existing light colors, strengths, ranges, cones, and shadow maps; changing a light during Play updates the haze in that frame. Object directional lights remain unshadowed. Emissive materials contribute bloom but do not illuminate the medium themselves.

## Authoring

This optional object belongs inside `display`; omitted fields use these defaults, except fog is disabled by default:

```json
"volumetric_fog": {
  "enabled": true,
  "density": 0.035,
  "albedo": [0.9, 0.94, 1.0],
  "anisotropy": 0.3,
  "base_height": 0.0,
  "height_falloff": 0.25,
  "start_distance": 0.25,
  "max_distance": 40.0,
  "noise_amount": 0.65,
  "noise_scale": 0.35,
  "wind": [0.15, 0.025, 0.07],
  "light_intensity": 1.0,
  "ambient": 0.25,
  "steps": 48
}
```

- **Density** (0–2) controls extinction per world unit. Zero bypasses both passes. Albedo (RGB 0–1) controls scattering versus absorption; black absorbs without adding light.
- **Forward scattering** (anisotropy, −0.8…0.8) favors looking toward a light at positive values; zero is isotropic. **Light scattering** (0–4) scales incoming illumination without changing extinction. **Ambient scattering** (0–1) adds a simple ambient/environment horizon approximation.
- **Base height** (−100000…100000) sets the constant-density lower layer. **Height falloff** (0–10) exponentially thins it above that height. Zero falloff gives uniform height density.
- **Start distance** (0–1000) and **View distance** (1–1000) bound the integration from the camera near plane. Start cannot exceed view distance. Opaque geometry stops each ray early; background rays also receive haze.
- **Density variation** (0–1) blends two octaves of smooth world-space noise. **Noise scale** (0.01–4) controls frequency; larger values give smaller features. **Wind** (each axis −100…100 world units/second) moves this field using the simulation clock.
- **Ray steps** (16–96, default 48) trade GPU work for finer beams and less sampling noise. The showcase uses 80 for narrow window shadows. Cost also increases with resolution and active light count.

Settings support Undo/Redo, saving, isolated Play, and existing camera-driven post-process volumes. Those boxes blend the camera's fog settings; they are not localized physical smoke containers. Continuous settings blend; step count changes at the midpoint. The Bonfire display preset now enables fog. **Set Volumetric Fog Density** and **Set Volumetric Light Intensity** Blueprint actions accept a numeric Value, write transient overrides after volume blending, and validate before writing. Setting density also enables fog. Capturing/saving keeps the authored settings.

## Rendering and limits

The HDR pass runs after SSAO/heat and before bloom. It integrates single scattering with Beer–Lambert transmittance and a Henyey–Greenstein phase function. Each segment accumulates `T * (1 - exp(-density * length)) * albedo * incoming_light`, then updates T. Compositing gives `scene * T + scattered_light`, preserving alpha.

Scattering and transmittance share a half-resolution RGBA16Float target. A depth-aware full-resolution composite rejects samples across silhouettes. If a thin silhouette leaves no matching coarse ray, that pixel is traced at full resolution to avoid dark outlines and foreground leaks. The 2×2 depth reduction uses the nearest opaque depth. Point/spot scattering softens the inverse-square singularity within a fixed 0.75-world-unit emitter radius.

Shadow sampling uses one hardware comparison per light per step, reusing surface shadow maps. Sun coverage is limited to the existing scene-fitted shadow bounds. Only existing eligible opaque/alpha-masked casters block light; transparent and unlit geometry follow the renderer's existing no-shadow policy. Transparent color receives fog using the opaque depth behind it. The medium does not self-shadow and has no multiple scattering, temporal accumulation, local smoke simulation, or baked-GI sampling. Fixed spatial jitter avoids temporal shimmer but can leave fine noise, especially in narrow beams.

The older top-level [analytic fog](fog.md) remains independent and applies first; enabling both compounds extinction. Disable or reduce that fog when tuning a dense volume. 2D extraction and raw `draw_linear` diagnostics bypass volumetrics. Editor UI/overlays render afterward. Disabled/zero-density fog releases its frame-sized targets and skips both passes. Targets and downstream bloom bindings rebuild after resolution or effect-source changes.

## Verification

```sh
BOZZARD_VOLUME_CAPTURE_DIR="$PWD/work/volumetric-tests" cargo test -p bozzard-render --test volumetric --offline --locked
cargo run -p bozzard-player --offline --locked -- --smoke --backend metal --scene examples/demo/scenes/light-shafts-lab.json --output work/volumetric-shafts
```

Use the corresponding Vulkan or DX12 backend on other platforms. Native GPU tests cover analytic homogeneous scattering/absorption, opaque/near depth, thin silhouettes, sun/point/spot scattering shadows independent of surface shadows, light removal/reordering, deterministic animated density, raw/zero-density bypass, resize down to one pixel, and toggling upstream/downstream effects. CPU tests cover validation, serialization, camera-volume blending, Blueprint overrides, and 2D isolation. Player smoke saves `loaded-3d-before-volumetrics.ppm`, `loaded-3d.ppm`, and `loaded-3d-animated.ppm` for comparisons and verifies scene save/reload.
