use super::*;

impl SceneRenderer {
    /// Compare compatible shader-graph instances with the individual-graph path.
    pub fn set_shader_graph_instancing_enabled(&mut self, enabled: bool) {
        if self.instancing.graph_enabled != enabled {
            self.instancing.graph_enabled = enabled;
            self.instancing.plan = None;
            self.occlusion.invalidate();
            self.shadow_frame = None;
            self.shadows.sun_cache.clear();
            self.shadows.spots.invalidate();
            self.shadows.points.invalidate();
        }
    }

    pub(in crate::scene) fn prepare_instanced_graphs(
        &mut self,
        gpu: &Gpu,
        draws: &[PreparedDraw],
        batches: &[Batch],
        output_mask: u8,
    ) {
        for batch in batches.iter().filter(|batch| batch.indices.len() > 1) {
            let draw = &draws[batch.indices[0]];
            if self
                .variant_key(draw, batch.indices.len() as u32, output_mask)
                .is_some()
            {
                continue;
            }
            let Some(source) = &draw.object.material.shader else {
                continue;
            };
            let auxiliary = output_mask != 0;
            let key = (source.id, auxiliary);
            let flavor = usize::from(draw.pbr);
            if self.graphs[&key].instanced[flavor].is_some() {
                continue;
            }
            let layout = gpu
                .device
                .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: Some("instanced graph pipeline layout"),
                    bind_group_layouts: &[
                        Some(&self.instancing.layout),
                        draw.pbr.then(|| self.pbr.material_layout()),
                        Some(&self.shadows.sample_layout),
                        Some(&self.environment.layout),
                    ],
                    immediate_size: 0,
                });
            let module = gpu
                .device
                .create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some("instanced graph shader"),
                    source: wgpu::ShaderSource::Wgsl(
                        instance_module_text_for(
                            graph_module_text(draw.pbr, source),
                            self.instancing.arena_enabled(),
                        )
                        .into(),
                    ),
                });
            let pipeline = scene_pipeline(
                gpu,
                "instanced graph pipeline",
                &layout,
                &module,
                draw.pbr,
                false,
                auxiliary,
            );
            // Variants belong to the existing bounded graph cache and retire
            // with their parent. Compile only host/output flavors actually used.
            std::sync::Arc::make_mut(self.graphs.get_mut(&key).unwrap()).instanced[flavor] =
                Some(pipeline);
            self.stats.graph_instanced_compilations += 1;
        }
    }
}
