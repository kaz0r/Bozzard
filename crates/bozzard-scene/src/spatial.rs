//! Ray primitives shared by editor picking and headless gameplay queries.
use glam::Vec3;
pub fn box_entry(bounds: [Vec3; 2], origin: Vec3, direction: Vec3, limit: f32) -> Option<f64> {
    let mut near = 0.0_f64;
    let mut far = f64::from(limit) * (1.0 + 8.0 * f64::from(f32::EPSILON)) + 1e-6;
    for axis in 0..3 {
        // Conservative padding includes f32 triangle/transform rounding near box edges.
        // f64 slab arithmetic avoids overflow and handles tiny nonzero directions.
        let scale = bounds[0][axis]
            .abs()
            .max(bounds[1][axis].abs())
            .max(origin[axis].abs())
            .max(1.0);
        let pad = f64::from(scale) * 8.0 * f64::from(f32::EPSILON);
        let min = f64::from(bounds[0][axis]) - pad;
        let max = f64::from(bounds[1][axis]) + pad;
        let o = f64::from(origin[axis]);
        let d = f64::from(direction[axis]);
        if d == 0.0 {
            if o < min || o > max {
                return None;
            }
        } else {
            let a = (min - o) / d;
            let b = (max - o) / d;
            near = near.max(a.min(b));
            far = far.min(a.max(b));
            if near > far {
                return None;
            }
        }
    }
    (far >= near && far > 0.0).then_some(near)
}

pub fn triangle_hit([a, b, c]: [Vec3; 3], o: Vec3, d: Vec3) -> Option<f32> {
    let e1 = b - a;
    let e2 = c - a;
    let p = d.cross(e2);
    let det = e1.dot(p);
    if det.abs() < 1e-8 {
        return None;
    }
    let t = o - a;
    let u = t.dot(p) / det;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = t.cross(e1);
    let v = d.dot(q) / det;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let distance = e2.dot(q) / det;
    (distance > 0.0).then_some(distance)
}
