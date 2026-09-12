# Camera focus, bokeh, and auto exposure

Open `examples/demo/scenes/lens-lab.json` to see a focused gold sculpture against distant lights. **Play** runs the Lens Director Blueprint, slowly shifting focus between the sculpture and the light rows over roughly 12.6 seconds. Stop restores the authored focus. The bonfire scene uses a subtler lens and tightly bounded exposure to preserve its dark clearing.

```sh
cargo run -p bozzard-editor-app -- --scene examples/demo/scenes/lens-lab.json
cargo run -p bozzard-editor-app -- --scene examples/demo/scenes/bonfire-lab.json
```

Both effects live under **Effects → Fine tuning → Post Processing**. They are disabled when absent from older scene files and support ordinary Undo/Redo, saving, Play isolation, and camera-driven effect volumes. Applying existing named presets resets them to their disabled defaults; the bonfire scene's authored lens is tuned to its camera position.

## Focus and bokeh

**Camera focus & bokeh** exposes depth of field, focus distance, lens length, aperture, and maximum blur. Lower f-stops and longer lenses increase defocus; zero maximum blur bypasses the pass. The focused plane remains sharp. Near objects blur outward, and distant highlights form round bokeh disks.

```json
"depth_of_field": {
  "enabled": true,
  "focus_distance": 5.0,
  "focal_length_mm": 50.0,
  "aperture": 2.8,
  "max_blur_radius": 18.0
}
```

Place this object inside `display`. These are the defaults, except `enabled` defaults to false. Focus accepts 0.5–1000 world units measured along the camera's forward axis from its near plane. Lens length accepts 10–200 mm, aperture 0.7–32, and blur radius 0–32 pixels at a viewport height of 1080 pixels. All values must be finite. One world unit is treated as one meter and the virtual sensor height is 24 mm. Lens length controls defocus independently of the camera's framing/FOV. Orthographic views use the same axial-distance model as an artistic effect.

The signed circle-of-confusion radius follows `0.5 * viewport_height / sensor_height * f² / (aperture * (focus - f)) * (1 - focus / depth)`, bounded by maximum blur. Negative radii belong to the foreground. The renderer downsamples HDR color and CoC, gathers a 96-tap disk into separate near/far layers, then composites at full resolution using CoC to preserve focused silhouettes. Foreground compositing estimates the visible background around blurred objects. Highlight color stays in HDR until the later bloom and display passes.

This is a depth-buffer approximation: hidden backgrounds cannot be reconstructed exactly, transparent objects use the opaque depth behind them, and large radii or very small lights can reveal gather sampling. The aperture is circular; there are no blade shapes, autofocus, or temporal accumulation. The foreground/background split follows the common approach described in [AMD's depth-of-field documentation](https://gpuopen.com/manuals/fidelityfx_sdk/techniques/depth-of-field/); this renderer uses its own WGSL implementation without the FidelityFX SDK.

**Set Focus Distance** enables depth of field and sets its focus plane. **Set Aperture (f-stop)** changes aperture. Both Blueprint actions accept a numeric Value and write validated transient global overrides after volume blending. They do not alter authored settings. For manual focus while playing Lens Lab, disable the Lens Director graph; otherwise it writes focus every tick.

## Eye adaptation

**Auto exposure** exposes eye adaptation, strength, minimum/maximum EV, target brightness, separate brighten/darken speeds, and center weighting. The existing **Exposure EV** remains additive compensation after auto exposure. Minimum/maximum limits apply to the automatic contribution before strength and manual compensation.

```json
"auto_exposure": {
  "enabled": true,
  "strength": 1.0,
  "min_ev": -2.0,
  "max_ev": 2.0,
  "target_gray": 0.18,
  "speed_up": 1.5,
  "speed_down": 3.0,
  "center_weight": 0.65
}
```

Place this object inside `display`. All defaults are shown except `enabled` defaults to false. Strength and center weighting accept 0–1; EV bounds accept −16…16 with minimum ≤ maximum; target gray accepts 0.01–0.5; adaptation rates accept 0.01–20 per second. Larger rates adapt faster. Zero strength bypasses metering. Effect volumes blend strength from zero when entering/leaving a disabled exposure setup.

A fixed 128×128 grid builds a 256-bin log-luminance histogram spanning −12…16 stops, with optional center weighting. The darkest and brightest 2% of weighted samples are trimmed, then the mean log luminance determines the target EV. This limits the influence of tiny sparks and black borders. Metering runs after volumetrics and before depth of field/bloom, so lens blur and bloom do not feed back into adaptation.

The GPU stores the current EV and approaches the target using `1 - exp(-rate * dt)`. Brightening and darkening can use different rates. Rendering the same nonzero simulation time freezes adaptation, so paused and repeated draws do not keep changing brightness. First use, rewinding the simulation clock, and an explicit history reset meter immediately. Independent stills at time zero meter immediately on every draw. The editor’s Live preview advances its own effects clock and can be paused. Resize preserves history. Disabling the effect or raw/2D rendering clears its history.

`SceneRenderer::reset_display_history()` starts fresh on a camera cut, scene replacement, or independent capture. Hosts that switch cameras during continuous nonzero simulation time should call it. Ordinary camera movement retains adaptation. History belongs to the renderer's view and is not scene data; hosts rendering independent views should use separate renderer instances or reset between views. Auto exposure performs no GPU-to-CPU readback or frame stall.

## Pipeline and checks

Order: scene HDR/particles → reflections → SSAO/heat → volumetric fog → TAA/motion blur → exposure metering → depth of field → bloom → exposure/grade/tone mapping → optional FXAA/grain/vignette. Overlays and UI render afterward. `draw_linear` and 2D extraction bypass both effects. Disabled DOF releases its frame-sized intermediate textures; disabled auto exposure skips its compute dispatches. Enabled DOF costs three render passes and three half-resolution RGBA16Float targets plus its full-resolution HDR output. Auto exposure uses a fixed 1024-byte histogram and 16-byte persistent state.

```sh
BOZZARD_OPTICS_CAPTURE_DIR="$PWD/work/optics-tests" cargo test -p bozzard-render --test optics --offline --locked
cargo test --workspace --offline --locked
cargo clippy --workspace --all-targets --offline --locked -- -D warnings
cargo run -p bozzard-player --offline --locked -- --smoke --backend metal --scene examples/demo/scenes/lens-lab.json --output work/optics-lens-lab
```

Native GPU tests cover near/far defocus, focus changes, sharp occluders, zero/disabled/raw bypass, resize and upstream/downstream toggles, hardware/software sRGB parity, metering targets and limits, adaptation direction/timing, pause, equal elapsed time at different draw rates, highlight trimming, and history resets. CPU tests cover validation, JSON round trips, volume blending, editor history/Play/save/2D isolation, and transient Blueprint controls.

Player smoke writes `loaded-3d-before-optics.ppm`, `loaded-3d.ppm`, and `loaded-3d-animated.ppm`. Independent still captures reset exposure history. The save/reload check verifies authored display settings separately, then freezes the same runtime clock and Blueprint display overrides for its geometry/material comparison.
