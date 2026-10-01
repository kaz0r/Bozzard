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

#[derive(Clone, Copy, PartialEq)]
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
    lit: bool,
}

pub(super) struct Comparison {
    pub whole: bool,
    pub sun: bool,
    pub opaque: bool,
    pub stable_mask: Vec<bool>,
    pub unchanged: Vec<bool>,
}
impl Comparison {
    pub fn cold(count: usize) -> Self {
        Self {
            whole: false,
            sun: false,
            opaque: false,
            stable_mask: vec![false; count],
            unchanged: vec![false; count],
        }
    }
}
fn sun_key(scene: &RenderScene) -> (bool, u32, [f32; 3], f32, f32) {
    let l = scene.lighting;
    (
        l.shadows && l.sun_intensity > 0.,
        l.shadow_resolution,
        l.sun_direction,
        l.shadow_bias,
        l.shadow_normal_bias,
    )
}
fn light_key(l: &LocalLight) -> ShadowLight {
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
}
impl ShadowCaster {
    fn refresh(&mut self, d: &PreparedDraw) -> usize {
        let mut clones = 0;
        if self.mesh != d.object.mesh {
            self.mesh = d.object.mesh.clone();
            clones += 1;
        }
        if self.texture != d.object.material.texture {
            self.texture = d.object.material.texture.clone();
            clones += 1;
        }
        self.deformation = d.deformation;
        self.model = d.object.model;
        self.uv_scale = d.object.material.uv_scale;
        self.opacity = d.opacity;
        self.cutoff = d.cutoff;
        self.transparent = d.transparent;
        self.lit = d.object.material.lit;
        clones
    }
    pub fn matches(&self, d: &PreparedDraw) -> bool {
        self.deformation == d.deformation
            && self.model == d.object.model
            && self.mesh == d.object.mesh
            && self.texture == d.object.material.texture
            && self.uv_scale == d.object.material.uv_scale
            && self.opacity == d.opacity
            && self.cutoff == d.cutoff
            && self.transparent == d.transparent
            && self.lit == d.object.material.lit
    }
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
            lit: d.object.material.lit,
        }
    }
}

impl ShadowFrame {
    pub fn compare(
        &self,
        scene: &RenderScene,
        draws: &[PreparedDraw],
        culling: bool,
    ) -> Comparison {
        let mut result = Comparison::cold(draws.len());
        let mut previous = self.casters.iter();
        let mut opaque = self.casters.iter().filter(|c| !c.transparent);
        let mut all_same = true;
        let mut opaque_same = true;
        let mut opaque_count_same = true;
        for (index, d) in draws
            .iter()
            .enumerate()
            .filter(|(_, d)| d.object.material.lit)
        {
            let prior = previous.next();
            let unchanged = prior.is_some_and(|p| p.matches(d));
            result.unchanged[index] = unchanged;
            all_same &= unchanged;
            if !d.transparent {
                let old = opaque.next();
                opaque_count_same &= old.is_some();
                // Opaque and complete iterators usually refer to the same row.
                // Share the field comparison unless a receiver shifted them.
                let same = old.is_some_and(|p| {
                    if prior.is_some_and(|a| std::ptr::eq(a, p)) {
                        unchanged
                    } else {
                        p.matches(d)
                    }
                });
                opaque_same &= same;
                result.stable_mask[index] = same && d.deformation == 0;
            }
        }
        all_same &= previous.next().is_none();
        opaque_count_same &= opaque.next().is_none();
        let same_culling = self.culling == culling;
        if !opaque_count_same || !same_culling {
            result.stable_mask.fill(false);
        }
        result.opaque = opaque_same && opaque_count_same && same_culling;
        result.sun = all_same && same_culling && self.sun == sun_key(scene);
        result.whole = result.sun
            && self.lights.iter().copied().eq(scene
                .lights
                .iter()
                .filter(|l| l.casts_shadow())
                .map(light_key));
        result
    }
    pub fn refresh(
        &mut self,
        scene: &RenderScene,
        draws: &[PreparedDraw],
        culling: bool,
        unchanged: &[bool],
        stats: &mut FrameStats,
    ) {
        self.sun = sun_key(scene);
        self.culling = culling;
        self.lights.clear();
        self.lights.extend(
            scene
                .lights
                .iter()
                .filter(|l| l.casts_shadow())
                .map(light_key),
        );
        let mut row = 0;
        for (index, d) in draws
            .iter()
            .enumerate()
            .filter(|(_, d)| d.object.material.lit)
        {
            if row == self.casters.len() {
                self.casters.push(ShadowCaster::new(d));
                stats.shadow_metadata_built_casters += 1;
                stats.shadow_metadata_key_clones += 2;
            } else if unchanged[index] {
                stats.shadow_metadata_reused_casters += 1;
            } else {
                stats.shadow_metadata_updated_casters += 1;
                stats.shadow_metadata_key_clones += self.casters[row].refresh(d);
            }
            row += 1;
        }
        self.casters.truncate(row);
        if self.casters.capacity() > row.saturating_mul(2).max(64) {
            self.casters.shrink_to(row);
        }
        if self.lights.capacity() > self.lights.len().saturating_mul(2).max(8) {
            self.lights.shrink_to(self.lights.len());
        }
    }
    pub fn stable_casters(&self, draws: &[PreparedDraw], culling: bool) -> Vec<bool> {
        let mut mask = vec![false; draws.len()];
        let opaque = |d: &&PreparedDraw| !d.transparent && d.object.material.lit;
        if self.culling != culling
            || self.casters.iter().filter(|c| !c.transparent).count()
                != draws.iter().filter(opaque).count()
        {
            return mask;
        }
        for (previous, (index, draw)) in self.casters.iter().filter(|c| !c.transparent).zip(
            draws
                .iter()
                .enumerate()
                .filter(|(_, d)| !d.transparent && d.object.material.lit),
        ) {
            // Skinned meshes stay dynamic, including unchanged poses.
            mask[index] = draw.deformation == 0 && previous.matches(draw);
        }
        mask
    }
    pub fn same_local_casters(&self, other: &Self) -> bool {
        // Transparent lit receivers can move the fitted sun bounds, but never
        // write local depth. Compare opaque depth state once for all local maps.
        self.culling == other.culling
            && self
                .casters
                .iter()
                .filter(|c| !c.transparent)
                .eq(other.casters.iter().filter(|c| !c.transparent))
    }

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
    pub resolution: u32,
    pub uniform_row: Vec<u8>,
    pub sun_cache: sun_cache::Cache,
    pub sun_fit: sun_fit::Cache,
}

fn module_text(instanced: bool) -> String {
    let source = format!(
        "{}\n{}",
        include_str!("object.wgsl"),
        include_str!("shadow_cast.wgsl")
    );
    if !instanced {
        return source;
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
pub(super) fn target(gpu: &Gpu, resolution: u32) -> wgpu::TextureView {
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
            uniform_row: Vec::new(),
            sun_cache: sun_cache::Cache::default(),
            sun_fit: sun_fit::Cache::default(),
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
    let view = sun_view(direction);
    let mut min = Vec3::splat(f32::INFINITY);
    let mut max = -min;
    for p in points {
        let p = view.transform_point3(p);
        min = min.min(p);
        max = max.max(p);
    }
    fit_extents(min, max, view, resolution)
}
pub(super) fn sun_view(direction: Vec3) -> Mat4 {
    let up = if direction.dot(Vec3::Y).abs() > 0.99 {
        Vec3::Z
    } else {
        Vec3::Y
    };
    glam::camera::rh::view::look_to_mat4(Vec3::ZERO, -direction, up)
}
pub(super) fn fit_extents(
    min: Vec3,
    max: Vec3,
    view: Mat4,
    resolution: u32,
) -> Option<(Mat4, f32, f32)> {
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
        bounds: &[[Vec3; 2]],
        reuse_depth: bool,
    ) -> Result<(bool, bool)> {
        let fit_started = std::time::Instant::now();
        let light = scene.lighting;
        let direction = Vec3::from(light.sun_direction).normalize();
        let cached = (self.sun_fit_caching && self.state_caching).then(|| {
            self.shadows.sun_fit.prepare(
                sun_view(direction),
                light.shadow_resolution,
                draws
                    .iter()
                    .zip(bounds)
                    .map(|(d, b)| (d.object.model, *b, d.object.material.lit)),
            )
        });
        let fit = if let Some(cached) = &cached
            && !cached.fallback
        {
            self.stats.sun_bounds_reused = cached.reused;
            self.stats.sun_bounds_recomputed = cached.rebuilt;
            cached.fit
        } else {
            self.stats.sun_bounds_fallback = cached.is_some();
            self.stats.sun_bounds_recomputed =
                draws.iter().filter(|d| d.object.material.lit).count()
                    + cached.as_ref().map_or(0, |c| c.rebuilt);
            fit(
                draws
                    .iter()
                    .filter(|d| d.object.material.lit)
                    .flat_map(|d| {
                        corners(self.mesh_for(&d.object).bounds)
                            .map(|p| d.object.model.transform_point3(p))
                    }),
                direction,
                light.shadow_resolution,
            )
        };
        self.stats.sun_fit_ms = fit_started.elapsed().as_secs_f64() * 1000.;
        let enabled = light.shadows && light.sun_intensity > 0. && fit.is_some();
        let resolution = if enabled { light.shadow_resolution } else { 1 };
        ensure!(
            resolution <= gpu.device.limits().max_texture_dimension_2d,
            "shadow resolution exceeds device limit"
        );
        let target_changed = resolution != self.shadows.resolution;
        if target_changed {
            self.shadows.sun_cache.clear();
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
        let row = float_bytes(matrix.to_cols_array().into_iter().chain([
            (light.shadow_bias + texel) / range,
            light.shadow_normal_bias,
            if enabled { 1. } else { 0. },
            1. / resolution as f32,
        ]));
        let unchanged = !target_changed && self.shadows.uniform_row == row;
        if !unchanged || !self.state_caching || !self.shadow_preparation_cache {
            gpu.queue.write_buffer(&self.shadows.uniform, 0, &row);
        }
        self.shadows.uniform_row = row;
        Ok((unchanged, reuse_depth && unchanged))
    }
    pub(super) fn draw_shadows(
        &self,
        encoder: &mut crate::profiling::Encoder,
        scene: &RenderScene,
        draws: &[PreparedDraw],
        batches: &[instancing::Batch],
        plan: Option<&sun_cache::Plan>,
    ) -> (usize, u64) {
        let descriptor = |view, label| wgpu::RenderPassDescriptor {
            label: Some(label),
            color_attachments: &[],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            ..Default::default()
        };
        let mut counts = (0, 0);
        if let Some(plan) = plan.filter(|p| p.rebuild) {
            let mut pass = encoder.begin_render_pass(&descriptor(
                self.shadows.sun_cache.depth(),
                "sun static shadow casters",
            ));
            pass.set_bind_group(1, &self.shadows.caster_binding, &[]);
            counts = self.draw_shadow_casters(
                &mut pass,
                draws,
                batches,
                None,
                false,
                Some(&plan.static_mask),
            );
        }
        let mut pass =
            encoder.begin_render_pass(&descriptor(&self.shadows.depth, "sun shadow casters"));
        if !scene.lighting.shadows || self.shadows.resolution == 1 {
            return counts;
        }
        if plan.is_some() {
            self.shadows.sun_cache.copy(&mut pass);
            counts.0 += 1;
            counts.1 += 1;
        }
        pass.set_bind_group(1, &self.shadows.caster_binding, &[]);
        let dynamic = self.draw_shadow_casters(
            &mut pass,
            draws,
            batches,
            None,
            false,
            plan.map(|p| p.dynamic_mask.as_slice()),
        );
        (counts.0 + dynamic.0, counts.1 + dynamic.1)
    }
    pub(super) fn draw_shadow_casters(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        draws: &[PreparedDraw],
        batches: &[instancing::Batch],
        projection: Option<Mat4>,
        point: bool,
        mask: Option<&[bool]>,
    ) -> (usize, u64) {
        let mut counts = (0, 0);
        let casts = |index: usize| {
            let draw = &draws[index];
            mask.is_none_or(|m| m[index])
                && !draw.transparent
                && draw.object.material.lit
                && (!self.culling
                    || projection.is_none_or(|p| {
                        self.frustum_visible(
                            self.mesh_for(&draw.object).bounds,
                            p * draw.object.model,
                        )
                    }))
        };
        let mut was_instanced = None;
        let mut submit = |index: usize, instances: std::ops::Range<u32>, slot: Option<usize>| {
            let draw = &draws[index];
            if casts(index) {
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
                    if self.instancing.shadow_batching() {
                        &self.instancing.shadow_bindings[slot].binding
                    } else {
                        &self.instancing.bindings[slot].binding
                    }
                });
                let mesh = self.mesh_for(&draw.object);
                pass.set_bind_group(0, binding, &[]);
                pass.set_vertex_buffer(0, mesh.vertices.slice(mesh.vertex_offset..));
                pass.set_index_buffer(mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
                let count = instances.end - instances.start;
                pass.draw_indexed(0..mesh.count, 0, instances);
                counts.0 += 1;
                counts.1 += u64::from(mesh.count / 3) * count as u64;
            }
        };
        let mut covered = vec![false; draws.len()];
        for batch in batches {
            for &index in &batch.indices {
                covered[index] = true;
            }
            // Instance indices address the original packed buffer, including a
            // nonzero first instance. Skip rejected members without repacking
            // uniforms or submitting extra triangles.
            if batch.slot.is_some() && self.instancing.shadow_batches_enabled {
                let mut start = None;
                for offset in 0..=batch.indices.len() {
                    if offset < batch.indices.len() && casts(batch.indices[offset]) {
                        start.get_or_insert(offset);
                    } else if let Some(first) = start.take() {
                        submit(
                            batch.indices[first],
                            first as u32..offset as u32,
                            batch.slot,
                        );
                    }
                }
            } else if batch.slot.is_some() && batch.indices.iter().all(|&i| casts(i)) {
                submit(batch.indices[0], 0..batch.indices.len() as u32, batch.slot);
            } else {
                for &index in &batch.indices {
                    submit(index, 0..1, None);
                }
            }
        }
        // The reference color-batch path also needs camera-culled casters.
        for (index, &covered) in covered.iter().enumerate() {
            if !covered {
                submit(index, 0..1, None);
            }
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

#[cfg(test)]
mod fit_cache_tests {
    use super::*;
    fn bits(value: Option<(Mat4, f32, f32)>) -> Option<Vec<u32>> {
        value.map(|(m, r, t)| {
            m.to_cols_array()
                .into_iter()
                .chain([r, t])
                .map(f32::to_bits)
                .collect()
        })
    }
    #[test]
    fn cached_extents_match_original_fit_bytes_through_edits() {
        let mut cache = sun_fit::Cache::default();
        let mut seed = 17u64;
        let mut random = || {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            ((seed >> 32) as u32 as f32 / u32::MAX as f32 - 0.5) * 40.
        };
        let bounds = [Vec3::splat(-0.5), Vec3::splat(0.5)];
        let mut inputs: Vec<_> = (0..64)
            .map(|_| {
                (
                    Mat4::from_translation(Vec3::new(random(), random(), random())),
                    bounds,
                    true,
                )
            })
            .collect();
        for tick in 0..5000 {
            let index = tick % inputs.len();
            inputs[index].0 = Mat4::from_translation(Vec3::new(random(), random(), random()))
                * Mat4::from_rotation_y(random())
                * Mat4::from_scale(Vec3::new(random(), random(), random()));
            if tick % 17 == 0 {
                inputs[index].2 = !inputs[index].2;
            }
            if tick % 29 == 0 {
                inputs[index].1[1].x += 0.125;
            }
            if tick % 31 == 0 {
                inputs.swap(1, 7);
            }
            let direction = match tick / 100 % 5 {
                0 => Vec3::Y,
                1 => -Vec3::Y,
                2 => Vec3::new(0.01, 1., 0.01).normalize(),
                3 => Vec3::new(0.4, 0.85, 0.35).normalize(),
                _ => Vec3::new(-0.3, -0.4, -0.5).normalize(),
            };
            let resolution = if tick % 3 == 0 { 512 } else { 2048 };
            let expected = fit(
                inputs
                    .iter()
                    .filter(|i| i.2)
                    .flat_map(|(m, b, _)| corners(*b).map(|p| m.transform_point3(p))),
                direction,
                resolution,
            );
            let result = cache.prepare(sun_view(direction), resolution, inputs.iter().copied());
            assert!(!result.fallback);
            assert_eq!(bits(result.fit), bits(expected), "tick {tick}");
            if tick % 100 != 0 && inputs.iter().filter(|i| i.2).count() > 4 {
                assert!(result.reused > 0, "tick {tick}");
            }
        }
        for scale in [0., -0., 1e-20, 1e20] {
            for item in &mut inputs {
                item.0 = Mat4::from_scale(Vec3::splat(scale));
                item.2 = true;
            }
            let result = cache.prepare(sun_view(Vec3::Z), 256, inputs.iter().copied());
            let expected = fit(
                inputs
                    .iter()
                    .flat_map(|(m, b, _)| corners(*b).map(|p| m.transform_point3(p))),
                Vec3::Z,
                256,
            );
            assert_eq!(bits(result.fit), bits(expected));
        }
        inputs.clear();
        assert!(
            cache
                .prepare(sun_view(Vec3::Z), 256, inputs.iter().copied())
                .fit
                .is_none()
        );
        inputs.push((
            Mat4::from_scale(Vec3::splat(3e38)),
            [Vec3::splat(-2.), Vec3::splat(2.)],
            true,
        ));
        assert!(
            cache
                .prepare(sun_view(Vec3::Z), 256, inputs.iter().copied())
                .fallback
        );
        inputs[0].0 = Mat4::IDENTITY;
        assert!(
            !cache
                .prepare(sun_view(Vec3::Z), 256, inputs.iter().copied())
                .fallback
        );
    }
}

#[cfg(test)]
mod metadata_tests {
    use super::*;
    fn draw(id: u64) -> PreparedDraw {
        PreparedDraw {
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
                motion_id: id,
                model: Mat4::from_translation(Vec3::X * id as f32),
                mesh: MeshKind::ModelPart(format!("mesh-{id}"), 0),
                material: Material {
                    metallic: None,
                    roughness: None,
                    surface_overrides: Default::default(),
                    tint: [1.; 3],
                    uv_scale: [1.; 2],
                    texture: TextureKind::Imported(format!("texture-{id}")),
                    lit: true,
                    shader: None,
                },
            },
        }
    }
    #[test]
    fn direct_classification_and_retained_metadata_match_original_snapshots() {
        let mut scene = RenderScene {
            skin_poses: Default::default(),
            shader_time: 0.,
            particles: vec![],
            fog: Default::default(),
            gi: None,
            lights: vec![LocalLight {
                directional: false,
                position: [0., 0., 1.],
                direction: [0., 0., -1.],
                color: [1.; 3],
                intensity: 2.,
                range: 10.,
                spot_angles: None,
                shadows: Some(Default::default()),
            }],
            environment: EnvironmentSettings::disabled(),
            display: Default::default(),
            lighting: Default::default(),
            view_projection: Mat4::IDENTITY,
            items: vec![],
        };
        let mut spot = scene.lights[0];
        spot.spot_angles = Some([20., 35.]);
        scene.lights.push(spot);
        let mut draws: Vec<_> = (0..67).map(draw).collect();
        let mut culling = true;
        let mut retained = ShadowFrame::new(&scene, &draws, culling);
        let mut seed = 23u64;
        for tick in 0..5000 {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            let index = (seed >> 32) as usize % draws.len();
            match tick % 20 {
                0 => draws[index].object.model *= Mat4::from_translation(Vec3::X * 0.1),
                1 => draws[index].transparent = !draws[index].transparent,
                2 => draws[index].object.material.lit = !draws[index].object.material.lit,
                3 => draws[index].object.material.uv_scale[0] += 0.1,
                4 => {
                    draws[index].object.material.texture =
                        TextureKind::Imported(format!("updated-{tick}"))
                }
                5 => {
                    draws[index].object.mesh =
                        MeshKind::ModelPart(format!("updated-{tick}"), tick % 3)
                }
                6 => draws[index].deformation = draws[index].deformation.wrapping_add(1),
                7 => draws[index].opacity = (tick % 100) as f32 / 100.,
                8 => draws[index].cutoff = (tick % 90) as f32 / 100.,
                9 => draws.swap(index, 0),
                10 => draws.push(draw(tick as u64 + 10000)),
                11 => {
                    if draws.len() > 1 {
                        draws.remove(index);
                    }
                }
                12 => scene.lighting.shadow_bias += 0.00001,
                13 => scene.lighting.sun_direction[0] += 0.01,
                14 => scene.lights[0].position[0] += 0.1,
                15 => {
                    scene.lights[0].intensity = if scene.lights[0].intensity > 0. {
                        0.
                    } else {
                        2.
                    }
                }
                16 => scene.lights.swap(0, 1),
                17 => culling = !culling,
                18 => scene.lighting.shadows = !scene.lighting.shadows,
                _ => {
                    scene.lights[1].shadows.as_mut().unwrap().normal_bias += 0.001;
                }
            }
            let original = ShadowFrame::new(&scene, &draws, culling);
            let result = retained.compare(&scene, &draws, culling);
            assert_eq!(result.whole, retained == original, "whole at {tick}");
            assert_eq!(result.sun, retained.same_sun(&original), "sun at {tick}");
            assert_eq!(
                result.opaque,
                retained.same_local_casters(&original),
                "opaque at {tick}"
            );
            assert_eq!(
                result.stable_mask,
                retained.stable_casters(&draws, culling),
                "mask at {tick}"
            );
            let mut stats = FrameStats::default();
            retained.refresh(&scene, &draws, culling, &result.unchanged, &mut stats);
            assert!(retained == original, "refreshed at {tick}");
            assert_eq!(
                stats.shadow_metadata_built_casters
                    + stats.shadow_metadata_updated_casters
                    + stats.shadow_metadata_reused_casters,
                draws.iter().filter(|d| d.object.material.lit).count()
            );
        }
        let comparison = retained.compare(&scene, &draws, culling);
        assert!(comparison.whole);
        let mut stats = FrameStats::default();
        retained.refresh(&scene, &draws, culling, &comparison.unchanged, &mut stats);
        assert_eq!(stats.shadow_metadata_key_clones, 0);
        assert_eq!(stats.shadow_metadata_built_casters, 0);
        assert_eq!(stats.shadow_metadata_updated_casters, 0);
    }
}
