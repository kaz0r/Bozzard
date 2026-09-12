# Mesh Collider and Rigidbody

On a Mesh Renderer, choose **Add Component → Mesh Collider**. Imported surface children use their own correctly centered triangles. Cubes, quads, whole imported models and legacy surface transforms are supported. Adding Mesh Collider replaces an existing Box Collider without removing Rigidbody.

- **Without enabled Rigidbody:** static, two-sided triangle surfaces, retaining holes and concavities. Triangle surfaces are not filled solids.
- **With enabled Rigidbody:** Rapier simulates a **solid convex hull**, including mass, inertia, angular velocity, friction, restitution, continuous collision detection and sleeping. Holes/concavities are filled by the hull. Flat/zero-volume meshes are rejected; use a Box Collider for a thin slab.
- Add Rigidbody before or after Mesh Collider. Rigidbody adds a Box Collider only when no shape exists. The two collider types remain mutually exclusive. Player Controller retains its upright kinematic Box Collider; Mesh Collider cannot share an entity with Player Controller or Trigger.
- Tilted bodies can topple and settle; impacts transfer motion between bodies. **Spin** initializes angular velocity (degrees/second), rather than overwriting physical rotation each frame. Changing/removing Spin changes this velocity. Blueprint transform writes are teleports; scale/shape edits rebuild the shape while preserving velocity.
- Box movement, player spawn safety, follow-camera obstruction and Blueprint contacts use the convex envelope for dynamic meshes. Rapier contacts include dynamic mesh/mesh pairs and survive sleeping. Static mesh/mesh overlaps are not queried.
- Geometry is **baked when added**. Transform changes move it, but renderer/source-file edits do not automatically recook it. **Rebuild from Mesh Renderer** preserves Enabled and leaves the previous component intact on failure. Stale imported surfaces must be rebound first.
- Baked triangles serialize into scenes/prefabs independently of the renderer/source. Geometry, convex envelopes and query BVHs are shared; body velocities and solver handles are independent. Destroy removes solver bodies/colliders; Stop drops the runtime world and restores authored data. Removing a renderer keeps its collider; removing the last collider removes Rigidbody. These edits support Undo/Redo.
- Collider guides show mesh bounds and selected triangle/hull wires, sampled at 2,048 triangles for dense meshes. Sampling never reduces collision accuracy.

## Settings and limits

Rigidbody remains stored under the existing `gravity` scene key. Existing non-player gravity objects now use real dynamics without conversion. Defaults: acceleration 9.81, max fall speed 50, jump speed 5, mass 1 kg, friction 0.6, restitution 0, angular damping 0.1. Runtime velocity/grounding reset on spawn. Disabling the component or its collider pauses it and resets velocity.

Dynamic roots support nonuniform and mirrored scale. Parented bodies require a positive, uniformly scaled, non-sheared parent transform; colliding descendants of a Rigidbody are rejected instead of silently treating them as compound bodies. Compound bodies, dynamic concave decomposition and joints are not exposed. Static transform edits and kinematic player poses are synchronized as teleports, not moving-platform/rider simulation. The editor's WASD box-mover shortcut remains box-only; Space can jump either grounded body shape.

The app steps at 60 Hz. Public gravity steps accept `(0, 1]` seconds and substep to at most 1/60 s. Rapier uses eight solver iterations, CCD and tighter static contacts; contacts still have numerical tolerances, not mathematically exact separation. Player movement retains the existing swept-box solver. Rapier/Parry add only CPU physics/math dependencies, not graphics or model importers.

Cooking is synchronous and capped at 100,000 triangles per component; zero-area triangles are discarded and empty/non-finite geometry is rejected. Prefer simplified collision meshes for large models. Serialized geometry increases scene/prefab size; existing file limits apply. Convex envelopes are cached, not rebuilt on every tick.

```sh
cargo test -p bozzard-scene --test rigidbody --test gravity
cargo test -p bozzard-demo --test mesh_colliders
cargo test -p bozzard-editor --test mesh_colliders
python3 tools/check_headless.py
```
