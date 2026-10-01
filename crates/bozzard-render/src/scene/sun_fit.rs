use super::*;

#[derive(Default)]
pub(super) struct Cache {
    view: Option<[u32; 16]>,
    entries: Vec<Option<Entry>>,
}
struct Entry {
    model: [u32; 16],
    bounds: [u32; 6],
    extent: [Vec3; 2],
}
pub(super) struct Prepared {
    pub fit: Option<(Mat4, f32, f32)>,
    pub fallback: bool,
    pub reused: usize,
    pub rebuilt: usize,
}
impl Cache {
    pub fn bytes(&self) -> usize {
        self.entries.capacity() * std::mem::size_of::<Option<Entry>>()
    }
    pub fn clear(&mut self) {
        *self = Self::default();
    }
    pub fn prepare(
        &mut self,
        view: Mat4,
        resolution: u32,
        inputs: impl ExactSizeIterator<Item = (Mat4, [Vec3; 2], bool)>,
    ) -> Prepared {
        let key = view.to_cols_array().map(f32::to_bits);
        if self.view != Some(key) {
            self.entries.clear();
            self.view = Some(key);
        }
        self.entries.resize_with(inputs.len(), || None);
        if self.entries.capacity() > self.entries.len().saturating_mul(2).max(64) {
            self.entries.shrink_to(self.entries.len());
        }
        let mut min = Vec3::splat(f32::INFINITY);
        let mut max = -min;
        let mut prepared = Prepared {
            fit: None,
            fallback: false,
            reused: 0,
            rebuilt: 0,
        };
        for (slot, (model, bounds, lit)) in self.entries.iter_mut().zip(inputs) {
            if !lit {
                *slot = None;
                continue;
            }
            let model_key = model.to_cols_array().map(f32::to_bits);
            let bounds_key = [
                bounds[0].x.to_bits(),
                bounds[0].y.to_bits(),
                bounds[0].z.to_bits(),
                bounds[1].x.to_bits(),
                bounds[1].y.to_bits(),
                bounds[1].z.to_bits(),
            ];
            let extent = if let Some(entry) = slot
                .as_ref()
                .filter(|e| e.model == model_key && e.bounds == bounds_key)
            {
                prepared.reused += 1;
                entry.extent
            } else {
                prepared.rebuilt += 1;
                let mut lo = Vec3::splat(f32::INFINITY);
                let mut hi = -lo;
                for p in shadows::corners(bounds) {
                    // Preserve the reference's two transforms and their order.
                    // Multiplying view*model first changes floating-point rounding.
                    let p = view.transform_point3(model.transform_point3(p));
                    if !p.is_finite() {
                        // Non-finite reductions can depend on SIMD NaN ordering.
                        // Use the original corner loop for the complete fit.
                        *slot = None;
                        prepared.fallback = true;
                        return prepared;
                    }
                    lo = lo.min(p);
                    hi = hi.max(p);
                }
                let extent = [lo, hi];
                *slot = Some(Entry {
                    model: model_key,
                    bounds: bounds_key,
                    extent,
                });
                extent
            };
            // Preserve object order, including equal signed-zero extrema.
            min = min.min(extent[0]);
            max = max.max(extent[1]);
        }
        prepared.fit = shadows::fit_extents(min, max, view, resolution);
        prepared
    }
}
