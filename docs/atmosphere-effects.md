# Atmosphere and motion effects

Open the wider `examples/demo/scenes/atmosphere-lab.json` for the combined stack, or `bonfire-lab.json` for the close-up camera.

```sh
cargo run -p bozzard-editor-app -- --scene examples/demo/scenes/atmosphere-lab.json
```

## Apply effects in the editor

Click **Effects** in the top menu. The right panel has the common controls together:

- **Apply preset** chooses Neutral, Cinematic, Bonfire, Neon, or Noir. Non-neutral looks also enable TAA, motion blur, and reflections; lens and exposure adaptation remain opt-in.
- **Live preview** runs particles, wind, shimmer, grain, and eye adaptation in an isolated preview. It starts particle effects with a short warm-up. Uncheck it to pause. Preview does not run scripts, gravity, or gameplay, and never writes runtime particles to the scene. **Play** runs the full simulation.
- **Before** temporarily shows the base viewport with default display mapping, without particles or fog. It does not edit the document.
- **Focus selected object** sets the focus distance using the editor camera. Aperture controls the amount of defocus.
- **Make selected surface wet** applies a smooth dielectric material to a selected mesh and enables reflections. Mesh primitives and imported meshes both support roughness/metallic factors.
- **Add particles** creates Smoke, Ash, or Sparks at the selection. Their Inspector exposes wind, size, color, opacity, curl and trails. The same presets are under **Create → Particles**.
- **Fine tuning** contains all detailed controls from this branch. **Quality** adjusts reflection, blur, and volumetric sampling. Effect volumes can be created at the camera, then edited below the controls.

Effect edits, material presets, particle creation, and volumes support ordinary Undo/Redo and saving. Stop Play before changing authored effects. All display effects and particles stay out of the 2D view.

## Particles

The scene-level `particle_emitter` component supports deterministic Smoke, Ash, and Sparks presets. Simulation uses bounded substeps, world-space wind, a smooth divergence-free curl field, drag, gravity, randomized lifetimes and size, and ash rotation. Old particles drain when emission is disabled. Removing an emitter removes its runtime particles.

The renderer batches particles into one instanced draw, sorts them back to front, samples opaque depth for occlusion and soft intersections, and shares scene-light/shadow bindings. Smoke and ash scatter ambient, sun, and local illumination; sparks emit HDR color and stretch along velocity into tapered trails. Nearby point-light scattering is bounded to avoid glowing smoke discs.

Budgets are 2,048 particles per emitter and 16,384 globally. The Inspector allows lower budgets. Particle counts and triangle counts are separate from mesh statistics. Runtime particle positions, ages, and random sequences are not serialized.

## TAA and motion blur

Temporal anti-aliasing uses eight Halton subpixel samples, camera and per-object reprojection, depth/normal rejection, HDR neighborhood clipping, and reactive coverage for translucent surfaces and particles. Runtime object identities include their ECS generation and world; imported surfaces retain separate transform history. Resize, rewind, explicit history reset, large camera cuts, and raw rendering invalidate history. Repeated paused frames hold the resolved image. TAA replaces the final FXAA filter when enabled.

Motion blur uses camera/object velocity with shutter angle referenced to a 60 Hz frame. A 180-degree shutter represents 1/120 second. Tile maxima let moving silhouettes spread into background pixels; depth checks protect stationary foreground objects. Blur is limited by the authored radius and an absolute 128-pixel cap. Paused frames produce a sharp still; newly spawned objects do not inherit old velocities. Sparks use their own trails instead of mesh motion blur.

## Reflections

Screen-space reflections use the actual shaded surface normal (including imported normal maps), roughness, Fresnel color, and material occlusion. Perspective-correct screen traversal and a short binary refinement find opaque depth intersections. Hits are filtered with roughness and depth protection, then replace the corresponding environment-specular contribution. Fog attenuates the reflected contribution.

The trace has a bounded 16–128 steps. Off-screen rays, missing geometry, backfaces, and rough surfaces retain environment lighting. Screen-space reflections cannot reflect geometry absent from the current view and do not solve multilayer glass or reflection recursion. **Make selected surface wet** is a useful starting point for visible ground reflections.

## Rendering and verification

Geometry writes HDR plus normal/roughness, motion/reactivity, and Fresnel/occlusion buffers. Particles composite before reflections. SSAO, heat shimmer, and volumetrics follow; TAA and motion blur precede exposure metering, bokeh, bloom, grading, and film effects. Temporal history is ping-ponged, while downstream bokeh/bloom textures are reused when only the input binding changes. No new dependencies were added; the simulation remains headless.

Meaningful regression coverage lives in `bozzard-scene/tests/particles.rs`, `bozzard-render/tests/particles.rs`, `bozzard-render/tests/temporal.rs`, and the editor's effects tests. It covers simulation bounds, lighting, intersections, sorting, trails, thin-geometry accumulation, disocclusion, object identity, motion silhouettes, foreground protection, reflection hits/misses, pause, rewind, resize, material/volume save/undo, and 2D isolation. Native editor smoke checks exercise the existing edit/Play/save workflow with the Effects panel visible.

Implementation references: [Karis, High-Quality Temporal Supersampling](https://www.advances.realtimerendering.com/s2014/index.html) and [McGuire and Mara, Efficient GPU Screen-Space Ray Tracing](https://jcgt.org/published/0003/04/04/paper.pdf). The implementation uses bounded traversal and conservative rejection tailored to Bozzard's renderer.
