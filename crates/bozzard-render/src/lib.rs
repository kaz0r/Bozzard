//! Native WebGPU renderer. No dependency on the ECS, simulation, or a window toolkit.
pub use wgpu;
mod mipmap;
mod pbr;
mod scene;
pub use pbr::{MaterialMap, ModelShading};
pub use scene::{
    DisplaySettings, DrawItem, EnvironmentSettings, Lighting, Material, MeshKind, ModelImage,
    ModelPart, ModelUploadStats, PendingUpload, RenderScene, SceneRenderer, TextureKind,
    UploadContext, UploadData, UploadProgress, UploadSource,
};

use anyhow::{Context, Result, ensure};
use std::{io::Write, path::Path, sync::mpsc, time::Duration};

/// Explicit backend selection makes CI fail instead of silently testing a different API.
#[derive(Debug, Clone, Copy)]
pub enum Backend {
    Metal,
    Vulkan,
    Dx12,
}

impl Backend {
    pub fn native() -> Self {
        if cfg!(target_os = "macos") {
            Self::Metal
        } else if cfg!(target_os = "windows") {
            Self::Dx12
        } else {
            Self::Vulkan
        }
    }
    fn flag(self) -> wgpu::Backends {
        match self {
            Self::Metal => wgpu::Backends::METAL,
            Self::Vulkan => wgpu::Backends::VULKAN,
            Self::Dx12 => wgpu::Backends::DX12,
        }
    }
}

impl std::str::FromStr for Backend {
    type Err = anyhow::Error;
    fn from_str(value: &str) -> Result<Self> {
        match value {
            "metal" => Ok(Self::Metal),
            "vulkan" => Ok(Self::Vulkan),
            "dx12" => Ok(Self::Dx12),
            _ => anyhow::bail!("unknown backend '{value}'; use metal, vulkan, or dx12"),
        }
    }
}

pub fn instance(backend: Backend) -> wgpu::Instance {
    let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
    descriptor.backends = backend.flag();
    descriptor.flags |= wgpu::InstanceFlags::VALIDATION;
    wgpu::Instance::new(descriptor)
}

#[derive(Clone)]
pub struct Gpu {
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
}

impl Gpu {
    /// Hardware jobs must reject software adapters even if driver discovery falls back.
    pub fn require_hardware(&self) -> Result<()> {
        ensure!(
            matches!(
                self.adapter.get_info().device_type,
                wgpu::DeviceType::IntegratedGpu | wgpu::DeviceType::DiscreteGpu
            ),
            "an integrated or discrete GPU is required for this hardware check"
        );
        Ok(())
    }

    pub async fn request(
        instance: &wgpu::Instance,
        surface: Option<&wgpu::Surface<'_>>,
        software: bool,
    ) -> Result<Self> {
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                force_fallback_adapter: software,
                compatible_surface: surface,
                ..Default::default()
            })
            .await
            .context("no compatible graphics adapter (graphics checks must not skip)")?;
        let info = adapter.get_info();
        println!(
            "adapter={:?} backend={:?} type={:?} driver={:?} driver_info={:?}",
            info.name, info.backend, info.device_type, info.driver, info.driver_info
        );
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("Bozzard device"),
                required_features: wgpu::Features::empty(),
                // Keep baseline features while allowing native/Retina-sized render targets.
                required_limits: wgpu::Limits::downlevel_defaults()
                    .using_resolution(adapter.limits()),
                ..Default::default()
            })
            .await
            .context("creating graphics device")?;
        Ok(Self {
            adapter,
            device,
            queue,
        })
    }

    pub fn wait(&self) -> Result<()> {
        self.device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: Some(Duration::from_secs(30)),
            })
            .context("GPU completion timeout or device failure")?;
        Ok(())
    }
}

/// First rendering primitive; accepts presentation data rather than borrowing the world.
pub struct TriangleRenderer {
    pipeline: wgpu::RenderPipeline,
    vertices: wgpu::Buffer,
}

impl TriangleRenderer {
    pub fn new(gpu: &Gpu, format: wgpu::TextureFormat) -> Self {
        let shader = gpu
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("Bozzard triangle WGSL"),
                source: wgpu::ShaderSource::Wgsl(include_str!("triangle.wgsl").into()),
            });
        let pipeline = gpu
            .device
            .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("Bozzard triangle pipeline"),
                layout: None,
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_main"),
                    compilation_options: Default::default(),
                    buffers: &[Some(wgpu::VertexBufferLayout {
                        array_stride: 8,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &wgpu::vertex_attr_array![0 => Float32x2],
                    })],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs_main"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                primitive: Default::default(),
                depth_stencil: None,
                multisample: Default::default(),
                multiview_mask: None,
                cache: None,
            });
        let vertices = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Bozzard triangle vertices"),
            size: 24,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self { pipeline, vertices }
    }

    pub fn draw(&self, gpu: &Gpu, view: &wgpu::TextureView, offset: [f32; 2]) {
        let points = [[-0.6_f32, -0.5_f32], [0.6, -0.5], [0.0, 0.6]];
        let mut bytes = [0_u8; 24];
        for (i, point) in points.iter().enumerate() {
            for axis in 0..2 {
                let start = (i * 2 + axis) * 4;
                bytes[start..start + 4]
                    .copy_from_slice(&(point[axis] + offset[axis]).to_le_bytes());
            }
        }
        gpu.queue.write_buffer(&self.vertices, 0, &bytes);
        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Bozzard frame"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Bozzard color pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.02,
                            g: 0.03,
                            b: 0.05,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_vertex_buffer(0, self.vertices.slice(..));
            pass.draw(0..3, 0..1);
        }
        gpu.queue.submit([encoder.finish()]);
    }
}

pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl Frame {
    /// Dependency-free PPM diagnostics; writes the actual output even if validation later fails.
    pub fn write_ppm(&self, path: &Path) -> Result<()> {
        let mut file = std::io::BufWriter::new(std::fs::File::create(path)?);
        write!(file, "P6\n{} {}\n255\n", self.width, self.height)?;
        for pixel in self.rgba.chunks_exact(4) {
            file.write_all(&pixel[..3])?;
        }
        file.flush()?;
        Ok(())
    }

    /// Independent geometric/color oracle. Ignores edge pixels where rasterization varies.
    pub fn verify_triangle(&self, offset: [f32; 2]) -> Result<()> {
        let mut foreground = 0;
        let mut background = 0;
        for y in 0..self.height {
            for x in 0..self.width {
                let px = (x as f32 + 0.5) / self.width as f32 * 2.0 - 1.0 - offset[0];
                let py = 1.0 - (y as f32 + 0.5) / self.height as f32 * 2.0 - offset[1];
                // Three inward half-plane distances, scaled consistently per edge.
                let sides = [
                    py + 0.5,
                    0.36 - 1.1 * px - 0.6 * py,
                    0.36 + 1.1 * px - 0.6 * py,
                ];
                let expected = if sides.iter().all(|d| *d > 0.03) {
                    foreground += 1;
                    [13_u8, 204, 166, 255]
                } else if sides.iter().any(|d| *d < -0.03) {
                    background += 1;
                    [5_u8, 8, 13, 255]
                } else {
                    continue;
                };
                let start = ((y * self.width + x) * 4) as usize;
                let actual = &self.rgba[start..start + 4];
                ensure!(
                    actual.iter().zip(expected).all(|(a, e)| a.abs_diff(e) <= 2),
                    "pixel mismatch at ({x}, {y}): expected {expected:?}, got {actual:?}"
                );
            }
        }
        ensure!(
            foreground > 100 && background > 100,
            "insufficient image coverage"
        );
        println!("pixels_ok foreground={foreground} background={background}");
        Ok(())
    }
}

/// Offscreen rendering still needs an adapter; it is unrelated to a GPU-free server.
pub fn render_offscreen(gpu: &Gpu, renderer: &TriangleRenderer, offset: [f32; 2]) -> Result<Frame> {
    capture_offscreen(gpu, 257, 193, |view| {
        renderer.draw(gpu, view, offset);
        Ok(())
    })
}

/// Render to a linear RGBA8 target and read it back. The supplied pipeline must use that format.
pub fn capture_offscreen(
    gpu: &Gpu,
    width: u32,
    height: u32,
    draw: impl FnOnce(&wgpu::TextureView) -> Result<()>,
) -> Result<Frame> {
    // Deliberately unaligned width exercises padded texture-to-buffer readback.
    ensure!(
        width > 0 && height > 0 && width <= 4096 && height <= 4096,
        "capture dimensions must be within 1..=4096"
    );
    let extent = wgpu::Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
    };
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Bozzard offscreen target"),
        size: extent,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    draw(&texture.create_view(&Default::default()))?;
    read_texture(gpu, &texture, width, height)
}

/// Read an existing RGBA8 render target. The texture must have COPY_SRC usage.
pub fn read_texture(gpu: &Gpu, texture: &wgpu::Texture, width: u32, height: u32) -> Result<Frame> {
    ensure!(
        width > 0 && height > 0 && width <= 4096 && height <= 4096,
        "invalid readback dimensions"
    );
    let extent = wgpu::Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
    };
    let row_bytes = (width * 4).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
        * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Bozzard readback"),
        size: u64::from(row_bytes * height),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(row_bytes),
                rows_per_image: Some(height),
            },
        },
        extent,
    );
    gpu.queue.submit([encoder.finish()]);
    let (sender, receiver) = mpsc::channel();
    buffer
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
    gpu.wait()?;
    receiver
        .recv_timeout(Duration::from_secs(5))
        .context("readback callback missing")??;
    let mapped = buffer.slice(..).get_mapped_range()?;
    let mut rgba = Vec::with_capacity((width * height * 4) as usize);
    for row in mapped.chunks_exact(row_bytes as usize) {
        rgba.extend_from_slice(&row[..(width * 4) as usize]);
    }
    drop(mapped);
    buffer.unmap();
    Ok(Frame {
        width,
        height,
        rgba,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn image_oracle_rejects_a_clear_only_frame() {
        let frame = Frame {
            width: 257,
            height: 193,
            rgba: [5, 8, 13, 255].repeat(257 * 193),
        };
        assert!(frame.verify_triangle([0.0, 0.0]).is_err());
    }
}
