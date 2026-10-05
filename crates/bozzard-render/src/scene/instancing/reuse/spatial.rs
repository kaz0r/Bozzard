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

/// Temporary spatial hierarchy for a source-order sweep. Only preceding source
/// objects are active, so each subtree's bounds and maximum emitted rank describe
/// the same eligible population. Queries and activation updates share one cap.
pub(super) struct InversionIndex {
    nodes: Vec<InversionNode>,
    root: Option<usize>,
    stack: Vec<usize>,
    leaves: Vec<usize>,
}
struct InversionNode {
    bounds: Bounds,
    parent: usize,
    last_rank: usize,
    // A leaf stores [source index, usize::MAX]; an interior stores two nodes.
    children: [usize; 2],
}
impl InversionIndex {
    pub fn new(bounds: &[Bounds], ranks: &[usize]) -> Self {
        let mut indices: Vec<_> = ranks
            .iter()
            .enumerate()
            .filter_map(|(index, &rank)| (rank != usize::MAX).then_some(index))
            .collect();
        let mut nodes = Vec::with_capacity(indices.len().saturating_mul(2).saturating_sub(1));
        let root = (!indices.is_empty()).then(|| Self::build(&mut indices, bounds, &mut nodes));
        let mut leaves = vec![usize::MAX; ranks.len()];
        for (index, node) in nodes.iter().enumerate() {
            if node.children[1] == usize::MAX {
                leaves[node.children[0]] = index;
            }
        }
        Self {
            nodes,
            root,
            stack: Vec::new(),
            leaves,
        }
    }
    fn build(indices: &mut [usize], boxes: &[Bounds], nodes: &mut Vec<InversionNode>) -> usize {
        let mut low = [f64::INFINITY; 3];
        let mut high = [f64::NEG_INFINITY; 3];
        let center = |index: usize, axis: usize| {
            f64::from(boxes[index][0][axis]) * 0.5 + f64::from(boxes[index][1][axis]) * 0.5
        };
        for &index in indices.iter() {
            for axis in 0..3 {
                let value = center(index, axis);
                low[axis] = low[axis].min(value);
                high[axis] = high[axis].max(value);
            }
        }
        let children = if indices.len() == 1 {
            [indices[0], usize::MAX]
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
            [
                Self::build(left, boxes, nodes),
                Self::build(right, boxes, nodes),
            ]
        };
        let node = nodes.len();
        nodes.push(InversionNode {
            bounds: [Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)],
            parent: usize::MAX,
            last_rank: 0,
            children,
        });
        if children[1] != usize::MAX {
            for child in children {
                nodes[child].parent = node;
            }
        }
        node
    }
    pub fn len(&self) -> usize {
        self.nodes.len()
    }
    pub fn activate(
        &mut self,
        index: usize,
        rank: usize,
        bounds: Bounds,
        examined: &mut usize,
        budget: usize,
    ) -> bool {
        debug_assert_ne!(rank, usize::MAX);
        let mut current = self.leaves[index];
        debug_assert_ne!(current, usize::MAX);
        while current != usize::MAX {
            *examined += 1;
            if *examined > budget {
                return false;
            }
            let children = self.nodes[current].children;
            let (bounds, rank) = if children[1] == usize::MAX {
                (bounds, rank + 1)
            } else {
                let left = &self.nodes[children[0]];
                let right = &self.nodes[children[1]];
                (
                    [
                        left.bounds[0].min(right.bounds[0]),
                        left.bounds[1].max(right.bounds[1]),
                    ],
                    left.last_rank.max(right.last_rank),
                )
            };
            let node = &mut self.nodes[current];
            // A new leaf contained by the preceding aggregate with no greater
            // rank cannot change any ancestor. Stop only after updating children.
            if node.last_rank == rank && node.bounds == bounds {
                break;
            }
            node.last_rank = rank;
            node.bounds = bounds;
            current = node.parent;
        }
        true
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
            // Only preceding source indices have been activated. Zero means an
            // empty subtree; other stored ranks are one greater than real ranks.
            if node.last_rank <= rank + 1 || !overlaps(node.bounds, bounds) {
                continue;
            }
            if node.children[1] == usize::MAX {
                if node.children[0] < index {
                    output.push(node.children[0]);
                }
            } else {
                self.stack.extend(node.children);
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
        assert!(hierarchy.query(100, ranks[100], bounds[100], &mut output, &mut work, COUNT));
        assert!(output.is_empty());
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
            assert!(hierarchy.activate(
                index,
                ranks[index],
                bounds[index],
                &mut work,
                COUNT * COUNT * 2
            ));
        }
        // A failed query cannot certify partial output; a fresh query gets
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
    fn activation_failure_discards_the_sweep_and_fresh_queries_reset_scratch() {
        let bounds = [[Vec3::ZERO, Vec3::ONE]; 2];
        let ranks = [1, 0];
        let mut failed = InversionIndex::new(&bounds, &ranks);
        let mut work = 0;
        assert!(!failed.activate(0, ranks[0], bounds[0], &mut work, 1));
        assert_eq!(work, 2);
        // Callers discard the failed ephemeral certificate and start fresh;
        // they never continue a partially activated hierarchy after failure.
        let mut fresh = InversionIndex::new(&bounds, &ranks);
        let mut output = Vec::new();
        work = 0;
        assert!(fresh.query(1, ranks[1], bounds[1], &mut output, &mut work, 10));
        assert!(output.is_empty());
        assert!(fresh.activate(0, ranks[0], bounds[0], &mut work, 10));
        assert!(fresh.query(1, ranks[1], bounds[1], &mut output, &mut work, 10));
        assert_eq!(output, [0]);
        work = 0;
        assert!(!fresh.query(1, ranks[1], bounds[1], &mut output, &mut work, 1));
        assert_eq!(work, 2);
        work = 0;
        assert!(fresh.query(1, ranks[1], bounds[1], &mut output, &mut work, 10));
        assert_eq!(output, [0]);
    }

    #[test]
    fn factory_source_order_sweep_certifies_exact_pairs_within_shared_work_cap() {
        let data = include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/data/factory_ordering_envelopes.bin"
        ));
        assert_eq!(&data[..8], b"BZBC0001");
        let word = |offset: usize| u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap());
        let count = word(8) as usize;
        assert_eq!(count, 2891);
        assert_eq!(word(12), 0, "the captured certificate uses world bounds");
        assert_eq!(data.len(), 28 + count * 28);
        let mut ranks = Vec::with_capacity(count);
        let mut bounds = Vec::with_capacity(count);
        for index in 0..count {
            let start = 28 + index * 28;
            ranks.push(word(start) as usize);
            bounds.push([
                Vec3::from_array(std::array::from_fn(|axis| {
                    f32::from_bits(word(start + 4 + axis * 4))
                })),
                Vec3::from_array(std::array::from_fn(|axis| {
                    f32::from_bits(word(start + 16 + axis * 4))
                })),
            ]);
        }
        let expected: Vec<_> = (0..count)
            .flat_map(|after| {
                (0..after).filter_map({
                    let bounds = &bounds;
                    let ranks = &ranks;
                    move |before| {
                        (ranks[before] > ranks[after] && overlaps(bounds[before], bounds[after]))
                            .then_some((before, after))
                    }
                })
            })
            .collect();
        assert_eq!(expected.len(), 3111);
        let mut hierarchy = InversionIndex::new(&bounds, &ranks);
        let mut output = Vec::new();
        let mut discovered = Vec::new();
        let mut work = 0;
        let mut query_visits = 0;
        let work_cap = (count * 64).min(super::super::MAX_PLAN_CANDIDATES);
        for index in 0..count {
            let preceding_work = work;
            assert!(
                hierarchy.query(
                    index,
                    ranks[index],
                    bounds[index],
                    &mut output,
                    &mut work,
                    work_cap
                ),
                "query work cap at source object {index}"
            );
            query_visits += work - preceding_work;
            discovered.extend(output.iter().map(|&before| (before, index)));
            assert!(
                hierarchy.activate(index, ranks[index], bounds[index], &mut work, work_cap),
                "activation work cap at source object {index}"
            );
        }
        assert_eq!(discovered, expected);
        assert!(discovered.len() <= (count * 8).min(32_768));
        assert!(work <= work_cap);
        // Pinned main's X-axis active sweep on these same envelope/rank bits:
        // enumerate neighbors before rejecting non-inversions or Y/Z separation.
        // Neighbors and query/update visits are distinct counted primitives.
        let mut sweep: Vec<_> = (0..count).collect();
        sweep.sort_by_key(|&index| ranks[index]);
        sweep.sort_by(|&a, &b| bounds[a][0].x.total_cmp(&bounds[b][0].x));
        let mut active: Vec<usize> = Vec::new();
        let mut main_neighbors = 0;
        let mut main_pairs = Vec::new();
        for index in sweep {
            active.retain(|&other| bounds[other][1].x >= bounds[index][0].x);
            main_neighbors += active.len();
            for &other in &active {
                let (before, after) = (index.min(other), index.max(other));
                if ranks[before] > ranks[after] && overlaps(bounds[index], bounds[other]) {
                    main_pairs.push((before, after));
                }
            }
            active.push(index);
        }
        main_pairs.sort_unstable_by_key(|&(before, after)| (after, before));
        assert_eq!(main_pairs, expected);
        println!(
            "factory_ordering_sweep_proof population={count} main_x_candidate_neighbors={main_neighbors} query_node_visits={query_visits} activation_updates={} total_work={work} work_cap={work_cap} exact_inversion_pairs={} retained_pair_cap={}",
            work - query_visits,
            discovered.len(),
            (count * 8).min(32_768),
        );
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
