# Compute verification and performance

Recorded on Apple M2 Pro / Metal, Rust 1.95.0, wgpu 30.0.1, optimized build:

```sh
cargo run --release -p bozzard-render --example compute_benchmark
```

The workload performs 32 dependent sine/scale/add iterations per element. Means cover 20 warm
dispatches and five transfer samples after three warm-ups. CPU data allocation and GPU shader
compilation are outside the timed loop. The CPU baseline is a Rust `f32::sin` loop, not a
hand-vectorized library. Results describe this machine and workload, not a general speed claim.
The initial recording ran alongside regression checks; use an idle machine for tuning.

| Mean milliseconds | 256 elements / 1 KiB | 262,144 elements / 1 MiB |
| --- | ---: | ---: |
| CPU kernel | 0.0445 | 46.0242 |
| GPU pass timestamp | 0.0141 | 0.2727 |
| CPU encode | 0.0018 | 0.0014 |
| Queue submit | 0.0275 | 0.0225 |
| Resident dispatch + completion wait | 0.2936 | 0.5574 |
| Typed upload + completion wait | 0.1704 | 1.5583 |
| Typed readback + unpack + completion wait | 0.2089 | 2.9060 |
| New warm GPU pipeline/binding/resource/upload allocations | 0 | 0 |

Rows measure different operations: do not add GPU timestamps to roundtrip times. Upload includes
typed JSON packing; readback includes typed JSON reconstruction. Input JSON construction and
Rhai conversion are excluded. This is not raw memory bandwidth. Waits expose measurement latency;
gameplay polls without blocking and keeps visual outputs resident. The small workload is better
on CPU after submission overhead. The large compute-heavy workload benefits from GPU execution,
but transfers cost more than the kernel. [Raw output](measurements/compute-metal.json) is retained.

## Regression matrix

| Area | Evidence |
| --- | --- |
| CPU shader/layout | `bozzard-compute/tests/layout.rs`: WGSL errors, reflection, padding, matrices/arrays, numeric overflow, runtime-tail minimum size |
| CPU protocol | `bozzard-compute/tests/runtime.rs`: ordering, snapshots, catch-up ticks, deferred/rejected batches, quotas, ownership, stale handles, cancellation, device failure, paused delivery |
| Native GPU | `bozzard-render/tests/compute.rs`: CPU oracle, partial groups, ordered range transfers, texture pixels, cache/pool reuse, readback saturation, in-flight cancellation, old-world callbacks, resize/restart bounds |
| Scripts | `bozzard-scene/tests/compute_scripts.rs`: real Rhai hooks, named jobs, explicit sharing, disable/re-enable, save/load/restart, headless policy and lazy unused state |
| Presentation/reload | `bozzard-editor/tests/compute.rs`: real samples, generated material and shader-graph/imported-model sampling, animation, multiple viewports, pause/Stop, compatible/invalid/incompatible/device-invalid edits |
| Export | `bozzard-project/tests/export.rs` removes original sources; packaged `--smoke` also runs shaders from a relocated executable with empty cwd/PATH |
| Native editor | Manual Play/Stop, View → Compute resources and jobs, animated preview, counters, asset entry/binding/layout/source inspection |
| Headless boundary | `tools/check_headless.py`: Naga CPU reflection allowed; wgpu/windowing/importers excluded from server |

Native GPU and packaged-executable checks passed locally on Metal. CI runs the workspace and
packaged examples on Metal/Vulkan/DX12; consult the PR checks for the exact revision. The workspace
gate is formatting, strict all-targets Clippy, all workspace tests and the headless audit.
Unavailable GPU adapters fail checks rather than skip them.

Pools/caches have explicit caps; warm tests verify stable allocation counters. This is bounded
engine-resource accounting, not a claim about process/driver memory high-water marks. Device
errors are captured centrally and posted to the simulation inbox; protocol tests exercise
failure propagation without inducing a physical GPU reset.
