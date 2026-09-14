use super::*;

fn bounds(b: &CollisionBox) -> [DVec3; 2] {
    let [x, y, z] = b.edges;
    let inradius = [(x, y.cross(z)), (y, z.cross(x)), (z, x.cross(y))]
        .into_iter()
        .map(|(edge, normal)| edge.dot(normal.normalize_or_zero()).abs())
        .fold(f64::INFINITY, f64::min);
    if inradius <= 0.0 {
        // Singular geometry must still reach the existing SAT implementation.
        return [DVec3::splat(f64::NEG_INFINITY), DVec3::splat(f64::INFINITY)];
    }
    // SAT allows radius.max(1e-6) * 1e-6 on every separating axis. The
    // inradius lower-bounds every support radius, so this expansion encloses
    // every tolerated intersection, even for tiny, mirrored or sheared boxes.
    // A raw AABB (or fixed epsilon) would lose near-touching SAT contacts.
    let extent = (x.abs() + y.abs() + z.abs()) * (1.0 + 1e-6 * (1e-6 / inradius).max(1.0));
    let rounding = (b.center.abs() + extent).max(DVec3::ONE) * (64.0 * f64::EPSILON);
    [b.center - extent - rounding, b.center + extent + rounding]
}

pub(super) fn overlaps(boxes: &[CollisionBox], output: &mut Vec<(String, String)>) {
    let mut entries: Vec<_> = boxes
        .iter()
        .enumerate()
        .map(|(i, b)| (i, bounds(b)))
        .collect();
    // Sweep the widest axis, avoiding quadratic work for vertical/depth-only layouts.
    let spread = entries.iter().fold(
        [DVec3::splat(f64::INFINITY), DVec3::splat(f64::NEG_INFINITY)],
        |[min, max], (_, b)| [min.min(b[0]), max.max(b[1])],
    );
    let extent = spread[1] - spread[0];
    let axis = (0..3)
        .max_by(|&a, &b| extent[a].total_cmp(&extent[b]))
        .unwrap();
    entries.sort_unstable_by(|a, b| a.1[0][axis].total_cmp(&b.1[0][axis]).then(a.0.cmp(&b.0)));
    for (slot, &(i, a)) in entries.iter().enumerate() {
        for &(j, b) in &entries[slot + 1..] {
            if b[0][axis] > a[1][axis] {
                break;
            }
            if a[0].cmple(b[1]).all() && b[0].cmple(a[1]).all() && boxes[i].intersects(&boxes[j]) {
                let (i, j) = (i.min(j), i.max(j));
                output.push((boxes[i].id.clone(), boxes[j].id.clone()));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sweep_matches_exhaustive_sat_for_shear_extreme_scales_and_touching() {
        let mut world = World::new();
        let entity = world.spawn();
        let mut seed = 1234567_u64;
        let mut random = || {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            ((seed >> 32) as u32 as f64 / u32::MAX as f64) * 2. - 1.
        };
        let mut boxes = Vec::new();
        for i in 0..500 {
            let scale = [1e-10, 1e-5, 1., 1e5, 1e10][i % 5];
            let edges = [
                DVec3::new(random(), random(), random()) * scale,
                DVec3::new(random(), random(), random()) * scale,
                DVec3::new(random(), random(), random()) * scale,
            ];
            let center = DVec3::new(random(), random(), random()) * scale * 20.;
            boxes.push(CollisionBox {
                id: format!("{:04}", boxes.len()),
                entity,
                corners: [Vec3::ZERO; 8],
                center,
                edges,
            });
            // Exact and epsilon-separated face contacts exercise the SAT tolerance.
            boxes.push(CollisionBox {
                id: format!("{:04}", boxes.len()),
                entity,
                corners: [Vec3::ZERO; 8],
                center: center + edges[0] * (2. + 1e-6),
                edges,
            });
        }
        // Degenerate and nearly parallel edges conservatively fall back or expand.
        for epsilon in [0., 1e-16, 1e-10] {
            boxes.push(CollisionBox {
                id: format!("{:04}", boxes.len()),
                entity,
                corners: [Vec3::ZERO; 8],
                center: DVec3::ZERO,
                edges: [DVec3::X, DVec3::new(1., epsilon, 0.), DVec3::Z],
            });
        }
        let mut expected = Vec::new();
        for (i, a) in boxes.iter().enumerate() {
            for b in &boxes[i + 1..] {
                if a.intersects(b) {
                    expected.push((a.id.clone(), b.id.clone()));
                }
            }
        }
        let mut actual = Vec::new();
        overlaps(&boxes, &mut actual);
        actual.sort();
        assert_eq!(actual, expected);
    }
}
