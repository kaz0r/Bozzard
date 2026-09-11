# Performance measurements

Bozzard has two small, reproducible benchmark paths. They answer different questions: the editor example measures CPU authoring and inspection work, while the player benchmark measures a synchronized render loop for several renderer configurations. Neither reports frames per second, GPU timestamp queries, or a direct GPU execution time.

## Editor CPU paths

Run the editor example against a scene file. A release build is the intended comparison point:

```sh
cargo run --release -p bozzard-editor --example benchmark_editor --locked --offline -- \
  examples/sponza/scene.json 200
```

The final argument is the number of samples (10–2000; the default is 200). Scene opening is printed as one `load_ms` measurement. Each repeated path performs 10 untimed warm-up iterations, then records wall-clock CPU elapsed time for `render_extract`, collision extraction, selected-surface bounds, center picking, and a grid of pick queries. The output reports the median and p95 in milliseconds. These spans include the Rust work executed by the operation and exclude GPU submission or presentation. The example checks that the scene, dirty state, and undo history are unchanged.

## Synchronized renderer benchmark

The player benchmark requires smoke mode and a scene. `N` is restricted to 1–1000:

```sh
cargo run --release -p bozzard-player --locked --offline -- \
  --smoke --scene examples/sponza/scene.json \
  --hardware --backend metal --benchmark-frames 30 \
  --output work/sponza/performance
```

The benchmark first renders reference, culling, and full state-cache configurations and requires exact pixel equality. It then runs three warm-up iterations and interleaves the configurations for `N` measured frames. For every frame, `cpu_ms`, `prepare_ms`, `encode_ms`, and `submit_ms` come from renderer CPU statistics. `synchronized_wall_median_ms` is an `Instant` span around draw plus an explicit device wait, so it includes CPU work, GPU completion, and wait overhead. It is useful for comparing the same device and workload; it is not FPS or a GPU timestamp.

Keep device, backend, scene, render size, shadow settings, and build mode fixed when comparing runs. Record the printed medians together with surface, visibility, triangle, shadow, and pipeline-bind counts. Those counts are reported from the last measured frame for each mode; the timing fields are medians across the measured frames.

The benchmark’s reference/culling/cache comparison is a diagnostic for renderer correctness and CPU-side cost. It does not establish image quality, power use, GPU occupancy, or a windowed presentation rate. For Sponza setup and the ignored dataset location, see [the Sponza reproduction](sponza.md).

## Recorded Sponza measurements

The editor picking comparison used 200 same-process samples, alternating BVH and linear traversal order for each paired measurement. The reported values use midpoint medians in milliseconds; p95 values, when printed by the example, use nearest-rank selection. The wider 1,681-ray checks were untimed and compared object and surface identities against the linear oracle.

| View | Center BVH | Center linear | Grid BVH | Grid linear |
| --- | ---: | ---: | ---: | ---: |
| Corridor | 0.006667 | 1.309292 | 0.009834 | 1.295583 |
| Atrium | 0.006646 | 1.330771 | 0.011355 | 1.321001 |
| Overview | 0.005750 | 1.316250 | 0.009708 | 1.290750 |

The wider checks recorded exact object/surface identity agreement for 1,681 corridor hits, 1,675 atrium hits, and 1,661 overview hits (5,043 rays total). The BVH contains 262,267 triangles, 65,781 nodes, and 3,154,060 resident bytes; construction took 35.4–36.6 ms. A successful mesh replacement builds its index once; catalog clones, undo/redo, and duplicates reuse it. A failed reload keeps the matching last-good index. Picking is an interaction path, so these measurements do not imply a viewport frame-rate improvement. The original ECS-cache hypothesis was rejected because the measured editor operation baseline was about 0.003 ms outside picking.

For context, an Apple M2 Pro/Metal release renderer run at 800×500 with a 4096 shadow map and 100 frames reported these medians:

| Mode | CPU | Synchronized wall | Prepare | Encode | Submit |
| --- | ---: | ---: | ---: | ---: | ---: |
| Reference | 0.678 ms | 3.286 ms | 0.378 ms | 0.275 ms | 0.018 ms |
| Optimized | 0.644 ms | 3.319 ms | 0.368 ms | 0.249 ms | 0.019 ms |

The optimized mode retained 89 of 103 color surfaces and one pipeline bind while retaining all 103 shadow draws. The close synchronized wall medians do not establish a renderer timing gain. Editor CPU paths and the renderer loop are measured separately; full viewport UI cost and GPU timestamp data are outside this report.

The documented results were validated locally on the Mac test host with workspace tests, formatting, all-target Clippy with warnings denied, headless checks, release native editor smoke, and the release native player graphics suite plus Sponza benchmark. Sixteen before/after PPM diagnostics, including loaded Sponza output, were byte-identical. These are local validations for the unpushed work and do not represent CI results.
