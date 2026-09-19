# Occlusion culling

The renderer can skip opaque geometry hidden behind larger opaque surfaces.
This changes rendering only; occluded objects still simulate, participate in
queries and cast shadows. The editor enables it through **View → Occlusion
culling**. The player enables it by default; `--no-occlusion` and the renderer's
`set_occlusion_enabled(false)` select the reference path.

## Reproduce a scene

```sh
cargo run -p bozzard-demo --example occlusion_lab -- work/occlusion-lab.json
cargo run -p bozzard-editor-app -- --scene work/occlusion-lab.json
cargo run -p bozzard-player --release -- --scene work/occlusion-lab.json
```

The generator refuses existing output files. It writes a portable warehouse with
1,024 stock objects, surrounding walls and a shutter script. Enter Play and press
**Space** to open or close the shutter. The stock stays in the world throughout.
Shadows are disabled in this fixture to isolate color-pass work. Compare the
editor's occlusion toggle, renderer statistics and Debug capture while viewing
the closed shutter, then after opening it or moving the inspection camera.

## Visibility and limits

A separate full-resolution depth prepass draws up to 32 nearby opaque occluders,
with at most 100,000 depth triangles. Occluders must occupy at least 2% of the
viewport. Max-depth reduction builds an R32Float hierarchy starting with 8×8 pixel
tiles; compute shaders test the projected bounds of each existing draw batch and
write indexed indirect arguments. The color pass preserves its original order and
depth buffer. Background and padded pixels, near-plane crossings and equal-depth
surfaces remain conservative. Small scenes (fewer than 64 visible surfaces or four
batches), scenes without eligible occluders, and more than 16,384 batches bypass
the extra passes.

Transparent surfaces, shader graphs and deformed geometry are not culled by this
stage. Alpha-masked, transparent, generated-texture, text, sprite, shader-graph and
deformed surfaces cannot become occluders. This intentionally leaves some hidden
work visible rather than approximating its coverage. Frustum culling and authored
[LOD](lod.md) remain separate stages. Occlusion does not reduce imported-asset
residency requirements or alter shadow-caster selection.

Readback uses at most three asynchronous buffers and never waits for the GPU in
ordinary rendering. Completed batch visibility can omit CPU draw commands only
when the actual view-projection matrix (including temporal jitter), viewport,
selected depth geometry/transforms/sidedness, and all candidate bounds/depths/counts
match exactly. Asset replacement invalidates reuse even when IDs and bounds stay
the same. Any changed inputs use current-frame GPU visibility. Disabling state
caching also disables this reuse. Pipelines, depth targets, query buffers and CPU
storage are retained until their required sizes change.

`FrameStats` exposes tested batches, depth draws/triangles, allocated occlusion
bytes, cache reuse and completed GPU results with their source frame IDs. In an
indirect frame, color triangles are an upper bound and hidden commands still count
as submitted commands. In a cached frame, skipped commands/triangles are already
absent from those counters. Do not subtract old GPU savings from a later frame or
subtract cached savings twice. Logical occlusion allocation is separate from the
imported-asset residency budget; it includes depth/pyramid textures and readback.

## Correctness and performance checks

```sh
cargo test -p bozzard-render occlusion --lib
cargo test -p bozzard-player --release native_occlusion -- --ignored --nocapture
```

The native check requires a hardware graphics adapter. It compares exact pixels
against disabled culling across moving occluders/cameras, odd viewport sizes,
near-plane crossings, reflected transforms, alpha holes and same-ID geometry
replacement. It also checks cached visibility removes hidden draw commands and
retains exact pixels. The normal player GPU smoke suite includes these correctness
checks. The ignored release check additionally interleaves reference and enabled
frames for static/moving hidden spheres, cheap cubes and an open view, with three
warmup iterations, 22 measured iterations and a GPU wait per frame. CPU and
synchronized CPU/GPU/wait medians are local fixture measurements, not FPS promises.

On Apple M2 Pro/Metal at 320×320, a release run on 2026-09-19 measured:

| Fixture | Reference CPU | Enabled CPU | Reference synchronized | Enabled synchronized |
| --- | ---: | ---: | ---: | ---: |
| 1,024 hidden spheres, static shutter | 0.748 ms | 0.725 ms | 2.722 ms | 1.203 ms |
| Same spheres, moving shutter | 0.756 ms | 0.805 ms | 2.802 ms | 2.019 ms |
| 1,024 cheap hidden cubes | 0.695 ms | 0.715 ms | 1.434 ms | 1.331 ms |
| Open sphere view, no eligible occluder | 0.728 ms | 0.759 ms | 2.250 ms | 1.908 ms |

The static cases submit one color command instead of 33 after visibility completes.
The moving case still submits 33 indirect commands but skips 1,572,864 hidden
triangles on the GPU. The open case bypasses GPU culling; its wall-time difference
is run-to-run/scheduling variation, not an occlusion saving. Query preparation has
a CPU cost, and benefits depend on actual geometry, coverage, motion and hardware.
