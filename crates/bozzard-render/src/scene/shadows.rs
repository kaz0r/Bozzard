use super::*;

/// Exact depth-producing state, independent of camera, exposure and light color.
/// Asset publication invalidates the retained frame even when asset IDs are reused.
#[derive(PartialEq)]
pub(super) struct ShadowFrame {
    sun: (bool, u32, [f32; 3], f32, f32),
    lights: Vec<ShadowLight>,
    casters: Vec<ShadowCaster>,
    culling: bool,
}

#[derive(PartialEq)]
struct ShadowLight {
    position: [f32; 3],
    direction: [f32; 3],
    range: f32,
    angles: Option<[f32; 2]>,
    bias: f32,
    normal_bias: f32,
}

#[derive(Clone, PartialEq)]
pub(super) struct ShadowCaster {
    deformation: u64,
    model: Mat4,
    mesh: MeshKind,
    texture: TextureKind,
    uv_scale: [f32; 2],
    opacity: f32,
    cutoff: f32,
    transparent: bool,
}

impl ShadowCaster {
    pub fn new(d: &PreparedDraw) -> Self {
        Self {
            deformation: d.deformation,
            model: d.object.model,
            mesh: d.object.mesh.clone(),
            texture: d.object.material.texture.clone(),
            uv_scale: d.object.material.uv_scale,
            opacity: d.opacity,
            cutoff: d.cutoff,
            transparent: d.transparent,
        }
    }
}

impl ShadowFrame {
    pub fn same_sun(&self, other: &Self) -> bool {
        self.sun == other.sun && self.casters == other.casters && self.culling == other.culling
    }

    pub fn new(scene: &RenderScene, draws: &[PreparedDraw], culling: bool) -> Self {
        let light = scene.lighting;
        Self {
            sun: (
                light.shadows && light.sun_intensity > 0.,
                light.shadow_resolution,
                light.sun_direction,
                light.shadow_bias,
                light.shadow_normal_bias,
            ),
            lights: scene
                .lights
                .iter()
                .filter(|l| l.casts_shadow())
                .map(|l| {
                    let shadow = l.shadows.unwrap();
                    ShadowLight {
                        position: l.position,
                        direction: if l.spot_angles.is_some() {
                            l.direction
                        } else {
                            [0.; 3]
                        },
                        range: l.range,
                        angles: l.spot_angles,
                        bias: shadow.bias,
                        normal_bias: shadow.normal_bias,
                    }
                })
                .collect(),
            casters: draws
                .iter()
                // Transparent receivers also affect the directional map's fitted bounds.
                .filter(|d| d.object.material.lit)
                .map(ShadowCaster::new)
                .collect(),
            culling,
        }
    }
}

pub(super) struct Shadows {
    pub spots: local_shadow_maps::ShadowMaps,
    pub points: local_shadow_maps::ShadowMaps,
    pub gi_uniform: wgpu::Buffer,
    pub gi_data: wgpu::Buffer,
    pub gi_snapshot: Option<std::sync::Arc<Vec<[f32; 4]>>>,
    pub local_lights: wgpu::Buffer,
    pub sample_layout: wgpu::BindGroupLayout,
    pub sample_binding: wgpu::BindGroup,
    caster_binding: wgpu::BindGroup,
    pub caster_layout: wgpu::BindGroupLayout,
    pub pipeline: wgpu::RenderPipeline,
    pub point_pipeline: wgpu::RenderPipeline,
    uniform: wgpu::Buffer,
    sampler: wgpu::Sampler,
    depth: wgpu::TextureView,
    resolution: u32,
}

fn module_text(instanced: bool) -> String {
    let source = include_str!("shadow_cast.wgsl");
    if !instanced {
        return source.into();
    }
    source
        .replace("@group(0) @binding(0) var<uniform> object: ObjectUniform;",
            &format!("@group(0) @binding(0) var<uniform> objects: array<ObjectUniform, {}>;\nvar<private> object: ObjectUniform;", instancing::MAX_INSTANCES))
        .replace("struct VertexOutput {", "struct VertexOutput { @location(1) @interpolate(flat) instance: u32,")
        .replace("fn vs_main(", "fn vs_main(@builtin(instance_index) instance: u32, ")
        .replace("var out: VertexOutput;", "object = objects[instance];\nvar out: VertexOutput;\nout.instance = instance;")
        .replace("front: bool) {", "front: bool) {\nobject = objects[in.instance];")
}

pub(super) fn pipeline(
    gpu: &Gpu,
    object_layout: &wgpu::BindGroupLayout,
    caster_layout: &wgpu::BindGroupLayout,
    instanced: bool,
    point: bool,
) -> wgpu::RenderPipeline {
    let layout = gpu
        .device
        .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("shadow pipeline layout"),
            bind_group_layouts: &[Some(object_layout), Some(caster_layout)],
            immediate_size: 0,
        });
    let shader = gpu
        .device
        .create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("depth caster"),
            source: wgpu::ShaderSource::Wgsl(module_text(instanced).into()),
        });
    gpu.device
        .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(if instanced {
                "instanced shadow pass"
            } else {
                "shadow pass"
            }),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: 32,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0=>Float32x3,1=>Float32x3,2=>Float32x2],
                })],
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
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: Default::default(),
                bias: wgpu::DepthBiasState {
                    constant: 0,
                    slope_scale: if point { 3. } else { 1. },
                    clamp: 0.,
                },
            }),
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        })
}
fn target(gpu: &Gpu, resolution: u32) -> wgpu::TextureView {
    gpu.device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some("sun shadow depth"),
            size: wgpu::Extent3d {
                width: resolution,
                height: resolution,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        })
        .create_view(&Default::default())
}
impl Shadows {
    pub fn new(gpu: &Gpu, object_layout: &wgpu::BindGroupLayout) -> Self {
        let uniform_entry = wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: wgpu::BufferSize::new(80),
            },
            count: None,
        };
        let caster_layout = gpu
            .device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("shadow caster frame"),
                entries: &[uniform_entry],
            });
        let sample_layout = gpu
            .device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("shadow receiver frame"),
                entries: &[
                    uniform_entry,
                    wgpu::BindGroupLayoutEntry {
                        binding: 8,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Depth,
                            view_dimension: wgpu::TextureViewDimension::D2Array,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 9,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: wgpu::BufferSize::new(point_shadows::UNIFORM_SIZE),
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 6,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Depth,
                            view_dimension: wgpu::TextureViewDimension::D2Array,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 7,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: wgpu::BufferSize::new(spot_shadows::UNIFORM_SIZE),
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 4,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: wgpu::BufferSize::new(64),
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 5,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage { read_only: true },
                            has_dynamic_offset: false,
                            min_binding_size: wgpu::BufferSize::new(656),
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 3,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: wgpu::BufferSize::new(local_lights::UNIFORM_SIZE),
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Depth,
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison),
                        count: None,
                    },
                ],
            });
        let uniform = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("shadow frame uniform"),
            size: 80,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let caster_binding = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("shadow caster frame"),
            layout: &caster_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform.as_entire_binding(),
            }],
        });
        let sampler = gpu.device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("shadow PCF comparison"),
            compare: Some(wgpu::CompareFunction::LessEqual),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let local_lights = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("local lights"),
            size: local_lights::UNIFORM_SIZE,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let gi_uniform = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("GI volume"),
            size: 64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let gi_data = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("empty GI"),
            size: 656,
            usage: wgpu::BufferUsages::STORAGE,
            mapped_at_creation: false,
        });
        let depth = target(gpu, 1);
        let spots = local_shadow_maps::ShadowMaps::new(
            gpu,
            &caster_layout,
            MAX_SHADOWED_SPOT_LIGHTS,
            spot_shadows::RESOLUTION,
        );
        let points = local_shadow_maps::ShadowMaps::new(
            gpu,
            &caster_layout,
            MAX_SHADOWED_POINT_LIGHTS * point_shadows::FACES,
            point_shadows::RESOLUTION,
        );
        let sample_binding = Self::binding(
            gpu,
            &sample_layout,
            &uniform,
            [&depth, &spots.depth, &points.depth],
            &sampler,
            &[
                &local_lights,
                &gi_uniform,
                &gi_data,
                &spots.uniform,
                &points.uniform,
            ],
        );
        let pipeline = pipeline(gpu, object_layout, &caster_layout, false, false);
        // Cube faces need the wider grazing-receiver bias used by the reference pass.
        let point_pipeline = self::pipeline(gpu, object_layout, &caster_layout, false, true);
        Self {
            spots,
            points,
            gi_uniform,
            gi_data,
            gi_snapshot: None,
            local_lights,
            sample_layout,
            sample_binding,
            caster_binding,
            caster_layout,
            pipeline,
            point_pipeline,
            uniform,
            sampler,
            depth,
            resolution: 1,
        }
    }
    pub fn rebind(&mut self, gpu: &Gpu) {
        self.sample_binding = Self::binding(
            gpu,
            &self.sample_layout,
            &self.uniform,
            [&self.depth, &self.spots.depth, &self.points.depth],
            &self.sampler,
            &[
                &self.local_lights,
                &self.gi_uniform,
                &self.gi_data,
                &self.spots.uniform,
                &self.points.uniform,
            ],
        );
    }
    fn binding(
        gpu: &Gpu,
        layout: &wgpu::BindGroupLayout,
        uniform: &wgpu::Buffer,
        depths: [&wgpu::TextureView; 3],
        sampler: &wgpu::Sampler,
        buffers: &[&wgpu::Buffer; 5],
    ) -> wgpu::BindGroup {
        gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("shadow receiver frame"),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 8,
                    resource: wgpu::BindingResource::TextureView(depths[2]),
                },
                wgpu::BindGroupEntry {
                    binding: 9,
                    resource: buffers[4].as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 6,
                    resource: wgpu::BindingResource::TextureView(depths[1]),
                },
                wgpu::BindGroupEntry {
                    binding: 7,
                    resource: buffers[3].as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: buffers[1].as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: buffers[2].as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: buffers[0].as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(depths[0]),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
            ],
        })
    }
}
pub(super) fn corners(bounds: [Vec3; 2]) -> impl Iterator<Item = Vec3> {
    (0..8).map(move |i| {
        Vec3::new(
            bounds[(i & 1) as usize].x,
            bounds[((i >> 1) & 1) as usize].y,
            bounds[((i >> 2) & 1) as usize].z,
        )
    })
}
/// Fits a sun-aligned light-space box around `points`, returning the projection, the fitted
/// depth range, and the world size of one shadow texel. A fitted box spreads the map's fixed
/// resolution over whatever it covers, so that texel size is what the bias has to keep up with.
fn fit(
    points: impl Iterator<Item = Vec3>,
    direction: Vec3,
    resolution: u32,
) -> Option<(Mat4, f32, f32)> {
    let up = if direction.dot(Vec3::Y).abs() > 0.99 {
        Vec3::Z
    } else {
        Vec3::Y
    };
    let view = glam::camera::rh::view::look_to_mat4(Vec3::ZERO, -direction, up);
    let mut min = Vec3::splat(f32::INFINITY);
    let mut max = -min;
    for p in points {
        let p = view.transform_point3(p);
        min = min.min(p);
        max = max.max(p);
    }
    if !min.is_finite() || !max.is_finite() {
        return None;
    }
    let extent = (max - min).max(Vec3::splat(0.1));
    let texel = extent.truncate() / (resolution as f32 - 4.);
    let center = (min * 0.5 + max * 0.5).truncate();
    let center = (center / texel).round() * texel;
    let half = extent.truncate() * 0.5 + texel * 2.;
    let pad = extent.z.max(1.) * 0.01;
    let near = -max.z - pad;
    let far = -min.z + pad;
    let matrix = glam::camera::rh::proj::directx::orthographic(
        center.x - half.x,
        center.x + half.x,
        center.y - half.y,
        center.y + half.y,
        near,
        far,
    ) * view;
    matrix
        .is_finite()
        .then_some((matrix, far - near, texel.max_element()))
}
impl SceneRenderer {
    pub(super) fn mesh_for(&self, object: &DrawItem) -> &MeshBuffers {
        if let Some(mesh) = self.skinning.mesh(object) {
            return mesh;
        }
        match &object.mesh {
            MeshKind::Text(text) => self.text.as_ref().unwrap().mesh(text).unwrap(),
            MeshKind::Sprite(sprite) => self.sprites.mesh(sprite).unwrap(),
            MeshKind::Quad => &self.quad,
            MeshKind::Cube => &self.cube,
            MeshKind::Sphere => &self.sphere,
            MeshKind::Imported(id) => &self.imported_meshes[id],
            MeshKind::ModelPart(id, index) => &self.models[id][*index].mesh,
        }
    }
    pub(super) fn update_shadows(
        &mut self,
        gpu: &Gpu,
        scene: &RenderScene,
        draws: &[PreparedDraw],
    ) -> Result<()> {
        let light = scene.lighting;
        let fit = fit(
            draws
                .iter()
                .filter(|d| d.object.material.lit)
                .flat_map(|d| {
                    corners(self.mesh_for(&d.object).bounds)
                        .map(|p| d.object.model.transform_point3(p))
                }),
            Vec3::from(light.sun_direction).normalize(),
            light.shadow_resolution,
        );
        let enabled = light.shadows && light.sun_intensity > 0. && fit.is_some();
        let resolution = if enabled { light.shadow_resolution } else { 1 };
        ensure!(
            resolution <= gpu.device.limits().max_texture_dimension_2d,
            "shadow resolution exceeds device limit"
        );
        if resolution != self.shadows.resolution {
            self.shadows.depth = target(gpu, resolution);
            self.shadows.rebind(gpu);
            self.shadows.resolution = resolution;
        }
        let (matrix, range, texel) = fit.unwrap_or((Mat4::IDENTITY, 1., 0.));
        // One slanted texel reads the depth of its neighbour, so the error a texel can place
        // under a surface scales with the texel's world size. Keeping the world-space bias at
        // least one texel wide is what stops that error from turning into acne once a stray
        // caster (a projectile crossing the scene) stretches the fitted box.
        // ponytail: one whole-texel frame bias for every surface; per-pixel slope bias, or
        // cascades that keep the box small, if the extra softening on large scenes matters.
        gpu.queue.write_buffer(
            &self.shadows.uniform,
            0,
            &float_bytes(matrix.to_cols_array().into_iter().chain([
                (light.shadow_bias + texel) / range,
                light.shadow_normal_bias,
                if enabled { 1. } else { 0. },
                1. / resolution as f32,
            ])),
        );
        Ok(())
    }
    pub(super) fn draw_shadows(
        &self,
        encoder: &mut crate::profiling::Encoder,
        scene: &RenderScene,
        draws: &[PreparedDraw],
        batches: &[instancing::Batch],
    ) -> (usize, u64) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("sun shadow casters"),
            color_attachments: &[],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &self.shadows.depth,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            ..Default::default()
        });
        if !scene.lighting.shadows || self.shadows.resolution == 1 {
            return (0, 0);
        }
        pass.set_bind_group(1, &self.shadows.caster_binding, &[]);
        self.draw_shadow_casters(&mut pass, draws, batches, None, false)
    }
    pub(super) fn draw_shadow_casters(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        draws: &[PreparedDraw],
        batches: &[instancing::Batch],
        projection: Option<Mat4>,
        point: bool,
    ) -> (usize, u64) {
        let mut counts = (0, 0);
        let casts = |draw: &PreparedDraw| {
            !draw.transparent
                && draw.object.material.lit
                && (!self.culling
                    || projection.is_none_or(|p| {
                        visibility::visible(
                            self.mesh_for(&draw.object).bounds,
                            p * draw.object.model,
                        )
                    }))
        };
        let mut batches = batches.iter().peekable();
        let mut index = 0;
        let mut was_instanced = None;
        while index < draws.len() {
            while batches.peek().is_some_and(|b| b.range.start < index) {
                batches.next();
            }
            // Reuse color-pass instance buffers only when the whole run casts into this map.
            // Offscreen casters and runs crossing a light frustum retain the exact single-draw path.
            let batch = batches.peek().filter(|b| {
                b.range.start == index
                    && b.slot.is_some()
                    && draws[b.range.clone()].iter().all(&casts)
            });
            let count = batch.map_or(1, |b| b.range.len());
            let slot = batch.and_then(|b| b.slot);
            let draw = &draws[index];
            if casts(draw) {
                let instanced = slot.is_some();
                if was_instanced != Some(instanced) {
                    pass.set_pipeline(if instanced {
                        &self.instancing.shadow_pipelines.as_ref().unwrap()[usize::from(point)]
                    } else if point {
                        &self.shadows.point_pipeline
                    } else {
                        &self.shadows.pipeline
                    });
                    was_instanced = Some(instanced);
                }
                let binding = slot.map_or(&self.objects[index].binding, |slot| {
                    &self.instancing.bindings[slot].binding
                });
                let mesh = self.mesh_for(&draw.object);
                pass.set_bind_group(0, binding, &[]);
                pass.set_vertex_buffer(0, mesh.vertices.slice(mesh.vertex_offset..));
                pass.set_index_buffer(mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..mesh.count, 0, 0..count as u32);
                counts.0 += 1;
                counts.1 += u64::from(mesh.count / 3) * count as u64;
            }
            index += count;
        }
        counts
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shadow_shaders_validate_and_share_the_color_uniform_stride() {
        for instanced in [false, true] {
            let source = module_text(instanced);
            let module = wgpu::naga::front::wgsl::parse_str(&source)
                .unwrap_or_else(|e| panic!("{}", e.emit_to_string(&source)));
            wgpu::naga::valid::Validator::new(
                wgpu::naga::valid::ValidationFlags::all(),
                wgpu::naga::valid::Capabilities::empty(),
            )
            .validate(&module)
            .unwrap();
            let uniform = module
                .types
                .iter()
                .find(|(_, ty)| ty.name.as_deref() == Some("ObjectUniform"))
                .unwrap()
                .1;
            let wgpu::naga::TypeInner::Struct { span, .. } = uniform.inner else {
                panic!("uniform struct")
            };
            assert_eq!(span as usize, OBJECT_UNIFORM_BYTES);
        }
    }
    #[test]
    fn bounds_fit_contains_corners_and_handles_vertical_sun() {
        let bounds = [Vec3::new(-20., -2., -10.), Vec3::new(25., 12., 10.)];
        for direction in [Vec3::Y, -Vec3::Y, Vec3::new(0.4, 0.8, 0.6).normalize()] {
            let (m, range, texel) = fit(corners(bounds), direction, 2048).unwrap();
            assert!(range > 0. && texel > 0.);
            for p in corners(bounds) {
                let q = m.project_point3(p);
                assert!(
                    q.x.abs() <= 1. && q.y.abs() <= 1. && (0.0..=1.0).contains(&q.z),
                    "{q:?}"
                );
            }
        }
        assert!(fit(std::iter::empty(), Vec3::Y, 2048).is_none());
    }
    #[test]
    fn one_texel_of_world_bias_tracks_the_fitted_area() {
        // A projectile flying far from the playable area used to stretch the fitted box
        // (and its texels) without changing the authored bias, so slanted floors broke out
        // in shadow acne. The frame bias now grows with the texel it has to cover.
        let direction = Vec3::new(0.4, 0.85, 0.35).normalize();
        let arena = [Vec3::new(-9., 0., -9.), Vec3::new(9., 1.5, 9.)];
        let stray = [Vec3::new(-9., 0., -9.), Vec3::new(140., 1.5, 9.)];
        let bias = |authored: f32, texel: f32, range: f32| (authored + texel) / range;
        let mut grew = false;
        for bounds in [arena, stray] {
            let (_, range, texel) = fit(corners(bounds), direction, 2048).unwrap();
            // Floor faces sit 0.85 to the sun, so a slanted texel misreads 0.62 texels of depth.
            let error = 0.62 * texel / range;
            assert!(bias(0.005, texel, range) >= error, "{texel} {range}");
            grew |= 0.005 / range < error;
        }
        assert!(
            grew,
            "the fitted box never stretched far enough to need the wider bias"
        );
    }
}
