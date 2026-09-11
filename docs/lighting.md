# Point and spot lights

Bozzard scenes may attach an optional `Light` component to any scene object. Lights are live ECS components and affect 3D rendering; they have no effect on 2D extraction. The editor can create a light from the scene-object menu and exposes its values in the inspector.

Each light has an `enabled` flag and a `kind`: `point` or `spot`. `color` is linear RGB with each channel in `0..=1`. `intensity` is luminous intensity in candela and accepts `0..=100000`. `range` is in world units and accepts `0.001..=100000`; it is independent of the object's scale. Defaults are enabled point lighting, white color, intensity `100`, and range `10`.

Spot lights additionally have `inner_angle_degrees` and `outer_angle_degrees` as half angles in degrees. The defaults are 20° and 30°. They must satisfy `0 <= inner_angle_degrees <= outer_angle_degrees <= 89.9`, and `outer_angle_degrees` must be at least 0.1°. A zero-width interval produces a hard cone; otherwise the cone uses squared smooth interpolation between the inner and outer angles. The spotlight direction is the object's local negative-Z direction after the object transform is applied, then normalized. Point lights do not use the direction.

The authored scene supports at most 32 `Light` components, including disabled components. Validation rejects a scene that exceeds this limit or contains non-finite or out-of-range values. The component follows the object's parent hierarchy for its world position and orientation. Editing, duplication, save/reopen, Undo/Redo, and Play follow the normal document rules: Play receives an isolated simulated world, while saving writes the authored document.

Direct light uses the renderer's shared PBR GGX plus Lambert contribution. Point and spot attenuation uses inverse square distance with a squared-distance floor of `0.0001`, multiplied by a smooth range cutoff:

```text
max(1 - (distance / range)^4, 0)^2
```

Sun lighting and its existing camera-independent shadows remain available. Point and spot lights currently have no local shadow maps. Bloom and baked GI are available as separate display and static diffuse-transport systems.

In the editor, the Hierarchy **+ Light** menu creates a point or spot light. Selecting any object also exposes the **Light (3D)** component checkbox in the inspector, which adds or removes the component. The inspector edits `enabled`, kind, color, intensity, range, and (for spots) both angles. Light changes are undoable and are disabled while Play is running, consistent with other authored component edits.

Light markers remain clickable in the 3D viewport. Disabled lights are shown as gray markers and still participate in selection. Selecting a light shows its range guide; selecting a spot also shows its inner and outer cones. Guides are editor overlays and are hidden in 2D and during Play.

## Practical checks

1. Open a 3D scene in `bozzard-editor-app`, use Hierarchy **+ Light** to create a point light, or select an object and enable **Light (3D)**. Confirm the inspector shows the defaults, then change color, intensity, and range and verify the nearby PBR surface responds.
2. Change the light to a spot. Rotate the object and confirm the cone follows local negative Z, including through a rotated or parented object. Set equal inner and outer angles and confirm the cone edge is hard; use distinct angles to confirm a smooth transition.
3. Disable the light and confirm its contribution disappears. Create duplicate lights, use Undo/Redo, save, close/reopen, and enter/stop Play; confirm authored values persist and Play edits do not publish back to the scene.
4. Try invalid values (a negative or non-finite color/intensity/range, angles outside the limits, or more than 32 authored light components including disabled ones) and confirm validation or inspector clamping rejects them. Test a very small and a very large object scale to confirm range remains in world units.
5. Render the same scene in 2D and 3D and confirm lights affect only the 3D result. Select enabled and disabled lights to inspect their colored or gray markers, then select a point and spot light to inspect the range and cone guides. Confirm there are no local light shadows; sun shadows remain governed by the Scene lighting controls.
6. In **Display (3D)**, enable Bloom and inspect the defaults: intensity `0.15`, threshold `1`, and Spread `0.7`. Lower or raise the scene-linear threshold and confirm only bright regions produce the glow; adjust Spread to change the radius.
7. Disable Bloom or set intensity to zero and confirm the glow disappears. Toggle it back on, use **Reset display**, save/reopen, and enter/stop Play to confirm display settings follow normal history and Play isolation. Verify 2D extraction and raw `draw_linear` diagnostics bypass Bloom.

## Verified implementation checks

The release Lighting Lab can be launched with:

```sh
cargo run -p bozzard-player --release -- --scene examples/demo/scenes/lighting-lab.json
```

Native Metal smoke validation passed the existing fixtures plus `local_lights_gpu_ok`, covering PBR and diffuse color response, inverse-square falloff, range cutoff, spot cones and penumbra, equal-angle hard cones, multiple lights, removal, the 32-component limit, invalid inputs, and unlit surfaces. The release Lighting Lab also rendered a colored point light and warm spotlight. A 30-frame 800×500 M2 Pro measurement reported 0.068 ms optimized renderer CPU median and 0.497 ms synchronized CPU+GPU+wait wall median; this is a renderer measurement and does not claim FPS.

Workspace CPU tests, all-target Clippy, and headless boundary checks passed. The new fixtures are included in the existing cross-platform `--smoke` CI coverage. Native light-inspector and guide capture also passed; see the editor validation in the GI section below.

## Bloom

Bloom is an optional 3D display effect configured under the scene's **Display (3D)** controls. It is disabled by default. `intensity` defaults to `0.15` and accepts `0..=10`; `threshold` defaults to `1` and accepts `0..=60000` scene-linear radiance before exposure; and `scatter` defaults to `0.7` and accepts `0..=1` (shown as **Spread** in the editor). The threshold uses a 50% soft knee.

The renderer extracts bright HDR scene color, builds up to six half-resolution `Rgba16Float` pyramid levels, then combines normalized linear downsample/tent-upsample levels. The result is added to HDR scene color before exposure, Reinhard tone mapping, and sRGB encoding. Constant fields therefore do not change brightness when the pyramid depth changes, and the composite preserves alpha. Bloom changes do not illuminate geometry.

Bloom is bypassed when disabled or when intensity is zero, by raw `draw_linear` diagnostics, and for 2D extraction. Disabling it releases the pyramid resources. Display controls follow normal Undo/Redo, save/reopen, and Play isolation rules; **Reset display** restores the defaults. Baked GI remains limited to the static diffuse workflow described below.

Bloom validation passed the workspace CPU tests, Clippy, headless checks, and formatting. Native Metal full smoke passed `bloom_gpu_ok`, covering halo formation, threshold, intensity, spread, disable, raw readback, alpha preservation, HDR-before-display ordering, constant-energy normalization, odd/tiny resize handling, and sRGB parity. The numeric oracle for a constant HDR-4 field with threshold 1 and intensity 1 produced HDR-7 before exposure, then the expected −3 EV exposure, Reinhard, and sRGB result across 64×64, 97×53, 1×1, 1×17, and 3×5 targets.

Visual review compared `work/bloom-gpu/bloom-off.png` and `work/bloom-gpu/bloom-on.png`; the saved Lighting Lab also rendered with bloom enabled at intensity `0.4`, threshold `0.6`, and scatter `0.8` in `work/bloom-lab/loaded-3d.png`. Save/reload pixel parity passed. A release 30-frame 800×500 M2 Pro measurement reported 0.200 ms CPU median and 0.978 ms synchronized CPU+GPU+wait wall median with bloom enabled; the same binary with bloom disabled measured 0.074 ms and 0.526 ms. These are renderer measurements, not FPS claims.

The cross-platform fixtures are included in the existing `--smoke` CI coverage. Logs are retained at `/tmp/bozzard-bloom-tests.log`, `/tmp/bozzard-bloom-clippy.log`, `/tmp/bozzard-bloom-gpu.log`, `/tmp/bozzard-bloom-lab.log`, and `/tmp/bozzard-bloom-lab-off.log`.

## Baked global illumination

Baked GI is an optional, static diffuse irradiance volume for 3D scenes. A scene has one bounded probe volume with `min` and `max` world-space corners, a `resolution` of 2–16 probes per axis, 64–1024 power-of-two rays per probe, and 1–4 diffuse bounces. **Fit scene** derives bounds from eligible static geometry; **Bake GI** runs the CPU bake asynchronously; **Clear bake** removes the saved result. The editor exposes GI enable, intensity `0..=10`, normal bias `0..=1`, volume bounds, grid resolution, ray count, and diffuse bounce count under **Scene lighting → Baked global illumination**.

The bake stores nine diffuse spherical-harmonic coefficients plus directional visibility moments per probe. It is serialized inline with the scene through shared `Arc` data, so save/reopen, Undo/Redo, and Play isolation preserve the authored bake. Runtime rendering evaluates the probe SH coefficients and trilinear visibility weights, then applies the configured intensity and normal bias to diffuse indirect light on receiving surfaces. CPU transport samples source textures at mip level zero.

Only eligible static 3D drawables contribute transport. A drawable's **Contribute to GI bake (static)** flag controls eligibility. Objects with spin, enabled gravity, or a player controller, together with their descendants, are excluded automatically; moving objects may still receive baked light. Alpha-blended transport is excluded. Enabled authored sun, point/spot lights, procedural sky inputs, static transforms, materials, volume settings, and loaded asset content contribute to the bake source fingerprint.

Changing a source asset, static scene input, light, or volume expires the fingerprint and disables the stale bake until it is rebuilt. A failed asset reload retains the last-good asset identity, so a valid existing bake is not invalidated by an unsuccessful replacement. There is no runtime rebaking. The current system supports one volume and diffuse transport only: no glossy or specular GI, caustics, alpha-blend transport, probe streaming, or multiple volumes.

Enable **Show volume and probes** to draw the editor-only volume and probe overlay. Cyan indicates a current bake, amber indicates an outdated or unbaked volume, and gray marks invalid probe data. The overlay is disabled in 2D and during Play. Baked indirect visibility does not create realtime local shadow maps; point and spot lights still have no local shadows.

The CPU bake example command is:

```sh
cargo run --release -p bozzard-editor --example bake_gi --locked --offline -- \
  examples/demo/scenes/gi-lab.json work/gi-lab/baked.json
```

The command produces the GI Lab bake with 384 probes and 251,904 packed probe bytes. The saved demo uses normal bias `0.2` to reduce near-wall artifacts; a release M2 Pro run took approximately 109.7 ms. Source, CPU transport, native Metal, and editor async/history/save/Play checks have passed for the focused GI paths, including constant-energy, directional SH, visibility, material, invalid-data, resize, background-bake, current-fingerprint, guides, and save coverage. The native editor capture at `work/editor-gi-complete` covers the lights, surface, and GI panels, a background bake, the current fingerprint, guides, and save. The final tuned-bias end-to-end Metal fixture (CPU bake → JSON round trip → extraction → render) changed 14,984 pixels, including 683 pixels that gained red/green from previously neutral values. These fixtures run in the existing hosted Metal/Vulkan/DX12 workflow; hosted CI has not been run for this revision because it has not been pushed.

The GI room and Sponza examples show the intended low-frequency approximation. Coarse grids and the 8×8 directional moment maps can produce mottling, light leaks, or excess darkness near thin walls, so volume placement, ray count, and normal bias affect quality. For the tuned Sponza bake, ignored artifacts are `work/sponza/gi-source.json`, `work/sponza/gi-baked.json`, and `work/sponza/gi-off.json`; the bake uses 768 probes, 256 rays, 3 diffuse bounces, bounds `[-12, -0.3, -5]` to `[12, 10, 5]`, and grid `[12, 8, 8]`. It baked in approximately 4,258 ms and passed save/reopen. Reviewed captures are `work/sponza/gi-final-on/loaded-3d.png` and `work/sponza/gi-final-off/loaded-3d.png`; GI darkens the covered corridor compared with the unoccluded sky lighting used when it is disabled. With GI enabled, the release binary measured 0.667 ms CPU and 5.652 ms synchronized CPU+GPU+wait wall over 30 frames at 800×500 on an M2 Pro; with GI disabled, the same binary measured 0.887 ms and 3.827 ms. Both had 89 visible surfaces, 14 culled surfaces, 257,752 color triangles, 103 shadow draws, and 262,267 shadow triangles. These are renderer measurements, not FPS claims or evidence of a CPU speedup.
