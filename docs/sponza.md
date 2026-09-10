# Sponza reproduction

The Sponza dataset lives inside the working tree under gitignored `work/sponza/` and must not be committed. For the tuned scene, launch the native editor first:

```sh
cargo run -p bozzard-editor-app --locked --offline -- \
  --scene examples/sponza/scene.json
```

The source layout is `work/sponza/glTF/Sponza.gltf` with its upstream resources. Durable scene files are `examples/sponza/scene.json` (tuned corridor), `examples/sponza/atrium.json`, and `examples/sponza/overview.json`. The tuned scenes use the authored lighting setup; inspected captures are `work/sponza/final-atrium/loaded-3d.ppm` and `work/sponza/final-overview/loaded-3d.ppm`.

Inspect the complete CPU import with:

```sh
cargo run -p bozzard-assets --example inspect --locked --offline -- work/sponza/glTF/Sponza.gltf
```

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

The benchmark was run on an Apple M2 Pro at 800×500 with a 4096 shadow map for 30 debug frames. The associated validation scope includes workspace formatting/tests/Clippy/headless checks, native editor validation, and the Metal Sponza rendering checks; this page records the reproduction and measurements rather than a new run.

The scene uses the current authored sun and ambient lighting, one camera-independent directional shadow map, procedural diffuse/specular environment lighting, PBR materials, HDR display encoding, staged GPU uploads, and conservative color-pass culling. The corridor, atrium, and overview captures were visually inspected at their durable viewpoints. The renderer still has one global sun/environment model: point or spot lights, cascaded shadows, HDR panorama import, local reflection probes, scene environment occlusion, GI/multibounce, atmospheric simulation, GPU timestamps, multidraw, instancing, and occlusion culling remain outside this scope.

Final native Metal validation also ran directly from `examples/sponza/scene.json` (`work/sponza/final-durable`). All graphics fixtures and exact scene save/reload passed. Shadow-enabled versus disabled captures differed at 158,181 corridor pixels, 144,930 atrium pixels, and 168,284 overview pixels. The checked-in views reference the ignored dataset; they contain no downloaded model or textures. These local commits have not been pushed or validated by Linux/Windows CI.

Uploads enforce a 4 MiB per-slice work budget with a soft 4 ms CPU target, not a hard latency bound. The final debug reproduction observed a 31.80 ms maximum CPU slice; frame-time guarantees are not claimed.
