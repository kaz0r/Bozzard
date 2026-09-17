use super::*;
use std::ops::Range;

// Fits the downlevel 16 KiB uniform-binding limit without storage-buffer features.
const MAX_INSTANCES: usize = 32;
pub(super) const BUFFER_BYTES: usize = OBJECT_UNIFORM_BYTES * MAX_INSTANCES;

pub(super) struct Pipelines {
    pub pipelines: [[wgpu::RenderPipeline; 2]; 2],
}
pub(super) struct InstanceBinding {
    buffer: wgpu::Buffer,
    texture: TextureKind,
    pub binding: wgpu::BindGroup,
    bytes: Vec<u8>,
}
pub(super) struct Instancing {
    enabled: bool,
    layout: wgpu::BindGroupLayout,
    pub pipelines: Option<Pipelines>,
    pub bindings: Vec<InstanceBinding>,
}
pub(super) struct Batch {
    pub range: Range<usize>,
    pub slot: Option<usize>,
}
impl Instancing {
    pub fn new(layout: wgpu::BindGroupLayout) -> Self {
        Self {
            enabled: true,
            layout,
            pipelines: None,
            bindings: Vec::new(),
        }
    }
}

/// Use the stock shader unchanged apart from selecting its per-invocation object.
/// Private variables are invocation-local in both vertex and fragment stages.
fn module_text(pbr: bool) -> String {
    host_text(pbr)
        .replace(
            "@group(0) @binding(0) var<uniform> object: ObjectUniform;",
            &format!("@group(0) @binding(0) var<uniform> objects: array<ObjectUniform, {MAX_INSTANCES}>;\nvar<private> object: ObjectUniform;"),
        )
        .replace("struct VertexOutput {", "struct VertexOutput {\n    @location(9) @interpolate(flat) instance: u32,")
        .replace("fn vs_main(", "fn vs_main(@builtin(instance_index) instance: u32, ")
        .replace("    var out: VertexOutput;", "    object = objects[instance];\n    var out: VertexOutput;\n    out.instance = instance;")
        .replace("-> SurfaceOutput {", "-> SurfaceOutput {\n    object = objects[in.instance];")
}

fn compatible(a: &PreparedDraw, b: &PreparedDraw) -> bool {
    !a.transparent
        && !b.transparent
        && a.shader.is_none()
        && b.shader.is_none()
        && a.deformation == 0
        && b.deformation == 0
        && !matches!(a.object.mesh, MeshKind::Text(_) | MeshKind::Sprite(_))
        && a.object.mesh == b.object.mesh
        && a.object.material.texture == b.object.material.texture
        && a.pbr == b.pbr
}

fn batches(draws: &[PreparedDraw], visible: &[bool], enabled: bool) -> Vec<Batch> {
    let mut result: Vec<Batch> = Vec::new();
    for (index, draw) in draws.iter().enumerate().filter(|(i, _)| visible[*i]) {
        // ponytail: consecutive runs only, preserving equal-depth winner/order exactly.
        // A measured need for cross-run grouping must define coplanar ordering first.
        if enabled
            && let Some(last) = result.last_mut()
            && last.range.end == index
            && last.range.len() < MAX_INSTANCES
            && compatible(&draws[last.range.start], draw)
        {
            last.range.end += 1;
        } else {
            result.push(Batch {
                range: index..index + 1,
                slot: None,
            });
        }
    }
    result
}

impl SceneRenderer {
    /// Compare the same ordered surfaces against the single-object reference path.
    pub fn set_instancing_enabled(&mut self, enabled: bool) {
        self.instancing.enabled = enabled;
        if !enabled {
            self.instancing.bindings.clear();
        }
    }

    pub(super) fn prepare_instances(
        &mut self,
        gpu: &Gpu,
        draws: &[PreparedDraw],
        visible: &[bool],
    ) -> Result<Vec<Batch>> {
        let mut batches = batches(draws, visible, self.instancing.enabled);
        let count = batches.iter().filter(|b| b.range.len() > 1).count();
        self.instancing.bindings.truncate(count);
        if count == 0 {
            return Ok(batches);
        }
        if self.instancing.pipelines.is_none() {
            let pipelines = std::array::from_fn(|auxiliary| {
                std::array::from_fn(|pbr| {
                    let layout =
                        gpu.device
                            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                                label: Some("instanced scene pipeline layout"),
                                bind_group_layouts: &[
                                    Some(&self.instancing.layout),
                                    (pbr == 1).then(|| self.pbr.material_layout()),
                                    Some(&self.shadows.sample_layout),
                                    Some(&self.environment.layout),
                                ],
                                immediate_size: 0,
                            });
                    let module = gpu
                        .device
                        .create_shader_module(wgpu::ShaderModuleDescriptor {
                            label: Some("instanced scene shader"),
                            source: wgpu::ShaderSource::Wgsl(module_text(pbr == 1).into()),
                        });
                    scene_pipeline(
                        gpu,
                        "instanced scene pipeline",
                        &layout,
                        &module,
                        pbr == 1,
                        false,
                        auxiliary == 1,
                    )
                })
            });
            self.instancing.pipelines = Some(Pipelines { pipelines });
        }
        for (slot, batch) in batches.iter_mut().filter(|b| b.range.len() > 1).enumerate() {
            let texture = &draws[batch.range.start].object.material.texture;
            if slot == self.instancing.bindings.len()
                || self.instancing.bindings[slot].texture != *texture
            {
                let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("instanced object uniforms"),
                    size: BUFFER_BYTES as u64,
                    usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                let binding =
                    self.texture_binding(gpu, texture, &buffer, &self.instancing.layout)?;
                let value = InstanceBinding {
                    buffer,
                    binding,
                    texture: texture.clone(),
                    bytes: Vec::new(),
                };
                if slot == self.instancing.bindings.len() {
                    self.instancing.bindings.push(value);
                } else {
                    self.instancing.bindings[slot] = value;
                }
            }
            let binding = &mut self.instancing.bindings[slot];
            let bytes: Vec<u8> = self.objects[batch.range.clone()]
                .iter()
                .flat_map(|b| b.uniform.as_ref().unwrap().iter().copied())
                .collect();
            if !self.state_caching || binding.bytes != bytes {
                gpu.queue.write_buffer(&binding.buffer, 0, &bytes);
                self.stats.instance_uniform_bytes += bytes.len();
                binding.bytes = bytes;
            }
            batch.slot = Some(slot);
        }
        Ok(batches)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn runs_split_at_limits_culling_and_incompatible_surfaces() {
        let draw = || PreparedDraw {
            deformation: 0,
            pbr_override: [-1.; 2],
            shader: None,
            pbr: false,
            opacity: 1.,
            cutoff: 0.,
            transparent: false,
            depth: 0.,
            object: DrawItem {
                motion_id: 1,
                model: Mat4::IDENTITY,
                mesh: MeshKind::Cube,
                material: Material {
                    metallic: None,
                    roughness: None,
                    surface_overrides: Default::default(),
                    tint: [1.; 3],
                    uv_scale: [1.; 2],
                    texture: TextureKind::White,
                    lit: false,
                    shader: None,
                },
            },
        };
        let mut draws: Vec<_> = (0..67).map(|_| draw()).collect();
        let mut visible = vec![true; draws.len()];
        let sizes = |draws: &[PreparedDraw], visible: &[bool], enabled| {
            batches(draws, visible, enabled)
                .iter()
                .map(|b| b.range.len())
                .collect::<Vec<_>>()
        };
        assert_eq!(sizes(&draws, &visible, true), [32, 32, 3]);
        assert_eq!(sizes(&draws, &visible, false), vec![1; 67]);
        visible[32] = false;
        assert_eq!(sizes(&draws, &visible, true), [32, 32, 2]);
        for kind in 0..4 {
            let mut b = draw();
            match kind {
                0 => b.deformation = 1,
                1 => b.shader = Some(1),
                2 => b.transparent = true,
                _ => b.object.material.texture = TextureKind::Checker,
            }
            assert!(!compatible(&draws[0], &b));
            assert!(!compatible(&b, &draws[0]));
        }
        draws[1].deformation = 1;
        assert_eq!(&sizes(&draws, &visible, true)[..3], &[1, 1, 30]);
    }
    #[test]
    fn stock_instanced_shaders_validate_on_baseline_capabilities() {
        for pbr in [false, true] {
            let source = module_text(pbr);
            let module = wgpu::naga::front::wgsl::parse_str(&source)
                .unwrap_or_else(|e| panic!("{}", e.emit_to_string(&source)));
            wgpu::naga::valid::Validator::new(
                wgpu::naga::valid::ValidationFlags::all(),
                wgpu::naga::valid::Capabilities::empty(),
            )
            .validate(&module)
            .unwrap();
        }
    }
}
