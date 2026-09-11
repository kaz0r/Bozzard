use super::*;

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
    pub pipeline: wgpu::RenderPipeline,
    pub point_pipeline: wgpu::RenderPipeline,
    uniform: wgpu::Buffer,
    sampler: wgpu::Sampler,
    depth: wgpu::TextureView,
    resolution: u32,
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
        let layout = gpu
            .device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("shadow pipeline layout"),
                bind_group_layouts: &[Some(object_layout), Some(&caster_layout)],
                immediate_size: 0,
            });
        let shader = gpu
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("sun depth caster"),
                source: wgpu::ShaderSource::Wgsl(include_str!("shadow_cast.wgsl").into()),
            });
        let make_pipeline = |label, slope_scale| {
            gpu.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(label), layout: Some(&layout),
            vertex: wgpu::VertexState { module: &shader, entry_point: Some("vs_main"), compilation_options: Default::default(), buffers: &[Some(wgpu::VertexBufferLayout { array_stride: 32, step_mode: wgpu::VertexStepMode::Vertex, attributes: &wgpu::vertex_attr_array![0=>Float32x3,1=>Float32x3,2=>Float32x2] })] },
            fragment: Some(wgpu::FragmentState { module: &shader, entry_point: Some("fs_main"), compilation_options: Default::default(), targets: &[] }),
            primitive: Default::default(),
            depth_stencil: Some(wgpu::DepthStencilState { format: wgpu::TextureFormat::Depth32Float, depth_write_enabled: Some(true), depth_compare: Some(wgpu::CompareFunction::Less), stencil: Default::default(), bias: wgpu::DepthBiasState { constant: 0, slope_scale, clamp: 0. } }),
            multisample: Default::default(), multiview_mask: None, cache: None,
        })
        };
        let pipeline = make_pipeline("sun/spot shadow pass", 1.);
        // Cube faces can see grazing receivers in every direction. Cover the 3x3
        // bilinear kernel's maximum L1 footprint (1.5 texels along each axis).
        let point_pipeline = make_pipeline("point shadow pass", 3.);
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
fn fit(
    points: impl Iterator<Item = Vec3>,
    direction: Vec3,
    resolution: u32,
) -> Option<(Mat4, f32)> {
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
    matrix.is_finite().then_some((matrix, far - near))
}
impl SceneRenderer {
    pub(super) fn mesh_for(&self, kind: &MeshKind) -> &MeshBuffers {
        match kind {
            MeshKind::Quad => &self.quad,
            MeshKind::Cube => &self.cube,
            MeshKind::Imported(id) => &self.imported_meshes[id],
            MeshKind::ModelPart(id, index) => &self.models[id][*index].mesh,
        }
    }
    pub(super) fn shading(&self, kind: &MeshKind) -> Option<&crate::pbr::UploadedShading> {
        match kind {
            MeshKind::ModelPart(id, index) => self.models[id][*index].shading.as_ref(),
            _ => None,
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
                    corners(self.mesh_for(&d.object.mesh).bounds)
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
        let (matrix, range) = fit.unwrap_or((Mat4::IDENTITY, 1.));
        gpu.queue.write_buffer(
            &self.shadows.uniform,
            0,
            &float_bytes(matrix.to_cols_array().into_iter().chain([
                light.shadow_bias / range,
                light.shadow_normal_bias,
                if enabled { 1. } else { 0. },
                1. / resolution as f32,
            ])),
        );
        Ok(())
    }
    pub(super) fn draw_shadows(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        scene: &RenderScene,
        draws: &[PreparedDraw],
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
        pass.set_pipeline(&self.shadows.pipeline);
        pass.set_bind_group(1, &self.shadows.caster_binding, &[]);
        self.draw_shadow_casters(&mut pass, draws, None)
    }
    pub(super) fn draw_shadow_casters(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        draws: &[PreparedDraw],
        projection: Option<Mat4>,
    ) -> (usize, u64) {
        let mut counts = (0, 0);
        for (draw, binding) in draws.iter().zip(&self.objects) {
            if draw.transparent || !draw.object.material.lit {
                continue;
            }
            let mesh = self.mesh_for(&draw.object.mesh);
            if self.culling
                && projection
                    .is_some_and(|p| !visibility::visible(mesh.bounds, p * draw.object.model))
            {
                continue;
            }
            pass.set_bind_group(0, &binding.binding, &[]);
            pass.set_vertex_buffer(0, mesh.vertices.slice(mesh.vertex_offset..));
            pass.set_index_buffer(mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..mesh.count, 0, 0..1);
            counts.0 += 1;
            counts.1 += u64::from(mesh.count / 3);
        }
        counts
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounds_fit_contains_corners_and_handles_vertical_sun() {
        let bounds = [Vec3::new(-20., -2., -10.), Vec3::new(25., 12., 10.)];
        for direction in [Vec3::Y, -Vec3::Y, Vec3::new(0.4, 0.8, 0.6).normalize()] {
            let (m, range) = fit(corners(bounds), direction, 2048).unwrap();
            assert!(range > 0.);
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
}
