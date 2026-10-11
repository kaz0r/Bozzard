//! Sphere, capsule and cylinder colliders. Rapier simulates the exact primitive, so a ball
//! rolls and a barrel tips over its rim. The CPU sweeps and queries see a fine convex
//! tessellation instead, the way they already see a Rigidbody's Mesh Collider hull.
use super::*;
use glam::{Mat3, Quat};
use rapier3d::math::Pose;
use rapier3d::prelude::SharedShape;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ColliderShape {
    #[default]
    Sphere,
    Capsule,
    Cylinder,
}
pub const COLLIDER_SHAPES: &[&str] = &["Sphere", "Capsule", "Cylinder"];
impl ColliderShape {
    pub const ALL: [Self; 3] = [Self::Sphere, Self::Capsule, Self::Cylinder];
}

/// The local axis a capsule or cylinder runs along.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ColliderAxis {
    X,
    #[default]
    Y,
    Z,
}
pub const COLLIDER_AXES: &[&str] = &["X", "Y", "Z"];
impl ColliderAxis {
    pub const ALL: [Self; 3] = [Self::X, Self::Y, Self::Z];
    pub fn index(self) -> usize {
        self as usize
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ShapeCollider {
    pub shape: ColliderShape,
    pub center: [f32; 3],
    pub radius: f32,
    /// Full extent along `axis`, rounded caps included. A sphere ignores it, and a capsule
    /// shorter than its diameter is a sphere.
    pub height: f32,
    pub axis: ColliderAxis,
    pub enabled: bool,
    /// Layers this collider belongs to; see [`LAYER_NAMES`].
    pub layers: u32,
    /// Layers this collider interacts with.
    pub mask: u32,
}
impl Default for ShapeCollider {
    fn default() -> Self {
        Self {
            shape: ColliderShape::Sphere,
            center: [0.0; 3],
            radius: 0.5,
            height: 2.0,
            axis: ColliderAxis::Y,
            enabled: true,
            layers: DEFAULT_LAYERS,
            mask: DEFAULT_MASK,
        }
    }
}

/// Segments around the axis and rings per hemisphere. Faces sit at most 1 - cos(7.5°),
/// under 1%, inside the true surface; vertices lie on it.
const SEGMENTS: usize = 24;
const RINGS: usize = 6;

impl ShapeCollider {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.center.iter().all(|v| v.is_finite()),
            "shape collider center must be finite"
        );
        ensure!(
            self.radius.is_finite() && self.radius >= 0.0001,
            "shape collider radius must be finite and at least 0.0001"
        );
        ensure!(
            self.shape == ColliderShape::Sphere
                || (self.height.is_finite() && self.height >= 0.0001),
            "shape collider height must be finite and at least 0.0001"
        );
        Ok(())
    }
    /// Half the straight section along the axis: a cylinder's half height, or the distance
    /// from a capsule's middle to the centre of either cap.
    pub fn half_length(&self) -> f32 {
        match self.shape {
            ColliderShape::Sphere => 0.0,
            ColliderShape::Capsule => ((self.height - 2.0 * self.radius) * 0.5).max(0.0),
            ColliderShape::Cylinder => self.height * 0.5,
        }
    }
    /// Turns the shape's own Y axis onto `axis`.
    fn orientation(&self) -> Quat {
        match self.axis {
            ColliderAxis::X => Quat::from_rotation_z(-std::f32::consts::FRAC_PI_2),
            ColliderAxis::Y => Quat::IDENTITY,
            ColliderAxis::Z => Quat::from_rotation_x(std::f32::consts::FRAC_PI_2),
        }
    }
    /// Local-space bounds, centre included.
    pub fn bounds(&self) -> [Vec3; 2] {
        let r = self.radius;
        let mut extent = Vec3::splat(r);
        extent[self.axis.index()] = match self.shape {
            ColliderShape::Sphere => r,
            ColliderShape::Capsule => self.half_length() + r,
            ColliderShape::Cylinder => self.half_length(),
        };
        let center = Vec3::from(self.center);
        [center - extent, center + extent]
    }
    /// Points on the true surface in local space, for the hull the CPU queries use.
    fn surface_points(&self) -> Vec<Vec3> {
        let (r, half) = (self.radius, self.half_length());
        let ring = |y: f32, radius: f32, points: &mut Vec<Vec3>| {
            for i in 0..SEGMENTS {
                let angle = i as f32 / SEGMENTS as f32 * std::f32::consts::TAU;
                points.push(Vec3::new(angle.cos() * radius, y, angle.sin() * radius));
            }
        };
        let mut points = Vec::new();
        match self.shape {
            ColliderShape::Cylinder => {
                ring(half, r, &mut points);
                ring(-half, r, &mut points);
            }
            ColliderShape::Sphere | ColliderShape::Capsule => {
                // Each hemisphere: rings from its equator to just short of its pole, then the pole.
                for sign in [1.0f32, -1.0] {
                    for step in 0..RINGS {
                        let latitude = step as f32 / RINGS as f32 * std::f32::consts::FRAC_PI_2;
                        ring(
                            sign * (half + latitude.sin() * r),
                            latitude.cos() * r,
                            &mut points,
                        );
                    }
                    points.push(Vec3::new(0.0, sign * (half + r), 0.0));
                }
            }
        }
        let (orientation, center) = (self.orientation(), Vec3::from(self.center));
        points
            .into_iter()
            .map(|p| center + orientation * p)
            .collect()
    }
    /// Line segments outlining the shape in local space, for editor guides: rings around
    /// the axis, the side lines and, for a sphere or capsule, arcs over the caps.
    pub fn outline(&self) -> Vec<[Vec3; 2]> {
        const STEPS: usize = 32;
        let (r, half) = (self.radius, self.half_length());
        let mut lines = Vec::new();
        // An arc of radius r around `center`, in the plane of `u` and `v`, between two angles.
        let mut arc = |center: Vec3, u: Vec3, v: Vec3, from: f32, to: f32| {
            let point = |t: f32| {
                let angle = from + (to - from) * t;
                center + (u * angle.cos() + v * angle.sin()) * r
            };
            for i in 0..STEPS {
                let (a, b) = (i as f32 / STEPS as f32, (i + 1) as f32 / STEPS as f32);
                lines.push([point(a), point(b)]);
            }
        };
        let (tau, pi) = (std::f32::consts::TAU, std::f32::consts::PI);
        match self.shape {
            ColliderShape::Sphere => {
                arc(Vec3::ZERO, Vec3::X, Vec3::Z, 0.0, tau);
                arc(Vec3::ZERO, Vec3::X, Vec3::Y, 0.0, tau);
                arc(Vec3::ZERO, Vec3::Z, Vec3::Y, 0.0, tau);
            }
            ColliderShape::Capsule | ColliderShape::Cylinder => {
                let top = Vec3::Y * half;
                arc(top, Vec3::X, Vec3::Z, 0.0, tau);
                arc(-top, Vec3::X, Vec3::Z, 0.0, tau);
                if self.shape == ColliderShape::Capsule {
                    for side in [Vec3::X, Vec3::Z] {
                        arc(top, side, Vec3::Y, 0.0, pi);
                        arc(-top, side, Vec3::Y, pi, tau);
                    }
                }
                for side in [Vec3::X, -Vec3::X, Vec3::Z, -Vec3::Z] {
                    lines.push([top + side * r, -top + side * r]);
                }
            }
        }
        let (orientation, center) = (self.orientation(), Vec3::from(self.center));
        lines
            .into_iter()
            .map(|line| line.map(|p| center + orientation * p))
            .collect()
    }
    /// The closed, convex triangle surface the CPU sweeps and queries use, shared between
    /// every collider with the same dimensions.
    pub fn tessellation(&self) -> Result<TriangleMesh> {
        type Cache = Mutex<HashMap<[u32; 7], TriangleMesh>>;
        static CACHE: OnceLock<Cache> = OnceLock::new();
        self.validate()?;
        let key = [
            self.shape as u32,
            self.axis as u32,
            self.radius.to_bits(),
            self.height.to_bits(),
            self.center[0].to_bits(),
            self.center[1].to_bits(),
            self.center[2].to_bits(),
        ];
        let cache = CACHE.get_or_init(Default::default);
        if let Some(mesh) = cache.lock().unwrap().get(&key) {
            return Ok(mesh.clone());
        }
        let mesh =
            hull_triangles(&self.surface_points()).context("shape collider has no volume")?;
        let mut cache = cache.lock().unwrap();
        // Authoring drags a radius through many values; keep the cache bounded.
        if cache.len() >= 512 {
            cache.clear();
        }
        cache.insert(key, mesh.clone());
        Ok(mesh)
    }
    /// World-space corners of the local bounds, checked for overflow.
    pub fn geometry(&self, matrix: Mat4) -> Result<[Vec3; 8]> {
        self.validate()?;
        let bounds = self.bounds();
        let corners = std::array::from_fn(|i| {
            matrix.transform_point3(Vec3::new(
                bounds[i & 1].x,
                bounds[(i >> 1) & 1].y,
                bounds[(i >> 2) & 1].z,
            ))
        });
        ensure!(
            matrix.is_finite()
                && matrix.inverse().is_finite()
                && corners.iter().all(|p| p.is_finite()),
            "shape collider world bounds overflow"
        );
        Ok(corners)
    }
    /// This collider as a closed surface in a collision snapshot.
    pub(crate) fn collision_mesh(
        &self,
        id: &str,
        entity: Entity,
        matrix: Mat4,
    ) -> Result<CollisionMesh> {
        self.geometry(matrix)?;
        Ok(CollisionMesh {
            id: id.to_owned(),
            entity,
            mesh: self.tessellation()?,
            matrix,
            solid: true,
            layers: self.layers,
            mask: self.mask,
        })
    }
    /// The Rapier shape for a body part scaled by `linear`. Scales that keep the primitive
    /// round give the exact shape; others (a squashed sphere, a sheared static parent) give
    /// the convex hull of the scaled surface.
    pub(crate) fn rapier_shape(&self, linear: Mat3) -> Result<SharedShape> {
        self.validate()?;
        let scale = Vec3::new(linear.x_axis.x, linear.y_axis.y, linear.z_axis.z);
        let diagonal = [
            linear.x_axis.y,
            linear.x_axis.z,
            linear.y_axis.x,
            linear.y_axis.z,
            linear.z_axis.x,
            linear.z_axis.y,
        ]
        .iter()
        .all(|v| v.abs() <= 1e-6 * scale.abs().max_element().max(1.0));
        let same = |a: f32, b: f32| (a - b).abs() <= 1e-5 * a.abs().max(b.abs());
        let axial = self.axis.index();
        let [u, v] = [(axial + 1) % 3, (axial + 2) % 3];
        let round = diagonal
            && scale.min_element() > 0.0
            && same(scale[u], scale[v])
            && (self.shape == ColliderShape::Cylinder || same(scale[u], scale[axial]));
        if !round {
            let points: Vec<_> = self.surface_points().iter().map(|p| linear * *p).collect();
            ensure!(
                points.iter().all(|p| p.is_finite()),
                "shape collider transform overflow"
            );
            return SharedShape::convex_hull(&points).context("shape collider has no volume");
        }
        let (radius, half) = (self.radius * scale[u], self.half_length() * scale[axial]);
        let shape = match self.shape {
            ColliderShape::Sphere => SharedShape::ball(radius),
            ColliderShape::Capsule => SharedShape::capsule_y(half, radius),
            ColliderShape::Cylinder => SharedShape::cylinder(half, radius),
        };
        let offset = linear * Vec3::from(self.center);
        if offset == Vec3::ZERO && self.axis == ColliderAxis::Y {
            return Ok(shape);
        }
        Ok(SharedShape::compound(vec![(
            Pose::from_parts(offset, self.orientation()),
            shape,
        )]))
    }
}

/// Outward-wound triangles of the convex hull of `points`.
fn hull_triangles(points: &[Vec3]) -> Result<TriangleMesh> {
    let shape = SharedShape::convex_hull(points).context("degenerate convex hull")?;
    let (vertices, indices) = shape
        .as_convex_polyhedron()
        .context("convex hull is not a polyhedron")?
        .to_trimesh();
    TriangleMesh::new(
        indices
            .iter()
            .map(|i| i.map(|j| vertices[j as usize].to_array()))
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn collider(shape: ColliderShape, axis: ColliderAxis) -> ShapeCollider {
        ShapeCollider {
            shape,
            axis,
            center: [1.0, 2.0, 3.0],
            radius: 0.5,
            height: 3.0,
            ..Default::default()
        }
    }

    #[test]
    fn tessellations_fill_their_bounds_and_stay_on_the_surface() {
        for shape in ColliderShape::ALL {
            for axis in ColliderAxis::ALL {
                let collider = collider(shape, axis);
                let mesh = collider.tessellation().unwrap();
                let [lo, hi] = collider.bounds();
                let [mlo, mhi] = mesh.bounds();
                assert!(
                    (mlo - lo).abs().max_element() < 1e-4 && (mhi - hi).abs().max_element() < 1e-4,
                    "{shape:?} {axis:?}: mesh bounds {mlo}..{mhi}, collider {lo}..{hi}"
                );
                // Every vertex is on the true surface: its distance from the axis segment
                // (or, for a cylinder, its radial distance) equals the radius.
                let center = Vec3::from(collider.center);
                let direction = collider.orientation() * Vec3::Y;
                for p in mesh
                    .triangles()
                    .iter()
                    .flatten()
                    .map(|p| Vec3::from_array(*p))
                {
                    let along = (p - center).dot(direction);
                    let radial = ((p - center) - direction * along).length();
                    match shape {
                        ColliderShape::Cylinder => {
                            assert!((radial - 0.5).abs() < 1e-4 && (along.abs() - 1.5).abs() < 1e-4)
                        }
                        _ => {
                            let half = collider.half_length();
                            let nearest = direction * along.clamp(-half, half);
                            assert!(((p - center - nearest).length() - 0.5).abs() < 1e-4);
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn outlines_trace_the_surface() {
        for shape in ColliderShape::ALL {
            for axis in ColliderAxis::ALL {
                let collider = collider(shape, axis);
                let [lo, hi] = collider.bounds();
                let lines = collider.outline();
                assert!(!lines.is_empty());
                for p in lines.iter().flatten() {
                    assert!(
                        p.cmpge(lo - 1e-4).all() && p.cmple(hi + 1e-4).all(),
                        "{shape:?} {axis:?}: {p} outside {lo}..{hi}"
                    );
                }
                // The outline reaches the far ends of the bounds along the axis.
                let reach = lines
                    .iter()
                    .flatten()
                    .map(|p| p[axis.index()])
                    .fold(f32::NEG_INFINITY, f32::max);
                assert!(
                    (reach - hi[axis.index()]).abs() < 1e-4,
                    "{shape:?} {axis:?}"
                );
            }
        }
    }

    #[test]
    fn a_short_capsule_is_a_sphere_and_bad_sizes_fail() {
        let capsule = ShapeCollider {
            shape: ColliderShape::Capsule,
            radius: 1.0,
            height: 0.5,
            ..Default::default()
        };
        assert_eq!(capsule.half_length(), 0.0);
        assert_eq!(capsule.bounds(), [Vec3::splat(-1.0), Vec3::splat(1.0)]);
        for broken in [
            ShapeCollider {
                radius: 0.0,
                ..Default::default()
            },
            ShapeCollider {
                shape: ColliderShape::Cylinder,
                height: f32::NAN,
                ..Default::default()
            },
            ShapeCollider {
                center: [f32::INFINITY, 0.0, 0.0],
                ..Default::default()
            },
        ] {
            assert!(broken.validate().is_err(), "{broken:?}");
        }
    }

    #[test]
    fn round_scales_keep_the_exact_primitive() {
        let ball = ShapeCollider::default();
        let exact = ball
            .rapier_shape(Mat3::from_diagonal(Vec3::splat(2.0)))
            .unwrap();
        assert_eq!(exact.as_ball().unwrap().radius, 1.0);
        // A squashed sphere is no longer a ball; its hull spans the scaled extents.
        let squashed = ball
            .rapier_shape(Mat3::from_diagonal(Vec3::new(2.0, 1.0, 1.0)))
            .unwrap();
        assert!(squashed.as_ball().is_none());
        let aabb = squashed.compute_local_aabb();
        assert!((aabb.maxs.x - 1.0).abs() < 1e-4 && (aabb.maxs.y - 0.5).abs() < 1e-4);
        // A cylinder may stretch along its own axis and stay exact.
        let cylinder = ShapeCollider {
            shape: ColliderShape::Cylinder,
            ..Default::default()
        };
        let stretched = cylinder
            .rapier_shape(Mat3::from_diagonal(Vec3::new(1.0, 3.0, 1.0)))
            .unwrap();
        let exact = stretched.as_cylinder().unwrap();
        assert_eq!((exact.half_height, exact.radius), (3.0, 0.5));
        // An offset or a turned axis becomes a one-child compound around the exact shape.
        let capsule = collider(ColliderShape::Capsule, ColliderAxis::X);
        let compound = capsule.rapier_shape(Mat3::IDENTITY).unwrap();
        let (pose, child) = &compound.as_compound().unwrap().shapes()[0];
        assert!(child.as_capsule().is_some());
        assert!((pose.translation - Vec3::new(1.0, 2.0, 3.0)).length() < 1e-6);
        let aabb = compound.compute_local_aabb();
        assert!((aabb.maxs.x - 2.5).abs() < 1e-4, "{aabb:?}");
    }
}
