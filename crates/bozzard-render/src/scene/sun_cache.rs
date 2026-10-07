use super::*;
pub(super) mod policy;
use local_shadow_maps::compaction;
pub(super) use policy::{DEPTH_BUDGET_BYTES, depth_bytes};
use policy::{certificate_admitted, depth_admitted, next_idle_age};

// Depth32Float has four logical payload bytes per texel. The sun and the two
// independent local banks bound extra static sources separately; required
// working shadow maps and driver/in-flight allocation overhead are not caches.

/// Per-draw caster change serials. Each frame the renderer stamps every lit
/// draw whose depth inputs differ from the previous frame's draw at the same
/// position; any change of draw count or lit/transparent layout (or a frame
/// without a comparison) starts a new epoch instead.
#[derive(Clone, Copy)]
pub(super) struct Serials<'a> {
    pub values: &'a [u64],
    pub epoch: u64,
    pub current: u64,
}
/// Proof that a static depth source holds exactly its member casters, each
/// unchanged since `built`. Memory is one bit per draw, without keys.
pub(super) struct Certificate {
    epoch: u64,
    built: u64,
    members: Vec<u64>,
    count: usize,
    draws: usize,
}
impl Certificate {
    pub fn new(static_mask: &[bool], serials: Serials<'_>) -> Self {
        let mut members = vec![0u64; static_mask.len().div_ceil(64)];
        let mut count = 0;
        for (index, _) in static_mask.iter().enumerate().filter(|(_, s)| **s) {
            members[index / 64] |= 1 << (index % 64);
            count += 1;
        }
        Self {
            epoch: serials.epoch,
            built: serials.current,
            members,
            count,
            draws: static_mask.len(),
        }
    }
    fn matches(&self, static_mask: &[bool], count: usize, serials: Serials<'_>) -> bool {
        self.epoch == serials.epoch
            && self.draws == static_mask.len()
            && serials.values.len() == static_mask.len()
            && self.count == count
            && static_mask.iter().enumerate().all(|(index, &stable)| {
                !stable
                    || (self.members[index / 64] >> (index % 64) & 1 == 1
                        && serials.values[index] <= self.built)
            })
    }
}

pub(super) struct Plan {
    pub static_mask: Vec<bool>,
    pub dynamic_mask: Vec<bool>,
    pub rebuild: bool,
}
pub(super) struct Cache {
    entry: Option<Entry>,
    idle_age: u32,
    spare_dynamic: Vec<bool>,
    range_enabled: bool,
    // Queue uploads happen before either render pass executes, so static and
    // dynamic layers must never alias their compacted uniform storage.
    range_layers: std::cell::RefCell<[RangeLayer; 2]>,
    pub range_draws_saved: std::cell::Cell<usize>,
    pub range_write_bytes: std::cell::Cell<usize>,
    pub range_plan_builds: std::cell::Cell<usize>,
    pub range_plan_reuses: std::cell::Cell<usize>,
}
#[derive(Default)]
struct RangeLayer {
    stream: compaction::Cache,
    topology: Option<instancing::SubsetPlan>,
}
impl RangeLayer {
    fn clear(&mut self) {
        self.stream.clear();
        self.topology = None;
    }
}
impl Default for Cache {
    fn default() -> Self {
        Self {
            entry: None,
            idle_age: 0,
            spare_dynamic: Vec::new(),
            range_enabled: true,
            range_layers: Default::default(),
            range_draws_saved: Default::default(),
            range_write_bytes: Default::default(),
            range_plan_builds: Default::default(),
            range_plan_reuses: Default::default(),
        }
    }
}
struct Entry {
    depth: wgpu::TextureView,
    binding: wgpu::BindGroup,
    pipeline: wgpu::RenderPipeline,
    resolution: u32,
    row: Vec<u8>,
    certificate: Option<Certificate>,
    valid: bool,
}
impl Cache {
    pub fn clear(&mut self) {
        self.entry = None;
        self.idle_age = 0;
        for layer in self.range_layers.get_mut() {
            layer.clear();
        }
    }
    pub fn reset_work_stats(&self) {
        self.range_draws_saved.set(0);
        self.range_write_bytes.set(0);
        self.range_plan_builds.set(0);
        self.range_plan_reuses.set(0);
    }
    pub fn set_range_enabled(&mut self, enabled: bool) {
        self.range_enabled = enabled;
        if !enabled {
            for layer in self.range_layers.get_mut() {
                layer.clear();
            }
        }
    }
    pub fn prepare_ranges(
        &self,
        renderer: &SceneRenderer,
        gpu: &Gpu,
        draws: &[PreparedDraw],
        batches: &[instancing::Batch],
        accepted: &[bool],
        dynamic: bool,
    ) -> Option<compaction::Plan> {
        // Native shadow lists already submit only accepted casters per group.
        if !self.range_enabled
            || !renderer.instancing.shadow_batching()
            || renderer.native_shadows_active()
        {
            return None;
        }
        let mut layers = self.range_layers.borrow_mut();
        let layer = &mut layers[usize::from(dynamic)];
        let reused = renderer.state_caching
            && layer
                .topology
                .as_ref()
                .is_some_and(|plan| plan.matches(renderer, draws, batches, accepted));
        if reused {
            self.range_plan_reuses.set(self.range_plan_reuses.get() + 1);
        } else {
            layer.topology = Some(renderer.shadow_subset_plan(draws, batches, accepted));
            self.range_plan_builds.set(self.range_plan_builds.get() + 1);
        }
        let topology = layer.topology.as_ref().unwrap();
        let plan = layer.stream.prepare_certified_subset(
            renderer,
            gpu,
            topology.batches.clone(),
            topology.saved,
        )?;
        self.range_draws_saved
            .set(self.range_draws_saved.get() + plan.saved);
        self.range_write_bytes
            .set(self.range_write_bytes.get() + plan.bytes);
        Some(plan)
    }
    /// Release source handles and keys after 60 unused updates; intermittent
    /// movers retain their static source during the bounded idle interval.
    pub fn age_unused(&mut self) {
        if self.entry.is_some() {
            if let Some(age) = next_idle_age(self.idle_age) {
                self.idle_age = age;
            } else {
                self.clear();
            }
        }
    }
    pub fn bytes(&self) -> u64 {
        self.entry
            .as_ref()
            .and_then(|entry| depth_bytes(entry.resolution))
            .unwrap_or(0)
    }

    // The cache key receives independent depth inputs and geometry cost estimates.
    #[allow(clippy::too_many_arguments)]
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        draws: &[PreparedDraw],
        batches: &[instancing::Batch],
        static_mask: Vec<bool>,
        row: &[u8],
        resolution: u32,
        geometry_work: (u64, u64),
        serials: Serials<'_>,
    ) -> Option<Plan> {
        self.prepare_with_budget(
            device,
            draws,
            batches,
            static_mask,
            row,
            resolution,
            geometry_work,
            DEPTH_BUDGET_BYTES,
            serials,
        )
    }
    #[allow(clippy::too_many_arguments)]
    pub fn prepare_with_budget(
        &mut self,
        device: &wgpu::Device,
        draws: &[PreparedDraw],
        batches: &[instancing::Batch],
        static_mask: Vec<bool>,
        row: &[u8],
        resolution: u32,
        geometry_work: (u64, u64),
        budget: u64,
        serials: Serials<'_>,
    ) -> Option<Plan> {
        let count = static_mask.iter().filter(|v| **v).count();
        if !depth_admitted(
            resolution,
            device.limits().max_texture_dimension_2d,
            budget.min(DEPTH_BUDGET_BYTES),
        ) || !certificate_admitted(draws.len(), row.len())
        {
            self.clear();
            return None;
        }
        let dynamic_count = draws
            .iter()
            .enumerate()
            .filter(|(i, d)| !d.transparent && d.object.material.lit && !static_mask[*i])
            .count();
        // Compare command work and geometry work separately: stronger batching
        // can leave a few expensive meshes that are still worth caching.
        let groups = batches
            .iter()
            .filter(|b| b.indices.iter().any(|i| static_mask[*i]))
            .count();
        let (static_triangles, dynamic_triangles) = geometry_work;
        if !profitable(
            count,
            dynamic_count,
            groups,
            static_triangles,
            dynamic_triangles,
            resolution,
        ) {
            self.clear();
            return None;
        }
        self.idle_age = 0;
        if self
            .entry
            .as_ref()
            .is_none_or(|e| e.resolution != resolution)
        {
            // Release our old view/binding/pipeline before allocating a resized
            // source. Submitted GPU commands keep their own resource handles.
            self.clear();
            self.entry = Some(Entry::new(device, resolution));
        }
        let entry = self.entry.as_mut().unwrap();
        let matches = entry.valid
            && entry.row == row
            && entry
                .certificate
                .as_ref()
                .is_some_and(|certificate| certificate.matches(&static_mask, count, serials));
        if !matches {
            entry.valid = false;
        }
        let mut dynamic_mask = std::mem::take(&mut self.spare_dynamic);
        dynamic_mask.clear();
        dynamic_mask.extend(
            draws
                .iter()
                .enumerate()
                .map(|(i, d)| !d.transparent && d.object.material.lit && !static_mask[i]),
        );
        Some(Plan {
            static_mask,
            dynamic_mask,
            rebuild: !matches,
        })
    }
    pub fn depth(&self) -> &wgpu::TextureView {
        &self.entry.as_ref().unwrap().depth
    }
    pub fn copy(&self, pass: &mut wgpu::RenderPass<'_>) {
        let entry = self.entry.as_ref().unwrap();
        pass.set_pipeline(&entry.pipeline);
        pass.set_bind_group(0, &entry.binding, &[]);
        pass.draw(0..3, 0..1);
    }
    /// Certify a rebuilt source and recycle the plan's masks: the dynamic mask
    /// stays here for the next plan, the static mask returns to the caller.
    pub fn finish(&mut self, plan: Plan, row: &[u8], serials: Serials<'_>) -> Vec<bool> {
        if plan.rebuild {
            self.finish_retained(row, Certificate::new(&plan.static_mask, serials));
        }
        self.spare_dynamic = plan.dynamic_mask;
        plan.static_mask
    }
    pub fn finish_retained(&mut self, row: &[u8], certificate: Certificate) {
        let entry = self.entry.as_mut().unwrap();
        entry.row.clear();
        entry.row.extend_from_slice(row);
        entry.row.shrink_to_fit();
        entry.certificate = Some(certificate);
        entry.valid = true;
    }
}

fn profitable(
    static_count: usize,
    dynamic_count: usize,
    groups: usize,
    static_triangles: u64,
    dynamic_triangles: u64,
    resolution: u32,
) -> bool {
    resolution > 1
        && static_count > 0
        && dynamic_count > 0
        && ((static_count >= 64 && static_count > dynamic_count && groups >= 32)
            || (static_triangles >= (u64::from(resolution).pow(2) / 32).max(10_000)
                && static_triangles > dynamic_triangles.saturating_mul(2)))
}
impl Entry {
    fn new(device: &wgpu::Device, resolution: u32) -> Self {
        let depth = shadows::target_device(device, resolution);
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("sun cached depth source"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Depth,
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            }],
        });
        let binding = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("sun cached depth source"),
            layout: &layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&depth),
            }],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("sun cached depth copy"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shadow_copy.wgsl").into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("sun cached depth copy"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("sun cached depth copy"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[],
            }),
            primitive: Default::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Always),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });
        Self {
            depth,
            binding,
            pipeline,
            resolution,
            row: Vec::new(),
            certificate: None,
            valid: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn certificates_reject_epoch_serial_membership_and_count_changes() {
        let mask: Vec<bool> = (0..200).map(|i| i % 3 == 0).collect();
        let count = mask.iter().filter(|m| **m).count();
        let mut values = vec![4u64; 200];
        fn serials(values: &[u64], epoch: u64) -> Serials<'_> {
            Serials {
                values,
                epoch,
                current: 9,
            }
        }
        let certificate = Certificate::new(&mask, serials(&values, 2));
        assert!(certificate.matches(&mask, count, serials(&values, 2)));
        // A later epoch (layout change or missing comparison) invalidates.
        assert!(!certificate.matches(&mask, count, serials(&values, 3)));
        // A static member changed after the build.
        values[63] = 10;
        assert!(!certificate.matches(&mask, count, serials(&values, 2)));
        // A dynamic (non-member) draw may change freely.
        values[63] = 4;
        values[64] = 10;
        assert!(certificate.matches(&mask, count, serials(&values, 2)));
        // Membership must be identical, not merely the same size.
        let mut moved = mask.clone();
        moved[0] = false;
        moved[1] = true;
        assert!(!certificate.matches(&moved, count, serials(&values, 2)));
        let mut grown = mask.clone();
        grown[1] = true;
        assert!(!certificate.matches(&grown, count + 1, serials(&values, 2)));
        assert!(!certificate.matches(&mask[..150], count, serials(&values[..150], 2)));
    }
    #[test]
    fn cache_policy_keeps_expensive_compact_groups_and_bypasses_small_scenes() {
        assert!(super::profitable(1, 1, 1, 100_000, 12, 1024));
        assert!(!super::profitable(1, 1, 1, 12, 12, 1024));
        assert!(!super::profitable(100, 0, 32, 100_000, 0, 1024));
        assert!(super::profitable(249, 8, 32, 3000, 96, 256));
        assert!(!super::profitable(1, 1, 1, 100_000, 12, 4096));
    }
    #[test]
    fn depth_copy_shader_validates() {
        let source = include_str!("shadow_copy.wgsl");
        let module = wgpu::naga::front::wgsl::parse_str(source)
            .unwrap_or_else(|e| panic!("{}", e.emit_to_string(source)));
        wgpu::naga::valid::Validator::new(
            wgpu::naga::valid::ValidationFlags::all(),
            wgpu::naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .unwrap();
    }
}
