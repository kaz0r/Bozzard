# Point and spot lights

Bozzard scenes may attach an optional `Light` component to any scene object. Lights are live ECS components and affect 3D rendering; they have no effect on 2D extraction. The editor can create a light from the scene-object menu and exposes its values in the inspector.

Each light has an `enabled` flag and a `kind`: `point` or `spot`. `color` is linear RGB with each channel in `0..=1`. `intensity` is luminous intensity in candela and accepts `0..=100000`. `range` is in world units and accepts `0.001..=100000`; it is independent of the object's scale. Defaults are enabled point lighting, white color, intensity `100`, and range `10`.

Spot lights additionally have `inner_angle_degrees` and `outer_angle_degrees` as half angles in degrees. The defaults are 20° and 30°. They must satisfy `0 <= inner_angle_degrees <= outer_angle_degrees <= 89.9`, and `outer_angle_degrees` must be at least 0.1°. A zero-width interval produces a hard cone; otherwise the cone uses squared smooth interpolation between the inner and outer angles. The spotlight direction is the object's local negative-Z direction after the object transform is applied, then normalized. Point lights do not use the direction.

The authored scene supports at most 32 `Light` components, including disabled components. Validation rejects a scene that exceeds this limit or contains non-finite or out-of-range values. The component follows the object's parent hierarchy for its world position and orientation. Editing, duplication, save/reopen, Undo/Redo, and Play follow the normal document rules: Play receives an isolated simulated world, while saving writes the authored document.

Direct light uses the renderer's shared PBR GGX plus Lambert contribution. Point and spot attenuation uses inverse square distance with a squared-distance floor of `0.0001`, multiplied by a smooth range cutoff:

```text
max(1 - (distance / range)^4, 0)^2
```

Sun lighting and its existing camera-independent shadows remain available. Point and spot lights currently have no local shadow maps. Bloom and baked GI are separate pending subsystems and are not implied by this feature.

In the editor, the Hierarchy **+ Light** menu creates a point or spot light. Selecting any object also exposes the **Light (3D)** component checkbox in the inspector, which adds or removes the component. The inspector edits `enabled`, kind, color, intensity, range, and (for spots) both angles. Light changes are undoable and are disabled while Play is running, consistent with other authored component edits.

Light markers remain clickable in the 3D viewport. Disabled lights are shown as gray markers and still participate in selection. Selecting a light shows its range guide; selecting a spot also shows its inner and outer cones. Guides are editor overlays and are hidden in 2D and during Play.

## Practical checks

1. Open a 3D scene in `bozzard-editor-app`, use Hierarchy **+ Light** to create a point light, or select an object and enable **Light (3D)**. Confirm the inspector shows the defaults, then change color, intensity, and range and verify the nearby PBR surface responds.
2. Change the light to a spot. Rotate the object and confirm the cone follows local negative Z, including through a rotated or parented object. Set equal inner and outer angles and confirm the cone edge is hard; use distinct angles to confirm a smooth transition.
3. Disable the light and confirm its contribution disappears. Create duplicate lights, use Undo/Redo, save, close/reopen, and enter/stop Play; confirm authored values persist and Play edits do not publish back to the scene.
4. Try invalid values (a negative or non-finite color/intensity/range, angles outside the limits, or more than 32 authored light components including disabled ones) and confirm validation or inspector clamping rejects them. Test a very small and a very large object scale to confirm range remains in world units.
5. Render the same scene in 2D and 3D and confirm lights affect only the 3D result. Select enabled and disabled lights to inspect their colored or gray markers, then select a point and spot light to inspect the range and cone guides. Confirm there are no local light shadows; sun shadows remain governed by the Scene lighting controls.

## Verified implementation checks

The release Lighting Lab can be launched with:

```sh
cargo run -p bozzard-player --release -- --scene examples/demo/scenes/lighting-lab.json
```

Native Metal smoke validation passed the existing fixtures plus `local_lights_gpu_ok`, covering PBR and diffuse color response, inverse-square falloff, range cutoff, spot cones and penumbra, equal-angle hard cones, multiple lights, removal, the 32-component limit, invalid inputs, and unlit surfaces. The release Lighting Lab also rendered a colored point light and warm spotlight. A 30-frame 800×500 M2 Pro measurement reported 0.068 ms optimized renderer CPU median and 0.497 ms synchronized CPU+GPU+wait wall median; this is a debug measurement and does not claim FPS.

Workspace CPU tests, all-target Clippy, and headless boundary checks passed. The new fixtures are included in the existing cross-platform `--smoke` CI coverage. Native editor-window capture remains unverified because the macOS screen was locked during the attempted capture; hosted CI validation for this subsystem remains pending.

The practical checks above describe the remaining hands-on editor workflow and are not a claim that the locked-screen UI check has passed.
