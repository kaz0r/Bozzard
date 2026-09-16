# Profiling, console and Blueprint debugging

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
reported by editor operations. Source navigation is available for live runtime objects, including spawned instances, and for
authored objects outside Play. Removed objects keep their recorded source.
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

## Blueprint debugger

Open the **Blueprint** tab and choose an object from **Blueprint object**. This picker lists every
object with a graph; during Play it becomes **Runtime object** and includes spawned instances.
Choose an attachment beside **Blueprint Editor**. Select an event or action header, then click
**Add breakpoint** (or **F9**). This enables **Debug Blueprints** automatically. A red dot marks
each breakpoint. Pure data nodes have no execution step; select them to inspect their values.

Press **Play**. Breakpoints stop **before** execution and bring the stopped node into view with
a yellow outline. The scene bar offers:

- **Pause / Continue**: suspend or resume simulation. Paused wall-clock time is discarded.
- **Step node** (**F10**): execute the stopped event/action, then stop before the next one. If
  no next node exists, finish the tick and remain paused. This includes event nodes.
- **Step tick** (**Shift+F10**): complete one fixed tick, ignoring breakpoints until its boundary.
- **Stop**: discard runtime state and return to authoring. Runtime failures preserve their node
  and values for inspection; Stop and fix the graph before running again.

The **Inspector** has three tabs:

- **Values** shows selected-node pins, event context (including collision normal/impulse), graph,
  object and scene variables, bounded list previews and pending timers. Values refresh at most
  five times a second while running and immediately after stepping. Event inputs remain available
  at tick boundaries from the last dispatch. **Previous output** means
  the last result stored by an action, not the result of executing it now.
- **Trace** retains the last 256 event/action entries, newest first. Select an entry to navigate
  to its source and inspect its **recorded values before execution**. **Return to live values**
  restores the live inspector. Clear, recording and JSON export controls are independent of the
  profiler. Export includes the currently paused node and any execution error, even when tracing
  is off. Green outlines show recent execution. Trace entries keep their scene identity.
- **Breakpoints** lists your saved breakpoints for this scene file. Entries also identify the
  runtime scene, object, attachment, graph name, node ID and node kind. Changed or missing graphs
  are marked unavailable instead of silently targeting a different node. Remove and set them
  again after renaming/reordering graphs; a saved spawned-instance breakpoint arms when that
  instance appears. Breakpoints live in workspace preferences, not authored scenes or saves.

For a short walkthrough, open `examples/demo/scenes/debug-lab.json`, select **Hero Cube →
Welcome to the console**, and set a breakpoint on **Log Info**. Play stops before the welcome log;
Step node emits it exactly once. Try the Values and Trace tabs, then Continue. You can pause
from the scene bar even when you have not set a breakpoint.

Simulation, physics, animation, timers and audio pause together. Resuming continues the same
execution queue; earlier systems do not run twice, and queued ECS commands flush only when the
tick completes. UI event callbacks can also suspend and resume without advancing a fixed tick.
Saving/replacing game state through the headless API is rejected while a Blueprint dispatch is
suspended, preventing an incomplete checkpoint; use Step tick to reach a boundary first.

Blueprint-requested prefab destruction supports stepping through **On Destroy** before removal.
Scene replacement and direct host/script teardown remain atomic operations. Their Blueprint
callbacks are recorded with an **Atomic teardown** label; a breakpoint there records its pins
and emits an explanatory console warning rather than stopping halfway through scene removal.
Script execution itself is outside this Blueprint debugger.

Disable **Debug Blueprints** to use the normal interpreter without continuations or pin snapshots.
With the debugger enabled, uncheck **Record execution** to reduce overhead while retaining
breakpoints. Trace rows are virtualized, pin/text previews are limited to 512 UTF-8 bytes plus
an ellipsis, and list previews contain at most eight items. Export streams atomically to JSON.
The trace and debugger state never enter authored scenes or game checkpoints.

The same controls work without a window. For the Debug Lab scene, a host can arm its welcome
log before the first tick:

```rust
use bozzard_scene::{BlueprintDebugger, Breakpoint, DebugCommand};
let breakpoint = Breakpoint {
    scene: "Debug Lab".into(), object: "hero-cube".into(), attachment: 2, node: 2,
};
demo.app.world.insert_resource(BlueprintDebugger::new([breakpoint]));
demo.app.step();
assert!(demo.app.is_paused());
demo.debug_command(DebugCommand::StepNode)?;
let values = demo.instance().inspect_blueprint(&demo.app.world, "hero-cube", 2, Some(2));
```

Custom hosts that dispatch UI callbacks outside fixed ticks should call
`SceneDemo::resume_debug_dispatch()` before their next `App::advance()`; the editor does this.

Reproduce the debugger benchmark with:

```sh
cargo run --release -p bozzard-demo --example benchmark_blueprint_debugger
```

On the development Apple M2 Pro, 2,048 continuously active graphs measured 1.93–1.98 ms/tick
with debugging disabled, comparable to the pre-debugger baseline of 1.89 ms/tick. Breakpoints
without tracing measured 2.10–2.12 ms/tick; recording 4,096 event/action snapshots per tick
measured 5.24–5.50 ms/tick, retaining only the newest 256. The mostly event-driven middleware
lab measured about 26 µs/tick in all three modes. These are warmed headless measurements;
trace overhead depends on the number and contents of executed nodes.
