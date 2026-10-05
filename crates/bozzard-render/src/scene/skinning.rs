//! GPU deformation shared by PBR, depth, shadows and temporal motion. Sources are uploaded once;
//! instances retain buffers, and unchanged palettes skip compute work.
use super::*;
use std::sync::Arc;

pub struct SkinData<'a> {
    pub signature: u64,
    pub bindings: usize,
    pub vertices: &'a [[u32; 8]],
}
#[derive(Clone, Debug, PartialEq)]
pub struct SkinPose {
    pub signature: u64,
    pub matrices: Arc<Vec<[f32; 16]>>,
}
pub(super) struct Source {
    pub signature: u64,
    pub bindings: usize,
    pub weights: wgpu::Buffer,
    pub count: usize,
    pub bounds: Vec<[Vec3; 2]>,
    pub part_bounds: Vec<Vec<[Vec3; 2]>>,
}
impl Source {
    pub fn bounds(skin: &SkinData<'_>, vertices: &[[f32; 8]]) -> Result<Vec<[Vec3; 2]>> {
        ensure!(
            skin.vertices.len() == vertices.len() && skin.bindings > 0 && skin.bindings <= 4096,
            "invalid uploaded skin size"
        );
        let mut bounds =
            vec![[Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)]; skin.bindings];
        for (v, influence) in vertices.iter().zip(skin.vertices) {
            let p = Vec3::from_slice(&v[..3]);
            let mut sum = 0.;
            for axis in 0..4 {
                let joint = influence[axis] as usize;
                let weight = f32::from_bits(influence[axis + 4]);
                ensure!(
                    joint < skin.bindings && weight.is_finite() && weight >= 0.,
                    "invalid uploaded skin influence"
                );
                sum += weight;
                if weight > 0. {
                    bounds[joint][0] = bounds[joint][0].min(p);
                    bounds[joint][1] = bounds[joint][1].max(p);
                }
            }
            ensure!(
                (sum - 1.).abs() < 0.001,
                "uploaded skin weights must sum to one"
            );
        }
        Ok(bounds)
    }
    /// Authored indexed part bounds are accumulated once at upload. The union
    /// of transformed positive-weight joint boxes contains every weighted
    /// position, so posing these boxes avoids rescanning vertices each frame.
    pub fn part_bounds(
        skin: &SkinData<'_>,
        vertices: &[[f32; 8]],
        indices: &[u32],
        parts: &[ModelPart<'_>],
    ) -> Result<Vec<Vec<[Vec3; 2]>>> {
        parts
            .iter()
            .map(|part| {
                let start = part.start as usize;
                let end = start
                    .checked_add(part.count as usize)
                    .context("skin part range")?;
                let mut bounds = vec![
                    [Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)];
                    skin.bindings
                ];
                for &index in indices.get(start..end).context("skin part indices")? {
                    let point = Vec3::from_slice(
                        &vertices.get(index as usize).context("skin part vertex")?[..3],
                    );
                    let influence = skin
                        .vertices
                        .get(index as usize)
                        .context("skin part influence")?;
                    for axis in 0..4 {
                        let joint = influence[axis] as usize;
                        let weight = f32::from_bits(influence[axis + 4]);
                        ensure!(
                            joint < skin.bindings && weight.is_finite() && weight >= 0.,
                            "skin part influence"
                        );
                        if weight > 0. {
                            bounds[joint][0] = bounds[joint][0].min(point);
                            bounds[joint][1] = bounds[joint][1].max(point);
                        }
                    }
                }
                Ok(bounds)
            })
            .collect()
    }
}
struct DeformedBounds {
    whole: [Vec3; 2],
    parts: Vec<[Vec3; 2]>,
}
struct Tangents {
    output: wgpu::Buffer,
    binding: wgpu::BindGroup,
    count: u32,
}
struct Instance {
    palette: wgpu::Buffer,
    snapshot: Option<SkinPose>,
    meshes: Vec<MeshBuffers>,
    output: wgpu::Buffer,
    previous: wgpu::Buffer,
    shared_previous: Option<wgpu::Buffer>,
    binding: wgpu::BindGroup,
    tangents: Vec<Option<Tangents>>,
    shared_tangents: Option<Vec<Option<wgpu::Buffer>>>,
    revision: u64,
    /// Copy the final moving pose to history on the first stationary frame.
    moved: bool,
    hidden_pose: Option<SkinPose>,
}
#[derive(Default)]
pub(super) struct Skinning {
    pub sources: BTreeMap<String, Source>,
    instances: BTreeMap<String, BTreeMap<u64, Instance>>,
    pipeline: Option<(
        wgpu::BindGroupLayout,
        wgpu::ComputePipeline,
        wgpu::ComputePipeline,
    )>,
    revision: u64,
    pub dispatches: usize,
    pub shared_copies: usize,
    pub shared_actors: usize,
    pub culled_actors: usize,
    disabled: bool,
    bounds_cache: BTreeMap<(String, usize), (SkinPose, Arc<DeformedBounds>)>,
}
impl Skinning {
    pub fn remove(&mut self, id: &str) {
        self.sources.remove(id);
        self.instances.remove(id);
        self.bounds_cache.retain(|(asset, _), _| asset != id);
    }
    fn pipeline(&mut self, gpu: &Gpu) {
        if self.pipeline.is_some() {
            return;
        }
        let entries: Vec<_> = (0..5)
            .map(|binding| wgpu::BindGroupLayoutEntry {
                binding,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: if binding == 4 {
                        wgpu::BufferBindingType::Uniform
                    } else {
                        wgpu::BufferBindingType::Storage {
                            read_only: binding != 3,
                        }
                    },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            })
            .collect();
        let layout = gpu
            .device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("GPU skinning layout"),
                entries: &entries,
            });
        let pipeline_layout = gpu
            .device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("GPU skinning"),
                bind_group_layouts: &[Some(&layout)],
                immediate_size: 0,
            });
        let shader = gpu
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("GPU skinning"),
                source: wgpu::ShaderSource::Wgsl(include_str!("skinning.wgsl").into()),
            });
        let pipeline = |entry| {
            gpu.device
                .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                    label: Some(entry),
                    layout: Some(&pipeline_layout),
                    module: &shader,
                    entry_point: Some(entry),
                    compilation_options: Default::default(),
                    cache: None,
                })
        };
        self.pipeline = Some((layout, pipeline("positions"), pipeline("tangents")));
    }
    fn instance(&self, gpu: &Gpu, source: &Source, parts: &[UploadedPart]) -> Instance {
        let storage = |label, size, usage| {
            gpu.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size,
                usage: usage | wgpu::BufferUsages::STORAGE,
                mapped_at_creation: false,
            })
        };
        let vertices = storage(
            "skinned vertices",
            source.count as u64 * 32,
            wgpu::BufferUsages::VERTEX
                | wgpu::BufferUsages::COPY_SRC
                | wgpu::BufferUsages::COPY_DST,
        );
        let previous = storage(
            "previous skinned vertices",
            source.count as u64 * 32,
            wgpu::BufferUsages::VERTEX
                | wgpu::BufferUsages::COPY_DST
                | wgpu::BufferUsages::COPY_SRC,
        );
        let palette = storage(
            "skin matrix palette",
            source.bindings as u64 * 64,
            wgpu::BufferUsages::COPY_DST,
        );
        let bind = |tangent: &wgpu::Buffer, output: &wgpu::Buffer, count: u32, offset: u32| {
            let params = gpu
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("skin dispatch range"),
                    contents: &[count, offset, 0, 0]
                        .into_iter()
                        .flat_map(u32::to_le_bytes)
                        .collect::<Vec<_>>(),
                    usage: wgpu::BufferUsages::UNIFORM,
                });
            let buffers = [tangent, &source.weights, &palette, output, &params];
            gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("skin dispatch"),
                layout: &self.pipeline.as_ref().unwrap().0,
                entries: &buffers
                    .iter()
                    .enumerate()
                    .map(|(i, b)| wgpu::BindGroupEntry {
                        binding: i as u32,
                        resource: b.as_entire_binding(),
                    })
                    .collect::<Vec<_>>(),
            })
        };
        let binding = bind(&parts[0].mesh.vertices, &vertices, source.count as u32, 0);
        let tangents = parts
            .iter()
            .map(|p| {
                p.shading.as_ref().map(|s| {
                    let output = storage(
                        "skinned PBR tangents",
                        s.vertices.size(),
                        wgpu::BufferUsages::VERTEX
                            | wgpu::BufferUsages::COPY_SRC
                            | wgpu::BufferUsages::COPY_DST,
                    );
                    let count = (s.vertices.size() / 48) as u32;
                    Tangents {
                        binding: bind(
                            &s.vertices,
                            &output,
                            count,
                            (p.mesh.vertex_offset / 32) as u32,
                        ),
                        output,
                        count,
                    }
                })
            })
            .collect();
        let meshes = parts
            .iter()
            .map(|p| MeshBuffers {
                vertices: vertices.clone(),
                indices: p.mesh.indices.clone(),
                count: p.mesh.count,
                bounds: p.mesh.bounds,
                vertex_offset: p.mesh.vertex_offset,
            })
            .collect();
        Instance {
            palette,
            snapshot: None,
            meshes,
            output: vertices,
            previous,
            shared_previous: None,
            binding,
            tangents,
            shared_tangents: None,
            revision: 0,
            moved: false,
            hidden_pose: None,
        }
    }
    pub fn prepare(
        &mut self,
        gpu: &Gpu,
        scene: &RenderScene,
        models: &BTreeMap<String, Vec<UploadedPart>>,
        encoder: &mut crate::profiling::Encoder,
        view_projection: Mat4,
        culling: bool,
    ) -> Result<()> {
        self.dispatches = 0;
        self.shared_copies = 0;
        self.shared_actors = 0;
        self.culled_actors = 0;
        let mut active: BTreeMap<String, BTreeSet<u64>> = BTreeMap::new();
        if scene.skin_poses.is_empty() {
            self.instances.clear();
            return Ok(());
        }
        self.pipeline(gpu);
        let mut shadow_views = Vec::new();
        for light in scene.lights.iter().filter(|light| light.casts_shadow()) {
            if light.spot_angles.is_some() {
                shadow_views.push(spot_shadows::projection(light)?);
            } else {
                shadow_views.extend(point_shadows::projections(light)?);
            }
        }
        let mut pose_bounds: BTreeMap<(String, usize), Arc<DeformedBounds>> = BTreeMap::new();
        let mut required = BTreeSet::new();
        for item in &scene.items {
            let Some(pose) = scene.skin_poses.get(&item.motion_id) else {
                continue;
            };
            let (MeshKind::Imported(asset) | MeshKind::ModelPart(asset, _)) = &item.mesh else {
                continue;
            };
            let Some(source) = self.sources.get(asset) else {
                continue;
            };
            ensure!(
                pose.signature == source.signature && pose.matrices.len() == source.bindings,
                "cooked rig no longer matches the model; recook the Animator rig"
            );
            let key = (asset.clone(), Arc::as_ptr(&pose.matrices) as usize);
            let bounds = if let Some(bounds) = pose_bounds.get(&key) {
                bounds.clone()
            } else if let Some((old, bounds)) = self.bounds_cache.get(&key).filter(|(old, _)| {
                old.signature == pose.signature && Arc::ptr_eq(&old.matrices, &pose.matrices)
            }) {
                let _ = old;
                let bounds = bounds.clone();
                pose_bounds.insert(key.clone(), bounds.clone());
                bounds
            } else {
                ensure!(
                    pose.matrices.iter().flatten().all(|v| v.is_finite()),
                    "invalid skin pose"
                );
                let bounds = Arc::new(DeformedBounds {
                    whole: pose_bounds_for(&source.bounds, pose)?,
                    parts: source
                        .part_bounds
                        .iter()
                        .map(|joints| pose_bounds_for(joints, pose))
                        .collect::<Result<_>>()?,
                });
                pose_bounds.insert(key.clone(), bounds.clone());
                self.bounds_cache
                    .insert(key, (pose.clone(), bounds.clone()));
                bounds
            };
            let selected_bounds = match item.mesh {
                MeshKind::ModelPart(_, part) => {
                    bounds.parts.get(part).copied().unwrap_or(bounds.whole)
                }
                _ => bounds.whole,
            };
            let casts = item.material.lit
                && ((scene.lighting.shadows && scene.lighting.sun_intensity > 0.)
                    || shadow_views.iter().any(|projection| {
                        visibility::visible(selected_bounds, *projection * item.model)
                    }));
            if self.disabled
                || !culling
                || casts
                || visibility::visible(selected_bounds, view_projection * item.model)
            {
                required.insert((asset.clone(), item.motion_id));
            }
        }
        self.bounds_cache
            .retain(|key, _| pose_bounds.contains_key(key));
        // Preserve all actors' prior geometry before an owner overwrites its
        // private compute target. Equal physical current buffers may share the
        // resulting history stream; different reentry histories remain private.
        let mut histories: Vec<(wgpu::Buffer, wgpu::Buffer)> = Vec::new();
        let mut copied_history = BTreeSet::new();
        for item in &scene.items {
            let (MeshKind::Imported(asset) | MeshKind::ModelPart(asset, _)) = &item.mesh else {
                continue;
            };
            let Some(pose) = scene.skin_poses.get(&item.motion_id) else {
                continue;
            };
            if !required.contains(&(asset.clone(), item.motion_id))
                || !copied_history.insert((asset.clone(), item.motion_id))
            {
                continue;
            }
            let Some(instance) = self
                .instances
                .get_mut(asset)
                .and_then(|actors| actors.get_mut(&item.motion_id))
            else {
                continue;
            };
            if instance.hidden_pose.is_some() || instance.snapshot.is_none() {
                continue;
            }
            let changed = !same_pose(instance.snapshot.as_ref().unwrap(), pose);
            if changed || instance.moved {
                let current = &instance.meshes[0].vertices;
                if !self.disabled
                    && let Some((_, previous)) =
                        histories.iter().find(|(source, _)| source == current)
                {
                    instance.shared_previous = Some(previous.clone());
                } else {
                    encoder.copy_buffer_to_buffer(
                        current,
                        0,
                        &instance.previous,
                        0,
                        current.size(),
                    );
                    instance.shared_previous = None;
                    histories.push((current.clone(), instance.previous.clone()));
                }
                instance.moved = false;
            }
        }
        let mut shared_outputs: BTreeMap<(String, usize), (u64, wgpu::Buffer)> = BTreeMap::new();
        let mut fresh_histories: Vec<(wgpu::Buffer, wgpu::Buffer)> = Vec::new();
        for item in &scene.items {
            let Some(pose) = scene.skin_poses.get(&item.motion_id) else {
                continue;
            };
            let (MeshKind::Imported(asset) | MeshKind::ModelPart(asset, _)) = &item.mesh else {
                continue;
            };
            let Some(parts) = models.get(asset) else {
                continue;
            };
            let source = self
                .sources
                .get(asset)
                .context("animated mesh lacks GPU skin data; reimport the model")?;
            ensure!(
                pose.signature == source.signature && pose.matrices.len() == source.bindings,
                "cooked rig no longer matches the model; recook the Animator rig"
            );
            if !active
                .entry(asset.clone())
                .or_default()
                .insert(item.motion_id)
            {
                continue;
            }
            if !self
                .instances
                .get(asset)
                .is_some_and(|m| m.contains_key(&item.motion_id))
            {
                let instance = self.instance(gpu, source, parts);
                self.instances
                    .entry(asset.clone())
                    .or_default()
                    .insert(item.motion_id, instance);
            }
            if !required.contains(&(asset.clone(), item.motion_id)) {
                self.culled_actors += 1;
                let instance = self
                    .instances
                    .get_mut(asset)
                    .unwrap()
                    .get_mut(&item.motion_id)
                    .unwrap();
                instance.hidden_pose = Some(pose.clone());
                let bounds = &pose_bounds[&(asset.clone(), Arc::as_ptr(&pose.matrices) as usize)];
                for (part, mesh) in instance.meshes.iter_mut().enumerate() {
                    mesh.bounds = bounds.parts.get(part).copied().unwrap_or(bounds.whole);
                }
                continue;
            }
            let instance = self
                .instances
                .get_mut(asset)
                .unwrap()
                .get_mut(&item.motion_id)
                .unwrap();
            let size = source.count as u64 * 32;
            let bounds = &pose_bounds[&(asset.clone(), Arc::as_ptr(&pose.matrices) as usize)];
            for (part, mesh) in instance.meshes.iter_mut().enumerate() {
                mesh.bounds = bounds.parts.get(part).copied().unwrap_or(bounds.whole);
            }
            let mut restored_history = false;
            if let Some(hidden) = instance.hidden_pose.take()
                && (scene.display.temporal_aa.enabled || scene.display.motion_blur.enabled)
            {
                restore_previous(
                    gpu,
                    &self.pipeline.as_ref().unwrap().0,
                    &self.pipeline.as_ref().unwrap().1,
                    source,
                    parts,
                    instance,
                    &hidden,
                    encoder,
                );
                self.dispatches += 1;
                instance.shared_previous = None;
                instance.moved = true;
                restored_history = true;
            }
            let shared_key = (asset.clone(), Arc::as_ptr(&pose.matrices) as usize);
            // A retained follower's physical buffer belongs to another actor
            // and may be overwritten with a different palette this frame.
            // Only private outputs or already certified current outputs may
            // take the unchanged-palette shortcut. Otherwise reconstruct or
            // share the requested pose before the rendering passes begin.
            let safe_current = instance.meshes[0].vertices == instance.output
                || shared_outputs
                    .get(&shared_key)
                    .is_some_and(|(_, vertices)| *vertices == instance.meshes[0].vertices);
            let unchanged_pose = instance
                .snapshot
                .as_ref()
                .is_some_and(|old| same_pose(old, pose));
            if safe_current && unchanged_pose {
                if !restored_history {
                    // Its prior pose equals the certified current pose. A
                    // previous buffer borrowed from another actor may change
                    // next frame after their current poses have diverged.
                    instance.shared_previous = Some(instance.meshes[0].vertices.clone());
                    instance.moved = false;
                }
                shared_outputs.insert(
                    shared_key,
                    (item.motion_id, instance.meshes[0].vertices.clone()),
                );
                continue;
            }
            // An immutable retained palette was already validated. Only inspect new data.
            ensure!(
                pose.matrices.iter().flatten().all(|v| v.is_finite()),
                "invalid skin pose"
            );
            let first = instance.snapshot.is_none();
            let shared = (!self.disabled)
                .then(|| shared_outputs.get(&shared_key).map(|(owner, _)| *owner))
                .flatten();
            if shared.is_none() {
                gpu.queue.write_buffer(
                    &instance.palette,
                    0,
                    &float_bytes(pose.matrices.iter().flatten().copied()),
                );
            }
            if let Some(owner) = shared {
                // The current palette is immutable, so its exact physical
                // position/tangent streams can be referenced by every actor.
                let instances = self.instances.get(asset).unwrap();
                let owner = &instances[&owner];
                let vertices = owner.meshes[0].vertices.clone();
                let tangents = (0..owner.tangents.len())
                    .map(|part| current_tangent(owner, part).cloned())
                    .collect();
                let instance = self
                    .instances
                    .get_mut(asset)
                    .unwrap()
                    .get_mut(&item.motion_id)
                    .unwrap();
                for mesh in &mut instance.meshes {
                    mesh.vertices = vertices.clone();
                }
                instance.shared_tangents = Some(tangents);
                self.shared_actors += 1;
            } else {
                // This actor owns the dispatch target this frame. Restore its
                // private handles before exposing the newly computed geometry.
                for mesh in &mut instance.meshes {
                    mesh.vertices = instance.output.clone();
                }
                instance.shared_tangents = None;
                let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some("skin geometry and PBR tangents"),
                    timestamp_writes: None,
                });
                pass.set_pipeline(&self.pipeline.as_ref().unwrap().1);
                pass.set_bind_group(0, &instance.binding, &[]);
                pass.dispatch_workgroups((source.count as u32).div_ceil(64), 1, 1);
                self.dispatches += 1;
                pass.set_pipeline(&self.pipeline.as_ref().unwrap().2);
                for tangent in instance.tangents.iter().flatten() {
                    pass.set_bind_group(0, &tangent.binding, &[]);
                    pass.dispatch_workgroups(tangent.count.div_ceil(64), 1, 1);
                    self.dispatches += 1;
                }
            }
            let instance = self
                .instances
                .get_mut(asset)
                .unwrap()
                .get_mut(&item.motion_id)
                .unwrap();
            if first && !restored_history {
                let current = &instance.meshes[0].vertices;
                if !self.disabled
                    && let Some((_, previous)) =
                        fresh_histories.iter().find(|(source, _)| source == current)
                {
                    instance.shared_previous = Some(previous.clone());
                } else {
                    encoder.copy_buffer_to_buffer(current, 0, &instance.previous, 0, size);
                    instance.shared_previous = None;
                    fresh_histories.push((current.clone(), instance.previous.clone()));
                }
            }
            let bounds = &pose_bounds[&shared_key];
            for (part, mesh) in instance.meshes.iter_mut().enumerate() {
                mesh.bounds = bounds.parts.get(part).copied().unwrap_or(bounds.whole);
            }
            self.revision = self.revision.wrapping_add(1);
            instance.revision = self.revision;
            instance.snapshot = Some(pose.clone());
            instance.moved = !first || restored_history;
            if unchanged_pose && !restored_history {
                instance.shared_previous = Some(instance.meshes[0].vertices.clone());
                instance.moved = false;
            }
            shared_outputs.insert(
                shared_key,
                (item.motion_id, instance.meshes[0].vertices.clone()),
            );
        }
        self.instances.retain(|asset, instances| {
            let Some(ids) = active.get(asset) else {
                return false;
            };
            instances.retain(|id, _| ids.contains(id));
            !instances.is_empty()
        });
        Ok(())
    }
    pub fn invalidate(&mut self) {
        self.bounds_cache.clear();
        for instances in self.instances.values_mut() {
            for instance in instances.values_mut() {
                instance.snapshot = None;
            }
        }
    }
    fn get(&self, item: &DrawItem) -> Option<(&Instance, usize)> {
        let MeshKind::ModelPart(asset, index) = &item.mesh else {
            return None;
        };
        self.instances
            .get(asset)?
            .get(&item.motion_id)
            .map(|i| (i, *index))
    }
    pub fn mesh(&self, item: &DrawItem) -> Option<&MeshBuffers> {
        let (i, p) = self.get(item)?;
        i.meshes.get(p)
    }
    pub fn previous(&self, item: &DrawItem) -> Option<&wgpu::Buffer> {
        self.get(item).map(|(i, _)| previous_buffer(i))
    }
    pub fn tangents(&self, item: &DrawItem) -> Option<&wgpu::Buffer> {
        let (i, p) = self.get(item)?;
        current_tangent(i, p)
    }
    pub fn revision(&self, item: &DrawItem) -> u64 {
        self.get(item).map_or(0, |(i, _)| i.revision)
    }
    pub(super) fn instance_geometry(
        &self,
        item: &DrawItem,
        motion_required: bool,
    ) -> Option<instancing::GeometryKey> {
        let (instance, part) = self.get(item)?;
        instance.snapshot.as_ref()?;
        let mesh = instance.meshes.get(part)?;
        Some(instancing::GeometryKey {
            vertices: mesh.vertices.clone(),
            previous: motion_required.then(|| previous_buffer(instance).clone()),
            tangents: current_tangent(instance, part).cloned(),
        })
    }
}

fn same_pose(old: &SkinPose, pose: &SkinPose) -> bool {
    old.signature == pose.signature
        && (Arc::ptr_eq(&old.matrices, &pose.matrices) || old.matrices == pose.matrices)
}
fn previous_buffer(instance: &Instance) -> &wgpu::Buffer {
    instance
        .shared_previous
        .as_ref()
        .unwrap_or(&instance.previous)
}
fn current_tangent(instance: &Instance, part: usize) -> Option<&wgpu::Buffer> {
    if let Some(shared) = &instance.shared_tangents {
        shared.get(part)?.as_ref()
    } else {
        instance.tangents.get(part)?.as_ref().map(|t| &t.output)
    }
}

fn pose_bounds_for(joints: &[[Vec3; 2]], pose: &SkinPose) -> Result<[Vec3; 2]> {
    let mut bounds = [Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)];
    let mut rounding_scale = [0f64; 3];
    for (matrix, bound) in pose.matrices.iter().zip(joints) {
        if !bound[0].is_finite() {
            continue;
        }
        let absolute_position = bound[0].abs().max(bound[1].abs()).to_array();
        for (axis, scale) in rounding_scale.iter_mut().enumerate() {
            let terms = (0..3)
                .map(|column| {
                    f64::from(matrix[column * 4 + axis].abs())
                        * f64::from(absolute_position[column])
                })
                .sum::<f64>()
                + f64::from(matrix[12 + axis].abs());
            *scale = scale.max(terms);
        }
        let matrix = Mat4::from_cols_array(matrix);
        for corner in shadows::corners(*bound) {
            let p = matrix.transform_point3(corner);
            bounds[0] = bounds[0].min(p);
            bounds[1] = bounds[1].max(p);
        }
    }
    ensure!(bounds.iter().all(|p| p.is_finite()), "skin bounds overflow");
    // Upload validation permits a small weight-sum tolerance, and the GPU
    // combines matrix columns before multiplying positions. Cover that scale
    // error and floating point rounding, including large terms that cancel at
    // the transformed box boundary. Positive weights sum to within .001 of 1;
    // 64 epsilon covers the matrix blend and subsequent dot products.
    let rounding = Vec3::from_array(
        rounding_scale.map(|terms| (terms * 64. * f64::from(f32::EPSILON)) as f32),
    );
    let margin = bounds[0].abs().max(bounds[1].abs()) * 0.0011
        + (bounds[1] - bounds[0]).abs().max(Vec3::ONE) * 1e-5
        + rounding;
    bounds[0] -= margin;
    bounds[1] += margin;
    Ok(bounds)
}

#[allow(clippy::too_many_arguments)]
fn restore_previous(
    gpu: &Gpu,
    layout: &wgpu::BindGroupLayout,
    pipeline: &wgpu::ComputePipeline,
    source: &Source,
    parts: &[UploadedPart],
    instance: &Instance,
    pose: &SkinPose,
    encoder: &mut crate::profiling::Encoder,
) {
    // Use a separate palette: queue writes to the current palette all execute
    // before commands, so two dispatches cannot share successive queue writes.
    let palette = gpu
        .device
        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("reentering skin previous palette"),
            contents: &float_bytes(pose.matrices.iter().flatten().copied()),
            usage: wgpu::BufferUsages::STORAGE,
        });
    let params = gpu
        .device
        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("reentering skin previous range"),
            contents: &[source.count as u32, 0, 0, 0]
                .into_iter()
                .flat_map(u32::to_le_bytes)
                .collect::<Vec<_>>(),
            usage: wgpu::BufferUsages::UNIFORM,
        });
    let buffers = [
        &parts[0].mesh.vertices,
        &source.weights,
        &palette,
        &instance.previous,
        &params,
    ];
    let binding = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("reentering skin previous geometry"),
        layout,
        entries: &buffers
            .iter()
            .enumerate()
            .map(|(i, b)| wgpu::BindGroupEntry {
                binding: i as u32,
                resource: b.as_entire_binding(),
            })
            .collect::<Vec<_>>(),
    });
    let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
        label: Some("restore previous invisible pose"),
        ..Default::default()
    });
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, &binding, &[]);
    pass.dispatch_workgroups((source.count as u32).div_ceil(64), 1, 1);
}

impl SceneRenderer {
    pub fn set_skinning_optimizations_enabled(&mut self, enabled: bool) {
        self.skinning.disabled = !enabled;
    }
}

#[cfg(test)]
mod optimization_tests {
    use super::*;
    #[test]
    fn posed_part_joint_boxes_contain_weighted_vertices_without_whole_actor_extent() -> Result<()> {
        let vertices = [
            [-10., -1., 0., 0., 0., 1., 0., 0.],
            [-9., -1., 0., 0., 0., 1., 0., 0.],
            [-9.5, 1., 0., 0., 0., 1., 0., 0.],
            [9., -1., 0., 0., 0., 1., 0., 0.],
            [10., -1., 0., 0., 0., 1., 0., 0.],
            [9.5, 1., 0., 0., 0., 1., 0., 0.],
        ];
        let influences = [[0, 1, 0, 0, 0.75f32.to_bits(), 0.25f32.to_bits(), 0, 0]; 6];
        let skin = SkinData {
            signature: 7,
            bindings: 2,
            vertices: &influences,
        };
        let parts = std::array::from_fn::<_, 2, _>(|index| ModelPart {
            source_key: "0000000000000000",
            start: index as u32 * 3,
            count: 3,
            color: [1.; 4],
            alpha_cutoff: None,
            image: None,
            shading: None,
        });
        let indices = [0, 1, 2, 3, 4, 5];
        let joints = Source::part_bounds(&skin, &vertices, &indices, &parts)?;
        let whole = Source::bounds(&skin, &vertices)?;
        for tick in 0..32 {
            let matrices = [
                Mat4::from_rotation_z(tick as f32 * 0.13)
                    * Mat4::from_scale(Vec3::new(-1.2, 0.8, 1.)),
                Mat4::from_translation(Vec3::new(0.7, -0.3, 0.2))
                    * Mat4::from_rotation_y(tick as f32 * -0.09),
            ];
            let pose = SkinPose {
                signature: 7,
                matrices: Arc::new(matrices.map(|m| m.to_cols_array()).to_vec()),
            };
            let actor = pose_bounds_for(&whole, &pose)?;
            for (part, joints) in joints.iter().enumerate() {
                let bounds = pose_bounds_for(joints, &pose)?;
                assert!((bounds[1] - bounds[0]).length() < (actor[1] - actor[0]).length());
                for vertex in &vertices[part * 3..part * 3 + 3] {
                    let point = Vec3::from_slice(&vertex[..3]);
                    let weighted = matrices[0].transform_point3(point) * 0.75
                        + matrices[1].transform_point3(point) * 0.25;
                    assert!(
                        (weighted.cmpge(bounds[0] - Vec3::splat(1e-5))
                            & weighted.cmple(bounds[1] + Vec3::splat(1e-5)))
                        .all()
                    );
                }
            }
        }
        Ok(())
    }
    #[test]
    fn posed_bounds_cover_weighted_matrix_cancellation() -> Result<()> {
        let point = Vec3::new(0.3, 0., 0.);
        let weights = [0.1, 0.2, 0.3, 0.4];
        let matrices = [1e5, 2e5, -3e5, 4e5].map(|scale| {
            let mut matrix = Mat4::from_scale(Vec3::new(scale, 1., 1.));
            matrix.w_axis.x = -(scale * point.x);
            assert_eq!(matrix.transform_point3(point), Vec3::ZERO);
            matrix
        });
        let pose = SkinPose {
            signature: 9,
            matrices: Arc::new(matrices.map(|matrix| matrix.to_cols_array()).to_vec()),
        };
        let bounds = pose_bounds_for(&[[point; 2]; 4], &pose)?;
        // Match the GPU's weighted-matrix-first operation, which can differ
        // from the individually transformed joint points under cancellation.
        let blended = matrices[0] * weights[0]
            + matrices[1] * weights[1]
            + matrices[2] * weights[2]
            + matrices[3] * weights[3];
        let weighted = blended.transform_point3(point);
        assert!(weighted.x.abs() > 1e-5);
        assert!((weighted.cmpge(bounds[0]) & weighted.cmple(bounds[1])).all());
        Ok(())
    }
    #[test]
    fn shared_visible_poses_and_hidden_reentry_preserve_current_and_history() -> Result<()> {
        let gpu = pollster::block_on(Gpu::request(
            &crate::instance(crate::Backend::native()),
            None,
            false,
        ))?;
        let mut renderers = std::array::from_fn::<_, 2, _>(|_| {
            SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm)
        });
        renderers[0].set_skinning_optimizations_enabled(false);
        for renderer in &mut renderers {
            renderer.set_occlusion_enabled(false);
            renderer.upload_skinned_model(
                &gpu,
                "actor",
                &[
                    [-0.5, -0.5, 0., 0., 0., 1., 0., 1.],
                    [0.5, -0.5, 0., 0., 0., 1., 1., 1.],
                    [0., 0.5, 0., 0., 0., 1., 0.5, 0.],
                ],
                &[0, 1, 2],
                &[ModelPart {
                    source_key: "0000000000000000",
                    start: 0,
                    count: 3,
                    color: [1.; 4],
                    alpha_cutoff: None,
                    image: None,
                    shading: None,
                }],
                SkinData {
                    signature: 9,
                    bindings: 1,
                    vertices: &[[0, 0, 0, 0, 1f32.to_bits(), 0, 0, 0]; 3],
                },
            )?;
        }
        let mut scene = RenderScene {
            skin_poses: Default::default(),
            shader_time: 0.,
            particles: vec![],
            fog: Default::default(),
            gi: None,
            lights: vec![],
            environment: EnvironmentSettings::disabled(),
            display: DisplaySettings {
                tone_mapping: false,
                ..Default::default()
            },
            lighting: Lighting {
                shadows: false,
                ..Default::default()
            },
            view_projection: glam::camera::rh::proj::directx::orthographic(
                -2., 2., -2., 2., 0.1, 20.,
            ),
            items: (0..12)
                .map(|index| DrawItem {
                    motion_id: index + 1,
                    model: Mat4::from_translation(Vec3::new(
                        if index < 4 {
                            (index as f32 - 1.5) * 0.6
                        } else {
                            10. + index as f32
                        },
                        0.,
                        -4.,
                    )),
                    mesh: MeshKind::Imported("actor".into()),
                    material: Material {
                        metallic: None,
                        roughness: None,
                        surface_overrides: Default::default(),
                        tint: [0.4, 0.8, 0.3],
                        uv_scale: [1.; 2],
                        texture: TextureKind::White,
                        lit: false,
                        shader: None,
                    },
                })
                .collect(),
        };
        scene.display.temporal_aa.enabled = true;
        let capture = |renderer: &mut SceneRenderer, scene: &RenderScene| {
            crate::capture_offscreen(&gpu, 160, 160, |target| {
                renderer.draw(&gpu, target, [160; 2], scene)
            })
        };
        let mut retained_poses = BTreeMap::new();
        for tick in 0..15 {
            let pose = SkinPose {
                signature: 9,
                matrices: Arc::new(vec![
                    (Mat4::from_translation(Vec3::new(0., tick as f32 * 0.03, 0.))
                        * Mat4::from_rotation_y(tick as f32 * 0.04))
                    .to_cols_array(),
                ]),
            };
            for item in &scene.items {
                scene.skin_poses.insert(item.motion_id, pose.clone());
            }
            retained_poses.insert(tick, pose.clone());
            if tick == 1 {
                // It rejoins the shared current pose next tick, but its prior
                // pose differs and must keep a distinct motion vertex stream.
                scene.skin_poses.insert(
                    2,
                    SkinPose {
                        signature: 9,
                        matrices: Arc::new(vec![
                            (Mat4::from_translation(Vec3::new(0.1, 0.2, 0.))
                                * Mat4::from_rotation_y(0.3))
                            .to_cols_array(),
                        ]),
                    },
                );
            }
            if tick == 2 {
                scene.items[0].model = Mat4::from_translation(Vec3::new(20., 0., -4.));
            }
            if tick == 3 {
                scene.items.swap(1, 3);
            }
            if tick == 5 {
                scene.items[0].model = Mat4::from_translation(Vec3::new(-0.9, 0., -4.));
                // Reenter the private owner with its last visible palette.
                // Its current output is still valid and needs no dispatch.
                scene.skin_poses.insert(1, retained_poses[&1].clone());
            }
            if tick == 6 {
                scene.items[7].model = Mat4::from_translation(Vec3::new(0.2, -0.8, -4.));
            }
            if tick == 9 {
                // Put an unchanged follower before the old owner, then change
                // the owner's palette. Reusing the retained shared handle would
                // let the later owner dispatch overwrite the follower.
                scene.items.swap(0, 3);
                scene.skin_poses.insert(2, retained_poses[&8].clone());
            }
            if tick == 10 || tick == 11 {
                scene.skin_poses.insert(2, retained_poses[&8].clone());
            }
            if tick == 11 {
                let actor = scene
                    .items
                    .iter_mut()
                    .find(|item| item.motion_id == 1)
                    .unwrap();
                actor.model = Mat4::from_translation(Vec3::new(20., 0., -4.));
            }
            if tick == 12 {
                let actor = scene
                    .items
                    .iter_mut()
                    .find(|item| item.motion_id == 1)
                    .unwrap();
                actor.model = Mat4::from_translation(Vec3::new(-0.9, 0., -4.));
                // This actor was a follower at tick10. Its old owner has
                // overwritten the shared output during the invisible frame.
                scene.skin_poses.insert(1, retained_poses[&10].clone());
            }
            if tick == 13 {
                renderers[1].set_skinning_optimizations_enabled(false);
                // The old owner changes after optimization is disabled, while
                // a retained follower keeps its old pose and motion history.
                scene.skin_poses.insert(4, retained_poses[&12].clone());
            }
            if tick == 14 {
                renderers[1].set_skinning_optimizations_enabled(true);
            }
            assert_eq!(
                capture(&mut renderers[0], &scene)?.rgba,
                capture(&mut renderers[1], &scene)?.rgba,
                "visible skin pixels differ on tick{tick}"
            );
            if tick == 0 {
                assert_eq!(renderers[0].skinning.dispatches, 12);
                assert_eq!(renderers[1].skinning.dispatches, 1);
                assert_eq!(renderers[1].skinning.shared_copies, 0);
                assert_eq!(renderers[1].skinning.shared_actors, 3);
                assert_eq!(renderers[1].skinning.culled_actors, 8);
                let actors = &renderers[1].skinning.instances["actor"];
                assert_eq!(actors[&1].meshes[0].vertices, actors[&2].meshes[0].vertices);
                assert_eq!(previous_buffer(&actors[&1]), previous_buffer(&actors[&2]));
                assert_eq!(renderers[0].stats.color_draws, 4);
                assert_eq!(renderers[1].stats.color_draws, 1);
            }
            if tick == 2 {
                let actors = &renderers[1].skinning.instances["actor"];
                assert_eq!(actors[&2].meshes[0].vertices, actors[&3].meshes[0].vertices);
                assert_ne!(previous_buffer(&actors[&2]), previous_buffer(&actors[&3]));
                assert_eq!(
                    renderers[1].stats.color_draws, 2,
                    "different prior deformation must partition motion batches"
                );
            }
            if tick == 5 {
                assert_eq!(
                    renderers[1].skinning.dispatches, 2,
                    "one shared pose plus private-owner reentry history"
                );
            }
            if tick == 12 {
                assert_eq!(
                    renderers[1].skinning.dispatches, 3,
                    "shared current, follower reentry current and history"
                );
            }
            // Read exact GPU current and prior vertices for every visible actor,
            // so hidden pose history is tested independently of displayed pixels.
            for item in &scene.items {
                if item.model.w_axis.x > 2. {
                    continue;
                }
                for previous in [false, true] {
                    let mut bytes = Vec::new();
                    for renderer in &renderers {
                        let instance = &renderer.skinning.instances["actor"][&item.motion_id];
                        let source = if previous {
                            previous_buffer(instance)
                        } else {
                            &instance.meshes[0].vertices
                        };
                        let staging = gpu.device.create_buffer(&wgpu::BufferDescriptor {
                            label: Some("skin proof readback"),
                            size: 96,
                            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                            mapped_at_creation: false,
                        });
                        let mut encoder = gpu.device.create_command_encoder(&Default::default());
                        encoder.copy_buffer_to_buffer(source, 0, &staging, 0, 96);
                        gpu.queue.submit([encoder.finish()]);
                        let (tx, rx) = std::sync::mpsc::channel();
                        staging.map_async(wgpu::MapMode::Read, 0..96, move |result| {
                            let _ = tx.send(result);
                        });
                        gpu.wait()?;
                        rx.recv()??;
                        bytes.push(staging.get_mapped_range(0..96)?.to_vec());
                        staging.unmap();
                    }
                    assert_eq!(
                        bytes[0], bytes[1],
                        "skin actor{} previous={previous} tick{tick}",
                        item.motion_id
                    );
                }
            }
        }
        // Sun casters are always required even when outside the camera.
        scene.lighting.shadows = true;
        scene.lighting.shadow_resolution = 256;
        for item in &mut scene.items {
            item.material.lit = true;
        }
        let pose = SkinPose {
            signature: 9,
            matrices: Arc::new(vec![Mat4::from_rotation_x(0.2).to_cols_array()]),
        };
        for item in &scene.items {
            scene.skin_poses.insert(item.motion_id, pose.clone());
        }
        assert_eq!(
            capture(&mut renderers[0], &scene)?.rgba,
            capture(&mut renderers[1], &scene)?.rgba
        );
        assert_eq!(renderers[1].skinning.culled_actors, 0);
        assert_eq!(renderers[0].stats.shadow_draws, 12);
        assert_eq!(renderers[1].stats.shadow_draws, 1);
        println!(
            "skinning_proof 12->1dispatches currentcopies0 shared3 hidden8 color4->1 shadow12->1 distinct_history_partitions2 exact_GPU_current_previous_reentry_and_offscreen_sun"
        );
        Ok(())
    }
}
