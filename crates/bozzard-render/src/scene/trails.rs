//! Trail Renderer ribbons. The CPU turns each trail's points into a camera-facing strip
//! every frame, two vertices per point so neighbouring segments share their joint, and one
//! upload feeds an alpha-blended and an additive draw in a short pass of their own.
use super::*;
use glam::Vec4;

/// One point of a trail, ready to draw: world position, full width and linear RGBA.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TrailPoint {
    pub position: Vec3,
    pub width: f32,
    pub color: [f32; 4],
}
/// A trail as the renderer draws it, oldest point first.
#[derive(Clone, Debug, PartialEq)]
pub struct TrailRibbon {
    pub points: Vec<TrailPoint>,
    /// Light added to the scene rather than ordinary transparency.
    pub additive: bool,
}

/// Bounds one frame's upload: 4096 full-length trails.
const MAX_VERTICES: usize = 1 << 22;
/// Position, edge, RGBA.
const FLOATS: usize = 8;

pub(super) struct Trails {
    alpha: wgpu::RenderPipeline,
    additive: wgpu::RenderPipeline,
    uniform: wgpu::Buffer,
    binding: wgpu::BindGroup,
    vertices: wgpu::Buffer,
    capacity: usize,
    /// Vertices of the alpha ribbons, then of the additive ones.
    counts: [u32; 2],
}

/// Which way "towards the camera" points at a world position.
#[derive(Clone, Copy)]
enum Viewer {
    Eye(Vec3),
    /// Orthographic: the direction back toward the viewer.
    Toward(Vec3),
}
impl Viewer {
    fn of(view_projection: Mat4) -> Self {
        // The eye is the one point a perspective projection sends to w = 0; an orthographic
        // projection has none, and the same column is its viewing direction instead.
        let h = view_projection.inverse() * Vec4::new(0.0, 0.0, 1.0, 0.0);
        if h.w.abs() > 1e-8 * h.truncate().length() {
            Self::Eye(h.truncate() / h.w)
        } else {
            Self::Toward(-h.truncate().normalize_or_zero())
        }
    }
    fn toward(self, position: Vec3) -> Vec3 {
        match self {
            Self::Eye(eye) => eye - position,
            Self::Toward(direction) => direction,
        }
    }
}

/// Triangle-list vertices for `ribbons`, alpha ribbons first. Returns the alpha count.
pub(super) fn build(
    ribbons: &[TrailRibbon],
    view_projection: Mat4,
    out: &mut Vec<f32>,
) -> Result<[u32; 2]> {
    out.clear();
    let viewer = Viewer::of(view_projection);
    let mut counts = [0u32; 2];
    for (slot, additive) in [false, true].into_iter().enumerate() {
        for ribbon in ribbons.iter().filter(|r| r.additive == additive) {
            let points = &ribbon.points;
            ensure!(
                points.iter().all(|p| p.position.is_finite()
                    && p.width.is_finite()
                    && p.color.iter().all(|c| c.is_finite())),
                "trail points must be finite"
            );
            if points.len() < 2 {
                continue;
            }
            // Each point's half-width offset across the strip, facing the camera.
            let mut sides = Vec::with_capacity(points.len());
            let mut last = Vec3::ZERO;
            for (i, point) in points.iter().enumerate() {
                let before = points[i.saturating_sub(1)].position;
                let after = points[(i + 1).min(points.len() - 1)].position;
                let side = (after - before)
                    .cross(viewer.toward(point.position))
                    .normalize_or_zero();
                // Seen end-on there is no facing side; keep the previous one.
                let side = if side == Vec3::ZERO { last } else { side };
                last = side;
                sides.push(side * point.width.max(0.0) * 0.5);
            }
            let corner = |i: usize, edge: f32, out: &mut Vec<f32>| {
                let p = points[i];
                let at = p.position + sides[i] * edge;
                out.extend_from_slice(&[at.x, at.y, at.z, edge]);
                out.extend_from_slice(&p.color);
            };
            for i in 0..points.len() - 1 {
                for (point, edge) in [
                    (i, -1.),
                    (i, 1.),
                    (i + 1, -1.),
                    (i, 1.),
                    (i + 1, 1.),
                    (i + 1, -1.),
                ] {
                    corner(point, edge, out);
                }
                counts[slot] += 6;
            }
            ensure!(
                out.len() / FLOATS <= MAX_VERTICES,
                "trail ribbons exceed {MAX_VERTICES} vertices this frame"
            );
        }
    }
    Ok(counts)
}

impl Trails {
    pub(super) fn new(gpu: &Gpu) -> Self {
        let layout = gpu
            .device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("trail camera"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(64),
                    },
                    count: None,
                }],
            });
        let pipeline_layout = gpu
            .device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("trail ribbons"),
                bind_group_layouts: &[Some(&layout)],
                immediate_size: 0,
            });
        let shader = gpu
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("trail ribbons"),
                source: wgpu::ShaderSource::Wgsl(include_str!("trails.wgsl").into()),
            });
        let pipeline = |label, blend| {
            gpu.device
                .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some(label),
                    layout: Some(&pipeline_layout),
                    vertex: wgpu::VertexState {
                        module: &shader,
                        entry_point: Some("vs_main"),
                        compilation_options: Default::default(),
                        buffers: &[Some(wgpu::VertexBufferLayout {
                            array_stride: (FLOATS * 4) as u64,
                            step_mode: wgpu::VertexStepMode::Vertex,
                            attributes: &wgpu::vertex_attr_array![
                                0 => Float32x3,
                                1 => Float32,
                                2 => Float32x4
                            ],
                        })],
                    },
                    fragment: Some(wgpu::FragmentState {
                        module: &shader,
                        entry_point: Some("fs_main"),
                        compilation_options: Default::default(),
                        targets: &[Some(wgpu::ColorTargetState {
                            format: wgpu::TextureFormat::Rgba16Float,
                            blend: Some(blend),
                            write_mask: wgpu::ColorWrites::ALL,
                        })],
                    }),
                    primitive: Default::default(),
                    depth_stencil: Some(wgpu::DepthStencilState {
                        format: wgpu::TextureFormat::Depth32Float,
                        depth_write_enabled: Some(false),
                        depth_compare: Some(wgpu::CompareFunction::LessEqual),
                        stencil: Default::default(),
                        bias: Default::default(),
                    }),
                    multisample: Default::default(),
                    multiview_mask: None,
                    cache: None,
                })
        };
        let additive = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::SrcAlpha,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::Zero,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
        };
        let uniform = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("trail camera"),
            size: 64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let binding = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("trail camera"),
            layout: &layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform.as_entire_binding(),
            }],
        });
        Self {
            alpha: pipeline("alpha trail ribbons", wgpu::BlendState::ALPHA_BLENDING),
            additive: pipeline("additive trail ribbons", additive),
            uniform,
            binding,
            vertices: Self::vertex_buffer(gpu, 1024),
            capacity: 1024,
            counts: [0; 2],
        }
    }
    fn vertex_buffer(gpu: &Gpu, vertices: usize) -> wgpu::Buffer {
        gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("trail ribbon vertices"),
            size: (vertices * FLOATS * 4) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }
    /// Build and upload this frame's ribbons. Returns the number of triangles.
    pub(super) fn prepare(
        &mut self,
        gpu: &Gpu,
        ribbons: &[TrailRibbon],
        view_projection: Mat4,
        scratch: &mut Vec<f32>,
    ) -> Result<u64> {
        self.counts = build(ribbons, view_projection, scratch)?;
        let vertices = scratch.len() / FLOATS;
        if vertices > self.capacity {
            self.capacity = vertices.next_power_of_two();
            self.vertices = Self::vertex_buffer(gpu, self.capacity);
        }
        if vertices > 0 {
            gpu.queue
                .write_buffer(&self.vertices, 0, &float_bytes(scratch.iter().copied()));
            gpu.queue.write_buffer(
                &self.uniform,
                0,
                &float_bytes(view_projection.to_cols_array()),
            );
        }
        Ok(vertices as u64 / 3)
    }
    pub(super) fn has_geometry(&self) -> bool {
        self.counts.iter().any(|&count| count > 0)
    }
    pub(super) fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
        pass.set_bind_group(0, &self.binding, &[]);
        pass.set_vertex_buffer(0, self.vertices.slice(..));
        let [alpha, additive] = self.counts;
        if alpha > 0 {
            pass.set_pipeline(&self.alpha);
            pass.draw(0..alpha, 0..1);
        }
        if additive > 0 {
            pass.set_pipeline(&self.additive);
            pass.draw(alpha..alpha + additive, 0..1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ribbon(xs: &[f32], additive: bool) -> TrailRibbon {
        TrailRibbon {
            points: xs
                .iter()
                .map(|&x| TrailPoint {
                    position: Vec3::new(x, 0.0, 0.0),
                    width: 2.0,
                    color: [1.0, 0.5, 0.25, 1.0],
                })
                .collect(),
            additive,
        }
    }

    #[test]
    fn the_shader_validates() {
        let source = include_str!("trails.wgsl");
        let module = wgpu::naga::front::wgsl::parse_str(source)
            .unwrap_or_else(|e| panic!("{}", e.emit_to_string(source)));
        wgpu::naga::valid::Validator::new(
            wgpu::naga::valid::ValidationFlags::all(),
            wgpu::naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .unwrap();
    }

    #[test]
    fn ribbons_face_the_camera_and_alpha_draws_first() {
        // Looking down -Z from z = 5: a ribbon along X spreads along Y, across the view.
        let view_projection = glam::camera::rh::proj::directx::perspective(1.0, 1.0, 0.1, 100.0)
            * glam::camera::rh::view::look_at_mat4(Vec3::new(0.0, 0.0, 5.0), Vec3::ZERO, Vec3::Y);
        let mut out = Vec::new();
        let counts = build(
            &[
                ribbon(&[0.0, 1.0, 2.0], true),
                ribbon(&[0.0, 1.0], false),
                ribbon(&[3.0], false),
            ],
            view_projection,
            &mut out,
        )
        .unwrap();
        // Two segments for the additive ribbon, one for the alpha one, none for a lone point.
        assert_eq!(counts, [6, 12]);
        assert_eq!(out.len(), 18 * FLOATS);
        let vertex = |i: usize| &out[i * FLOATS..(i + 1) * FLOATS];
        // The first vertex is the alpha ribbon's start, half its width off the axis.
        assert_eq!(vertex(0)[..4], [0.0, 1.0, 0.0, -1.0]);
        assert_eq!(vertex(1)[..4], [0.0, -1.0, 0.0, 1.0]);
        assert_eq!(vertex(0)[4..], [1.0, 0.5, 0.25, 1.0]);
        for i in 0..18 {
            assert_eq!(vertex(i)[2], 0.0, "ribbon left the camera-facing plane");
        }
        // Orthographic from above: the same ribbon spreads along Z instead.
        let top = glam::camera::rh::proj::directx::orthographic(-5.0, 5.0, -5.0, 5.0, 0.1, 100.0)
            * glam::camera::rh::view::look_at_mat4(Vec3::new(0.0, 10.0, 0.0), Vec3::ZERO, Vec3::Z);
        build(&[ribbon(&[0.0, 1.0], false)], top, &mut out).unwrap();
        assert!(
            (out[2].abs() - 1.0).abs() < 1e-5 && out[1].abs() < 1e-5,
            "{:?}",
            &out[..4]
        );
    }

    #[test]
    fn non_finite_points_are_rejected() {
        let mut broken = ribbon(&[0.0, 1.0], false);
        broken.points[1].position.x = f32::NAN;
        assert!(build(&[broken], Mat4::IDENTITY, &mut Vec::new()).is_err());
    }
}
