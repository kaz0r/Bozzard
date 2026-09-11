# Sponza reproduction

The Sponza dataset lives inside the working tree under gitignored `work/sponza/` and must not be committed. For the tuned scene, launch the native editor first:

```sh
cargo run -p bozzard-editor-app --locked --offline -- \
  --scene examples/sponza/scene.json
```

To try the local-light shadow test, launch the opt-in spotlight fixture:

```sh
cargo run -p bozzard-editor-app -- --scene examples/sponza/spotlights.json
```

In the editor, select **Warm spot** or **Cool spot** and toggle **Cast shadows** to compare each spotlight's local shadowing.

The source layout is `work/sponza/glTF/Sponza.gltf` with its upstream resources. Durable scene files are `examples/sponza/scene.json` (tuned corridor), `examples/sponza/atrium.json`, `examples/sponza/overview.json`, and `examples/sponza/spotlights.json` (local-light test). The first three fixtures use the authored sun/sky setup; inspected captures are `work/sponza/final-atrium/loaded-3d.ppm` and `work/sponza/final-overview/loaded-3d.ppm`.

Inspect the complete CPU import with:

```sh
cargo run -p bozzard-assets --example inspect --locked --offline -- work/sponza/glTF/Sponza.gltf
```

In the editor, Alt-click imported geometry in the viewport or choose a surface row in the Hierarchy or **Imported surfaces** inspector. Ordinary clicks select the whole model for Move/Rotate/Scale. Double-click a row or press **F** to frame that surface; **Select whole model** returns to the owner selection, and **Shift+F** frames the whole layer. Press **Escape** to clear the whole-object or inspected-surface selection and its outline after higher-priority editing or navigation actions have finished. A source primitive can contain disconnected geometry, so the list does not automatically create separate editable entries for each disconnected piece.

The CPU-only surface reproduction is:

```sh
cargo run -p bozzard-editor --example inspect_surfaces --locked --offline -- \
  examples/sponza/scene.json
```

Picking and outline/framing are held until the CPU model identity matches the GPU resident last-good data. The native Model Workshop inspector capture is written to `work/editor-surfaces-smoke/editor-surface.ppm`.

Manual surface-override checklist:

- [ ] Open `examples/sponza/scene.json` and Alt-click imported geometry.
- [ ] Choose a row in **Imported surfaces**, change Tint, and confirm only that object's surface changes.
- [ ] Enable and adjust Metallic or Roughness; confirm the value multiplies the existing map, then use **Reset override**.
- [ ] Select the whole model, duplicate the owning object, select a surface on the copy, change its Tint, and confirm the original remains unchanged; Undo/redo the edit.
- [ ] Save, reopen, enter Play, and Stop; confirm overrides persist with the authored document and Play remains isolated.

For a reproducible CPU-only override round trip, choose a nonexistent output path:

```sh
cargo run -p bozzard-editor --example material_override --locked --offline -- \
  examples/sponza/scene.json work/sponza/material-override-scene.json
```

The example picks a camera-center PBR surface and exercises Undo/Redo, save/reopen, and Play isolation. A source-signature mismatch from changed geometry or names leaves an override stored but inactive with an inspector warning; unchanged reloads preserve overrides.

To package a model into a new scene, use the editor crate's import example and choose a destination that does not already exist:

```sh
cargo run -p bozzard-editor --example import_model --locked --offline -- \
  work/sponza/glTF/Sponza.gltf work/sponza/my-project/scene.json
```

Open the newly packaged scene with the following command. This import example uses its default camera; the tuned views are the three files under `examples/sponza/`.

```sh
cargo run -p bozzard-editor-app --locked --offline -- \
  --scene work/sponza/my-project/scene.json
```

The player can render a scene or run the frame benchmark. `N` is restricted to 1–1000:

```sh
cargo run -p bozzard-player --locked --offline -- \
  --scene examples/sponza/scene.json --smoke --output work/sponza/culling \
  --hardware --backend metal --benchmark-frames 30
```

The current 30-frame debug Metal benchmark reports 103 reference color draws, 262,267 color triangles, and 103 pipeline binds. Conservative culling reports 89 draws and 257,752 triangles; full state caching reduces compatible pipeline binds to 1. Shadow draws remain 103 with 262,267 triangles, and all configurations are pixel-identical. The measured CPU median is 18.189 ms for the reference and 17.045 ms optimized; synchronized CPU+GPU+wait wall median is 23.977 ms and 22.924 ms respectively. Culling-only wall median is 22.810 ms, so these figures do not establish a GPU-time improvement or FPS result.

These debug figures are retained as historical context. The current release picking and renderer measurements are documented in [Performance measurements](performance.md).

The benchmark was run on an Apple M2 Pro at 800×500 with a 4096 shadow map for 30 debug frames. The associated validation scope includes workspace formatting/tests/Clippy/headless checks, native editor validation, and the Metal Sponza rendering checks; this page records the reproduction and measurements rather than a new run.

The standard fixtures use the current authored sun and ambient lighting, one camera-independent directional shadow map, procedural diffuse/specular environment lighting, PBR materials, HDR display encoding, staged GPU uploads, and conservative color-pass culling. The corridor, atrium, and overview captures were visually inspected at their durable viewpoints. `spotlights.json` uses the same Sponza asset path and adds the authored **Warm spot** and **Cool spot** local lights for testing spotlight shadow maps; point-light shadows are not available. This spotlight fixture exercises local lights and shadows, not a GI bake or bloom, which are absent from these fixtures. Cascaded shadows, HDR panorama import, local reflection probes, scene environment occlusion, atmospheric simulation, GPU timestamps, multidraw, instancing, and occlusion culling remain outside this reproduction scope.

Final native Metal validation also ran directly from `examples/sponza/scene.json` (`work/sponza/final-durable`). All graphics fixtures and exact scene save/reload passed. Shadow-enabled versus disabled captures differed at 158,181 corridor pixels, 144,930 atrium pixels, and 168,284 overview pixels. The checked-in views reference the ignored dataset; they contain no downloaded model or textures. These local commits have not been pushed or validated by Linux/Windows CI.

Uploads enforce a 4 MiB per-slice work budget with a soft 4 ms CPU target, not a hard latency bound. The final debug reproduction observed a 31.80 ms maximum CPU slice; frame-time guarantees are not claimed.
