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
    previous: wgpu::Buffer,
    binding: wgpu::BindGroup,
    tangents: Vec<Option<Tangents>>,
    revision: u64,
    /// Copy the final moving pose to history on the first stationary frame.
    moved: bool,
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
}
impl Skinning {
    pub fn remove(&mut self, id: &str) {
        self.sources.remove(id);
        self.instances.remove(id);
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
            wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
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
                        wgpu::BufferUsages::VERTEX,
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
            previous,
            binding,
            tangents,
            revision: 0,
            moved: false,
        }
    }
    pub fn prepare(
        &mut self,
        gpu: &Gpu,
        scene: &RenderScene,
        models: &BTreeMap<String, Vec<UploadedPart>>,
        encoder: &mut crate::profiling::Encoder,
    ) -> Result<()> {
        let mut active: BTreeMap<String, BTreeSet<u64>> = BTreeMap::new();
        if scene.skin_poses.is_empty() {
            self.instances.clear();
            return Ok(());
        }
        self.pipeline(gpu);
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
            let instance = self
                .instances
                .get_mut(asset)
                .unwrap()
                .get_mut(&item.motion_id)
                .unwrap();
            let size = source.count as u64 * 32;
            if instance.snapshot.as_ref().is_some_and(|old| {
                old.signature == pose.signature
                    && (Arc::ptr_eq(&old.matrices, &pose.matrices) || old.matrices == pose.matrices)
            }) {
                if instance.moved {
                    encoder.copy_buffer_to_buffer(
                        &instance.meshes[0].vertices,
                        0,
                        &instance.previous,
                        0,
                        size,
                    );
                    instance.moved = false;
                }
                continue;
            }
            // An immutable retained palette was already validated. Only inspect new data.
            ensure!(
                pose.matrices.iter().flatten().all(|v| v.is_finite()),
                "invalid skin pose"
            );
            let first = instance.snapshot.is_none();
            if !first {
                encoder.copy_buffer_to_buffer(
                    &instance.meshes[0].vertices,
                    0,
                    &instance.previous,
                    0,
                    size,
                );
            }
            gpu.queue.write_buffer(
                &instance.palette,
                0,
                &float_bytes(pose.matrices.iter().flatten().copied()),
            );
            {
                let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some("skin geometry and PBR tangents"),
                    timestamp_writes: None,
                });
                pass.set_pipeline(&self.pipeline.as_ref().unwrap().1);
                pass.set_bind_group(0, &instance.binding, &[]);
                pass.dispatch_workgroups((source.count as u32).div_ceil(64), 1, 1);
                pass.set_pipeline(&self.pipeline.as_ref().unwrap().2);
                for tangent in instance.tangents.iter().flatten() {
                    pass.set_bind_group(0, &tangent.binding, &[]);
                    pass.dispatch_workgroups(tangent.count.div_ceil(64), 1, 1);
                }
            }
            if first {
                encoder.copy_buffer_to_buffer(
                    &instance.meshes[0].vertices,
                    0,
                    &instance.previous,
                    0,
                    size,
                );
            }
            let mut bounds = [Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)];
            for (matrix, bound) in pose.matrices.iter().zip(&source.bounds) {
                if !bound[0].is_finite() {
                    continue;
                }
                let matrix = Mat4::from_cols_array(matrix);
                for corner in shadows::corners(*bound) {
                    let p = matrix.transform_point3(corner);
                    bounds[0] = bounds[0].min(p);
                    bounds[1] = bounds[1].max(p);
                }
            }
            ensure!(bounds.iter().all(|p| p.is_finite()), "skin bounds overflow");
            for mesh in &mut instance.meshes {
                mesh.bounds = bounds;
            }
            self.revision = self.revision.wrapping_add(1);
            instance.revision = self.revision;
            instance.snapshot = Some(pose.clone());
            instance.moved = !first;
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
        self.get(item).map(|(i, _)| &i.previous)
    }
    pub fn tangents(&self, item: &DrawItem) -> Option<&wgpu::Buffer> {
        let (i, p) = self.get(item)?;
        i.tangents.get(p)?.as_ref().map(|t| &t.output)
    }
    pub fn revision(&self, item: &DrawItem) -> u64 {
        self.get(item).map_or(0, |(i, _)| i.revision)
    }
}
