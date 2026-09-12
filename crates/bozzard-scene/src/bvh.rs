//! Shared immutable triangle-bounds BVH for asset picking and cooked mesh collision.
use anyhow::{Result, ensure};
use glam::Vec3;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BvhNode {
    pub bounds: [Vec3; 2],
    /// Leaf range in triangle_order; branches (count == 0) have adjacent child nodes.
    pub first: u32,
    pub count: u32,
}
#[derive(Clone, Debug, PartialEq)]
pub struct TriangleBvh {
    pub nodes: Vec<BvhNode>,
    pub triangle_order: Vec<u32>,
}
struct Primitive {
    bounds: [Vec3; 2],
    center: Vec3,
    triangle: u32,
}
impl TriangleBvh {
    pub fn build(bounds: Vec<[Vec3; 2]>, check: &impl Fn() -> Result<()>) -> Result<Self> {
        ensure!(bounds.len() <= 1_000_000, "too many BVH triangles");
        let mut primitives = Vec::with_capacity(bounds.len());
        for (triangle, bounds) in bounds.into_iter().enumerate() {
            if triangle.is_multiple_of(1024) {
                check()?;
            }
            ensure!(
                bounds.iter().all(|p| p.is_finite()) && bounds[0].cmple(bounds[1]).all(),
                "invalid triangle bounds"
            );
            primitives.push(Primitive {
                bounds,
                center: bounds[0] * 0.5 + bounds[1] * 0.5,
                triangle: triangle as u32,
            });
        }
        let mut tree = Self {
            nodes: Vec::new(),
            triangle_order: Vec::new(),
        };
        if !primitives.is_empty() {
            tree.nodes.push(BvhNode::default());
            tree.split(0, 0, &mut primitives, check)?;
            tree.triangle_order = primitives.iter().map(|p| p.triangle).collect();
        }
        check()?;
        tree.nodes.shrink_to_fit();
        Ok(tree)
    }
    fn split(
        &mut self,
        node: usize,
        start: usize,
        primitives: &mut [Primitive],
        check: &impl Fn() -> Result<()>,
    ) -> Result<()> {
        // Median splits bound recursion by log2(import limit), even for coincident triangles.
        check()?;
        let mut bounds = [Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)];
        let mut centers = bounds;
        for p in primitives.iter() {
            bounds = [bounds[0].min(p.bounds[0]), bounds[1].max(p.bounds[1])];
            centers = [centers[0].min(p.center), centers[1].max(p.center)];
        }
        if primitives.len() <= 8 {
            self.nodes[node] = BvhNode {
                bounds,
                first: start as u32,
                count: primitives.len() as u32,
            };
            return Ok(());
        }
        let extent = centers[1] - centers[0];
        let axis = (0..3)
            .max_by(|&a, &b| extent[a].total_cmp(&extent[b]))
            .unwrap();
        let mid = primitives.len() / 2;
        primitives.select_nth_unstable_by(mid, |a, b| {
            a.center[axis]
                .total_cmp(&b.center[axis])
                .then(a.triangle.cmp(&b.triangle))
        });
        let child = self.nodes.len();
        self.nodes.extend([BvhNode::default(); 2]);
        self.nodes[node] = BvhNode {
            bounds,
            first: child as u32,
            count: 0,
        };
        let (left, right) = primitives.split_at_mut(mid);
        self.split(child, start, left, check)?;
        self.split(child + 1, start + mid, right, check)
    }
    pub fn query(&self, bounds: [Vec3; 2], visit: &mut impl FnMut(u32)) {
        if !self.nodes.is_empty() {
            self.visit(0, bounds, visit);
        }
    }
    fn visit(&self, index: usize, bounds: [Vec3; 2], visit: &mut impl FnMut(u32)) {
        let node = self.nodes[index];
        if node.bounds[0].cmpgt(bounds[1]).any() || node.bounds[1].cmplt(bounds[0]).any() {
            return;
        }
        if node.count > 0 {
            for triangle in
                &self.triangle_order[node.first as usize..(node.first + node.count) as usize]
            {
                visit(*triangle);
            }
        } else {
            self.visit(node.first as usize, bounds, visit);
            self.visit(node.first as usize + 1, bounds, visit);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounds_queries_are_conservative_and_prune_distant_triangles() {
        let bounds: Vec<_> = (0..10000)
            .map(|i| [Vec3::X * i as f32, Vec3::X * i as f32 + Vec3::ONE])
            .collect();
        let tree = TriangleBvh::build(bounds.clone(), &|| Ok(())).unwrap();
        let query = [Vec3::ZERO, Vec3::ONE];
        let mut found = Vec::new();
        tree.query(query, &mut |i| found.push(i));
        assert!(found.contains(&0) && found.contains(&1));
        assert!(found.len() <= 16);
        assert!(TriangleBvh::build(vec![[Vec3::NAN; 2]], &|| Ok(())).is_err());
        assert!(TriangleBvh::build(bounds, &|| anyhow::bail!("cancelled")).is_err());
    }
}
