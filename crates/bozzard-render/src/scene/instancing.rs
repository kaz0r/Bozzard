use super::*;
use std::collections::HashMap;

// Fits the downlevel 16 KiB uniform-binding limit without storage-buffer features.
pub(super) const MAX_INSTANCES: usize = 32;
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
    global: bool,
    plan: Option<Plan>,
    layout: wgpu::BindGroupLayout,
    pub pipelines: Option<Pipelines>,
    pub shadow_pipelines: Option<[wgpu::RenderPipeline; 2]>,
    pub bindings: Vec<InstanceBinding>,
}
#[derive(Clone)]
pub(super) struct Batch {
    pub indices: Vec<usize>,
    pub slot: Option<usize>,
}
impl Instancing {
    pub fn new(layout: wgpu::BindGroupLayout) -> Self {
        Self {
            enabled: true,
            global: true,
            plan: None,
            layout,
            pipelines: None,
            shadow_pipelines: None,
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
        if enabled
            && let Some(last) = result.last_mut()
            && last.indices.last() == index.checked_sub(1).as_ref()
            && last.indices.len() < MAX_INSTANCES
            && compatible(&draws[last.indices[0]], draw)
        {
            last.indices.push(index);
        } else {
            result.push(Batch {
                indices: vec![index],
                slot: None,
            });
        }
    }
    result
}

#[derive(Clone, PartialEq, Eq, Hash)]
enum MeshKey {
    Quad,
    Cube,
    Sphere,
    Imported(String),
    Part(String, usize),
}
#[derive(PartialEq, Eq, Hash)]
struct Key(MeshKey, TextureKind, bool);
fn key(draw: &PreparedDraw) -> Option<Key> {
    if draw.transparent || draw.shader.is_some() || draw.deformation != 0 {
        return None;
    }
    let mesh = match &draw.object.mesh {
        MeshKind::Quad => MeshKey::Quad,
        MeshKind::Cube => MeshKey::Cube,
        MeshKind::Sphere => MeshKey::Sphere,
        MeshKind::Imported(id) => MeshKey::Imported(id.clone()),
        MeshKind::ModelPart(id, part) => MeshKey::Part(id.clone(), *part),
        _ => return None,
    };
    Some(Key(mesh, draw.object.material.texture.clone(), draw.pbr))
}

struct Input {
    mesh: MeshKind,
    texture: TextureKind,
    model: Mat4,
    bounds: [Vec3; 2],
    shader: Option<u64>,
    deformation: u64,
    pbr: bool,
    transparent: bool,
    visible: bool,
}
impl Input {
    fn matches(&self, draw: &PreparedDraw, bounds: [Vec3; 2], visible: bool) -> bool {
        self.visible == visible
            && self.model == draw.object.model
            && self.bounds == bounds
            && self.mesh == draw.object.mesh
            && self.texture == draw.object.material.texture
            && self.shader == draw.shader
            && self.deformation == draw.deformation
            && self.pbr == draw.pbr
            && self.transparent == draw.transparent
    }
}
struct Plan {
    camera: Mat4,
    inputs: Vec<Input>,
    batches: Vec<Batch>,
}

// An opaque reorder can change an equal-depth winner. Retain original order for
// intersecting projected bounds (including their depth intervals). Disjoint
// bounds cannot cover the same sample at equal depth. Near-plane crossings use
// unbounded boxes, so uncertain projections always retain their dependencies.
fn projected_bounds(bounds: [Vec3; 2], matrix: Mat4) -> [Vec3; 2] {
    let mut result = [Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)];
    for corner in 0..8 {
        let point = matrix
            * Vec3::new(
                bounds[(corner & 1) as usize].x,
                bounds[((corner >> 1) & 1) as usize].y,
                bounds[((corner >> 2) & 1) as usize].z,
            )
            .extend(1.);
        if !point.is_finite() || point.w <= 1e-6 {
            return [Vec3::splat(f32::NEG_INFINITY), Vec3::splat(f32::INFINITY)];
        }
        let point = point.truncate() / point.w;
        result[0] = result[0].min(point);
        result[1] = result[1].max(point);
    }
    // Cover projection roundoff and Depth32Float quantization conservatively.
    let padding = Vec3::splat(2e-5);
    [result[0] - padding, result[1] + padding]
}

fn enqueue(
    index: usize,
    group: usize,
    queues: &mut [BTreeSet<usize>],
    ready: &mut BTreeSet<(usize, usize)>,
) {
    if let Some(&first) = queues[group].first() {
        ready.remove(&(first, group));
    }
    queues[group].insert(index);
    ready.insert((*queues[group].first().unwrap(), group));
}

fn global_batches(draws: &[PreparedDraw], inputs: &[Input], camera: Mat4) -> Vec<Batch> {
    let mut groups = HashMap::new();
    let mut group_of = vec![0; draws.len()];
    let mut queues: Vec<BTreeSet<usize>> = Vec::new();
    let mut sweep = Vec::new();
    let mut boxes = vec![[Vec3::ZERO; 2]; draws.len()];
    let mut world_boxes = vec![[Vec3::ZERO; 2]; draws.len()];
    // In an orthographic view, coincident screen/depth samples map to the same
    // world point. Disjoint world bounds can therefore remove false projected
    // overlaps. Expand by the inverse camera's NDC-roundoff footprint; retain
    // projected-only ordering for perspective cameras and uncertain inverses.
    let inverse = camera.inverse();
    let world_padding = (camera.x_axis.w == 0.
        && camera.y_axis.w == 0.
        && camera.z_axis.w == 0.
        && inverse.is_finite()
        && inverse.w_axis.w.abs() > 1e-6)
        .then(|| {
            (inverse.x_axis.truncate().abs()
                + inverse.y_axis.truncate().abs()
                + inverse.z_axis.truncate().abs())
                * (2e-5 / inverse.w_axis.w.abs())
        });
    for (index, (draw, input)) in draws.iter().zip(inputs).enumerate() {
        if !input.visible || draw.transparent {
            continue;
        }
        let group = key(draw).and_then(|key| groups.get(&key).copied());
        group_of[index] = group.unwrap_or_else(|| {
            let group = queues.len();
            queues.push(BTreeSet::new());
            if let Some(key) = key(draw) {
                groups.insert(key, group);
            }
            group
        });
        boxes[index] = projected_bounds(input.bounds, camera * input.model);
        if let Some(padding) = world_padding {
            let bounds = projected_bounds(input.bounds, input.model);
            world_boxes[index] = [bounds[0] - padding, bounds[1] + padding];
        }
        sweep.push(index);
    }
    sweep.sort_by(|&a, &b| boxes[a][0].x.total_cmp(&boxes[b][0].x).then(a.cmp(&b)));
    let mut followers = vec![Vec::new(); draws.len()];
    let mut pending = vec![0usize; draws.len()];
    let mut active: Vec<usize> = Vec::new();
    for &index in &sweep {
        let bounds = boxes[index];
        active.retain(|&other| boxes[other][1].x >= bounds[0].x);
        for &other in &active {
            let previous = boxes[other];
            if previous[1].y < bounds[0].y
                || bounds[1].y < previous[0].y
                || previous[1].z < bounds[0].z
                || bounds[1].z < previous[0].z
            {
                continue;
            }
            if world_padding.is_some() {
                let a = world_boxes[index];
                let b = world_boxes[other];
                if a[1].cmplt(b[0]).any() || b[1].cmplt(a[0]).any() {
                    continue;
                }
            }
            let (before, after) = (index.min(other), index.max(other));
            followers[before].push(after);
            pending[after] += 1;
        }
        active.push(index);
    }
    let mut ready = BTreeSet::new();
    for &index in &sweep {
        if pending[index] == 0 {
            enqueue(index, group_of[index], &mut queues, &mut ready);
        }
    }
    let mut result = Vec::new();
    while let Some(&(_, group)) = ready.first() {
        let mut indices = Vec::new();
        while indices.len() < MAX_INSTANCES {
            let Some(index) = queues[group].pop_first() else {
                break;
            };
            ready.remove(&(index, group));
            if let Some(&next) = queues[group].first() {
                ready.insert((next, group));
            }
            indices.push(index);
            for &next in &followers[index] {
                pending[next] -= 1;
                if pending[next] == 0 {
                    enqueue(next, group_of[next], &mut queues, &mut ready);
                }
            }
        }
        result.push(Batch {
            indices,
            slot: None,
        });
    }
    // Transparent items retain their original back-to-front order and single draws.
    for (index, draw) in draws.iter().enumerate() {
        if inputs[index].visible && draw.transparent {
            result.push(Batch {
                indices: vec![index],
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

    /// Compare scene-wide grouping with the former consecutive-run batcher.
    pub fn set_global_batching_enabled(&mut self, enabled: bool) {
        self.instancing.global = enabled;
        self.instancing.plan = None;
    }

    pub(super) fn prepare_instances(
        &mut self,
        gpu: &Gpu,
        draws: &[PreparedDraw],
        visible: &[bool],
        camera: Mat4,
    ) -> Result<Vec<Batch>> {
        let mut batches = if self.instancing.enabled && self.instancing.global {
            let unchanged = self.instancing.plan.as_ref().is_some_and(|plan| {
                plan.camera == camera
                    && plan.inputs.len() == draws.len()
                    && plan.inputs.iter().zip(draws).zip(visible).all(
                        |((input, draw), &visible)| {
                            input.matches(draw, self.mesh_for(&draw.object).bounds, visible)
                        },
                    )
            });
            if !unchanged {
                let inputs = draws
                    .iter()
                    .zip(visible)
                    .map(|(draw, &visible)| Input {
                        mesh: draw.object.mesh.clone(),
                        texture: draw.object.material.texture.clone(),
                        model: draw.object.model,
                        bounds: self.mesh_for(&draw.object).bounds,
                        shader: draw.shader,
                        deformation: draw.deformation,
                        pbr: draw.pbr,
                        transparent: draw.transparent,
                        visible,
                    })
                    .collect::<Vec<_>>();
                self.instancing.plan = Some(Plan {
                    camera,
                    batches: global_batches(draws, &inputs, camera),
                    inputs,
                });
            }
            self.instancing.plan.as_ref().unwrap().batches.clone()
        } else {
            batches(draws, visible, self.instancing.enabled)
        };
        let count = batches.iter().filter(|b| b.indices.len() > 1).count();
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
        for (slot, batch) in batches
            .iter_mut()
            .filter(|b| b.indices.len() > 1)
            .enumerate()
        {
            let texture = &draws[batch.indices[0]].object.material.texture;
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
                    bytes: Vec::with_capacity(BUFFER_BYTES),
                };
                if slot == self.instancing.bindings.len() {
                    self.instancing.bindings.push(value);
                } else {
                    self.instancing.bindings[slot] = value;
                }
            }
            let binding = &mut self.instancing.bindings[slot];
            let uniforms = batch.indices.iter().map(|&index| &self.objects[index]);
            let unchanged = binding.bytes.len() == batch.indices.len() * OBJECT_UNIFORM_BYTES
                && uniforms
                    .clone()
                    .zip(binding.bytes.chunks_exact(OBJECT_UNIFORM_BYTES))
                    .all(|(object, bytes)| object.uniform.as_ref().unwrap().as_slice() == bytes);
            if !self.state_caching || !unchanged {
                binding.bytes.clear();
                for object in uniforms {
                    binding
                        .bytes
                        .extend_from_slice(object.uniform.as_ref().unwrap());
                }
                gpu.queue.write_buffer(&binding.buffer, 0, &binding.bytes);
                self.stats.instance_uniform_bytes += binding.bytes.len();
            }
            batch.slot = Some(slot);
        }
        Ok(batches)
    }

    pub(super) fn prepare_instanced_shadows(&mut self, gpu: &Gpu) {
        if self.instancing.shadow_pipelines.is_none() {
            self.instancing.shadow_pipelines = Some(std::array::from_fn(|point| {
                shadows::pipeline(
                    gpu,
                    &self.instancing.layout,
                    &self.shadows.caster_layout,
                    true,
                    point == 1,
                )
            }));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn runs_split_at_limits_culling_and_incompatible_surfaces() {
        let draw = || PreparedDraw {
            source_item: 0,
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
                .map(|b| b.indices.len())
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
