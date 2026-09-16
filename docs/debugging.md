# Profiling and the debug console

Open **Debug** in the editor toolbar, or **View → Debug · Profiler and Console**.
The bottom workspace has two tabs. Recording is off until you press **Record**; the console
collects messages independently. Hiding the panel does not stop a capture—the toolbar says
**Debug · Recording** while it is active.

## Try it

```sh
cargo run -p bozzard-editor-app -- --scene examples/demo/scenes/debug-lab.json
```

1. Open **Debug → Profiler**, press **Record**, then **Play**.
2. Click any chart bar to freeze the capture and inspect that frame. The game keeps running.
   **Pause capture** also stops recording; **Record** resumes following the newest frame.
3. Expand CPU simulation, viewport rendering, GPU passes or graphics memory. Drag the panel's
   top edge to resize it; scroll the details to reach lower sections.
4. Open **Console**. The welcome message comes from Hero Cube's third graph. Select it and
   press **Go to hero-cube** to open that graph with its Log Info node selected.
5. With the mouse over the game viewport, **L** logs a message, **W** emits a sample warning,
   and **E** deliberately attempts Set Text on a mesh without Text Rendering. That failure
   identifies the object, attachment and node. Stop and Play again to reset the demonstration.
6. Filter by **Info / Warnings / Errors**, a source, or text. Search includes object IDs and
   asset references. Select a row for the complete message and **Copy message**. Selecting
   turns off **Follow newest**, so an active game cannot pull you away from an error.
7. **Export JSON…** saves both the frame history and console, including timestamps and source
   references. **Clear capture** and **Clear console** operate independently.

The scene is an intentionally small debugging example. For physics, animation, navigation,
particles and UI in one capture, open `examples/demo/scenes/middleware-lab.json`.

## What the measurements mean

| Measurement | Scope |
| --- | --- |
| Editor CPU | Work in an editor update: simulation, panels and viewport preparation. Excludes egui GPU rendering and window presentation. |
| Interval | Wall time between updates, including scheduling, waiting and presentation. Not CPU work. |
| CPU simulation | Named ECS systems and nested gameplay stages, including each fixed tick. Parent times include children; do not sum both. |
| Prepare / Encode / Submit | Viewport CPU work. Submit includes finishing the command encoder and submitting it; it does not wait for the GPU. |
| GPU passes | Timestamp differences on the GPU, converted using the device timestamp period. Excludes uploads, egui and presentation. Passes can overlap; the sum is not total GPU frame time. |
| Draws / triangles | Visible mesh and shadow draws/triangles. Excludes fullscreen effects, text and particle draw batches. Particle population/dispatch counts are separate. |
| Graphics memory | Live backend buffer/texture allocations, sampled once a second. Includes editor graphics. Not total process RAM or total VRAM. Not historical per-frame memory. |

A cached viewport is labelled **Viewport reused or not drawn**. New GPU samples carry their
renderer frame ID and only attach to that frame, even when they arrive after capture pauses.
Some adapters do not support timestamp queries. Some drivers advertise support but return zero
or reversed samples; those passes are **Unavailable**, never an invented zero-duration result.
This can occur on macOS 26 ([upstream wgpu report](https://github.com/gfx-rs/wgpu/issues/9414)).
Valid passes in the same frame remain usable. CPU timings work independently.

## Logging from gameplay

Blueprint **Log Info / Log Warning / Log Error** accept Text; **Print Number** remains supported.
All carry the object ID, attachment, node and tick. In Rhai, `print(text)` / `log_info(text)`,
`log_warning(text)` and `log_error(text)` include the originating object. Runtime script failures
also carry their asset and attachment. Logging at Error severity does not itself throw.
Editor failures are captured as they occur, including asset-loading and audio/backend errors
reported by editor operations. Source navigation is available for objects still in the authored
scene; runtime-only or removed objects keep their recorded source but cannot be selected there.
Messages and frames retain their scene path, so opening another scene cannot redirect an old
message to an unrelated object with the same ID.

## Bounds and overhead

The editor retains 240 frames, at most 512 CPU spans and 256 GPU passes per frame, and 2,048
console entries. Consecutive identical messages collapse with a repetition count. Messages are
limited to 4 KiB at UTF-8 boundaries; source references are bounded too. Omitted spans/passes,
discarded messages and skipped GPU captures are reported. Export is streamed through a buffered
writer and atomically replaces its destination, without building a second copy of the capture.

Disabled CPU profiling performs no profiling clock reads or allocations. Enabled captures reuse
CPU span storage. GPU profiling allocates three reusable query/resolve/readback slots only on
first use, then polls asynchronously. If all slots are busy, it skips the sample instead of
waiting. Skinning shares the scene command encoder, avoiding a separate queue submission.
Console rows are virtualized and text matching is cached until messages or filters change.
Diagnostics never enter authored scenes or gameplay saves.

Reproduce the CPU overhead benchmark:

```sh
cargo run --release -p bozzard-demo --example benchmark_diagnostics
```

On the development Apple M2 Pro, two alternating runs of the middleware lab measured median
25.67–26.21 µs/tick with capture off and 26.83–27.17 µs/tick recording 16 spans: approximately
1 µs/tick extra. A 100,000-message flood took 30.2 ms and retained exactly 2,048 entries. These are
machine-dependent simulation-only measurements, not rendering or GUI benchmarks.

Custom Rust systems can use `App::add_named_system` and `bozzard_diagnostics::measure` for readable
nested spans. A headless host enables `Diagnostics.profiler.recording` and calls `begin_frame()`
before each capture interval, then reads `spans` and `console.events` from the resource.
