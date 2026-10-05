use super::*;

/// Bounded uniform grid for conservative movement envelopes. Large, uncertain,
/// or out-of-range envelopes stay in a broad list and can never be omitted.
pub(super) struct Index {
    cell_size: f64,
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
            cell_size: 1.,
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
        let mut sizes: Vec<_> = bounds
            .iter()
            .zip(ranks)
            .filter(|(_, rank)| **rank != usize::MAX)
            .map(|(b, _)| (b[1] - b[0]).max_element())
            .filter(|v| v.is_finite() && *v > 0.)
            .collect();
        sizes.sort_unstable_by(f32::total_cmp);
        let mut result = Self {
            cell_size: sizes
                .get(sizes.len() / 2)
                .copied()
                .unwrap_or(1.)
                .max(0.0001) as f64,
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
            let a = (bounds[0][axis] as f64 / self.cell_size).floor();
            let b = (bounds[1][axis] as f64 / self.cell_size).floor();
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

#[cfg(test)]
mod tests {
    use super::*;
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
