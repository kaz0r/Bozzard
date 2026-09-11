use super::*;

const RESOLUTION: u32 = 1024;
pub(super) const UNIFORM_SIZE: u64 = MAX_SHADOWED_SPOT_LIGHTS as u64 * 80;

struct Caster {
    uniform: wgpu::Buffer,
    binding: wgpu::BindGroup,
}
pub(super) struct SpotShadows {
    pub uniform: wgpu::Buffer,
    pub depth: wgpu::TextureView,
    layers: Vec<wgpu::TextureView>,
    casters: Vec<Caster>,
    matrices: Vec<Mat4>,
}
fn target(gpu: &Gpu, count: usize) -> (wgpu::TextureView, Vec<wgpu::TextureView>) {
    let resolution = if count == 0 { 1 } else { RESOLUTION };
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("spotlight shadow depth array"),
        size: wgpu::Extent3d {
            width: resolution,
            height: resolution,
            depth_or_array_layers: count.max(1) as u32,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Depth32Float,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let depth = texture.create_view(&wgpu::TextureViewDescriptor {
        dimension: Some(wgpu::TextureViewDimension::D2Array),
        ..Default::default()
    });
    let layers = (0..count)
        .map(|i| {
            texture.create_view(&wgpu::TextureViewDescriptor {
                dimension: Some(wgpu::TextureViewDimension::D2),
                base_array_layer: i as u32,
                array_layer_count: Some(1),
                ..Default::default()
            })
        })
        .collect();
    (depth, layers)
}

/// The cone's local -Z uses WebGPU's 0..1 perspective depth, independent of the camera.
fn projection(light: &LocalLight) -> Result<Mat4> {
    light.validate()?;
    let outer = light
        .spot_angles
        .context("shadow projection needs a spotlight")?[1];
    let direction = Vec3::from(light.direction).normalize();
    let up = if direction.y.abs() > 0.99 {
        Vec3::Z
    } else {
        Vec3::Y
    };
    let near = (light.range * 0.001).min(0.05);
    let view = glam::camera::rh::view::look_to_mat4(Vec3::from(light.position), direction, up);
    let matrix = glam::camera::rh::proj::directx::perspective(
        2. * outer.to_radians(),
        1.,
        near,
        light.range,
    ) * view;
    ensure!(matrix.is_finite(), "invalid spotlight shadow projection");
    Ok(matrix)
}
impl SpotShadows {
    pub fn new(gpu: &Gpu, caster_layout: &wgpu::BindGroupLayout) -> Self {
        let uniform = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("spotlight shadow receivers"),
            size: UNIFORM_SIZE,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let casters = (0..MAX_SHADOWED_SPOT_LIGHTS)
            .map(|_| {
                let uniform = gpu.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("spotlight shadow caster"),
                    size: 80,
                    usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                let binding = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("spotlight shadow caster"),
                    layout: caster_layout,
                    entries: &[wgpu::BindGroupEntry {
                        binding: 0,
                        resource: uniform.as_entire_binding(),
                    }],
                });
                Caster { uniform, binding }
            })
            .collect();
        let (depth, layers) = target(gpu, 0);
        Self {
            uniform,
            depth,
            layers,
            casters,
            matrices: Vec::new(),
        }
    }
    /// The slot order is shared with local_lights::uniform; no stable-ID or stale-layer state.
    pub fn update(&mut self, gpu: &Gpu, lights: &[LocalLight]) -> Result<bool> {
        let active: Vec<_> = lights.iter().filter(|l| l.casts_shadow()).collect();
        ensure!(
            active.len() <= MAX_SHADOWED_SPOT_LIGHTS,
            "too many spotlight shadow maps"
        );
        let matrices: Vec<_> = active
            .iter()
            .map(|l| projection(l))
            .collect::<Result<_>>()?;
        let changed = active.len() != self.layers.len();
        if changed {
            ensure!(
                RESOLUTION <= gpu.device.limits().max_texture_dimension_2d
                    && active.len() as u32 <= gpu.device.limits().max_texture_array_layers,
                "spotlight shadow maps exceed device limits"
            );
            (self.depth, self.layers) = target(gpu, active.len());
        }
        let mut bytes = vec![0; UNIFORM_SIZE as usize];
        for (slot, (light, matrix)) in active.iter().zip(&matrices).enumerate() {
            let shadow = light.shadows.unwrap();
            let row = float_bytes(matrix.to_cols_array().into_iter().chain([
                shadow.bias,
                shadow.normal_bias,
                0.,
                1. / RESOLUTION as f32,
            ]));
            bytes[slot * 80..(slot + 1) * 80].copy_from_slice(&row);
            // Each pass has its own buffer: queue writes all precede command execution.
            gpu.queue.write_buffer(&self.casters[slot].uniform, 0, &row);
        }
        gpu.queue.write_buffer(&self.uniform, 0, &bytes);
        self.matrices = matrices;
        Ok(changed)
    }
}
impl SceneRenderer {
    pub(super) fn update_spot_shadows(&mut self, gpu: &Gpu, scene: &RenderScene) -> Result<()> {
        if self.shadows.spots.update(gpu, &scene.lights)? {
            self.shadows.rebind(gpu);
        }
        Ok(())
    }
    pub(super) fn draw_spot_shadows(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        draws: &[PreparedDraw],
    ) -> (usize, u64) {
        let mut counts = (0, 0);
        for (slot, matrix) in self.shadows.spots.matrices.iter().enumerate() {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("spotlight shadow casters"),
                color_attachments: &[],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.shadows.spots.layers[slot],
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });
            pass.set_pipeline(&self.shadows.pipeline);
            pass.set_bind_group(1, &self.shadows.spots.casters[slot].binding, &[]);
            let (draws, triangles) = self.draw_shadow_casters(&mut pass, draws, Some(*matrix));
            counts.0 += draws;
            counts.1 += triangles;
        }
        counts
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cone_projection_tracks_position_direction_and_webgpu_depth() {
        for direction in [
            Vec3::NEG_Z,
            Vec3::Y,
            -Vec3::Y,
            Vec3::new(1., 2., -3.).normalize(),
        ] {
            for range in [0.001_f32, 10., 100_000.] {
                for angle in [0.1_f32, 30., 89.9] {
                    // A tiny cone/range at a large origin would exceed f32 world precision.
                    let position = Vec3::new(0.2, -0.1, 0.3) * range.min(10.);
                    let light = LocalLight {
                        position: position.to_array(),
                        direction: direction.to_array(),
                        color: [1.; 3],
                        intensity: 1.,
                        range,
                        spot_angles: Some([0., angle]),
                        shadows: Some(Default::default()),
                    };
                    let matrix = projection(&light).unwrap();
                    let near = (range * 0.001).min(0.05);
                    for (distance, depth) in [(near, 0.), (range, 1.)] {
                        let p = matrix.project_point3(position + direction * distance);
                        // Narrow cones amplify f32 translation cancellation near the light.
                        // Check depth here and check axis placement at the far plane.
                        assert!((p.z - depth).abs() < 0.002, "{p:?} {range} {angle}");
                        if depth == 1. {
                            assert!(p.x.abs() < 0.002 && p.y.abs() < 0.002);
                        }
                    }
                    let up = if direction.y.abs() > 0.99 {
                        Vec3::Z
                    } else {
                        Vec3::Y
                    };
                    let side = direction.cross(up).normalize();
                    let edge = position
                        + direction * range * 0.5
                        + side * (range * 0.5 * angle.to_radians().tan());
                    assert!((matrix.project_point3(edge).x.abs() - 1.).abs() < 0.002);
                    assert!((matrix * (position - direction * range).extend(1.)).w < 0.);
                }
            }
        }
    }
}
