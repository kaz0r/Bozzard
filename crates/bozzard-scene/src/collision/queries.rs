//! CPU query geometry. Mesh rays use the same triangle/BVH primitives as editor picking.
use super::*;
#[derive(Clone, Debug, PartialEq)]
pub struct QueryHit {
    pub object: String,
    pub position: Vec3,
    pub normal: Vec3,
    pub distance: f32,
}
#[derive(Clone, Debug, PartialEq)]
pub struct Contact {
    pub other: String,
    pub normal: Vec3,
    pub impulse: f32,
}
impl CollisionBox {
    fn matrix(&self) -> Mat4 {
        Mat4::from_cols(
            self.edges[0].as_vec3().extend(0.),
            self.edges[1].as_vec3().extend(0.),
            self.edges[2].as_vec3().extend(0.),
            self.center.as_vec3().extend(1.),
        )
    }
    fn ray(&self, o: Vec3, d: Vec3, limit: f32) -> Option<(f32, Vec3)> {
        let inverse = self.matrix().inverse();
        let origin = inverse.transform_point3(o);
        let direction = inverse.transform_vector3(d);
        let mut near = 0f32;
        let mut far = limit;
        let mut normal = Vec3::ZERO;
        for i in 0..3 {
            if direction[i] == 0. {
                if origin[i].abs() > 1. {
                    return None;
                }
            } else {
                let a = (-1. - origin[i]) / direction[i];
                let b = (1. - origin[i]) / direction[i];
                if a.min(b) > near {
                    near = a.min(b);
                    normal = Vec3::ZERO;
                    normal[i] = -direction[i].signum();
                }
                far = far.min(a.max(b));
                if near > far {
                    return None;
                }
            }
        }
        if near == 0. {
            normal = -direction.normalize_or_zero();
        }
        Some((
            near,
            inverse
                .transpose()
                .transform_vector3(normal)
                .normalize_or_zero(),
        ))
    }
    fn sphere(&self, center: Vec3, radius: f32) -> bool {
        if self
            .matrix()
            .inverse()
            .transform_point3(center)
            .abs()
            .max_element()
            <= 1.
        {
            return true;
        }
        const FACES: [[usize; 3]; 12] = [
            [0, 1, 3],
            [0, 3, 2],
            [4, 6, 7],
            [4, 7, 5],
            [0, 4, 5],
            [0, 5, 1],
            [2, 3, 7],
            [2, 7, 6],
            [0, 2, 6],
            [0, 6, 4],
            [1, 5, 7],
            [1, 7, 3],
        ];
        FACES.iter().any(|indices| {
            triangle_distance(center, indices.map(|i| self.corners[i])) <= radius * radius
        })
    }
}
fn triangle_distance(p: Vec3, [a, b, c]: [Vec3; 3]) -> f32 {
    // Closest point on a triangle, including vertex and edge Voronoi regions.
    let ab = b - a;
    let ac = c - a;
    let ap = p - a;
    let d1 = ab.dot(ap);
    let d2 = ac.dot(ap);
    let q = if d1 <= 0. && d2 <= 0. {
        a
    } else {
        let bp = p - b;
        let d3 = ab.dot(bp);
        let d4 = ac.dot(bp);
        if d3 >= 0. && d4 <= d3 {
            b
        } else {
            let vc = d1 * d4 - d3 * d2;
            if vc <= 0. && d1 >= 0. && d3 <= 0. {
                a + ab * (d1 / (d1 - d3))
            } else {
                let cp = p - c;
                let d5 = ab.dot(cp);
                let d6 = ac.dot(cp);
                if d6 >= 0. && d5 <= d6 {
                    c
                } else {
                    let vb = d5 * d2 - d1 * d6;
                    if vb <= 0. && d2 >= 0. && d6 <= 0. {
                        a + ac * (d2 / (d2 - d6))
                    } else {
                        let va = d3 * d6 - d5 * d4;
                        if va <= 0. && d4 - d3 >= 0. && d5 - d6 >= 0. {
                            b + (c - b) * ((d4 - d3) / ((d4 - d3) + (d5 - d6)))
                        } else {
                            let inv = 1. / (va + vb + vc);
                            a + ab * (vb * inv) + ac * (vc * inv)
                        }
                    }
                }
            }
        }
    };
    p.distance_squared(q)
}
pub(super) fn charge(budget: &mut usize, work: usize) -> Result<()> {
    ensure!(
        *budget >= work,
        "blueprint spatial query budget exceeded (1000000 tests/tick)"
    );
    *budget -= work;
    Ok(())
}
impl CollisionSnapshot {
    pub fn raycast(
        &self,
        origin: Vec3,
        direction: Vec3,
        distance: f32,
        ignore: Option<&str>,
    ) -> Result<Option<QueryHit>> {
        self.raycast_budget(
            origin,
            direction,
            distance,
            ignore,
            u32::MAX,
            &mut 1_000_000,
        )
    }
    pub fn overlap_box(
        &self,
        center: Vec3,
        size: Vec3,
        ignore: Option<&str>,
        capacity: usize,
    ) -> Result<Vec<String>> {
        self.overlap_box_budget(center, size, ignore, u32::MAX, capacity, &mut 1_000_000)
    }
    pub fn overlap_sphere(
        &self,
        center: Vec3,
        radius: f32,
        ignore: Option<&str>,
        capacity: usize,
    ) -> Result<Vec<String>> {
        self.overlap_sphere_budget(center, radius, ignore, u32::MAX, capacity, &mut 1_000_000)
    }

    pub(crate) fn raycast_budget(
        &self,
        origin: Vec3,
        direction: Vec3,
        distance: f32,
        ignore: Option<&str>,
        mask: u32,
        budget: &mut usize,
    ) -> Result<Option<QueryHit>> {
        ensure!(
            origin.is_finite()
                && direction.is_finite()
                && direction.length_squared().is_finite()
                && direction.length_squared() > 0.
                && distance.is_finite()
                && distance >= 0.,
            "ray needs a finite origin, nonzero direction and nonnegative distance"
        );
        let direction = direction.normalize();
        let mut best: Option<QueryHit> = None;
        for body in &self.boxes {
            charge(budget, 1)?;
            if ignore == Some(body.id.as_str()) || body.layers & mask == 0 {
                continue;
            }
            if let Some((t, n)) = body.ray(
                origin,
                direction,
                best.as_ref().map_or(distance, |h| h.distance),
            ) {
                if best
                    .as_ref()
                    .is_some_and(|h| h.distance == t && h.object < body.id)
                {
                    continue;
                }
                best = Some(QueryHit {
                    object: body.id.clone(),
                    position: origin + direction * t,
                    normal: n,
                    distance: t,
                });
            }
        }
        for mesh in &self.meshes {
            charge(budget, 1)?;
            if ignore == Some(mesh.id.as_str()) || mesh.layers & mask == 0 {
                continue;
            }
            let inverse = mesh.matrix.inverse();
            if let Some((t, n)) = mesh.mesh.raycast(
                inverse.transform_point3(origin),
                inverse.transform_vector3(direction),
                best.as_ref().map_or(distance, |h| h.distance),
                budget,
            )? {
                if best
                    .as_ref()
                    .is_some_and(|h| h.distance == t && h.object < mesh.id)
                {
                    continue;
                }
                let mut normal = inverse.transpose().transform_vector3(n).normalize_or_zero();
                if normal.dot(direction) > 0. {
                    normal = -normal;
                }
                best = Some(QueryHit {
                    object: mesh.id.clone(),
                    position: origin + direction * t,
                    normal,
                    distance: t,
                });
            }
        }
        Ok(best)
    }
    fn query_box(&self, center: Vec3, size: Vec3) -> Result<CollisionBox> {
        ensure!(
            center.is_finite() && size.is_finite(),
            "query bounds must be finite"
        );
        let collider = BoxCollider {
            center: center.to_array(),
            size: size.to_array(),
            enabled: true,
            ..Default::default()
        };
        let (center, edges, corners) = collider.geometry(Mat4::IDENTITY)?;
        // The entity is never used for a query shape.
        let entity = self
            .boxes
            .first()
            .map(|b| b.entity)
            .or_else(|| self.meshes.first().map(|m| m.entity))
            .context("empty query geometry")?;
        Ok(CollisionBox {
            id: String::new(),
            entity,
            center,
            edges,
            corners,
            layers: u32::MAX,
            mask: u32::MAX,
        })
    }
    pub(crate) fn overlap_box_budget(
        &self,
        center: Vec3,
        size: Vec3,
        ignore: Option<&str>,
        mask: u32,
        capacity: usize,
        budget: &mut usize,
    ) -> Result<Vec<String>> {
        BoxCollider {
            center: center.to_array(),
            size: size.to_array(),
            enabled: true,
            ..Default::default()
        }
        .validate()?;
        if self.boxes.is_empty() && self.meshes.is_empty() {
            return Ok(vec![]);
        }
        let shape = self.query_box(center, size)?;
        let mut hits = Vec::new();
        for body in &self.boxes {
            charge(budget, 1)?;
            if ignore != Some(body.id.as_str())
                && body.layers & mask != 0
                && body.intersects(&shape)
            {
                ensure!(
                    hits.len() < capacity,
                    "overlap result exceeds list capacity"
                );
                hits.push(body.id.clone());
            }
        }
        for mesh in &self.meshes {
            charge(budget, 1)?;
            if ignore != Some(mesh.id.as_str())
                && mesh.layers & mask != 0
                && mesh.overlap_budget(&shape, budget)?
            {
                ensure!(
                    hits.len() < capacity,
                    "overlap result exceeds list capacity"
                );
                hits.push(mesh.id.clone());
            }
        }
        hits.sort();
        Ok(hits)
    }
    pub(crate) fn overlap_sphere_budget(
        &self,
        center: Vec3,
        radius: f32,
        ignore: Option<&str>,
        mask: u32,
        capacity: usize,
        budget: &mut usize,
    ) -> Result<Vec<String>> {
        ensure!(
            center.is_finite()
                && radius.is_finite()
                && radius > 0.
                && (radius * radius).is_finite(),
            "sphere needs finite center and positive radius"
        );
        if self.boxes.is_empty() && self.meshes.is_empty() {
            return Ok(vec![]);
        }
        let shape = self.query_box(center, Vec3::splat((radius * 2.).max(0.0001)))?;
        let mut hits = Vec::new();
        for b in &self.boxes {
            charge(budget, 1)?;
            if ignore != Some(b.id.as_str()) && b.layers & mask != 0 && b.sphere(center, radius) {
                ensure!(
                    hits.len() < capacity,
                    "overlap result exceeds list capacity"
                );
                hits.push(b.id.clone());
            }
        }
        for mesh in &self.meshes {
            if ignore == Some(mesh.id.as_str()) || mesh.layers & mask == 0 {
                continue;
            }
            charge(
                budget,
                if mesh.solid {
                    mesh.mesh.triangles().len() + 1
                } else {
                    1
                },
            )?;
            let mut hit = mesh.containment(&shape).is_some();
            let mut exhausted = false;
            mesh.candidates(&shape, DVec3::ZERO, |t| {
                if hit {
                    return;
                }
                if *budget == 0 {
                    exhausted = true;
                    return;
                }
                *budget -= 1;
                hit |= triangle_distance(center, t.map(|v| v.as_vec3())) <= radius * radius;
            });
            ensure!(!exhausted, "blueprint spatial query budget exceeded");
            if hit {
                ensure!(
                    hits.len() < capacity,
                    "overlap result exceeds list capacity"
                );
                hits.push(mesh.id.clone());
            }
        }
        hits.sort();
        Ok(hits)
    }
}
impl SceneInstance {
    pub fn query_geometry(&self, world: &World) -> Result<CollisionSnapshot> {
        Ok(self.collision_geometry(world)?.0)
    }
}
impl SceneInstance {
    pub(crate) fn blueprint_contacts(
        &self,
        world: &World,
        snapshot: &CollisionSnapshot,
        matrices: &BTreeMap<String, Mat4>,
    ) -> BTreeMap<String, Vec<Contact>> {
        let mut result = world
            .resource::<crate::physics::Physics>()
            .map(|p| p.blueprint_contacts(world, matrices))
            .unwrap_or_default();
        // Static and kinematic contacts have geometric normals and no solver impulse.
        for (a, b) in &snapshot.overlaps {
            if result
                .get(a)
                .is_some_and(|v| v.iter().any(|c| &c.other == b))
            {
                continue;
            }
            let ab = snapshot
                .boxes
                .binary_search_by(|c| c.id.cmp(a))
                .ok()
                .map(|i| &snapshot.boxes[i]);
            let bb = snapshot
                .boxes
                .binary_search_by(|c| c.id.cmp(b))
                .ok()
                .map(|i| &snapshot.boxes[i]);
            let normal = match (ab, bb) {
                (Some(a), Some(b)) => response::contact_normal(a, b)
                    .map(|n| n.as_vec3())
                    .unwrap_or_else(|| (a.center - b.center).as_vec3().normalize_or_zero()),
                (Some(a), None) => snapshot
                    .meshes
                    .iter()
                    .find(|m| &m.id == b)
                    .and_then(|m| m.contact_normal(a))
                    .map(|n| n.as_vec3())
                    .unwrap_or(Vec3::Y),
                (None, Some(b)) => snapshot
                    .meshes
                    .iter()
                    .find(|m| &m.id == a)
                    .and_then(|m| m.contact_normal(b))
                    .map(|n| -n.as_vec3())
                    .unwrap_or(Vec3::NEG_Y),
                _ => continue,
            };
            result.entry(a.clone()).or_default().push(Contact {
                other: b.clone(),
                normal,
                impulse: 0.,
            });
            result.entry(b.clone()).or_default().push(Contact {
                other: a.clone(),
                normal: -normal,
                impulse: 0.,
            });
        }
        for contacts in result.values_mut() {
            contacts.sort_by(|a, b| a.other.cmp(&b.other));
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn triangle_query_budgets_fail_instead_of_returning_partial_results() {
        let mut scene =
            Scene::from_json(r#"{"version":1,"name":"budget","views":{},"objects":[]}"#).unwrap();
        scene.objects.push(Object {
            id: "mesh".into(),
            name: "mesh".into(),
            mesh_collider: Some(MeshCollider {
                enabled: true,
                layers: DEFAULT_LAYERS,
                mask: DEFAULT_MASK,
                mesh: TriangleMesh::new(vec![[[-1., -1., 0.], [1., -1., 0.], [0., 1., 0.]]])
                    .unwrap(),
            }),
            ..Default::default()
        });
        let mut world = World::default();
        let instance = scene.spawn(&mut world).unwrap();
        let q = instance.query_geometry(&world).unwrap();
        assert!(
            q.overlap_sphere_budget(Vec3::ZERO, 1., None, u32::MAX, 8, &mut 1)
                .is_err()
        );
        assert!(
            q.overlap_box_budget(Vec3::ZERO, Vec3::ONE, None, u32::MAX, 8, &mut 1)
                .is_err()
        );
        assert!(
            q.raycast_budget(Vec3::Z, Vec3::NEG_Z, 2., None, u32::MAX, &mut 1)
                .is_err()
        );
    }
}
