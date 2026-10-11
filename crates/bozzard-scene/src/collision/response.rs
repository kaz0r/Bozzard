use super::*;

const SKIN: f64 = 1e-5;
const MAX_CONTACTS: usize = 8;

#[derive(Debug)]
pub struct MoveResult {
    pub requested: Vec3,
    /// World displacement, including any initial-overlap recovery.
    pub applied: Vec3,
    pub contacts: Vec<String>,
    /// World normals pointing from contacted obstacles toward the mover, in hit order.
    /// May repeat; these are not index-correlated with sorted, unique `contacts`.
    pub contact_normals: Vec<Vec3>,
}
fn axes(a: &CollisionBox, b: &CollisionBox) -> impl Iterator<Item = DVec3> {
    let (a, b) = (a.edges, b.edges);
    let faces = |e: [DVec3; 3]| [e[1].cross(e[2]), e[2].cross(e[0]), e[0].cross(e[1])];
    faces(a)
        .into_iter()
        .chain(faces(b))
        .chain(
            a.into_iter()
                .flat_map(move |u| b.into_iter().map(move |v| u.cross(v))),
        )
        .filter_map(|axis| {
            let length = axis.length();
            (length > 0.0).then(|| axis / length)
        })
}
fn radius(a: &CollisionBox, b: &CollisionBox, axis: DVec3) -> f64 {
    a.edges
        .iter()
        .chain(&b.edges)
        .map(|e| e.dot(axis).abs())
        .sum()
}
pub(super) fn penetration(a: &CollisionBox, b: &CollisionBox) -> Option<(f64, DVec3)> {
    let mut best = (f64::INFINITY, DVec3::ZERO);
    for axis in axes(a, b) {
        let distance = (a.center - b.center).dot(axis);
        let depth = radius(a, b, axis) - distance.abs();
        if depth <= 1e-9 {
            return None;
        }
        if depth < best.0 {
            best = (depth, if distance >= 0.0 { axis } else { -axis });
        }
    }
    Some(best)
}
/// Earliest contact while both boxes retain their orientations and dimensions.
fn sweep(a: &CollisionBox, b: &CollisionBox, delta: DVec3) -> Option<(f64, DVec3)> {
    let mut enter = f64::NEG_INFINITY;
    let mut exit = f64::INFINITY;
    let mut normal = DVec3::ZERO;
    for axis in axes(a, b) {
        let distance = (a.center - b.center).dot(axis);
        let speed = delta.dot(axis);
        let extent = radius(a, b, axis);
        if speed.abs() <= 1e-14 {
            // Tangential motion along an already touching face is free.
            if distance.abs() >= extent - 1e-9 {
                return None;
            }
            continue;
        }
        let first = (-extent - distance) / speed;
        let last = (extent - distance) / speed;
        let near = first.min(last);
        if near > enter {
            enter = near;
            normal = if speed > 0.0 { -axis } else { axis };
        }
        exit = exit.min(first.max(last));
        if enter > exit {
            return None;
        }
    }
    if !(-1e-9..=1.0).contains(&enter) || exit < 0.0 || normal.dot(delta) >= -1e-14 {
        return None;
    }
    Some((enter.max(0.0), normal))
}
/// Collider geometry a run of consecutive moves shares. Any other component or
/// hierarchy change since the last move invalidates it.
#[derive(Default)]
pub(crate) struct MoveCache(std::sync::Mutex<Option<MoveGeometry>>);
struct MoveGeometry {
    world: (u64, u64),
    hierarchy: u64,
    snapshot: CollisionSnapshot,
    /// Conservative broad-phase bounds, parallel to `snapshot.boxes`.
    bounds: Vec<[DVec3; 2]>,
}
impl MoveGeometry {
    fn new(world: (u64, u64), hierarchy: u64, snapshot: CollisionSnapshot) -> Self {
        Self {
            world,
            hierarchy,
            bounds: snapshot.boxes.iter().map(broad_phase::bounds).collect(),
            snapshot,
        }
    }
}
#[cfg(test)]
thread_local! {
    /// Test oracle: test every obstacle instead of those whose bounds meet the mover.
    static FULL_SCAN: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}
fn prefilter() -> bool {
    #[cfg(test)]
    if FULL_SCAN.with(std::cell::Cell::get) {
        return false;
    }
    true
}
/// Bounds of every pose a box reaches within `distance`, padded for rounding. A box
/// whose own bounds miss this region cannot be penetrated or swept into on the way.
fn reach(mover: &CollisionBox, distance: f64) -> [DVec3; 2] {
    let [low, high] = broad_phase::bounds(mover);
    let margin = distance * (1.0 + 1e-6) + low.abs().max(high.abs()).max_element().max(1.0) * 1e-9;
    [low - margin, high + margin]
}
impl Clone for MoveCache {
    fn clone(&self) -> Self {
        Self::default()
    }
}

impl SceneInstance {
    /// Move one enabled box by a world displacement, stopping/sliding against other
    /// enabled boxes and triangle meshes held static during this query. No rotation sweep or pushing.
    /// Deep initial overlap is recovered within eight iterations or fails without mutation.
    /// A mover cannot carry enabled child colliders; compound-body motion is not supported.
    pub fn move_box(&self, world: &mut World, id: &str, displacement: Vec3) -> Result<MoveResult> {
        ensure!(displacement.is_finite(), "movement must be finite");
        let mut cache = self
            .move_cache
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("move cache lock poisoned"))?;
        let current = world.component_mutation_revision();
        let geometry = match cache.take() {
            Some(geometry)
                if geometry.world == current && geometry.hierarchy == self.hierarchy_revision =>
            {
                geometry
            }
            _ => MoveGeometry::new(
                current,
                self.hierarchy_revision,
                self.collision_geometry(world)?.0,
            ),
        };
        let (result, geometry) = self.move_box_in(world, geometry, id, displacement);
        *cache = geometry;
        result
    }

    /// Returns the geometry for the next move only if it still matches the world exactly.
    fn move_box_in(
        &self,
        world: &mut World,
        mut geometry: MoveGeometry,
        id: &str,
        displacement: Vec3,
    ) -> (Result<MoveResult>, Option<MoveGeometry>) {
        let snapshot = &geometry.snapshot;
        let Some(index) = snapshot.boxes.iter().position(|b| b.id == id) else {
            return (
                Err(anyhow::anyhow!("mover needs an enabled box collider")),
                Some(geometry),
            );
        };
        let mut mover = snapshot.boxes[index].clone();
        // A mover only meets obstacles its layer mask allows, matching the solver's groups.
        let (layers, mask) = (mover.layers, mover.mask);
        let obstacles = || {
            snapshot
                .boxes
                .iter()
                .zip(&geometry.bounds)
                .enumerate()
                .filter(move |&(other, (b, _))| {
                    other != index && layers_interact(layers, mask, b.layers, b.mask)
                })
                .map(|(_, entry)| entry)
        };
        // Boxes whose bounds miss a region fail SAT everywhere in it, so skipping them
        // leaves every hit, its order and every tie unchanged.
        let near = |region: [DVec3; 2]| -> Vec<&CollisionBox> {
            let prefilter = prefilter();
            obstacles()
                .filter(|(_, bounds)| !prefilter || broad_phase::touch(bounds, &region))
                .map(|(b, _)| b)
                .collect()
        };
        let meshes: Vec<_> = snapshot
            .meshes
            .iter()
            .filter(|m| layers_interact(layers, mask, m.layers, m.mask))
            .collect();
        // Only an object with children can carry a compound collider.
        let has_children = self.hierarchy_objects.contains(id)
            && self
                .document
                .objects
                .iter()
                .any(|object| object.parent.as_deref() == Some(id));
        if has_children {
            let parents: BTreeMap<_, _> = self
                .document
                .objects
                .iter()
                .map(|object| (object.id.as_str(), object.parent.as_deref()))
                .collect();
            for other in obstacles()
                .map(|(b, _)| &b.id)
                .chain(meshes.iter().map(|m| &m.id))
            {
                let mut parent = parents[other.as_str()];
                while let Some(ancestor) = parent {
                    if ancestor == id {
                        return (
                            Err(anyhow::anyhow!(
                                "moving compound collider hierarchies is not supported"
                            )),
                            Some(geometry),
                        );
                    }
                    parent = parents[ancestor];
                }
            }
        }
        let original_center = mover.center;
        let mut contacts = std::collections::BTreeSet::new();
        let mut contact_normals = Vec::new();
        let mut boxes = near(reach(&mover, 0.0));
        let mut recovered = false;
        for _ in 0..MAX_CONTACTS {
            let deepest = boxes
                .iter()
                .filter_map(|b| penetration(&mover, b).map(|hit| (b.id.as_str(), hit)))
                .chain(
                    meshes
                        .iter()
                        .filter_map(|m| m.penetration(&mover).map(|hit| (m.id.as_str(), hit))),
                )
                .max_by(|a, b| a.1.0.total_cmp(&b.1.0));
            let Some((other, (depth, normal))) = deepest else {
                break;
            };
            mover.center += normal * (depth + SKIN);
            contacts.insert(other.to_owned());
            contact_normals.push(normal.as_vec3());
            if !recovered {
                // Recovery left the region the candidates cover; test every obstacle.
                boxes = obstacles().map(|(b, _)| b).collect();
                recovered = true;
            }
        }
        if !(boxes.iter().all(|b| penetration(&mover, b).is_none())
            && meshes.iter().all(|m| m.penetration(&mover).is_none()))
        {
            return (
                Err(anyhow::anyhow!("cannot recover initial box penetration")),
                Some(geometry),
            );
        }
        let mut remaining = displacement.as_dvec3();
        // Sliding never travels farther than the requested displacement.
        let boxes = near(reach(&mover, remaining.length()));
        for _ in 0..MAX_CONTACTS {
            if remaining.length_squared() < 1e-20 {
                break;
            }
            let first = boxes
                .iter()
                .filter_map(|b| sweep(&mover, b, remaining).map(|hit| (b.id.as_str(), hit)))
                .chain(
                    meshes
                        .iter()
                        .filter_map(|m| m.sweep(&mover, remaining).map(|hit| (m.id.as_str(), hit))),
                )
                .min_by(|a, b| a.1.0.total_cmp(&b.1.0));
            let Some((other, (time, normal))) = first else {
                mover.center += remaining;
                break;
            };
            let approach = -remaining.dot(normal);
            let travel = (time - SKIN / approach).clamp(0.0, 1.0);
            mover.center += remaining * travel;
            remaining *= 1.0 - travel;
            remaining -= normal * remaining.dot(normal).min(0.0);
            contacts.insert(other.to_owned());
            contact_normals.push(normal.as_vec3());
        }
        let applied = (mover.center - original_center).as_vec3();
        if !applied.is_finite() {
            return (
                Err(anyhow::anyhow!("movement exceeds world precision limits")),
                Some(geometry),
            );
        }
        let entity = mover.entity;
        let object = self.object_indices[&entity];
        // The geometry above matches the live cache, so this parent is the one it used.
        let parent = match self.with_render_transforms(world, None, |matrices| {
            Ok(self.document.objects[object]
                .parent
                .as_ref()
                .map(|parent| matrices[self.object_indices[&self.entities[parent]]])
                .unwrap_or(Mat4::IDENTITY))
        }) {
            Ok(parent) => parent,
            Err(error) => return (Err(error), None),
        };
        let Some(&original) = world.get::<Transform>(entity) else {
            return (
                Err(anyhow::anyhow!("mover transform missing")),
                Some(geometry),
            );
        };
        let mut next = original;
        next.translation = (Vec3::from_array(original.translation)
            + parent.inverse().transform_vector3(applied))
        .to_array();
        if let Err(error) = next.validate() {
            return (Err(error), Some(geometry));
        }
        *world.get_mut::<Transform>(entity).unwrap() = next;
        // Validate the actual f32 world result, including descendants, before publishing.
        let validation = (|| -> Result<(Vec3, Option<CollisionSnapshot>)> {
            if has_children {
                // Children move too: check every collider at its new pose.
                let (result, _) = self.collision_geometry(world)?;
                let actual = result
                    .boxes
                    .iter()
                    .find(|b| b.id == id)
                    .context("mover lost its box collider")?;
                representable(id, actual, &result.boxes, &result.meshes)?;
                return Ok(((actual.center - original_center).as_vec3(), None));
            }
            // Only the mover's pose changed, so the other obstacles keep their geometry.
            let matrix =
                self.with_render_transforms(world, None, |matrices| Ok(matrices[object]))?;
            let mut moved = CollisionSnapshot::default();
            self.object_colliders(world, id, entity, matrix, &mut moved)?;
            let actual = moved.boxes.first().context("mover lost its box collider")?;
            let meshes = snapshot.meshes.iter().filter(|m| m.id != id);
            representable(
                id,
                actual,
                near(reach(actual, 0.0)),
                meshes.chain(&moved.meshes),
            )?;
            Ok(((actual.center - original_center).as_vec3(), Some(moved)))
        })();
        match validation {
            Ok((applied, moved)) => {
                let result = MoveResult {
                    requested: displacement,
                    applied,
                    contacts: contacts.into_iter().collect(),
                    contact_normals,
                };
                let Some(moved) = moved else {
                    return (Ok(result), None);
                };
                let range = object_range(&geometry.snapshot.boxes, id, |b| &b.id);
                let bounds = moved.boxes.iter().map(broad_phase::bounds);
                geometry.bounds.splice(range.clone(), bounds);
                geometry.snapshot.boxes.splice(range, moved.boxes);
                let range = object_range(&geometry.snapshot.meshes, id, |m| &m.id);
                geometry.snapshot.meshes.splice(range, moved.meshes);
                geometry.world = world.component_mutation_revision();
                (Ok(result), Some(geometry))
            }
            Err(error) => {
                *world.get_mut::<Transform>(entity).unwrap() = original;
                (Err(error), None)
            }
        }
    }
}

/// The moved box must not penetrate any interacting obstacle once rounded to f32.
fn representable<'a>(
    id: &str,
    actual: &CollisionBox,
    boxes: impl IntoIterator<Item = &'a CollisionBox>,
    meshes: impl IntoIterator<Item = &'a CollisionMesh>,
) -> Result<()> {
    let allowed =
        |layers: u32, mask: u32| layers_interact(actual.layers, actual.mask, layers, mask);
    ensure!(
        boxes
            .into_iter()
            .filter(|b| b.id != id && allowed(b.layers, b.mask))
            .all(|b| penetration(actual, b).is_none())
            && meshes
                .into_iter()
                .filter(|m| allowed(m.layers, m.mask))
                .all(|m| m.penetration(actual).is_none()),
        "movement cannot be represented without penetration at this world scale"
    );
    Ok(())
}

/// One object's entries are contiguous in a geometry snapshot, which is in ID order.
fn object_range<T>(items: &[T], id: &str, key: impl Fn(&T) -> &String) -> std::ops::Range<usize> {
    let start = items
        .iter()
        .position(|item| key(item).as_str() >= id)
        .unwrap_or(items.len());
    let end = start
        + items[start..]
            .iter()
            .take_while(|item| key(item) == id)
            .count();
    start..end
}

/// Minimum separating axis also defines a normal at touching (zero-depth) contacts.
pub(super) fn contact_normal(a: &CollisionBox, b: &CollisionBox) -> Option<DVec3> {
    let mut best = (f64::INFINITY, DVec3::ZERO);
    for axis in axes(a, b) {
        let distance = (a.center - b.center).dot(axis);
        let extent = radius(a, b, axis);
        let depth = extent - distance.abs();
        if depth < -extent.max(1e-6) * 1e-6 {
            return None;
        }
        if depth < best.0 {
            best = (depth, if distance >= 0. { axis } else { -axis });
        }
    }
    best.0.is_finite().then_some(best.1)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Moves through a dense, rotated, sheared and layered scene give bit-identical
    /// results whether or not obstacles are pruned by their bounds.
    #[test]
    fn bounds_pruning_matches_testing_every_obstacle() {
        let mut seed = 0x9e37_79b9_7f4a_7c15_u64;
        let mut random = move || {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (seed >> 11) as f64 / (1u64 << 53) as f64
        };
        let mut objects = Vec::new();
        for group in 0..4 {
            objects.push(serde_json::json!({
                "id": format!("group-{group}"), "name": "Group",
                "transform": {
                    "translation": [group as f32 * 3. - 4., 0., 0.],
                    "rotation_degrees": [0., group as f32 * 25., group as f32 * 10.],
                    "scale": [1. + group as f32 * 0.4, 1., 0.6]
                }
            }));
        }
        let (layers, masks) = ([1, 1, 2, 3], [u32::MAX, 1, 3, 2]);
        for i in 0..160 {
            let mut object = serde_json::json!({
                "id": format!("box-{i:03}"), "name": "Box",
                "transform": {
                    "translation": [random() * 14. - 7., random() * 6. - 3., random() * 14. - 7.],
                    "rotation_degrees": [random() * 360., random() * 360., random() * 360.],
                    "scale": [0.3 + random() * 1.5, 0.3 + random() * 1.5, 0.3 + random() * 1.5]
                },
                "collider": {
                    "center": [random() * 0.4 - 0.2, 0., 0.],
                    "size": [0.2 + random(), 0.2 + random(), 0.2 + random()],
                    "layers": layers[i % 4],
                    "mask": masks[i / 4 % 4]
                }
            });
            if i % 5 == 0 {
                object["parent"] = serde_json::json!(format!("group-{}", i / 5 % 4));
            }
            objects.push(object);
        }
        let scene = Scene::from_json(
            &serde_json::json!({
                "version": 1, "name": "pruning oracle", "views": {}, "objects": objects
            })
            .to_string(),
        )
        .unwrap();
        let (mut pruned, mut full) = (World::new(), World::new());
        let a = scene.spawn(&mut pruned).unwrap();
        let b = scene.spawn(&mut full).unwrap();
        let mut contacts = 0;
        for step in 0..400 {
            let id = format!("box-{:03}", (random() * 160.) as usize);
            let reach = if step % 3 == 0 { 8. } else { 1.5 };
            let delta = Vec3::new(
                ((random() * 2. - 1.) * reach) as f32,
                ((random() * 2. - 1.) * reach) as f32,
                ((random() * 2. - 1.) * reach) as f32,
            );
            if step % 50 == 25 {
                // Outside edits must reach both shared geometries.
                let nudged = [random() as f32, 0., random() as f32];
                for (instance, world) in [(&a, &mut pruned), (&b, &mut full)] {
                    let entity = instance.entity(&id).unwrap();
                    world.get_mut::<Transform>(entity).unwrap().translation = nudged;
                }
            }
            let left = a.move_box(&mut pruned, &id, delta);
            FULL_SCAN.with(|scan| scan.set(true));
            let right = b.move_box(&mut full, &id, delta);
            FULL_SCAN.with(|scan| scan.set(false));
            match (left, right) {
                (Ok(left), Ok(right)) => {
                    assert_eq!(
                        left.applied.to_array().map(f32::to_bits),
                        right.applied.to_array().map(f32::to_bits)
                    );
                    assert_eq!(left.contacts, right.contacts);
                    let normals = |r: &MoveResult| {
                        r.contact_normals
                            .iter()
                            .map(|n| n.to_array().map(f32::to_bits))
                            .collect::<Vec<_>>()
                    };
                    assert_eq!(normals(&left), normals(&right));
                    contacts += usize::from(!left.contacts.is_empty());
                }
                (Err(left), Err(right)) => assert_eq!(left.to_string(), right.to_string()),
                (left, right) => panic!("pruned {left:?} but full scan {right:?}"),
            }
            let pose = |instance: &SceneInstance, world: &World| {
                let transform = world
                    .get::<Transform>(instance.entity(&id).unwrap())
                    .unwrap();
                transform.translation.map(f32::to_bits)
            };
            assert_eq!(pose(&a, &pruned), pose(&b, &full), "step {step}");
        }
        assert!(
            contacts > 50,
            "the scene should exercise contacts, got {contacts}"
        );
    }
}
