use super::*;

/// Bounded uniform grid for conservative movement envelopes. Large, uncertain,
/// or out-of-range envelopes stay in a broad list and can never be omitted.
pub(in crate::scene::instancing) struct Index {
    cell_size: [f64; 3],
    buckets: HashMap<[i32; 3], Vec<usize>>,
    cells: Vec<Vec<[i32; 3]>>,
    broad: Vec<usize>,
    all: Vec<usize>,
    marks: Vec<u64>,
    epoch: u64,
}

impl Default for Index {
    fn default() -> Self {
        Self {
            cell_size: [1.; 3],
            buckets: HashMap::new(),
            cells: Vec::new(),
            broad: Vec::new(),
            all: Vec::new(),
            marks: Vec::new(),
            epoch: 0,
        }
    }
}
impl Index {
    pub fn new(bounds: &[Bounds], ranks: &[usize]) -> Self {
        // Independent axis scales preserve separation for long, thin boxes;
        // a wide X extent must not merge thousands of disjoint Y/depth rows.
        let cell_size = std::array::from_fn(|axis| {
            let mut sizes: Vec<_> = bounds
                .iter()
                .zip(ranks)
                .filter(|(_, rank)| **rank != usize::MAX)
                .map(|(b, _)| b[1][axis] - b[0][axis])
                .filter(|v| v.is_finite() && *v > 0.)
                .collect();
            sizes.sort_unstable_by(f32::total_cmp);
            f64::from(
                sizes
                    .get(sizes.len() / 2)
                    .copied()
                    .unwrap_or(1.)
                    .max(0.0001),
            )
        });
        let mut result = Self {
            cell_size,
            cells: (0..bounds.len()).map(|_| Vec::new()).collect(),
            marks: vec![0; bounds.len()],
            all: ranks
                .iter()
                .enumerate()
                .filter(|(_, r)| **r != usize::MAX)
                .map(|(i, _)| i)
                .collect(),
            ..Self::default()
        };
        for index in result.all.clone() {
            result.update(index, bounds[index]);
        }
        result
    }
    fn range(&self, bounds: Bounds) -> Option<([i32; 3], [i32; 3])> {
        let mut start = [0; 3];
        let mut end = [0; 3];
        let mut count = 1usize;
        for axis in 0..3 {
            let a = (bounds[0][axis] as f64 / self.cell_size[axis]).floor();
            let b = (bounds[1][axis] as f64 / self.cell_size[axis]).floor();
            if !a.is_finite()
                || !b.is_finite()
                || a < i32::MIN as f64
                || b > i32::MAX as f64
                || b < a
            {
                return None;
            }
            start[axis] = a as i32;
            end[axis] = b as i32;
            count = count.checked_mul((b - a + 1.) as usize)?;
            if count > 64 {
                return None;
            }
        }
        Some((start, end))
    }
    pub fn update(&mut self, index: usize, bounds: Bounds) {
        let mut cells = std::mem::take(&mut self.cells[index]);
        for cell in &cells {
            if let Some(bucket) = self.buckets.get_mut(cell) {
                bucket.retain(|&other| other != index);
                if bucket.is_empty() {
                    self.buckets.remove(cell);
                }
            }
        }
        cells.clear();
        self.broad.retain(|&other| other != index);
        if let Some((a, b)) = self.range(bounds) {
            for x in a[0]..=b[0] {
                for y in a[1]..=b[1] {
                    for z in a[2]..=b[2] {
                        let cell = [x, y, z];
                        self.buckets.entry(cell).or_default().push(index);
                        cells.push(cell);
                    }
                }
            }
        } else {
            self.broad.push(index);
        }
        self.cells[index] = cells;
    }
    pub fn query(&mut self, bounds: Bounds, output: &mut Vec<usize>) {
        output.clear();
        self.epoch = self.epoch.wrapping_add(1);
        if self.epoch == 0 {
            self.marks.fill(0);
            self.epoch = 1;
        }
        if let Some((a, b)) = self.range(bounds) {
            for &index in &self.broad {
                self.marks[index] = self.epoch;
                output.push(index);
            }
            for x in a[0]..=b[0] {
                for y in a[1]..=b[1] {
                    for z in a[2]..=b[2] {
                        if let Some(bucket) = self.buckets.get(&[x, y, z]) {
                            for &index in bucket {
                                if self.marks[index] != self.epoch {
                                    self.marks[index] = self.epoch;
                                    output.push(index);
                                }
                            }
                        }
                    }
                }
            }
        } else {
            output.extend_from_slice(&self.all);
        }
        // Stable candidate order preserves deterministic capacity/fallback behavior.
        output.sort_unstable();
    }
}

/// Temporary balanced hierarchy for initial inverted-order discovery. Geometry
/// bounds and ordering ranges can both prune a complete subtree before any
/// neighbor leaves are expanded; every node visit consumes the work budget.
pub(super) struct InversionIndex {
    nodes: Vec<InversionNode>,
    root: Option<usize>,
    stack: Vec<usize>,
}
struct InversionNode {
    bounds: Bounds,
    first: usize,
    last_rank: usize,
    children: Option<[usize; 2]>,
}
impl InversionIndex {
    pub fn new(bounds: &[Bounds], ranks: &[usize]) -> Self {
        let mut indices: Vec<_> = ranks
            .iter()
            .enumerate()
            .filter_map(|(index, &rank)| (rank != usize::MAX).then_some(index))
            .collect();
        let mut nodes = Vec::with_capacity(indices.len().saturating_mul(2).saturating_sub(1));
        let root =
            (!indices.is_empty()).then(|| Self::build(&mut indices, bounds, ranks, &mut nodes));
        Self {
            nodes,
            root,
            stack: Vec::new(),
        }
    }
    fn build(
        indices: &mut [usize],
        boxes: &[Bounds],
        ranks: &[usize],
        nodes: &mut Vec<InversionNode>,
    ) -> usize {
        let mut bounds = [Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)];
        let mut first = usize::MAX;
        let mut last_rank = 0;
        let mut low = [f64::INFINITY; 3];
        let mut high = [f64::NEG_INFINITY; 3];
        let center = |index: usize, axis: usize| {
            f64::from(boxes[index][0][axis]) * 0.5 + f64::from(boxes[index][1][axis]) * 0.5
        };
        for &index in indices.iter() {
            bounds[0] = bounds[0].min(boxes[index][0]);
            bounds[1] = bounds[1].max(boxes[index][1]);
            first = first.min(index);
            last_rank = last_rank.max(ranks[index]);
            for axis in 0..3 {
                let value = center(index, axis);
                low[axis] = low[axis].min(value);
                high[axis] = high[axis].max(value);
            }
        }
        let children = if indices.len() == 1 {
            None
        } else {
            let axis = (0..3)
                .max_by(|&a, &b| {
                    (high[a] - low[a])
                        .total_cmp(&(high[b] - low[b]))
                        .then(b.cmp(&a))
                })
                .unwrap();
            let middle = indices.len() / 2;
            indices.select_nth_unstable_by(middle, |&a, &b| {
                center(a, axis).total_cmp(&center(b, axis)).then(a.cmp(&b))
            });
            let (left, right) = indices.split_at_mut(middle);
            Some([
                Self::build(left, boxes, ranks, nodes),
                Self::build(right, boxes, ranks, nodes),
            ])
        };
        let node = nodes.len();
        nodes.push(InversionNode {
            bounds,
            first,
            last_rank,
            children,
        });
        node
    }
    pub fn len(&self) -> usize {
        self.nodes.len()
    }
    pub fn query(
        &mut self,
        index: usize,
        rank: usize,
        bounds: Bounds,
        output: &mut Vec<usize>,
        examined: &mut usize,
        budget: usize,
    ) -> bool {
        output.clear();
        self.stack.clear();
        self.stack.extend(self.root);
        while let Some(index_of_node) = self.stack.pop() {
            *examined += 1;
            if *examined > budget {
                return false;
            }
            let node = &self.nodes[index_of_node];
            // The leaf sought is earlier in original order but later in the
            // emitted schedule. Both inequalities are strict; original peers
            // cannot become dependencies just because their boxes coincide.
            if node.first >= index || node.last_rank <= rank || !overlaps(node.bounds, bounds) {
                continue;
            }
            if let Some(children) = node.children {
                self.stack.extend(children);
            } else {
                output.push(node.first);
            }
        }
        output.sort_unstable();
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hierarchy_queries_match_exact_inversions_and_bound_every_node_visit() {
        const COUNT: usize = 257;
        let mut bounds: Vec<_> = (0..COUNT)
            .map(|index| {
                let center = Vec3::new(
                    (index % 17) as f32,
                    ((index / 17) % 7) as f32,
                    (index % 5) as f32,
                );
                let size = Vec3::new(
                    (index % 4) as f32 + 0.1,
                    (index % 3) as f32 + 0.2,
                    (index % 2) as f32 + 0.3,
                );
                [center - size, center + size]
            })
            .collect();
        bounds[0] = [Vec3::splat(f32::NEG_INFINITY), Vec3::splat(f32::INFINITY)];
        bounds[1] = [Vec3::splat(-1e30), Vec3::splat(1e30)];
        bounds[2] = [Vec3::new(-10000., -0.1, -0.1), Vec3::new(10000., 0.1, 0.1)];
        let ranks: Vec<_> = (0..COUNT)
            .map(|index| {
                if index % 19 == 18 {
                    usize::MAX
                } else {
                    index * 73 % COUNT
                }
            })
            .collect();
        let eligible = ranks.iter().filter(|&&rank| rank != usize::MAX).count();
        let mut hierarchy = InversionIndex::new(&bounds, &ranks);
        assert_eq!(hierarchy.len(), 2 * eligible - 1);
        let mut output = Vec::new();
        let mut work = 0;
        for index in 0..COUNT {
            if ranks[index] == usize::MAX {
                continue;
            }
            let expected: Vec<_> = (0..index)
                .filter(|&other| {
                    ranks[other] != usize::MAX
                        && ranks[other] > ranks[index]
                        && overlaps(bounds[index], bounds[other])
                })
                .collect();
            assert!(hierarchy.query(
                index,
                ranks[index],
                bounds[index],
                &mut output,
                &mut work,
                COUNT * COUNT * 2
            ));
            assert_eq!(output, expected, "missed/extra inversion at {index}");
        }
        // Capacity failure discards partial output; a fresh query still gets
        // the exact complete set, including uncertain and enormous boxes.
        work = 0;
        assert!(!hierarchy.query(100, ranks[100], bounds[100], &mut output, &mut work, 0));
        assert_eq!(work, 1);
        let expected: Vec<_> = (0..100)
            .filter(|&other| {
                ranks[other] != usize::MAX
                    && ranks[other] > ranks[100]
                    && overlaps(bounds[100], bounds[other])
            })
            .collect();
        work = 0;
        assert!(hierarchy.query(
            100,
            ranks[100],
            bounds[100],
            &mut output,
            &mut work,
            COUNT * 2
        ));
        assert_eq!(output, expected);
    }

    #[test]
    fn queries_keep_every_overlap_after_updates_and_bound_sparse_work() {
        let mut bounds: Vec<_> = (0..4096)
            .map(|i| {
                let origin = Vec3::new(i as f32 * 3., 0., 0.);
                [origin - Vec3::splat(0.4), origin + Vec3::splat(0.4)]
            })
            .collect();
        let ranks: Vec<_> = (0..bounds.len()).collect();
        let mut index = Index::new(&bounds, &ranks);
        let mut candidates = Vec::new();
        index.query(bounds[100], &mut candidates);
        assert!(
            candidates.len() < 8,
            "sparse query scanned {} candidates",
            candidates.len()
        );
        bounds[200] = bounds[100];
        index.update(200, bounds[200]);
        index.query(bounds[100], &mut candidates);
        assert!(candidates.contains(&100) && candidates.contains(&200));
        for (other, &b) in bounds.iter().enumerate() {
            if overlaps(bounds[100], b) {
                assert!(candidates.contains(&other));
            }
        }
        bounds[200] = [Vec3::splat(-1e20), Vec3::splat(1e20)];
        index.update(200, bounds[200]);
        index.query(bounds[3000], &mut candidates);
        assert!(candidates.contains(&200));
        index.query(bounds[200], &mut candidates);
        assert_eq!(candidates.len(), bounds.len());
    }
}
