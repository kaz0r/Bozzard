//! Ordered UI/text composition after display effects. Uniform storage and texture bindings are reused.
use super::*;
use std::collections::HashMap;
pub(super) struct HudRenderer {
    pipeline: wgpu::RenderPipeline,
    sampler: wgpu::Sampler,
    encode: bool,
    uniform: wgpu::Buffer,
    stride: u64,
    capacity: usize,
    bytes: Vec<u8>,
    bindings: HashMap<TextureKind, wgpu::BindGroup>,
}
impl HudRenderer {
    pub fn new(gpu: &Gpu, format: wgpu::TextureFormat) -> Self {
        let shader = gpu
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("authored UI overlay"),
                source: wgpu::ShaderSource::Wgsl(include_str!("hud.wgsl").into()),
            });
        let layout = gpu
            .device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("UI inputs"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: true,
                            min_binding_size: wgpu::BufferSize::new(96),
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
            });
        let pipeline_layout = gpu
            .device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("UI layout"),
                bind_group_layouts: &[Some(&layout)],
                immediate_size: 0,
            });
        let pipeline=gpu.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor{label:Some("UI overlay"),layout:Some(&pipeline_layout),vertex:wgpu::VertexState{module:&shader,entry_point:Some("vs_main"),compilation_options:Default::default(),buffers:&[Some(wgpu::VertexBufferLayout{array_stride:32,step_mode:wgpu::VertexStepMode::Vertex,attributes:&wgpu::vertex_attr_array![0=>Float32x3,1=>Float32x3,2=>Float32x2]})]},fragment:Some(wgpu::FragmentState{module:&shader,entry_point:Some("fs_main"),compilation_options:Default::default(),targets:&[Some(wgpu::ColorTargetState{format,blend:Some(wgpu::BlendState::ALPHA_BLENDING),write_mask:wgpu::ColorWrites::ALL})]}),primitive:Default::default(),depth_stencil:None,multisample:Default::default(),multiview_mask:None,cache:None});
        let stride = 96u64.div_ceil(u64::from(
            gpu.device.limits().min_uniform_buffer_offset_alignment,
        )) * u64::from(gpu.device.limits().min_uniform_buffer_offset_alignment);
        Self {
            pipeline,
            sampler: gpu.device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("UI image sampler"),
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                ..Default::default()
            }),
            encode: !format.is_srgb(),
            uniform: Self::buffer(gpu, stride),
            stride,
            capacity: 1,
            bytes: Vec::new(),
            bindings: HashMap::new(),
        }
    }
    fn buffer(gpu: &Gpu, size: u64) -> wgpu::Buffer {
        gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("UI uniform arena"),
            size,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }
    pub fn invalidate(&mut self) {
        self.bindings.clear();
    }
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &mut self,
        gpu: &Gpu,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        size: [u32; 2],
        scene: &RenderScene,
        renderer: &SceneRenderer,
        raw: bool,
        scale: f32,
    ) -> Result<()> {
        let mut draws = Vec::new();
        let mut textures = std::collections::HashSet::new();
        for item in &scene.items {
            let (mesh, screen, opacity, clip) = match &item.mesh {
                MeshKind::Text(text) => {
                    let Some(screen) = text.screen else {
                        continue;
                    };
                    let Some(mesh) = renderer.text.as_ref().and_then(|r| r.mesh(text)) else {
                        continue;
                    };
                    (mesh, screen, text.opacity, text.clip)
                }
                MeshKind::Sprite(sprite) => {
                    let Some(screen) = sprite.screen else {
                        continue;
                    };
                    let Some(mesh) = renderer.sprites.mesh(sprite) else {
                        continue;
                    };
                    (mesh, screen, sprite.opacity, sprite.clip)
                }
                _ => continue,
            };
            ensure!(
                item.material
                    .tint
                    .iter()
                    .all(|v| v.is_finite() && (0.0..=1.).contains(v)),
                "invalid UI tint"
            );
            let scissor = if let Some([x, y, w, h]) = clip {
                let x0 = (x * scale).floor().clamp(0., size[0] as f32) as u32;
                let y0 = (y * scale).floor().clamp(0., size[1] as f32) as u32;
                let x1 = ((x + w) * scale).ceil().clamp(0., size[0] as f32) as u32;
                let y1 = ((y + h) * scale).ceil().clamp(0., size[1] as f32) as u32;
                if x1 <= x0 || y1 <= y0 {
                    continue;
                }
                [x0, y0, x1 - x0, y1 - y0]
            } else {
                [0, 0, size[0], size[1]]
            };
            textures.insert(item.material.texture.clone());
            draws.push((mesh, screen, opacity, &item.material, scissor));
        }
        ensure!(draws.len() <= 4096, "UI view exceeds 4096 draws");
        if draws.len() > self.capacity {
            self.capacity = draws.len().next_power_of_two();
            self.uniform = Self::buffer(gpu, self.stride * self.capacity as u64);
            self.bindings.clear();
        }
        self.bindings.retain(|key, _| textures.contains(key));
        let layout = self.pipeline.get_bind_group_layout(0);
        for key in textures {
            if self.bindings.contains_key(&key) {
                continue;
            }
            let texture = renderer.texture_view(&key)?;
            let binding = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("UI texture"),
                layout: &layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                            buffer: &self.uniform,
                            offset: 0,
                            size: wgpu::BufferSize::new(96),
                        }),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(texture),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::Sampler(&self.sampler),
                    },
                ],
            });
            self.bindings.insert(key, binding);
        }
        self.bytes.clear();
        self.bytes.resize(draws.len() * self.stride as usize, 0);
        for (i, (_, screen, opacity, material, _)) in draws.iter().enumerate() {
            let values = screen
                .matrix(size, scale)
                .to_cols_array()
                .into_iter()
                .chain(material.tint)
                .chain([
                    *opacity,
                    if self.encode && !raw { 1. } else { 0. },
                    0.,
                    0.,
                    0.,
                ]);
            for (j, value) in values.enumerate() {
                self.bytes[i * self.stride as usize + j * 4..i * self.stride as usize + j * 4 + 4]
                    .copy_from_slice(&value.to_le_bytes());
            }
        }
        if !self.bytes.is_empty() {
            gpu.queue.write_buffer(&self.uniform, 0, &self.bytes);
        }
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("authored UI after display"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        pass.set_pipeline(&self.pipeline);
        for (i, (mesh, _, _, material, clip)) in draws.iter().enumerate() {
            pass.set_bind_group(
                0,
                &self.bindings[&material.texture],
                &[(i as u64 * self.stride) as u32],
            );
            pass.set_scissor_rect(clip[0], clip[1], clip[2], clip[3]);
            pass.set_vertex_buffer(0, mesh.vertices.slice(mesh.vertex_offset..));
            pass.set_index_buffer(mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..mesh.count, 0, 0..1);
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hud_pixels_ignore_camera_and_effects_follow_resize_and_release_resources() -> Result<()> {
        let instance = crate::instance(crate::Backend::native());
        let gpu = pollster::block_on(Gpu::request(
            &instance,
            None,
            cfg!(not(target_os = "macos")),
        ))?;
        let mut renderer = SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm);
        let mut scene = RenderScene {
            skin_poses: Default::default(),
            particles: vec![],
            fog: Default::default(),
            gi: None,
            lights: vec![],
            environment: Default::default(),
            display: Default::default(),
            lighting: Default::default(),
            view_projection: Mat4::IDENTITY,
            shader_time: 0.,
            items: vec![DrawItem {
                motion_id: 0,
                model: Mat4::from_translation(Vec3::splat(1000.)),
                mesh: MeshKind::Text(TextMesh {
                    text: "HUD 42".into(),
                    font_size: 28.,
                    screen: Some(ScreenText {
                        anchor: [1., 0.],
                        offset: [-20., 20.],
                    }),
                    alignment: TextAlignment::Right,
                    ..Default::default()
                }),
                material: Material {
                    metallic: None,
                    roughness: None,
                    surface_overrides: Default::default(),
                    tint: [0., 1., 0.],
                    uv_scale: [1.; 2],
                    texture: TextureKind::Text,
                    lit: false,
                    shader: None,
                },
            }],
        };
        let mut occluder = scene.items[0].clone();
        occluder.mesh = MeshKind::Quad;
        occluder.material.texture = TextureKind::White;
        occluder.material.tint = [0.1; 3];
        occluder.model = Mat4::from_scale_rotation_translation(
            Vec3::new(3., 3., 1.),
            glam::Quat::IDENTITY,
            Vec3::new(0., 0., 0.1),
        );
        scene.items.push(occluder);
        let capture = |renderer: &mut SceneRenderer, scene: &RenderScene, w, h| {
            crate::capture_offscreen(&gpu, w, h, |target| {
                renderer.draw(&gpu, target, [w, h], scene)
            })
        };
        let coordinates = |image: &crate::Frame| -> Vec<[usize; 2]> {
            image
                .rgba
                .chunks_exact(4)
                .enumerate()
                .filter(|(_, p)| p[1] > 180 && p[0] < 20 && p[2] < 20)
                .map(|(i, _)| [i % image.width as usize, i / image.width as usize])
                .collect()
        };
        let a = capture(&mut renderer, &scene, 320, 240)?;
        let original = coordinates(&a);
        assert!(original.len() > 80);
        assert!(
            original
                .iter()
                .all(|p| p[0] > 180 && p[0] < 301 && p[1] >= 20 && p[1] < 65)
        );
        scene.view_projection = Mat4::from_translation(Vec3::splat(4.));
        scene.display.exposure_ev = -8.;
        scene.display.vignette.intensity = 1.;
        let same_glyphs = |actual: Vec<[usize; 2]>, expected: Vec<[usize; 2]>| {
            let actual: BTreeSet<_> = actual.into_iter().collect();
            let expected: BTreeSet<_> = expected.into_iter().collect();
            // Antialiased edges blend with the world behind them; changed background
            // brightness can move the color threshold by a few edge pixels.
            assert!(actual.intersection(&expected).count() * 100 >= expected.len() * 95);
            assert!(actual.symmetric_difference(&expected).count() * 100 <= expected.len() * 10);
        };
        same_glyphs(
            coordinates(&capture(&mut renderer, &scene, 320, 240)?),
            original.clone(),
        );
        let resized = coordinates(&capture(&mut renderer, &scene, 640, 360)?);
        same_glyphs(
            resized,
            original.iter().map(|p| [p[0] + 320, p[1]]).collect(),
        );
        if let MeshKind::Text(text) = &mut scene.items[0].mesh {
            text.text = "HUD 99".into();
        }
        assert_ne!(
            coordinates(&capture(&mut renderer, &scene, 320, 240)?),
            original
        );
        scene.items.clear();
        assert!(coordinates(&capture(&mut renderer, &scene, 320, 240)?).is_empty());
        assert!(renderer.text.is_none() && renderer.hud.is_none());
        println!("hud_gpu_ok anchoring resize camera_effect_isolation dynamic_text cleanup");
        Ok(())
    }
}
