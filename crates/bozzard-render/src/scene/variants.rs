//! Bounded, demand-created surface variants. All resource layouts remain shared;
//! only proven stock-map substitutions, winding and consumed outputs specialize.
use super::instancing::Batch;
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct Key {
    graph: Option<u64>,
    pbr: bool,
    transparent: bool,
    instanced: bool,
    native: bool,
    merged: bool,
    pub mask: u8,
    raster: u8,
    maps: u8,
    fast: bool,
}
#[derive(Default)]
pub(super) struct VariantCache {
    pub pipelines: BTreeMap<Key, wgpu::RenderPipeline>,
    idle: std::collections::VecDeque<Key>,
}
/// A known graph flavor to compile during loading, before its first rendered frame.
pub struct ShaderWarmup<'a> {
    pub shader: &'a ShaderSource,
    pub pbr: bool,
    pub transparent: bool,
    pub instanced: bool,
    pub output_mask: u8,
    pub cull_mode: Option<wgpu::Face>,
}
pub(super) fn raster_class(
    models: &BTreeMap<String, Vec<UploadedPart>>,
    draw: &PreparedDraw,
    enabled: bool,
    determinant: f32,
) -> u8 {
    if !enabled || !draw.pbr {
        return 0;
    }
    let MeshKind::ModelPart(id, index) = &draw.object.mesh else {
        return 0;
    };
    let Some(shading) = models
        .get(id)
        .and_then(|p| p.get(*index))
        .and_then(|p| p.shading.as_ref())
    else {
        return 0;
    };
    if shading.double_sided {
        0
    } else if determinant > 0. {
        1
    } else {
        2
    }
}
pub(super) fn mask(stores: [bool; 3]) -> u8 {
    u8::from(stores[0]) | u8::from(stores[1]) << 1 | u8::from(stores[2]) << 2
}
impl SceneRenderer {
    /// Precompile at most sixteen known graph flavors. Returns new pipeline count.
    /// Instanced requests use the currently selected device capability policy.
    pub fn prewarm_shader_variants(
        &mut self,
        gpu: &Gpu,
        requests: &[ShaderWarmup<'_>],
    ) -> Result<usize> {
        ensure!(
            requests.len() <= 16,
            "at most sixteen shader flavors can be prewarmed together"
        );
        self.configure_arena(gpu, 0);
        let mut pending = Vec::with_capacity(requests.len());
        for request in requests {
            ensure!(request.output_mask < 8, "invalid shader output mask");
            ensure!(
                request.shader.numeric_parameters.len() <= 16
                    && request
                        .shader
                        .numeric_parameters
                        .iter()
                        .flatten()
                        .all(|v| v.is_finite()),
                "invalid shader numeric parameters"
            );
            let key = Key {
                graph: Some(request.shader.id),
                pbr: request.pbr,
                transparent: request.transparent,
                instanced: request.instanced,
                native: request.instanced && self.instancing.arena_enabled(),
                merged: false,
                mask: request.output_mask,
                raster: match request.cull_mode {
                    Some(wgpu::Face::Back) => 1,
                    Some(wgpu::Face::Front) => 2,
                    None => 0,
                },
                maps: 15,
                fast: self.shader_optimizations,
            };
            let source = variant_source(key, Some(request.shader));
            let module = wgpu::naga::front::wgsl::parse_str(&source).map_err(|error| {
                anyhow::anyhow!("shader warmup: {}", error.emit_to_string(&source))
            })?;
            wgpu::naga::valid::Validator::new(
                wgpu::naga::valid::ValidationFlags::all(),
                wgpu::naga::valid::Capabilities::all(),
            )
            .validate(&module)
            .context("invalid shader warmup")?;
            pending.push((key, source));
        }
        let mut compiled = 0;
        for (key, source) in pending {
            if self.surface_variants.pipelines.contains_key(&key) {
                continue;
            }
            let pipeline = self.compile_surface_variant(gpu, key, source);
            self.surface_variants.pipelines.insert(key, pipeline);
            self.surface_variants.idle.push_back(key);
            compiled += 1;
        }
        while self.surface_variants.idle.len() > 16 {
            self.surface_variants
                .pipelines
                .remove(&self.surface_variants.idle.pop_front().unwrap());
        }
        Ok(compiled)
    }
    pub fn set_shader_optimizations_enabled(&mut self, enabled: bool) {
        if self.shader_optimizations != enabled {
            self.shader_optimizations = enabled;
            self.occlusion.invalidate();
        }
    }
    pub(super) fn variant_key(&self, draw: &PreparedDraw, instances: u32, mask: u8) -> Option<Key> {
        let optimized = self.shader_optimizations;
        let maps = if optimized && draw.shader.is_none() && draw.pbr {
            match &draw.object.mesh {
                MeshKind::ModelPart(id, index) => {
                    self.models[id][*index].shading.as_ref().unwrap().map_mask
                }
                _ => 15,
            }
        } else {
            15
        };
        let key = Key {
            graph: draw.shader,
            pbr: draw.pbr,
            transparent: draw.transparent,
            instanced: instances > 1,
            native: instances > 1 && self.instancing.arena_enabled(),
            merged: self.world_text.entry(draw.source_item).is_some(),
            mask,
            raster: draw.raster,
            maps,
            fast: optimized,
        };
        (mask != 7
            || key.raster != 0
            || maps != 15
            || !optimized
            || key.transparent
            || self.surface_variants.pipelines.contains_key(&key))
        .then_some(key)
    }
    pub(super) fn prepare_surface_variants(
        &mut self,
        gpu: &Gpu,
        draws: &[PreparedDraw],
        batches: &[Batch],
        mask: u8,
        particles: bool,
    ) {
        // Keep every pipeline consumed by either pass active together. Pruning
        // separately for each mask could evict opaque variants before encoding.
        let needed: BTreeMap<_, _> = batches
            .iter()
            .filter_map(|batch| {
                let draw = &draws[batch.indices[0]];
                let mask = if particles && draw.transparent {
                    7
                } else {
                    mask
                };
                self.variant_key(draw, batch.indices.len() as u32, mask)
                    .map(|key| (key, draw))
            })
            .collect();
        let active: BTreeSet<_> = needed.keys().copied().collect();
        self.surface_variants.idle.retain(|k| !active.contains(k));
        for key in self.surface_variants.pipelines.keys() {
            if !active.contains(key) && !self.surface_variants.idle.contains(key) {
                self.surface_variants.idle.push_back(*key);
            }
        }
        while self.surface_variants.idle.len() > 16 {
            self.surface_variants
                .pipelines
                .remove(&self.surface_variants.idle.pop_front().unwrap());
        }
        for (key, draw) in needed {
            if self.surface_variants.pipelines.contains_key(&key) {
                continue;
            }
            let source = variant_source(key, draw.object.material.shader.as_deref());
            let pipeline = self.compile_surface_variant(gpu, key, source);
            self.surface_variants.pipelines.insert(key, pipeline);
            self.stats.surface_variant_compilations += 1;
        }
    }
    fn compile_surface_variant(&self, gpu: &Gpu, key: Key, source: String) -> wgpu::RenderPipeline {
        let module = gpu
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("specialized surface outputs and maps"),
                source: wgpu::ShaderSource::Wgsl(source.into()),
            });
        let object_layout = if key.instanced {
            &self.instancing.layout
        } else {
            &self.layout
        };
        let layout = gpu
            .device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("specialized surface pipeline layout"),
                bind_group_layouts: &[
                    Some(object_layout),
                    key.pbr.then(|| self.pbr.material_layout()),
                    Some(&self.shadows.sample_layout),
                    Some(&self.environment.layout),
                ],
                immediate_size: 0,
            });
        pipeline(gpu, &layout, &module, key)
    }
}
fn variant_source(key: Key, graph: Option<&ShaderSource>) -> String {
    let source = match graph {
        Some(source) => graph_module_text(key.pbr, source),
        None => host_text(key.pbr),
    };
    let source = if key.instanced {
        instancing::instance_module_text_for(source, key.native)
    } else {
        source
    };
    let source = if key.merged {
        source.replacen("@builtin(instance_index)", "@builtin(vertex_index)", 1)
    } else {
        source
    };
    output_module(source, key.mask)
}
fn output_module(mut source: String, mask: u8) -> String {
    if mask == 7 {
        return source;
    }
    if mask & 2 == 0 {
        source = source.replacen(", @location(8) previous_position: vec3<f32>", "", 1)
            .replacen("@location(8) previous:vec4<f32>,", "", 1)
            .replacen("@location(3) previous:vec4<f32>,", "", 1)
            .replacen("    out.previous=(frame.previous_view_projection * object.previous_model)*vec4<f32>(previous_position,1.0);", "", 1);
        let fragment = source.find("@fragment").expect("surface fragment");
        let output = source[fragment..].find("struct SurfaceOutput").unwrap() + fragment;
        // ShaderSource may contain arbitrary helper structs and local variables;
        // only the stock fragment's removed interpolant is rewritten.
        let body = source[fragment..output].replace("in.previous", "vec4<f32>(0.0)");
        source.replace_range(fragment..output, &body);
    }
    for (bit, declaration) in [
        (1, "    @location(1) normal_roughness:vec4<f32>,"),
        (2, "    @location(2) motion_depth_reactive:vec4<f32>,"),
        (4, "    @location(3) fresnel_occlusion:vec4<f32>,"),
    ] {
        if mask & bit == 0 {
            source = source.replacen(declaration, "", 1);
        }
    }
    let start = source
        .find("fn surface_output(")
        .expect("surface output function");
    let body = source[start..].find('{').unwrap() + start;
    let end = source[body..].find("\n}").unwrap() + body + 2;
    let mut fields = vec!["color"];
    if mask & 1 != 0 {
        fields.push("vec4<f32>(normalize(normal),roughness)");
    }
    if mask & 2 != 0 {
        fields.push("vec4<f32>(select(vec2<f32>(0),current_uv-previous_uv,valid),select(1.0,log2(max(1.0-previous_ndc.z,0.00000001)),valid),max(reactive,object.surface_factors.w))");
    }
    if mask & 4 != 0 {
        fields.push("vec4<f32>(f0,ao)");
    }
    let motion = if mask & 2 != 0 {
        "let current_uv=position.xy/frame.viewport.xy;\nlet previous_ndc=previous.xyz/max(previous.w,0.000001);\nlet previous_uv=previous_ndc.xy*vec2<f32>(0.5,-0.5)+0.5;\nlet valid=previous.w>0.00001 && previous_ndc.z>=0.0 && previous_ndc.z<=1.0;"
    } else {
        ""
    };
    source.replace_range(
        body..end,
        &format!(
            "{{\n{motion}\nreturn SurfaceOutput({});\n}}",
            fields.join(",")
        ),
    );
    source
}
fn pipeline(
    gpu: &Gpu,
    layout: &wgpu::PipelineLayout,
    module: &wgpu::ShaderModule,
    key: Key,
) -> wgpu::RenderPipeline {
    let basic = wgpu::VertexBufferLayout {
        array_stride: 32,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &wgpu::vertex_attr_array![0=>Float32x3,1=>Float32x3,2=>Float32x2],
    };
    let pbr = wgpu::VertexBufferLayout {
        array_stride: 48,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &wgpu::vertex_attr_array![3=>Float32x4,4=>Float32x2,5=>Float32x2,6=>Float32x2,7=>Float32x2],
    };
    let buffers = [
        Some(basic),
        key.pbr.then_some(pbr),
        (key.mask & 2 != 0).then(previous_vertex_layout),
    ];
    let mut constants = vec![
        ("fast_unlit", u8::from(key.fast) as f64),
        ("skip_zero_local", u8::from(key.fast) as f64),
    ];
    if key.pbr {
        constants.extend([
            ("map_normal", u8::from(key.maps & 1 != 0) as f64),
            ("map_mr", u8::from(key.maps & 2 != 0) as f64),
            ("map_ao", u8::from(key.maps & 4 != 0) as f64),
            ("map_emissive", u8::from(key.maps & 8 != 0) as f64),
        ]);
    }
    gpu.device
        .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("consumed surface output variant"),
            layout: Some(layout),
            vertex: wgpu::VertexState {
                module,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &buffers,
            },
            fragment: Some(wgpu::FragmentState {
                module,
                entry_point: Some("fs_main"),
                compilation_options: wgpu::PipelineCompilationOptions {
                    constants: &constants,
                    ..Default::default()
                },
                targets: &geometry::color_targets_mask(
                    wgpu::TextureFormat::Rgba16Float,
                    key.transparent,
                    key.mask,
                ),
            }),
            primitive: wgpu::PrimitiveState {
                cull_mode: match key.raster {
                    1 => Some(wgpu::Face::Back),
                    2 => Some(wgpu::Face::Front),
                    _ => None,
                },
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: Some(!key.transparent),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn all_consumed_output_masks_validate_for_both_hosts_and_instances() {
        for pbr in [false, true] {
            for mode in 0..3 {
                for mask in 0..8 {
                    let instanced = mode != 0;
                    let source = host_text(pbr);
                    let source = if instanced {
                        instancing::instance_module_text_for(source, mode == 2)
                    } else {
                        source
                    };
                    let source = output_module(source, mask);
                    let module = wgpu::naga::front::wgsl::parse_str(&source).unwrap_or_else(|e| {
                        panic!(
                            "pbr={pbr},instance={instanced},mask={mask}: {}",
                            e.emit_to_string(&source)
                        )
                    });
                    wgpu::naga::valid::Validator::new(
                        wgpu::naga::valid::ValidationFlags::all(),
                        wgpu::naga::valid::Capabilities::all(),
                    )
                    .validate(&module)
                    .unwrap();
                    if mask & 2 == 0 {
                        assert!(!source.contains("previous_position"));
                    }
                }
            }
        }
    }
}
