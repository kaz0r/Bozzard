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
fn axes(a: &CollisionBox, b: &CollisionBox) -> Vec<DVec3> {
    let faces = |e: [DVec3; 3]| [e[1].cross(e[2]), e[2].cross(e[0]), e[0].cross(e[1])];
    faces(a.edges)
        .into_iter()
        .chain(faces(b.edges))
        .chain(
            a.edges
                .into_iter()
                .flat_map(|u| b.edges.map(|v| u.cross(v))),
        )
        .filter_map(|axis| {
            let length = axis.length();
            (length > 0.0).then(|| axis / length)
        })
        .collect()
}
fn radius(a: &CollisionBox, b: &CollisionBox, axis: DVec3) -> f64 {
    a.edges
        .iter()
        .chain(&b.edges)
        .map(|e| e.dot(axis).abs())
        .sum()
}
fn penetration(a: &CollisionBox, b: &CollisionBox) -> Option<(f64, DVec3)> {
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
impl SceneInstance {
    /// Move one enabled box by a world displacement, stopping/sliding against other
    /// enabled boxes held static during this query. No gravity, rotation sweep or pushing.
    /// Deep initial overlap is recovered within eight iterations or fails without mutation.
    /// A mover cannot carry enabled child colliders; compound-body motion is not supported.
    pub fn move_box(&self, world: &mut World, id: &str, displacement: Vec3) -> Result<MoveResult> {
        ensure!(displacement.is_finite(), "movement must be finite");
        let snapshot = self.collisions(world)?;
        let mut boxes = snapshot.boxes;
        let index = boxes
            .iter()
            .position(|b| b.id == id)
            .context("mover needs an enabled box collider")?;
        let mut mover = boxes.remove(index);
        for other in &boxes {
            let mut parent = self
                .document
                .objects
                .iter()
                .find(|o| o.id == other.id)
                .and_then(|o| o.parent.as_deref());
            while let Some(ancestor) = parent {
                ensure!(
                    ancestor != id,
                    "moving compound collider hierarchies is not supported"
                );
                parent = self
                    .document
                    .objects
                    .iter()
                    .find(|o| o.id == ancestor)
                    .and_then(|o| o.parent.as_deref());
            }
        }
        let original_center = mover.center;
        let mut contacts = std::collections::BTreeSet::new();
        let mut contact_normals = Vec::new();
        for _ in 0..MAX_CONTACTS {
            let deepest = boxes
                .iter()
                .filter_map(|b| penetration(&mover, b).map(|hit| (b, hit)))
                .max_by(|a, b| a.1.0.total_cmp(&b.1.0));
            let Some((other, (depth, normal))) = deepest else {
                break;
            };
            mover.center += normal * (depth + SKIN);
            contacts.insert(other.id.clone());
            contact_normals.push(normal.as_vec3());
        }
        ensure!(
            boxes.iter().all(|b| penetration(&mover, b).is_none()),
            "cannot recover initial box penetration"
        );
        let mut remaining = displacement.as_dvec3();
        for _ in 0..MAX_CONTACTS {
            if remaining.length_squared() < 1e-20 {
                break;
            }
            let first = boxes
                .iter()
                .filter_map(|b| sweep(&mover, b, remaining).map(|hit| (b, hit)))
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
            contacts.insert(other.id.clone());
            contact_normals.push(normal.as_vec3());
        }
        let applied = (mover.center - original_center).as_vec3();
        ensure!(
            applied.is_finite(),
            "movement exceeds world precision limits"
        );
        let object = self.document.objects.iter().find(|o| o.id == id).unwrap();
        let matrices = self.global_transforms(world)?;
        let parent = object
            .parent
            .as_ref()
            .map(|id| matrices[id])
            .unwrap_or(Mat4::IDENTITY);
        let original = *world
            .get::<Transform>(mover.entity)
            .context("mover transform missing")?;
        let mut next = original;
        next.translation = (Vec3::from_array(original.translation)
            + parent.inverse().transform_vector3(applied))
        .to_array();
        next.validate()?;
        *world.get_mut::<Transform>(mover.entity).unwrap() = next;
        // Validate the actual f32 world result, including descendants, before publishing.
        let validation = (|| -> Result<Vec3> {
            let result = self.collisions(world)?;
            let actual = result.boxes.iter().find(|b| b.id == id).unwrap();
            ensure!(
                result
                    .boxes
                    .iter()
                    .filter(|b| b.id != id)
                    .all(|b| penetration(actual, b).is_none()),
                "movement cannot be represented without penetration at this world scale"
            );
            Ok((actual.center - original_center).as_vec3())
        })();
        match validation {
            Ok(applied) => Ok(MoveResult {
                requested: displacement,
                applied,
                contacts: contacts.into_iter().collect(),
                contact_normals,
            }),
            Err(error) => {
                *world.get_mut::<Transform>(mover.entity).unwrap() = original;
                Err(error)
            }
        }
    }
}
