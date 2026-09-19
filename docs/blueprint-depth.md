# Blueprint authoring depth

All features below use the same validated interpreter in editor Play, player and headless server. The six pin types remain Exec/Text/Number/Bool/Vector/Object; lists are stored in blackboards and expose scalar element pins.

## Shared state and lists

Open **Blueprint → Blackboards** to declare values in **Graph**, **Object**, or **Scene** scope. Graph defaults are private to an attachment. Object defaults are shared by every attachment on that owner; Scene defaults are shared across all owners. Select a variable/list node to choose its scope and declaration. The declaration determines the pin type. Type changes that leave incompatible wires create a repairable draft.

Legacy `variables: {"score": 0}` graphs retain their private number semantics. New declarations use `blackboard` on a graph, object, or scene:

```json
{
  "score": {"scalar": {"number": 0}},
  "inventory": {"list": {"element": "object", "capacity": 32, "values": []}}
}
```

There are at most 64 declarations per board, and 1–1024 elements per list. Lists are homogeneous and cannot nest. **List Push**, **List Set**, **List Remove**, **List Clear**, **List Get**, and **List Length** implement growth and indexing. Indices are zero-based integers; out-of-range indices and capacity overflow report an error before changing that list. Guard optional accesses with List Length and Branch. Text still has a 4096-byte limit. Object references remain persistent IDs; guard references to destroyed objects with Is Valid Object. Duplicating/capturing prefab objects remaps Object values in board defaults as well as graph constants.

The Target Range player now has **look, move and jump** (50 nodes), **shoot and recoil** (16), and **weapon selection** (33), sharing seven Object numbers. Its existing gameplay regression tests cover the split from the 97-node attachment.

## Events and timers

**On Enable** fires on the first enabled tick and each subsequent disabled→enabled transition. **On Start** fires only once during an attachment's lifetime. **On Disable** fires on an enabled→disabled transition and cancels its pending delays; enabling again preserves its variables. **Set Graph Enabled** takes a Target, an attachment index (zero-based), and Enabled. Flag changes are observed by event dispatch on the next tick. Inspector checkboxes author the initial flags.

**On Destroy** runs while the owner's components still exist, before a prefab removal or scene unload. It can read the owner's last position and write shared state. New prefab graphs normally start next tick, but a prefab destroyed before then still receives On Destroy. Active cameras remain protected from Destroy Prefab.

**On Collision Enter** is distinct from sensor overlap. It emits once per new solid collider contact, ordered by other-object ID, with **Other**, **Normal** (world-space, pointing toward the owner), and **Impulse** (summed normal solver impulse). Rapier supplies physical contact data, including sleeping contacts. Geometric static/kinematic pairs have zero solver impulse. Trigger-volume-only contacts use the existing overlap events instead. Contact data is snapshotted before graph actions.

**Delay / After** schedules its Then output once after the requested finite, nonnegative seconds. Each invocation has its own timer, bounded to 256 pending timers per attachment. Zero seconds resumes next tick. Due timers resume before the tick's ordinary events, in insertion order; they retain the initiating event's Other/Normal/Impulse context. Data reads and Delta Seconds use the resumed tick's current state. Disabling cancels timers; pause does not advance them; destruction discards them; checkpoints preserve them. Cycles remain invalid: use events and variables for repetition.

## Math and queries

New math includes Lerp, Min/Max, Abs, Length, Normalize, Dot, Cross, Distance, Modulo, Power, Random, Cosine, Tangent, Arc Sine, Arc Cosine, Atan2, degrees/radians conversion, Floor/Ceil/Round/Sqrt, and Lerp Vectors. Lerp extrapolates outside 0–1; Normalize of zero is zero; Modulo uses Euclidean remainder; angles are radians unless labeled otherwise. Invalid domains, division/modulo by zero, and non-finite results report errors before assignment. Random is an execution node with Min/Max inputs and a latest Value output. Its deterministic per-attachment sequence is initialized from owner ID and attachment index and continues through save/load.

**Raycast** takes world Origin/Direction/Distance and an optional Ignore object. Direction need not be normalized. It returns Hit, Object, Position, Normal and Distance. Miss outputs are false/None/zero. Rays starting inside a box return distance zero. Ties use object-ID order.

**Sphere Overlap** and **Box Overlap** write a selected Object list and return Count. Box Size means full world-axis dimensions. Results replace the list in object-ID order; overflow fails without replacing it. These query enabled solid colliders, not render meshes or trigger-only volumes. Mesh tests use actual triangles, preserving holes; dynamic meshes use their existing cached convex hull.

**Line of Sight** tests the closed world-space segment From→To; an occluder at the endpoint blocks it. A zero-length segment is visible. All queries accept an Ignore Object; use Self to exclude the owner. Results are retained per action node until it executes again.

The editor picker and runtime share triangle intersection, conservative ray bounds, and the near-first BVH traversal. One million geometry/primitive tests per blueprint tick bounds repeated query work. A query snapshot is built lazily and reused across graphs until a transform/membership action invalidates it.

## Scenes and checkpoints

**Runtime scenes → Import scene into library** embeds a named `.json` scene, rebases its assets and assigns unique asset IDs. The top-level `runtime_scenes` map holds up to 64 named scenes (100,000 template objects combined); libraries cannot nest. Scenes share a preloaded asset catalog. Normal Save As and exported games relocate embedded scene asset paths too.

**Load Scene** replaces the running scene; **Load Scene Additively** appends objects under fresh persistent IDs and remaps their references and prefab membership. Additive loading retains existing active views/environment and fills missing views; duplicate Scene declarations must have identical defaults. **Restart Scene** restores the current replacement scene's initial state, discarding additive objects, runtime spawns and variable changes. These operations apply at tick boundaries and validate a candidate before changing live membership. Replacement levels should supply the view used by the host.

**Load Scene Async** and **Load Scene Additively Async** prepare a scene in a worker while
simulation continues. **Scene Loading Status** provides Loading, Progress (0–1), Handle
and Error outputs. **Cancel Scene Loading** prevents publication; **Unload Scene** accepts
an additive instance's handle. See [scene loading and lifetime](scene-loading.md).

**Save Game State** and **Load Game State** accept a slot of 1–64 letters, digits, underscores or hyphens. The slot is never a path. Hosts using `SceneDemo::new_with_prefabs` persist slots under `BOZZARD_SAVE_DIR`, or the user's data directory under `bozzard/saves/<scene-name hash>`. Explicit save I/O is synchronous at the tick boundary and uses a synced temporary file followed by rename. Checkpoints are limited to 64 MiB; headless `SceneDemo::new` can use bounded in-memory slots or configure `GameSaves::in_directory`.

Checkpoints include additive-instance ownership, scene membership/transforms/components, private and shared variables/lists, started/enabled/overlap state, pending timers and their event contexts, random sequences, latest query/spawn outputs, visibility, cursor mode, game-flow state, gravity and rigidbody linear/angular velocities, sleeping state, and display overrides. The scene catalog must match. Invalid saves are checked before replacement. Physics reconstructs contacts on its next step; solver warm-start caches and visual particle history are not checkpointed. This is a gameplay checkpoint, not a bit-identical physics replay format. `save_game_json` / `load_game_json` expose the same format without disk I/O. Editor Stop restores the untouched authored document; ordinary editor Save still saves authoring state.

## Authoring UX

**Comment** adds a wrapped annotation card; any selected node can carry an annotation. **Reroute** supports all six pin types, including execution. **Find in graph** searches node titles, IDs, variable names and comments and frames the chosen result. Shift-click headers to toggle selection, drag a selected header to move the group, or use Select all. Copy/Paste buttons and platform clipboard events preserve selected nodes, their internal wires and required Graph defaults, assigning fresh IDs on paste. External wires are omitted; conflicting local declarations fail atomically.

When validation fails, the canvas keeps an unapplied draft and lists stale source/destination pins and before/after types. **Remove stale wires** removes only those wires; **Discard draft** restores the accepted graph. Failed file imports include the same wire diagnostics. Scene history records only accepted edits, and Play uses the accepted scene.

## Verification and optimization

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
python3 tools/check_headless.py
cargo run --release -p bozzard-demo --example benchmark_blueprints --locked
```

The added tests cover typed scopes/lists and invalid domains, timers and event context, contact normals/impulses, destruction, persistent checkpoints, random continuation, scene replacement/additive remapping/restart, query holes/rotations/ties/capacity, editor draft rendering/copy-paste/history/Play isolation, and exported scene catalogs after source removal.

The optimization pass replaces repeated graph/node/wire searches with one compiled index per attachment, shares embedded scene templates with `Arc`, caches query geometry until mutations, shares the picking BVH implementation, and precomputes which objects need hierarchical transform validation. Runtime counters assert that repeated queries reuse geometry and that graphs compile only once. The benchmark runs 2,048 graph attachments and 614,400 actions after warmup; it excludes scene loading and GPU work.

Local release measurement (three alternating runs, Apple M2 Pro): the unchanged base `a073f87` had a median **3.324 ms/tick**; this implementation had **1.921 ms/tick**, a **42.2% reduction** for that workload. This measures interpreter and root-transform action overhead, not overall game or rendering performance.
