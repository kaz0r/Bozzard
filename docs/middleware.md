# Middleware authoring

Middleware is scene content controlled by typed Blueprints. The node value types remain `Exec`, `Text`, `Number`, `Bool`, `Vector` and `Object`. Components own the richer data—clips, curves, navigation meshes, atlas frames and translations—and Blueprint nodes control it by object reference. Save/load game state includes middleware playback, navigation, UI overrides and accessibility preferences.

## Try the reference scenes

```sh
cargo run -p bozzard-editor-app -- --scene examples/demo/scenes/middleware-lab.json
cargo run -p bozzard-editor-app -- --scene examples/demo/scenes/ui-2d-lab.json
```

In the editor, press **Play**, then use the scene viewport. The 2D lab needs the **2D** view and **Enter** to start. To run without editor panels:

```sh
cargo run -p bozzard-player -- --scene examples/demo/scenes/middleware-lab.json
cargo run -p bozzard-player -- --scene examples/demo/scenes/ui-2d-lab.json --view 2d
```

The 3D lab has a skinned banner, an animation-event chime, a separate root-motion clip, a cube with translation/rotation/color/roughness curves, a patrol agent, smoke and sparks with lifetime curves, and a cinematic camera-cut timeline. The side panel's sliders and buttons run ordinary Blueprint graphs. Change the blend, SFX volume, streamed MP3 playback, text size, language or contrast. Scroll the panel or use **Tab / Shift-Tab** to reach controls; **Enter / Space** activates and arrow keys adjust a slider.

The native player keeps the mouse pointer visible and free while enabled UI controls or scroll areas are visible. Decorative HUD labels do not change mouse capture. A **Lock Cursor** Blueprint node can explicitly enable mouse-look during gameplay; **Unlock Cursor** releases it. Game Flow start, pause and game-over menus always release the pointer.

The 2D lab contains a four-frame courier atlas, solid tilemaps, a nine-slice panel, English/Swedish strings and editable start/pause/retry menus. **Escape** pauses; **Enter** resumes. Select the corresponding objects after stopping Play to inspect their components, clip events, layouts and Blueprints. Changes are undoable; Play uses a separate world.

The sample assets are original test content: a two-joint glTF, a one-second chime in four formats, and small procedural pixel images. Rebuild scenes with `cargo run -p bozzard-editor --example middleware_scenes`; rebuild images with `python3 tools/generate_middleware_art.py`.

## Audio

Import WAV/PCM, OGG/Vorbis, MP3 or FLAC. Drag the asset into the scene to create an **Audio Source**, or assign it to an existing source. Imported duration is baked into the scene for headless completion events; loading, saving and exporting refresh it from the catalog. Explicit **Reload** also checks files whose timestamps were preserved by another tool.

A source has volume, pitch, pan, looping, streaming, bus, autoplay and pause-with-game settings. Spatial sources attenuate between their minimum and maximum distance, using rolloff, and pan relative to the active camera or an enabled **Audio Listener**. Put **Audio Mixer** on one object to set master gain and the `Sfx`, `Music`, `Ui` and `Ambience` buses. Non-spatial UI/music sources can continue during game pause.

Blueprints provide **Play / Pause / Stop / Seek Audio**, source volume/pitch/pan, bus volume, playback queries and **On Audio Finished**. Completion and transport run without a sound device. Audio-finished chains and their Delay nodes can run while gameplay is paused, as can UI chains; ordinary gameplay timers stay frozen.

Native output uses Kira/CPAL with short parameter ramps. Device creation is lazy; a silent scene never needs an audio device. Short decoded effects share a bounded 64 MiB cache, with an 8 MiB per-clip limit; larger sounds stream even if the source requested caching. Streaming keeps compressed music file-backed. There are at most 256 authored sources. A missing device or decode failure is reported by the host.

## Animation and cinematics

Import a glTF/GLB with skins or animation. Animated models remain whole objects with an **Animator**, preserving joint weights and the imported PBR materials. **Cook model rig** imports its skeleton/clips; **Recook rig and reset controller** intentionally replaces controller states/events and supports Undo.

The Animator inspector authors named states, clip motions or one-dimensional blend trees, float parameters, ordered transitions, threshold/exit-time conditions, fade duration, clip events and root motion. Blend samples interpolate neighboring thresholds; events come from the dominant clip to avoid duplicate markers. Root motion extracts selected translation axes and yaw; a navigation agent and root motion cannot both own the object's movement.

Blueprints can play a state with a fade, pause/stop/seek, set parameters, query the state/progress, and receive **On Animation Event** with the marker name and time. Event context survives Delay and save/load. Animation sampling uses the same cooked rig on the server; native compute skinning feeds PBR, depth, shadow, culling and motion-vector paths. Paused poses reuse their cached palette, and deformation invalidates shadow caches.

Supported glTF tracks are translation, rotation and scale with step, linear and cubic-spline interpolation. Rotations use normalized quaternion interpolation. Four joint influences per vertex, 1,024 nodes, 4,096 skin bindings, 256 clips and one million keys bound import/runtime work. Morph targets and additional influence sets are rejected with an import error; convert those assets to the supported representation before import.

**Timeline** combines motion tracks, named markers and camera cuts. Its inspector edits target objects, clip duration, repeat/easing, keys and cut cameras. **Play / Pause / Stop / Seek Timeline** controls it; **On Timeline Event** receives marker names. Camera cuts select existing scene cameras. Stopping releases the override, and a removed cut camera falls back to the view's default camera.

The **State graph** shows states and numbered directed transitions. Click a source and destination
state (or the wildcard source) to create a connection; click a numbered badge to select it for
editing in the transition form. A yellow border marks the initial state and the active Play state
is highlighted. Dragging a node changes only its saved editor layout; transition priority remains
the order shown in the form. Rename and delete update or remove affected references through the
same validated authoring command and Undo path.

## Tweens and curves

Add **Tween**, enable autoplay or call **Play Tween**, and add tracks for translation, rotation in degrees, scale, color, metallic, roughness, light intensity or text opacity. Each track targets Self or another scene object. The inspector edits and previews step, linear or cubic Hermite curves, including tangents. Shared easing and Once / Loop / Ping-pong playback apply to the clip.

Blueprints expose play/pause/stop/seek, progress, scalar curve sampling and completion. Tracks have at most 4,096 keys and a tween has at most 128 tracks. Marker catch-up is bounded; an excessively large timestep/event density reports an error instead of producing an unbounded queue. Objects driven by middleware motion are excluded from static GI baking.

## Widgets, 2D and accessibility

Create a root **UI Canvas** and parent **UI Widget** objects beneath it. Widgets are panels, labels, images, buttons, toggles or sliders. Anchors/pivot/offset/size position a widget relative to its parent. Absolute, row, column and grid layouts support padding, gaps and growing children. Canvas scaling uses reference pixels (Fit, Width, Height or Pixels); a phase can restrict a canvas to Ready, Playing, Paused or Game over.

In the editor's 2D viewport, the canvas preview menu offers Fit, 16:9, 16:10, narrow and custom
sizes. These dimensions stay in the workspace and do not change the scene. Visible widget outlines,
an anchor/pivot guide and a selected resize handle use the runtime's resolved layout, including
clipping and canvas scale. Drag a widget to move it or its handle to resize it; Escape cancels and
each finished drag is one Undo step. A row, column or grid parent owns its children's placement,
so the Inspector explains which parent settings to edit instead.

Wrapped text has an intrinsic height measured by the same CPU shaping code used by rendering. Scrollable panels clip their children, draw a scroll thumb, accept wheel/Page keys and reveal controls when keyboard focus moves. Images support normalized atlas UV regions and nine-slice borders in source pixels. The editor previews a selected phase-specific canvas without changing the saved game phase.

Button/toggle/slider interaction emits **On UI Event** on that widget, with `Name` and numeric `Value`. Other nodes set text/value/visibility/enabled state, move focus, change language/text scale/contrast/reduced-motion preference, and start/pause/resume/restart/quit the game. Reduced motion is an exposed preference: authors can branch on **UI Reduced Motion** to select calmer gameplay or cinematic effects.

**Localization** contains one scene's language/key tables, with fallback language and literal-text fallback. Text scale supports 1×–3×. High contrast, accessible names, descriptions and keyboard shortcuts are authorable. Native player and editor publish widget roles, names, bounds, state and actions through AccessKit; controls use the same layout for rendering, pointer hits and accessibility. The accessibility tree is built only when requested by the platform. Focus, activation, value changes and scrolling feed the same Blueprint input path.

Game Flow scenes without canvases gain ordinary editable menu objects when opened or run. Once a canvas exists, the authored hierarchy owns the menus. Save the scene to retain and customize generated menus. See [Game flow](game-flow.md).

**Sprite** uses a regular grid atlas, tint, pivot, flips and frame clips with FPS/repeat/named frame events. **Play / Pause / Stop Sprite**, **Set Sprite Frame** and **On Sprite Event** expose playback. Atlas frames count from zero, left-to-right then top-to-bottom. Use transparent gutters around sprite art to avoid filtering neighboring cells; atlas cells must share dimensions.

**Tilemap** uses the same atlas, with at most 256×256 cells. Zero is empty; other cell values are atlas frame + 1. Paint in the inspector, mark tile IDs solid, or use **Get / Set Tile** in Blueprints. Adjacent solid tiles merge into compound collision boxes used by physics and queries. A tilemap's visible geometry is one shared batch; editing a tile invalidates the batch and its collision shape. Tilemaps and sprites lie in local XY; transforms orient them in the scene.

## Navigation

Add **Navigation Surface**, set bake bounds, cell size, radius, height, climb and slope, then **Bake**. Baking runs in a cancellable worker and reuses scene collision geometry. A stale indicator detects changed settings/static collision; rebake after edits. The inspector shows the walkable triangles and the bake result remains portable scene data.

The surface is a conservative heightfield mesh: each walkable cell becomes two triangles with radius/height clearance, slope filtering and climb-constrained neighbors. Separate bounded surfaces model stacked floors. Each surface has at most 16,384 cells. This ground-navigation implementation does not provide flying agents or off-mesh links.

Add **Navigation Agent** to the root at the agent's feet; place its visible mesh below that object as a child. Select a baked surface. Set speed/acceleration, stopping distance, separation, perception target, sight range/FOV and repath interval. Do not combine it with a rigidbody, Player Controller, root motion, or another agent ancestor.

The inspector authors states with Idle, Move to, Follow, Flee or Patrol behaviors. Ordered transitions respond to target seen/lost, arrived, blocked or time spent in state. Perception uses collision line-of-sight; movement uses deterministic A*, collision checks and nearby-agent separation. Repaths are limited to eight per tick with round-robin fairness, across at most 256 agents.

Blueprint nodes set a destination/target/state, stop navigation, query state/distance/velocity/vision and receive **On Navigation Event** (`Name`, `Other`, `Distance`). Paths and behavior state are saved and restored. Bake a new surface when changing the geometry; pathfinding does not silently rebake during Play.

## Particles

**Particle Emitter** is directly authorable in the inspector, including smoke/spark/dust presets and emission, lifetime, shape, forces, color, size and lighting parameters. Add **Particle Curves** for size, opacity, RGB and speed multipliers over normalized lifetime 0–1. Each channel supports up to 64 keys. New particles snapshot the authored curves; existing particles finish with their original settings. Editor effects preview runs independently from gameplay and uses the same native renderer.

Native player/editor use persistent GPU motion buffers for gravity, drag, wind and turbulence. The CPU schedules births/deaths and evaluates lifetime multipliers; the headless backend also integrates motion, providing a deterministic reference for tests. The renderer sorts particles on the GPU and interleaves their indirect draw batches with transparent meshes/sprites by depth. Opaque depth drives soft intersections. As with ordinary sorted transparency, intersecting transparent triangles within one mesh are not an order-independent transparency solution.

Storage is bounded to 16,384 live particles and 16,384 transparent surfaces in the mixed pass. Buffers, bind groups, slots and sort-stage uniforms are reused; unchanged paused frames skip simulation/sorting. Scenes without particles retain their original single-pass render path and allocate no particle backend. GPU motion is decorative and floating-point, not a cross-platform deterministic gameplay collision solver.

## Performance review

The implementation shares immutable rigs, poses, sprite batches and curve keys. Unchanged inspector passes avoid deep JSON/rig copies; navigation previews and font metrics are cached. Audio metadata probing/import copies stream through bounded buffers, decoded effects share a bounded cache, and music remains file-backed. Navigation uses nearby-agent buckets and a per-tick repath budget. GPU skinning reuses validated shared palettes; particle buffers and sort parameters persist, and paused particles skip dispatch and descriptor upload. The native editor smoke also checks that six idle frames produce no scene draws.

The reproducible CPU benchmark compares the same simulation with headless particle motion and native GPU-motion scheduling:

```sh
cargo run --release --locked --offline -p bozzard-editor --example middleware_benchmark
```

It warms 360 ticks, reports the median of 240 fixed ticks, and asserts that the stress scene fills the 16,384-particle budget. This measures simulation CPU work only; render extraction, GPU execution, readback and audio-device work are excluded. It does not measure an end-to-end frame-rate improvement.

Local measurements on Apple M2 Pro (September 15, 2026), with builds/tests stopped: median of three release process runs; parentheses show the range of per-run medians.

| Simulation workload | CPU reference, µs/tick | GPU-motion scheduling CPU, µs/tick |
| --- | ---: | ---: |
| Authored lab, 121 particles | 28.2 (28.2–28.8) | 24.1 (23.8–24.7) |
| Stress lab, 16,384 particles | 1,027.5 (1,023.1–1,045.0) | 55.1 (54.9–55.5) |

## Verification

```sh
cargo test -p bozzard-scene --tests
cargo test -p bozzard-audio --test mixing
cargo test -p bozzard-editor --test middleware
cargo test -p bozzard-project --test export
cargo test -p bozzard-render-assets --test skinning --test particles --test widgets
cargo run -p bozzard-editor --example middleware_capture
python3 tools/check_headless.py
```

The last two render commands require a native GPU/software adapter. The capture example writes both scene images to the OS temporary directory. Audio sample tests use an injected backend, so they can verify gain, pan, pause, streaming and cache sharing without speakers. The ignored `native_device_opens_and_accepts_static_and_streamed_playback` test explicitly exercises a real audio device. Full CI runs formatting, Clippy, workspace tests, headless dependency checks, native packaging and exported game-loop tests on Metal, Vulkan and DX12.
