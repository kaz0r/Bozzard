# Compute shaders from scripts and Rust

Import WGSL as a `compute_shader` asset, allocate reusable resources, and dispatch it from a
gameplay hook or compiled Rust system. The `.rs` gameplay files are **Rhai**, not native Rust;
the GPU executes separate WGSL. Blueprints keep their existing value types.

## Try it

```sh
cargo run -p bozzard-player -- --scene examples/demo/scenes/compute-waves.json
cargo run -p bozzard-player -- --scene examples/demo/scenes/compute-numbers.json
cargo run -p bozzard-editor-app -- --scene examples/demo/scenes/compute-waves.json
```

Waves animates a 513 × 257 texture directly on a material. Numbers runs two ordered buffer
dispatches, asynchronously prints `[7.0, 13.0, 19.0, 25.0, 31.0]`, and turns its cube green.
In the editor press **Play**, then **View → Compute resources and jobs** for live textures,
bounded buffer inspection, memory counters and job errors. Enable recording in **Debug →
Profiler** for CPU encode/submit time and per-kernel GPU timings, where supported.

Import `.wgsl` through the Asset Browser. Its Compute filter and asset details show entry
points, workgroup sizes, binding access, field offsets and array strides. **Open source** opens
the file in your configured application. The source preview is limited to 16 KiB.

## Minimal texture kernel

Declare an asset alongside the controlling script in the scene or prefab catalog:

```json
"waves": { "kind": "compute_shader", "path": "assets/compute/waves.compute.wgsl" }
```

```wgsl
struct Params { time: f32 }
@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var output: texture_storage_2d<rgba8unorm, write>;
@compute @workgroup_size(8, 8)
fn main(@builtin(global_invocation_id) id: vec3u) {
    let size = textureDimensions(output);
    if any(id.xy >= size) { return; }
    let wave = 0.5 + 0.5 * sin(f32(id.x) * 0.05 + params.time);
    textureStore(output, id.xy, vec4f(0.05, wave, 0.8, 1.0));
}
```

```rust
// Rhai gameplay hooks, in a .rs asset.
fn on_start(me) {
    if !compute_available() { return; }
    compute_create_texture("surface", 513, 257, "rgba8unorm");
    compute_bind_material(me, "base_color", compute_texture("surface"));
}
fn on_update(me, dt) {
    if !compute_available() { return; }
    compute_dispatch_extent("waves", "main",
        #{ output: compute_texture("surface") },
        #{ time: elapsed_time() }, [513, 257]);
}
```

`dispatch_extent` rounds up workgroups; **the kernel must guard its final partial group**.
Reflected variable names, not binding numbers, are resource-map keys. The one optional uniform
must be a struct named `params`; its fields form the parameter map. Use `#{}` without parameters.
Each accepted dispatch owns an immutable parameter snapshot, even across catch-up ticks.

## Rhai API

Resource names and named readback jobs persist in dedicated attachment state; top-level Rhai
variables do not persist between hooks. Handles/tickets are opaque, not numbers or blackboard
entries. Operations validate ownership and world identity.

| Function | Result / behavior |
| --- | --- |
| `compute_available()` | Whether an executor is installed |
| `compute_capabilities()` | Map: `available`, `backend`, `max_buffer_bytes`, `max_texture_dimension`, `max_workgroups` |
| `compute_create_buffer(name, asset, entry, binding, elements)` | Handle; reflected storage layout. `elements` is runtime array length; use `1` for a fixed layout. |
| `compute_create_texture(name, width, height, format)` | Handle; `rgba8unorm` or `rgba16float`, one mip, linear color, straight alpha |
| `compute_create_sampler(name, linear)` | Handle; clamp to edge, linear or nearest |
| `compute_buffer(name)`, `compute_texture(name)`, `compute_sampler(name)` | Lookup of expected resource kind |
| `compute_release(handle)` | Immediately invalidates handle; safely retires storage |
| `compute_write(handle, values)` | Full typed buffer write |
| `compute_write_range(handle, first, values)` | Array subrange; indices count elements, not bytes |
| `compute_dispatch(asset, entry, bindings, params, groups)` | Ticket; one to three positive workgroup counts |
| `compute_dispatch_extent(asset, entry, bindings, params, extent)` | Ticket; one to three positive element dimensions |
| `compute_readback(name, handle)` | Ticket for a named asynchronous buffer readback |
| `compute_readback_range(name, handle, first, count)` | Ticket for an array subrange |
| `compute_poll(name_or_ticket)` | `queued`, `submitted`, `complete`, `failed`, `cancelled`; absent name returns `missing` |
| `compute_error(name)` | Failure text, or empty for an existing nonfailed job |
| `compute_take_result(name)` | Completed typed data; consumes job/name; premature calls error |
| `compute_cancel(name_or_ticket)` | Removes queued work or suppresses submitted results; name overload also forgets job |
| `compute_bind_material(object, "base_color", texture)` | Runtime texture override; built-in/imported geometry and shader graph Texture Sample, Base Color slot |

Creation/lookup functions also have `compute_scene_` variants, such as
`compute_scene_create_buffer` and `compute_scene_buffer`. These explicitly share resources
between attachments; other operations take the returned handle. Scene resources survive their
creator's destruction until released or the world ends. Names must be unique in their scope.
Job names remain private to the requesting attachment.

```rust
// Queue once after a dispatch:
compute_readback("answer", compute_buffer("values"));
// In a later on_update, without busy-waiting:
if compute_poll("answer") == "complete" {
    print(compute_take_result("answer"));
}
```

See the complete [Numbers script](../examples/demo/scenes/assets/compute/numbers.rs).
Cancel a failed named job before reusing its name. Completed dispatch receipts can expire as
the job budget fills; readback results remain until taken or cancelled.

## Values and layout

Numeric layouts support 32-bit `f32`, `i32`, `u32`, vectors, matrices, fixed arrays and structs.
Storage also supports a top-level runtime array or final runtime-array struct member. Boolean,
atomic, 16/64-bit, pointer and arbitrary handle fields are rejected. Bindings support
read-only/read-write storage, one `params` uniform, sampled nonmultisampled 2D float textures,
ordinary samplers, and write-only `rgba8unorm`/`rgba16float` storage textures.

Vectors are arrays, matrices are arrays of **columns**, and structs are maps with exactly their
reflected fields. Padding is zeroed. Packing uses WGSL offsets/alignment/strides, never Rust/Rhai
memory layout. Fractional integers, overflow, NaN/infinity, unknown/missing fields and mismatched
lengths fail before submission. Tiny runtime tails must still satisfy reflected minimum binding
size, exposed in the inspector. Partial transfers require a top-level array, not a struct member.

A writable resource cannot alias another binding in the same dispatch. Use two buffers/textures
and swap input/output roles between dispatches for feedback. Dispatches run in order. Material
outputs stay on the GPU; readback supports buffers only. Generated textures use linear color,
straight alpha and transparent ordering, since alpha coverage is unknown without readback.

## Compiled Rust integration

`bozzard_scene::compute` re-exports GPU-free `bozzard-compute`. `SceneInstance::compute()` returns
a guard containing the same `Runtime` used by scripts. Given `instance: &SceneInstance`:

```rust,ignore
use bozzard_scene::compute::{BindingKind, Dispatch, Owner, Scope};
use std::{collections::BTreeMap, sync::Arc};
use serde_json::json;

let kernel = instance.compute_kernels()["numbers"].clone();
let BindingKind::Storage { layout, .. } = &kernel.entry("main")?.binding("values")?.kind
    else { anyhow::bail!("expected a buffer binding") };
let owner = Owner::new("my-system", 0);
let mut state = instance.compute();
let buffer = state.runtime.create_buffer(&owner, Scope::Scene, "numbers",
    Arc::new(layout.clone()), 5)?;
state.runtime.write(&owner, buffer, &json!([1., 2., 3., 4., 5.]))?;
let ticket = state.runtime.dispatch(&owner, Dispatch {
    asset: "numbers", kernel, entry: "main",
    bindings: &BTreeMap::from([("values".into(), buffer)]),
    params: &json!({"scale": 2., "add": 1.}), groups: [1, 1, 1],
})?;
let result = state.runtime.readback(&owner, buffer)?;
// Later: runtime.job(&owner, result), then runtime.take_result(...).
```

Create once, then retain/find the handle. Owner identities are a trusted engine contract: use
a stable object/attachment pair and avoid another system's namespace. Drop the guard before
ticking, rendering, or another API that locks compute state.

Custom hosts install `ComputeBridge` from `bozzard-render-assets`: prepare enabled capabilities
**before** scripts; poll before simulation; submit once after all catch-up ticks; sync generated
views before drawing viewports. Poll/submit also run without a drawable. The player/editor do
this already. Rust-only hosts call `Runtime::begin_tick` (or `SceneInstance::begin_compute_tick`)
at their simulation boundary.

## Lifetime, reload and headless policy

- Commands reserve immediately in call order. Later calls can use a handle created earlier in
  the same hook. Accepted compute commands are not rolled back if a later expression throws.
  Failed encoding fails the batch; backpressure defers it intact. Actual submission consumes it
  once regardless of presentation success or viewport count.
- Callbacks enqueue data; a permitted tick applies it. Paused repaints can finish transfers but
  cannot deliver results or redispatch. Step/resume delivers pending results.
- Disable cancels jobs but retains resources: re-enable does not rerun `on_start`. Destruction
  releases private resources. Stop/restart/load/world/device changes invalidate handles; late
  callbacks cannot change a replacement world. Submitted cancellation cannot interrupt shaders.
  Saves contain authored/CPU state only; recreate GPU visuals after load.
- Editing a kernel swaps compatible bindings before the next tick. Accepted jobs pin old source.
  Syntax, device validation and interface errors retain the working version and appear in the
  asset/runtime inspector. Interface changes require updated scripts and Stop/Play to recreate
  resources. Workgroup changes within device limits are compatible. All dynamically selected
  shaders must be declared: export cooks the catalog and prefab/scene dependencies.
- The server validates assets but reports compute unavailable. Guard optional visual paths with
  `compute_available()`. Required work errors; there is no silent WGSL emulation. Authoritative
  gameplay needs an explicit CPU implementation and determinism tests. A custom Rust host can
  install CPU capabilities and implement `Runtime::submit_with`: process ordered `Command`s,
  call `readback_done` with packed bytes and `submitted_work_done` once per batch. `Deferred`
  means no side effects; `Rejected` fails the batch. This protocol is not a WGSL CPU interpreter.

## Bounds and performance

| Budget | Bound |
| --- | --- |
| Source/catalog | 1 MiB each; 256 kernels / 32 MiB per scene |
| Reflection | 16 entries, 16 bindings/entry, groups 0–3, bindings 0–31; depth 16 / 4096 layout nodes |
| Resources | 256 / 256 MiB reserved per world; buffers at most 64 MiB and enabled device limit |
| Queue | 1024 commands plus reserved releases; 16 MiB upload bytes per batch |
| Jobs/submissions | 256 jobs; 32 CPU-tracked batches; executor permits 3 in flight across worlds |
| Readback | 8 slots, at most 1 MiB each; submitted cancellation holds slot until mapping completes |
| GPU caches | 128 pipelines, 256 binding sets; least recently used eviction |
| Upload staging | 3 reusable buffers, power-of-two growth up to 32 MiB each |
| Rhai data | 65,536 values / 16 nesting levels per conversion |
| Inspector | 1024 array elements/request, 4096 decoded values, 16 KiB text, 64 jobs |

Device limits can be lower. Quota exhaustion errors: stop producing work, take/cancel results,
retry on a later tick. Executor backpressure returns `false` and retains accepted commands.
Optional timestamp queries use bounded pools; missing timings are not zero-duration dispatches.
These budgets do **not** bound shader loop time; Rhai cannot interrupt GPU instructions.

Unused scenes leave runtime/channel/storage uninitialized. Warm workloads reuse GPU pipelines,
bindings, allocations and transfer pools. Parameter snapshots and command metadata still cost
CPU work. Keep visual outputs on the GPU and update only changed ranges. Measure your workload:

```sh
cargo run --release -p bozzard-render --example compute_benchmark
```

The benchmark excludes cold compilation, compares 256 and 262,144 elements, reports optional GPU
timestamps and synchronous measurement roundtrips, and verifies allocation reuse. Waits exist
only in measurement. See [recorded results and verification](compute-validation.md).

## Export and checks

```sh
cargo build --release -p bozzard-player
python3 tools/package.py --project examples/demo/compute-waves.bozzard.json --export-dir dist/compute-waves --verify
python3 tools/package.py --project examples/demo/compute-numbers.bozzard.json --export-dir dist/compute-numbers --verify
cargo test -p bozzard-compute
cargo test -p bozzard-scene --test compute_scripts
cargo test -p bozzard-render --test compute
cargo test -p bozzard-editor --test compute
python3 tools/check_headless.py
```

GPU checks require a supported native device/software adapter and fail rather than skip.
Packaging launches the extracted executable from a renamed folder with an empty cwd/PATH,
verifies animated pixels/numeric results, and writes PPM diagnostics. CI covers Metal/Vulkan/DX12.
Indirect dispatch, mesh generation, arbitrary render passes, subgroup code, Blueprint wrappers
and runtime Rust compilation remain outside this first API.
