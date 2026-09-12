# Mesh Collider

On an object with a Mesh Renderer, choose **Add Component → Mesh Collider**. Imported surface children get their own collider, using only that surface's correctly centered triangles. Cubes, quads, whole imported models and legacy surface transform overrides are supported.

- Existing **Box Collider + Rigidbody** bodies and the Player Controller collide with these triangle surfaces: swept movement, sliding, grounding and jumps use the normal collision path. Meshes participate in box/mesh overlap and Blueprint body enter/exit events. Player spawn checks and follow-camera obstruction also include them.
- Mesh Collider is **static, non-convex and two-sided**. It cannot share an entity with Box Collider, Rigidbody, Player Controller or Trigger. Mesh/mesh collision, convex dynamic bodies, impulses, friction, rotating obstacle sweeps and compound movers are not implemented. “Static” means held fixed during each box movement query; editing/animating its Transform changes its pose, but does not sweep or carry riders.
- These are triangle **surfaces**, not filled volumes. A box entirely inside a closed mesh without touching a triangle is not an overlap. Use box colliders for solid volume tests and Trigger components for sensors.
- Geometry is **baked when added**. It follows the object's Transform, but later renderer, source-file or child-transform edits do not automatically alter it. Use **Rebuild from Mesh Renderer** to recook it. Rebuild keeps Enabled; failures leave the previous collider intact. A stale imported surface must be rebound first.
- Baked triangles are saved in scenes/prefabs and remain usable without the source model or Mesh Renderer. Removing the renderer keeps the collider. Enable/disable, rebuild and component removal support Undo/Redo. Stop restores the authored state; spawned prefab instances have independent Enabled state.
- Toggle collider guides to see mesh bounds; selecting a mesh collider adds triangle wires. Dense wire displays sample at most 2,048 triangles, explicitly labelled. This does **not** reduce collision accuracy.

Geometry and its BVH are immutable and shared across scene snapshots/ECS/prefab clones. The BVH builder is shared with imported-mesh picking; movement tests only triangles in the swept box's local-space bounds, then perform world-space SAT using double precision. JSON loading rebuilds the BVH once, not on simulation ticks. No graphics/image-import dependencies were added to the headless simulation.

Cooking is limited to 100,000 triangles per component, discards zero-area triangles, and rejects empty/non-finite geometry. Large models should use separate surface colliders or simplified collision meshes. Geometry is embedded in JSON, so complex colliders increase scene/prefab file size; existing prefab file-size limits still apply. Cooking runs on the editor thread when explicitly requested, not each frame.

Checks:

```sh
cargo test -p bozzard-demo --test mesh_colliders
cargo test -p bozzard-editor --test mesh_colliders
cargo test -p bozzard-assets picking::
cargo test -p bozzard-scene bvh::
python3 tools/check_headless.py
```
