# Scene loading and lifetime

Runtime scenes are named entries in the scene's `runtime_scenes` library. They share
its asset catalog; scripts, prefab sources and imported assets are loaded by the host
when the project opens. Normal replacement/additive actions remain synchronous.
Async actions move validation and ECS/component preparation to a worker. Named lazy
sources can instead acquire another scene file or a scene address from a content pack:

```json
"runtime_scene_sources": {
  "annex": {"type": "file", "path": "levels/annex.json"},
  "island": {"type": "content", "catalog": "https://example.com/catalog.json", "address": "levels/island"}
}
```

Paths are relative to the scene file. Catalogs may also be local files; remote
downloads use the content pack HTTPS and immutable cache workflow. Lazy sources use
the same **Load Scene Async** / **Load Scene Additively Async** nodes and script calls.
The synchronous nodes only load embedded scenes.

The host worker reads bounded files, prepares prefab/script/compute dependencies,
decodes assets and validates the combined scene before publication. Entities and the
decoded catalog become visible together. Existing asset IDs keep their meaning;
another file using the same ID for a different asset fails without changing the live
scene. Cancellation or a malformed dependency likewise leaves the old scene running.
Decoded files used by existing levels are shared when their sources match.

## Blueprint and script controls

- **Load Scene Async** / `load_scene_async(name)` prepares a replacement.
- **Load Scene Additively Async** / `add_scene_async(name)` prepares additional objects.
- **Scene Loading Status** returns Loading, Progress (0–1), Handle and Error.
  Scripts read `scene_loading()`, `scene_load_progress()`, `loaded_scene_handle()`
  and `scene_load_error()`.
- **Cancel Scene Loading** / `cancel_scene_load()` prevents publication, including a
  result that has finished preparing. The current scene remains playable.
- **Unload Scene** / `unload_scene(handle)` removes one additive instance. Save its
  handle when loading finishes; the status describes only the latest operation.
  Replacement result handles cannot be unloaded.

There is one worker per runtime. Cancellation is checked between object preparations;
validation and a component initializer finish their current call first. A cancelled
worker must exit and be polled before another request is accepted; Loading remains true
while cancellation finishes. The Rust phase distinguishes Cancelling from Cancelled.
Preparation failures
and stale results appear in Error, so gameplay can display the failure and retry.
The existing synchronous actions retain their normal error behavior.

## Publication and ownership

Workers own an isolated world and never run gameplay hooks. Publication occurs after
the shared script/Blueprint action pass, and is deferred while a debugger holds an
unfinished Blueprint tick. Changing the scene's authored membership or restarting it
invalidates a pending result. Moving live objects, advancing timers and changing runtime
blackboard values do not invalidate it; additive publication preserves those values,
entity handles and existing animation playback.

Each additive instance receives a unique `scene-N` handle and remapped persistent object
IDs. Hierarchy links, graphs, blackboards, prefab links, joints, middleware targets and
player camera references follow those IDs. Existing active views and environment remain;
an added scene supplies any missing views. Duplicate scene blackboard declarations must
have identical defaults. At most 64 additive instances may be loaded at once.

Prefabs spawned from a script or Blueprint inherit the creator's additive lifetime.
Unloading runs destruction hooks, then removes that instance's entities, physics bodies,
compute ownership, timers, object/graph blackboards, particles and middleware state.
Other levels' playback and shared mixer/accessibility preferences remain. Shared scene
blackboard declarations and their current values persist; object references to removed
members become None. Structural references from another level (for example a joint or
parent link) must be removed before unloading their target. Destruction hooks can have
normal gameplay side effects; they are not a rollback transaction.

Checkpoints retain and validate ownership, including spawned prefabs, so a restored
instance can still be unloaded. Pending workers are not checkpointed; saving records
only published scene state. Old checkpoints without ownership remain readable;
their existing objects belong to the base scene. Restart discards all additive instances.

Restoring a checkpoint in a fresh runtime prepares its saved asset, prefab, script and
compute catalog before replacing any entities. Script/Blueprint **Load Game** uses the
worker automatically when dependencies have not been loaded yet, with the same status
and Cancel controls. Existing prepared catalogs restore synchronously. Global asset IDs
cannot change their source; older checkpoints may reuse a catalog that has since grown.
The save keeps its resolved file paths and immutable content generation, so the matching
project files/cache generation must remain available. It does not substitute the latest
download for the saved version. Missing or invalid dependencies leave the live scene intact.

## Editor preparation

Opening a scene already prepares its file-backed assets in a worker. The Play button
also prepares prefab/script catalogs, the runtime world and decoded assets in a worker,
using the editor's loading progress and Cancel control. The authored revision must still
match when Play is accepted. Cancellation or edits made during preparation leave the
editor in Edit mode; Stop restores the original authoring assets and document.

The file picker lists folders in a separate worker and caches the sorted result until
navigation or Refresh. Only visible rows are drawn. A slow or unavailable folder leaves
the path field and Cancel usable; enter the complete scene path to open it without
waiting for the listing. One browser worker is retained until its current filesystem
call returns, including after cancellation, so repeated dialogs do not accumulate
blocked threads.

The Blueprint pane's **Runtime scenes** menu is available without selecting an object or adding a graph. It can link a file or content address and
remove a link, with Undo/Redo. Linking does not acquire the input. Save As rebases
local references without requiring future scene files to exist. Export/content cooking
walks local lazy scene references, including cycles, and packages their dependencies.
Local content addresses are cooked into scene files; remote catalogs remain downloadable
references. A release supports at most 64 transitive scene files.

## Editing multiple scenes

**File → Open scene additively…** keeps the current document and opens another in
the shared viewport. The **Open scenes** list above the hierarchy selects the active
document and toggles each scene's visibility. Clicking geometry in the viewport
activates its owning scene. The hierarchy, Inspector, Undo/Redo, Save, Save As and
Play operate on that active document. **Close active scene** leaves the others open;
closing a dirty document or quitting prompts for each affected scene separately.

Documents retain independent object and asset IDs, selections, edits and history.
The temporary viewport maps their IDs, shares decoded mesh/image data and reuses its
world until a document or asset changes. Imported surfaces remain selectable and
editable in their own scene. Asset refresh visits all open documents through one
retained worker; switching scenes cannot publish an old catalog into the new scene.
Opening a file twice activates its existing document. Save As cannot overwrite a
different open document, including a symbolic-link alias.

The active scene supplies environment/display settings. A camera-free chunk borrows
an inspection camera from another open document; hiding that document keeps its
camera available. The combined view has the engine's normal object/light limits and
does not use a GI bake made for only one scene. Up to 16 documents can be open.
Play runs the active scene, including its authored runtime scene links. The shared
editing view itself is not a new runtime scene or a merged save file.

To try it, open `examples/demo/scenes/scene-loading-lab.json`, then additively open
`examples/demo/scenes/streaming/annex.json`. All three annex pillars appear beside
the base cube. Select a pillar to edit the annex, toggle visibility, and switch scenes
to test separate edits and Undo. Save each scene to preserve its own changes.

## Rust host API

`SceneInstance::begin_scene_load`, `scene_load_status`, `cancel_scene_load` and
`poll_scene_load` expose the same operation. The shared Blueprint step polls
automatically; scripts also poll after their action pass. A custom host must poll
at its completed action/tick boundary. `loaded_scenes()` lists every current additive instance and its members.

For custom scheduling, `prepare_scene_load(name, additive)` produces a snapshot plan;
`start()` returns a background `bozzard_app::job::Job<PreparedScene>`. Poll the job,
then pass its result to `accept_scene_load` while retaining the job until acceptance
returns. Dropping or cancelling the job invalidates an unpublished result. The blocking
`prepare(&Progress)` method supports a caller's own worker. `spawn_prefab_for` assigns
a creator's lifetime; direct host `spawn_prefab` creates a persistent base-scene object.

File/content hosts install `bozzard_project::streaming::install` with their scene path
and initial decoded asset store. Editor Play and the native player do this automatically.
Custom hosts adopt changed `SceneAssets::generation` values before extracting their
next graphics/audio frame. The scene core accepts a `SceneLoaderHandle` for other
acquisition backends; it remains independent of importers and network libraries.
Prepared host resources follow the same cancellation and stale-scene guards as entities.

`begin_game_load(world, json)` prepares a checkpoint asynchronously; `prepare_game_load`
exposes its plan to custom schedulers. A host loader receives a plan whose
`checkpoint_document()` contains the saved scene with paths already relative to its
root. Prepare and bind those dependencies without replacing the saved document with a
newer scene file. `load_game_json` remains synchronous and rejects missing dependencies.

GPU residency still follows the extracted frame and its budget. Asset catalog entries
and compiled source caches are shared for the whole project; unloading one level does
not evict files used by another level.

## Verification

`cargo test -p bozzard-scene --test scene_loading` exercises publication, cancellation,
stale results, remapping, owned spawns, save/restore and both authoring paths. Editor
loading tests cover background Play, real script execution, cancellation and stale
publication. Native Open → Play → Start game → Stop has also passed with the script
target-range scene. A folder read that previously froze the UI remained pending while
the updated dialog accepted a scene path and opened it successfully.

Runtime file/content tests cover atomic publication, script startup, invalid dependencies,
cancelled/stale work, HTTP acquisition/cancellation, owned unload/checkpoints, fresh-runtime
restoration and relocated packs with lazy reference cycles.

Run the loading example with:

```sh
cargo run -p bozzard-player -- --scene examples/demo/scenes/scene-loading-lab.json
```

The annex loads automatically. **U** unloads it, **L** loads it again, and **C** cancels
an in-progress request; the center cube continues spinning. The same scene runs in editor
Play. `examples/demo/scene-loading-content.json` builds a portable version with the annex
and its dependencies included. Short key taps are retained until a simulation tick for
both scripts and Blueprints; held-state queries still report the actual released state.

Native editor/player runs verified file loading, quick U/L unload/reload taps, background
HTTP loading, C cancellation and a successful retry while the base cube kept animating.
Simultaneous multi-scene editor authoring remains part of section 5 completion work.
