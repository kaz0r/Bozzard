use super::*;

#[derive(PartialEq)]
struct Input {
    mesh: MeshKind,
    texture: Option<TextureKind>,
    deformation: u64,
    vertices: Option<wgpu::Buffer>,
    transparent: bool,
    lit: bool,
    excluded_graph: bool,
    coverage: shadows::ShadowCoverage,
}
impl Input {
    fn matches(
        &self,
        draw: &PreparedDraw,
        graphs: bool,
        coverage: shadows::ShadowCoverage,
    ) -> bool {
        self.mesh == draw.object.mesh
            && self.texture.as_ref()
                == (coverage == shadows::ShadowCoverage::Masked)
                    .then_some(&draw.object.material.texture)
            && self.vertices.as_ref()
                == draw
                    .shared_geometry
                    .as_ref()
                    .map(|geometry| &geometry.vertices)
            && (self.vertices.is_some() || self.deformation == draw.deformation)
            && self.transparent == draw.transparent
            && self.lit == draw.object.material.lit
            && self.excluded_graph == (!graphs && draw.shader.is_some())
            && self.coverage == coverage
    }
    fn new(draw: &PreparedDraw, graphs: bool, coverage: shadows::ShadowCoverage) -> Self {
        Self {
            mesh: draw.object.mesh.clone(),
            texture: (coverage == shadows::ShadowCoverage::Masked)
                .then(|| draw.object.material.texture.clone()),
            deformation: draw.deformation,
            vertices: draw.shared_geometry.as_ref().map(|g| g.vertices.clone()),
            transparent: draw.transparent,
            lit: draw.object.material.lit,
            excluded_graph: !graphs && draw.shader.is_some(),
            coverage,
        }
    }
}
pub(super) struct Plan {
    inputs: Vec<Input>,
    batches: Vec<Batch>,
    /// Native plans keep whole depth groups; `keyed` marks groups drawn from
    /// the native caster table (others are individual casters).
    native: bool,
    keyed: Vec<bool>,
}
/// Source membership and binding class determine the original accepted-run
/// count. Slot numbers and offsets do not affect that count; the replacement
/// stream owns independent bindings with zero-based instance ranges.
struct SubsetSource {
    accepted: Vec<bool>,
    batches: Vec<SubsetBatchSource>,
}
struct SubsetBatchSource {
    indices: Vec<usize>,
    instanced: bool,
}
impl SubsetSource {
    fn new(batches: &[Batch], accepted: &[bool]) -> Self {
        Self {
            accepted: accepted.to_vec(),
            batches: batches
                .iter()
                .map(|batch| SubsetBatchSource {
                    indices: batch.indices.clone(),
                    instanced: batch.slot.is_some(),
                })
                .collect(),
        }
    }
    fn matches(&self, batches: &[Batch], accepted: &[bool]) -> bool {
        self.accepted == accepted
            && self.batches.len() == batches.len()
            && self
                .batches
                .iter()
                .zip(batches)
                .all(|(a, b)| a.indices == b.indices && a.instanced == b.slot.is_some())
    }
}
/// Exact, retained Sun subset topology. Only accepted casters need depth-key
/// validation; byte comparisons cover the full mask and original member layout.
/// Storage contains at most the current source layout and accepted population.
pub(in crate::scene) struct SubsetPlan {
    source: SubsetSource,
    inputs: Vec<(usize, Input)>,
    pub batches: Vec<Batch>,
    pub saved: usize,
}
impl SubsetPlan {
    pub fn matches(
        &self,
        renderer: &SceneRenderer,
        draws: &[PreparedDraw],
        batches: &[Batch],
        accepted: &[bool],
    ) -> bool {
        draws.len() == accepted.len()
            && self.source.matches(batches, accepted)
            && self.inputs.iter().all(|(index, input)| {
                input.matches(
                    &draws[*index],
                    renderer.instancing.graph_enabled,
                    renderer.shadow_coverage_at(*index, &draws[*index]),
                )
            })
    }
}
#[derive(PartialEq, Eq, Hash)]
struct DepthKey<'a>(
    MeshKey<'a>,
    Option<&'a TextureKind>,
    shadows::ShadowCoverage,
    Option<&'a wgpu::Buffer>,
);
fn key(
    draw: &PreparedDraw,
    graphs: bool,
    coverage: shadows::ShadowCoverage,
) -> Option<DepthKey<'_>> {
    if draw.transparent
        || !draw.object.material.lit
        || (draw.deformation != 0 && draw.shared_geometry.is_none())
        || (!graphs && draw.shader.is_some())
    {
        return None;
    }
    let mesh = match &draw.object.mesh {
        MeshKind::Quad => MeshKey::Quad,
        MeshKind::Cube => MeshKey::Cube,
        MeshKind::Sphere => MeshKey::Sphere,
        MeshKind::Imported(id) => MeshKey::Imported(id),
        MeshKind::ModelPart(id, part) => MeshKey::Part(id, *part),
        _ => return None,
    };
    Some(DepthKey(
        mesh,
        (coverage == shadows::ShadowCoverage::Masked).then_some(&draw.object.material.texture),
        coverage,
        draw.shared_geometry
            .as_ref()
            .map(|geometry| &geometry.vertices),
    ))
}
fn groups(renderer: &SceneRenderer, draws: &[PreparedDraw], graphs: bool) -> Vec<Batch> {
    groups_filtered(renderer, draws, graphs, None, true, MAX_SHADOW_INSTANCES)
}
fn groups_filtered(
    renderer: &SceneRenderer,
    draws: &[PreparedDraw],
    graphs: bool,
    accepted: Option<&[bool]>,
    spatial: bool,
    capacity: usize,
) -> Vec<Batch> {
    let mut members: Vec<Vec<usize>> = Vec::new();
    // wgpu Buffer Eq/Hash use immutable handle identity, never storage contents.
    #[allow(clippy::mutable_key_type)]
    let mut groups: HashMap<DepthKey<'_>, usize> = HashMap::new();
    for (index, draw) in draws.iter().enumerate().filter(|(i, d)| {
        accepted.is_none_or(|mask| mask[*i]) && !d.transparent && d.object.material.lit
    }) {
        let key = key(draw, graphs, renderer.shadow_coverage_at(index, draw));
        if let Some(group) = key.as_ref().and_then(|key| groups.get(key)).copied() {
            members[group].push(index);
        } else {
            if let Some(key) = key {
                groups.insert(key, members.len());
            }
            members.push(vec![index]);
        }
    }
    let mut batches = Vec::new();
    for mut indices in members {
        if spatial && indices.len() > capacity {
            let centers: Vec<_> = indices
                .iter()
                .map(|&i| {
                    let draw = &draws[i];
                    let bounds = renderer.mesh_for(&draw.object).bounds;
                    draw.object
                        .model
                        .transform_point3((bounds[0] + bounds[1]) * 0.5)
                })
                .collect();
            spatial_order(&mut indices, &centers);
        }
        for indices in indices.chunks(capacity) {
            batches.push(Batch {
                indices: indices.to_vec(),
                slot: None,
                first_instance: 0,
            });
        }
    }
    batches
}
impl SceneRenderer {
    /// Reuse the certified depth key for a sun layer's accepted population.
    /// Keep source order; large full-scene spatial chunks must not fragment a
    /// small compatible subset. Unsupported casters remain singleton groups.
    pub(in crate::scene) fn shadow_subset_plan(
        &self,
        draws: &[PreparedDraw],
        source: &[Batch],
        accepted: &[bool],
    ) -> SubsetPlan {
        let batches = groups_filtered(
            self,
            draws,
            self.instancing.graph_enabled,
            Some(accepted),
            false,
            MAX_SHADOW_INSTANCES,
        );
        let saved =
            local_shadow_maps::compaction::subset_draws_saved(source, accepted, batches.len());
        let inputs = draws
            .iter()
            .enumerate()
            .filter(|(index, _)| accepted[*index])
            .map(|(index, draw)| {
                (
                    index,
                    Input::new(
                        draw,
                        self.instancing.graph_enabled,
                        self.shadow_coverage_at(index, draw),
                    ),
                )
            })
            .collect();
        SubsetPlan {
            source: SubsetSource::new(source, accepted),
            inputs,
            batches,
            saved,
        }
    }
}
// Stable one-axis ordering partitions large depth-compatible populations into
// compact neighborhoods before chunking. Nonfinite centers retain source order.
fn spatial_order(indices: &mut [usize], centers: &[Vec3]) {
    if !centers.iter().all(|c| c.is_finite()) {
        return;
    }
    let min = centers
        .iter()
        .copied()
        .fold(Vec3::splat(f32::INFINITY), Vec3::min);
    let max = centers
        .iter()
        .copied()
        .fold(Vec3::splat(f32::NEG_INFINITY), Vec3::max);
    let extent = max - min;
    let axis = if extent.x >= extent.y && extent.x >= extent.z {
        0
    } else if extent.y >= extent.z {
        1
    } else {
        2
    };
    let mut pairs: Vec<_> = indices
        .iter()
        .copied()
        .zip(centers.iter().map(|c| c[axis]))
        .collect();
    pairs.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
    for (index, (ordered, _)) in indices.iter_mut().zip(pairs) {
        *index = ordered;
    }
}
/// Copy established color bytes instead of recomputing floating-point fields.
fn uniform(color: &[u8; OBJECT_UNIFORM_BYTES]) -> [u8; SHADOW_UNIFORM_BYTES] {
    let mut result = [0; SHADOW_UNIFORM_BYTES];
    result[..64].copy_from_slice(&color[96..160]);
    result[64..72].copy_from_slice(&color[80..88]);
    result[72..76].copy_from_slice(&color[76..80]);
    result[76..80].copy_from_slice(&color[92..96]);
    result[80..88].copy_from_slice(&color[160..168]);
    result
}
fn layout(gpu: &Gpu) -> wgpu::BindGroupLayout {
    gpu.device
        .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("compact shadow object layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(SHADOW_BUFFER_BYTES as u64),
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        })
}
fn binding(
    renderer: &SceneRenderer,
    gpu: &Gpu,
    texture: &TextureKind,
    buffer: &wgpu::Buffer,
) -> Result<wgpu::BindGroup> {
    let sampler = match texture {
        TextureKind::ModelPart(id, index) => &renderer.models[id][*index].sampler,
        TextureKind::Text => &renderer.model_sampler,
        _ => &renderer.sampler,
    };
    Ok(gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("compact shadow object bindings"),
        layout: renderer.instancing.shadow_layout.as_ref().unwrap(),
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: buffer.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(renderer.texture_view(texture)?),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    }))
}
pub(super) fn prepare(
    renderer: &mut SceneRenderer,
    gpu: &Gpu,
    draws: &[PreparedDraw],
) -> Result<Vec<Batch>> {
    let graphs = renderer.instancing.graph_enabled;
    let native = renderer.native_shadows_active();
    let reused = renderer.instancing.shadow_plan.as_ref().is_some_and(|p| {
        p.native == native
            && p.inputs.len() == draws.len()
            && p.inputs
                .iter()
                .zip(draws)
                .enumerate()
                .all(|(index, (input, draw))| {
                    input.matches(draw, graphs, renderer.shadow_coverage_at(index, draw))
                })
    });
    if !reused {
        // Native groups are never split: one storage table serves any count.
        let batches = if native {
            groups_filtered(renderer, draws, graphs, None, false, usize::MAX)
        } else {
            groups(renderer, draws, graphs)
        };
        let keyed = batches
            .iter()
            .map(|batch| {
                let index = batch.indices[0];
                let draw = &draws[index];
                native && key(draw, graphs, renderer.shadow_coverage_at(index, draw)).is_some()
            })
            .collect();
        let mut inputs = renderer
            .instancing
            .shadow_plan
            .take()
            .map_or_else(Vec::new, |p| p.inputs);
        inputs.truncate(draws.len());
        for (index, draw) in draws.iter().enumerate() {
            let coverage = renderer.shadow_coverage_at(index, draw);
            if index == inputs.len() {
                inputs.push(Input::new(draw, graphs, coverage));
            } else if !inputs[index].matches(draw, graphs, coverage) {
                inputs[index] = Input::new(draw, graphs, coverage);
            }
        }
        renderer.instancing.shadow_plan = Some(Plan {
            inputs,
            batches,
            native,
            keyed,
        });
    }
    renderer.stats.shadow_batch_plan_reused = reused;
    let plan = renderer.instancing.shadow_plan.as_ref().unwrap();
    let mut output = std::mem::take(&mut renderer.instancing.shadow_frame_batches);
    for (index, batch) in plan.batches.iter().enumerate() {
        if index == output.len() {
            output.push(Batch {
                indices: Vec::new(),
                slot: None,
                first_instance: 0,
            });
        }
        output[index].indices.clear();
        output[index].indices.extend_from_slice(&batch.indices);
        output[index].slot = None;
    }
    output.truncate(plan.batches.len());
    if native {
        // Keyed groups address the native table by draw position; their
        // records are uploaded with the frame's ID streams.
        for (batch, &keyed) in output.iter_mut().zip(&plan.keyed) {
            batch.slot = keyed.then_some(0);
        }
        return Ok(output);
    }
    if renderer.instancing.shadow_layout.is_none() {
        renderer.instancing.shadow_layout = Some(layout(gpu));
    }
    let mut bindings = std::mem::take(&mut renderer.instancing.shadow_bindings);
    bindings.truncate(output.iter().filter(|b| b.indices.len() > 1).count() + 8);
    let result = prepare_bindings(renderer, gpu, draws, &mut output, &mut bindings);
    renderer.instancing.shadow_bindings = bindings;
    result?;
    Ok(output)
}
fn prepare_bindings(
    renderer: &mut SceneRenderer,
    gpu: &Gpu,
    draws: &[PreparedDraw],
    batches: &mut [Batch],
    bindings: &mut Vec<InstanceBinding>,
) -> Result<()> {
    for (slot, batch) in batches
        .iter_mut()
        .filter(|b| b.indices.len() > 1)
        .enumerate()
    {
        let texture = &draws[batch.indices[0]].object.material.texture;
        if slot == bindings.len() {
            let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("compact shadow instances"),
                size: SHADOW_BUFFER_BYTES as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let bind = binding(renderer, gpu, texture, &buffer)?;
            bindings.push(InstanceBinding {
                buffer,
                binding: bind,
                texture: texture.clone(),
                bytes: Vec::with_capacity(SHADOW_BUFFER_BYTES),
                revisions: Vec::new(),
                parameter_buffer: None,
                parameter_bytes: Vec::new(),
                parameter_revisions: Vec::new(),
                first_instance: 0,
            });
            renderer.stats.shadow_instance_buffer_allocations += 1;
        } else if bindings[slot].texture != *texture {
            bindings[slot].binding = binding(renderer, gpu, texture, &bindings[slot].buffer)?;
            bindings[slot].texture = texture.clone();
        }
        let current = &mut bindings[slot];
        let old_len = current.bytes.len();
        current
            .bytes
            .resize(batch.indices.len() * SHADOW_UNIFORM_BYTES, 0);
        current.revisions.resize(batch.indices.len(), 0);
        let mut changed_start = None;
        for (instance, &index) in batch.indices.iter().enumerate() {
            let start = instance * SHADOW_UNIFORM_BYTES;
            let end = start + SHADOW_UNIFORM_BYTES;
            let object = &renderer.objects[index];
            let changed = if !renderer.state_caching
                || start >= old_len
                || current.revisions[instance] != object.uniform_revision
            {
                let value = uniform(object.uniform.as_ref().unwrap());
                current.revisions[instance] = object.uniform_revision;
                if !renderer.state_caching || start >= old_len || current.bytes[start..end] != value
                {
                    current.bytes[start..end].copy_from_slice(&value);
                    true
                } else {
                    false
                }
            } else {
                false
            };
            if changed {
                changed_start.get_or_insert(start);
            } else if let Some(first) = changed_start.take() {
                gpu.queue
                    .write_buffer(&current.buffer, first as u64, &current.bytes[first..start]);
                renderer.stats.shadow_instance_uniform_bytes += start - first;
            }
        }
        if let Some(first) = changed_start {
            gpu.queue
                .write_buffer(&current.buffer, first as u64, &current.bytes[first..]);
            renderer.stats.shadow_instance_uniform_bytes += current.bytes.len() - first;
        }
        batch.slot = Some(slot);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn accepted_subset_certificate_tracks_mask_order_and_binding_class() {
        let accepted = [false, true, false, true];
        let mut batches = vec![Batch {
            indices: vec![2, 1, 0, 3],
            slot: Some(9),
            first_instance: 17,
        }];
        let source = SubsetSource::new(&batches, &accepted);
        assert!(source.matches(&batches, &accepted));
        // Synthetic streams own their binding and zero-based offsets. Physical
        // source slot/offset churn cannot change accepted-run admission.
        batches[0].slot = Some(4);
        batches[0].first_instance = 0;
        assert!(source.matches(&batches, &accepted));
        assert!(!source.matches(&batches, &[false, true, true, true]));
        batches[0].indices.swap(0, 1);
        assert!(!source.matches(&batches, &accepted));
        batches[0].indices.swap(0, 1);
        batches[0].slot = None;
        assert!(!source.matches(&batches, &accepted));
        batches[0].slot = Some(9);
        batches[0].indices.pop();
        assert!(!source.matches(&batches, &accepted));
        assert!(!source.matches(&[], &accepted));
    }
    #[test]
    fn accepted_subset_depth_inputs_validate_keys_without_transform_repacking() {
        let mut draw = PreparedDraw {
            preparation: Default::default(),
            source_item: 0,
            deformation: 0,
            shared_geometry: None,
            world_geometry_units: None,
            pbr_override: [-1.; 2],
            shader: None,
            pbr: false,
            raster: 0,
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
                    lit: true,
                    shader: None,
                },
            },
        };
        let masked = shadows::ShadowCoverage::Masked;
        let input = Input::new(&draw, true, masked);
        draw.object.model = Mat4::from_translation(Vec3::X);
        draw.opacity = 0.75;
        draw.cutoff = 0.5;
        assert!(input.matches(&draw, true, masked));
        draw.object.material.texture = TextureKind::Checker;
        assert!(!input.matches(&draw, true, masked));
        draw.object.material.texture = TextureKind::White;
        draw.shader = Some(123);
        assert!(input.matches(&draw, true, masked));
        assert!(!input.matches(&draw, false, masked));
        draw.shader = None;
        draw.deformation = 1;
        assert!(!input.matches(&draw, true, masked));
        draw.deformation = 0;
        draw.object.mesh = MeshKind::Sphere;
        assert!(!input.matches(&draw, true, masked));
        draw.object.mesh = MeshKind::Cube;
        draw.object.material.lit = false;
        assert!(!input.matches(&draw, true, masked));
        draw.object.material.lit = true;
        draw.transparent = true;
        assert!(!input.matches(&draw, true, masked));
        draw.transparent = false;
        assert!(!input.matches(&draw, true, shadows::ShadowCoverage::OpaqueCw));
        assert!(input.matches(&draw, true, masked));
    }
    #[test]
    fn compact_records_ignore_all_non_depth_color_fields() {
        let color = std::array::from_fn(|i| i as u8);
        let expected = uniform(&color);
        for index in 0..OBJECT_UNIFORM_BYTES {
            let depth = (96..160).contains(&index)
                || (80..88).contains(&index)
                || (76..80).contains(&index)
                || (92..96).contains(&index)
                || (160..168).contains(&index);
            let mut changed = color;
            changed[index] ^= 1;
            assert_eq!(uniform(&changed) != expected, depth, "byte {index}");
        }
        assert_eq!(SHADOW_BUFFER_BYTES, 16_320);
        const { assert!(SHADOW_BUFFER_BYTES <= 16 * 1024) };
    }
    #[test]
    fn depth_groups_partition_spatial_neighbors_before_capacity_chunks() {
        let mut indices: Vec<_> = (0..680).collect();
        let centers: Vec<_> = indices
            .iter()
            .map(|&i| Vec3::new((i % 4) as f32 * 100. + (i / 4) as f32 * 0.001, 0., 0.))
            .collect();
        spatial_order(&mut indices, &centers);
        assert_eq!(indices.len(), 680);
        for chunk in indices.chunks(MAX_SHADOW_INSTANCES) {
            assert!(chunk.iter().all(|&i| i % 4 == chunk[0] % 4));
        }
        let mut sorted = indices.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, (0..680).collect::<Vec<_>>());
    }
}
