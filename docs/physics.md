# Physics surface

Rapier owns dynamic bodies — their velocities, inertia and sleeping. The scene document stays the
public pose: editing an object's Transform is a teleport on the next tick. This page covers what
authors can now reach from the Inspector and from Blueprints and scripts.

Everything here is headless-testable: `crates/bozzard-scene/tests/{layers,compound,joints,capsule,shape_colliders}.rs`
exercise the collision filters, compound bodies, constraints, the character controller and the
sphere/capsule/cylinder colliders without a window or a GPU.

## Collision layers and masks

Every Box Collider, Shape Collider, Mesh Collider and Trigger volume carries two 32-bit fields:

- **Layers** — which layers this collider belongs to.
- **Collides with** (Triggers label it **Detects**) — which layers it will meet.

Eight are named for authoring: `Default`, `Player`, `Environment`, `Gameplay`, `Projectile`,
`Character`, `Sensor`, `Reserved 7`. The rest are reserved: unnamed, undrawn, but preserved on save.
A new collider is on `Default` and collides with everything, so an older scene keeps its behaviour.

Two colliders interact only when **each** one's membership intersects the other's filter, matching
Rapier's `InteractionGroups`:

```text
(a.layers & b.mask) != 0 && (b.layers & a.mask) != 0
```

One side refusing is enough. The same rule drives:

- Rapier's solver, through `ColliderBuilder::collision_groups`.
- The CPU swept-box mover used by the editor and **Move With Collision**.
- Overlap reporting, so `On Overlap Enter`/`On Collision Enter` do not fire for filtered pairs.
- Trigger and script/Blueprint overlap volumes.

Blueprint spatial queries (**Raycast**, **Sphere Overlap**, **Box Overlap**, **Line of Sight**) see
every enabled solid collider regardless of layers; use their Ignore object to exclude one. Add
per-query layer masks when a game needs them.

## Shape colliders

**Add Component → Shape Collider** gives an object a sphere, a capsule or a cylinder:

| Field | Meaning |
| --- | --- |
| Shape | Sphere, Capsule or Cylinder. |
| Center | Offset from the object's origin, in local units. |
| Radius | Local radius. |
| Height | Full length along Axis, rounded caps included. A capsule shorter than its diameter is a sphere. Hidden for a sphere. |
| Axis | The local axis a capsule or cylinder runs along. Hidden for a sphere. |
| Layers / Collides with | As for the other colliders. |

An object has one kind of collider: Box, Shape or Mesh. A new Shape Collider is a sphere sized to
the selected mesh's largest extent. Add a Rigidbody to make it fall; removing the collider removes
the Rigidbody with it, as for a box.

Rapier simulates the **exact primitive**, so a ball rolls down a slope, a drum stands on its flat end
and tips over its rim, and a capsule lying on its side rests on its radius. The shape follows the
object's scale: a uniform scale, or a cylinder stretched along its own axis, keeps the exact
primitive, while a scale that squashes it (an ellipsoid "sphere") becomes the convex hull of the
scaled surface. A non-zero Center or an X/Z axis is a one-shape compound around the primitive.

The CPU side — the swept box mover, overlap events, spawn validation, the follow camera's probe and
the Blueprint/script queries — sees a closed convex tessellation instead: 24 segments around the
axis and 6 rings per cap. Its vertices lie on the true surface and its faces sit less than 1% of the
radius inside it, so a ray or a box stops within that margin of the real wall, and a box in the
corner of a cylinder's bounding square does not count as touching it. Shapes are listed with the
meshes in a `CollisionSnapshot`; **Move With Collision** still moves only objects with a Box
Collider.

The editor draws a Shape Collider as its outline — rings and cap arcs — and the collider readout
counts boxes, shapes and meshes separately.

## Rigidbody settings

`Gravity` is the Rigidbody component. Alongside `enabled`, `acceleration`, `max_speed` and
`jump_speed` it now exposes:

| Field | Meaning |
| --- | --- |
| Mass | Total body mass in kg, shared equally across a compound body's shapes. |
| Friction | Contact friction, 0–10. |
| Restitution | Bounciness, 0–1. |
| Linear drag | Velocity damping per second: 0 keeps speed, 1 removes it in a second. |
| Angular damping | Angular velocity damping per second. |
| Gravity scale | Multiplier on Acceleration. 0 floats, negative falls upward. |

`linear_damping`, `gravity_scale` and `friction`/`restitution` are all per body, not global. The
dynamic-body fields hide for a Player Controller, whose motion is the character controller.

## Compound colliders

A Rigidbody no longer has to own its only collider. Every colliding descendant that does not start a
Rigidbody of its own becomes another **shape** of that body:

- The nearest self-or-ancestor with a `Gravity` component (and no Player Controller) is the body
  root. A nested Rigidbody is its own body, and its subtree is not folded into its parent.
- Each compound shape keeps its own transform and scale relative to the root, and its own
  `layers`/`mask`. Friction, restitution and mass come from the root.
- Contact events still name the object that owns the shape, not the body root, so a graph on an arm
  sees its own collisions.
- The authored mass is split equally across the shapes. Use per-shape density if a reference game
  needs an exact inertia tensor.

A dynamic compound child needs a rigid, positive-uniformly-scaled parent chain — the same rule the
root already had. A sheared child of a *static* body bakes the shear into its shape, preserving the
existing static-geometry escape hatch. Validation reports the child and the Rigidbody that rejected
it.

## Joints

**Add Component → Joint** constrains this body to another object, by ID:

| Kind | Behaviour |
| --- | --- |
| Fixed | Welds the two bodies. Zero anchors keep the authored relative pose instead of teleporting them together. |
| Hinge (revolute) | Rotation about one shared axis. Optional angle limits in degrees. |
| Ball socket (spherical) | Free rotation, positions locked. |
| Slider (prismatic) | Translation along one shared axis. Optional distance limits. |
| Rope | The bodies cannot separate past Max limit. |

Anchors are in each body's local space; hinge and slider axes are local to each body too, so a
rotated child keeps its authored axis. Limits are authored in degrees for a hinge and world units
for a slider or rope.

Both endpoints must resolve to a body with a collider. A joint authored on a compound child
constrains the body that owns it, and a joint whose endpoint body is destroyed at runtime is dropped
rather than failing the step. Jointed bodies do not collide with each other. One Joint per object.

The joints are Rapier impulse joints, so they are recreated when their authored values change or
when a body is rebuilt, and they are rebuilt after a checkpoint. Solver warm-start state is not
checkpointed, as with any other body.

## Capsule character controller

The Player Controller is now a Rapier `KinematicCharacterController` driving a capsule, not a
swept box:

| Field | Meaning |
| --- | --- |
| Capsule radius / Capsule height | The controller shape, in world units, independent of the visual scale. Height must be at least twice the radius. |
| Step height | Tallest obstacle stepped over without jumping, and the ground-snap distance. |
| Slope limit ° | Steeper floors slide the controller down instead of being climbed. |
| Snap to ground | Follow the floor across small ledges instead of walking off. |

One `move_shape` per tick combines the frame's walk, gravity and any platform carry, so a slope or a
step is resolved once. Horizontal walking is scaled by `cos(slope)` on a ramp, which is why a route
timed against the old box controller can need a few more ticks. Movement is swept, so the capsule
does not tunnel at normal speeds; dynamic bodies keep Rapier CCD for fast projectiles.

**Moving platforms** work for both kinds of platform: a dynamic body the player stands on passes its
linear velocity through, and a *teleported* static body — one moved by a Transform write, a Blueprint
**Set Position** or an editor gesture — passes its per-tick translation through. The controller
records the ground body it is standing on and adds that body's motion to the requested translation.
This also fixed teleported static colliders being invisible to Rapier until they became an active
island.

The authored Box Collider stays on the player, and is still what the CPU queries use: trigger
volumes, spawn/respawn validation, `obstructed_camera` and the Blueprint overlap events. Author the
box to match the capsule; the capsule is what blocks and moves.

## What is not here

- Dynamic concave decomposition. A dynamic Mesh Collider is still its convex hull; a static one
  keeps its holes. Mesh/mesh overlap is still not queried, and that includes a Shape Collider
  against a Shape or Mesh Collider; Rapier contacts still report a Rigidbody touching one.
- Trigger volumes are boxes. A spherical trigger needs a Trigger shape option.
- Joint contacts are always disabled; there is no per-joint toggle yet.
- Ragdolls, vehicles and force-driven characters are still dynamic bodies, not the controller.
- No layer matrix in the scene file and no named layers beyond the eight above.
- The character controller is not a selectable body for the editor's Space jump; that stays the
  legacy selected-box behaviour.