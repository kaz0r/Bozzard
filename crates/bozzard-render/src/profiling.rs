//! Pass-boundary timestamp queries work on Apple GPUs as well as Vulkan and DX12.
//! Three persistent readback slots avoid waiting for the GPU in the frame loop.
use crate::{Gpu, wgpu};
use anyhow::Result;
use std::{
    ops::{Deref, DerefMut},
    sync::{
        Arc,
        atomic::{AtomicU8, Ordering},
    },
};

const MAX_PASSES: usize = 256;
const BYTES: u64 = MAX_PASSES as u64 * 16;

#[derive(Clone, Debug, serde::Serialize)]
pub struct GpuPassTiming {
    pub name: String,
    pub milliseconds: Option<f64>,
}
#[derive(Clone, Debug, serde::Serialize)]
pub struct GpuFrameTiming {
    pub frame: u64,
    pub passes: Vec<GpuPassTiming>,
    pub omitted: usize,
    pub failed: bool,
}
struct Slot {
    queries: wgpu::QuerySet,
    resolve: wgpu::Buffer,
    readback: wgpu::Buffer,
    // 0 available; 1 in flight; 2 mapped; 3 mapping failed.
    state: Arc<AtomicU8>,
    frame: u64,
    labels: Vec<String>,
    omitted: usize,
}
impl Slot {
    fn new(gpu: &Gpu) -> Self {
        Self {
            queries: gpu.device.create_query_set(&wgpu::QuerySetDescriptor {
                label: Some("profiler pass timestamps"),
                ty: wgpu::QueryType::Timestamp,
                count: MAX_PASSES as u32 * 2,
            }),
            resolve: gpu.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("profiler resolved timestamps"),
                size: BYTES,
                usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            }),
            readback: gpu.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("profiler asynchronous readback"),
                size: BYTES,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            }),
            state: Arc::new(AtomicU8::new(0)),
            frame: 0,
            labels: Vec::new(),
            omitted: 0,
        }
    }
}
#[derive(Default)]
pub(crate) struct GpuProfiler {
    pub enabled: bool,
    serial: u64,
    slots: Vec<Slot>,
    pub skipped: u64,
}
struct Active {
    slot: usize,
    queries: wgpu::QuerySet,
    labels: Vec<String>,
    omitted: usize,
}
impl Active {
    fn allocate(&mut self, label: Option<&str>) -> Option<u32> {
        if self.labels.len() == MAX_PASSES {
            self.omitted += 1;
            return None;
        }
        let index = self.labels.len() as u32 * 2;
        let label = label.unwrap_or("Unnamed pass");
        self.labels
            .push(label[..label.floor_char_boundary(label.len().min(256))].into());
        Some(index)
    }
}
pub(crate) struct Encoder {
    raw: wgpu::CommandEncoder,
    pub frame: u64,
    active: Option<Active>,
}
impl Deref for Encoder {
    type Target = wgpu::CommandEncoder;
    fn deref(&self) -> &Self::Target {
        &self.raw
    }
}
impl DerefMut for Encoder {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.raw
    }
}
impl Encoder {
    pub fn begin_render_pass(
        &mut self,
        descriptor: &wgpu::RenderPassDescriptor<'_>,
    ) -> wgpu::RenderPass<'_> {
        let writes = self.active.as_mut().and_then(|active| {
            let index = active.allocate(descriptor.label)?;
            Some(wgpu::RenderPassTimestampWrites {
                query_set: &active.queries,
                beginning_of_pass_write_index: Some(index),
                end_of_pass_write_index: Some(index + 1),
            })
        });
        self.raw.begin_render_pass(&wgpu::RenderPassDescriptor {
            timestamp_writes: writes,
            ..descriptor.clone()
        })
    }
    pub fn begin_compute_pass(
        &mut self,
        descriptor: &wgpu::ComputePassDescriptor<'_>,
    ) -> wgpu::ComputePass<'_> {
        let writes = self.active.as_mut().and_then(|active| {
            let index = active.allocate(descriptor.label)?;
            Some(wgpu::ComputePassTimestampWrites {
                query_set: &active.queries,
                beginning_of_pass_write_index: Some(index),
                end_of_pass_write_index: Some(index + 1),
            })
        });
        self.raw.begin_compute_pass(&wgpu::ComputePassDescriptor {
            timestamp_writes: writes,
            ..descriptor.clone()
        })
    }
}
impl GpuProfiler {
    pub fn encoder(&mut self, gpu: &Gpu) -> Encoder {
        self.serial = self.serial.wrapping_add(1);
        let active = if self.enabled
            && gpu
                .device
                .features()
                .contains(wgpu::Features::TIMESTAMP_QUERY)
        {
            if self.slots.is_empty() {
                self.slots = (0..3).map(|_| Slot::new(gpu)).collect();
            }
            if let Some(slot) = self
                .slots
                .iter()
                .position(|s| s.state.load(Ordering::Acquire) == 0)
            {
                Some(Active {
                    slot,
                    queries: self.slots[slot].queries.clone(),
                    labels: std::mem::take(&mut self.slots[slot].labels),
                    omitted: 0,
                })
            } else {
                self.skipped = self.skipped.saturating_add(1);
                None
            }
        } else {
            None
        };
        Encoder {
            raw: gpu
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("scene frame"),
                }),
            frame: self.serial,
            active,
        }
    }
    pub fn submit(&mut self, gpu: &Gpu, mut encoder: Encoder) {
        let slot = encoder
            .active
            .take()
            .filter(|a| !a.labels.is_empty())
            .map(|active| {
                let slot = &mut self.slots[active.slot];
                slot.frame = encoder.frame;
                slot.labels = active.labels;
                slot.omitted = active.omitted;
                let count = slot.labels.len() as u32 * 2;
                encoder
                    .raw
                    .resolve_query_set(&slot.queries, 0..count, &slot.resolve, 0);
                encoder.raw.copy_buffer_to_buffer(
                    &slot.resolve,
                    0,
                    &slot.readback,
                    0,
                    u64::from(count) * 8,
                );
                active.slot
            });
        gpu.queue.submit([encoder.raw.finish()]);
        if let Some(index) = slot {
            let slot = &self.slots[index];
            let state = slot.state.clone();
            state.store(1, Ordering::Release);
            slot.readback.map_async(
                wgpu::MapMode::Read,
                0..slot.labels.len() as u64 * 16,
                move |result| {
                    state.store(if result.is_ok() { 2 } else { 3 }, Ordering::Release);
                },
            );
        }
    }
    pub fn poll(&mut self, gpu: &Gpu) -> Result<Vec<GpuFrameTiming>> {
        if self
            .slots
            .iter()
            .any(|s| s.state.load(Ordering::Acquire) == 1)
        {
            gpu.device.poll(wgpu::PollType::Poll)?;
        }
        let mut completed = Vec::new();
        for slot in &mut self.slots {
            let state = slot.state.load(Ordering::Acquire);
            if state < 2 {
                continue;
            }
            let mut passes = Vec::new();
            let mut failed = state == 3;
            if state == 2 {
                match slot
                    .readback
                    .get_mapped_range(0..slot.labels.len() as u64 * 16)
                {
                    Ok(bytes) => {
                        passes.reserve(slot.labels.len());
                        for (name, pair) in slot.labels.drain(..).zip(bytes.chunks_exact(16)) {
                            let start = u64::from_ne_bytes(pair[..8].try_into().unwrap());
                            let end = u64::from_ne_bytes(pair[8..].try_into().unwrap());
                            passes.push(GpuPassTiming {
                                name,
                                milliseconds: duration_ms(
                                    start,
                                    end,
                                    gpu.queue.get_timestamp_period(),
                                ),
                            });
                        }
                        drop(bytes);
                    }
                    Err(_) => {
                        failed = true;
                        slot.labels.clear();
                    }
                }
                slot.readback.unmap();
            } else {
                slot.labels.clear();
            }
            completed.push(GpuFrameTiming {
                frame: slot.frame,
                passes,
                omitted: slot.omitted,
                failed,
            });
            slot.state.store(0, Ordering::Release);
        }
        Ok(completed)
    }
}

// Some drivers advertise support but return zero/invalid samples. Never turn
// unavailable measurements into an apparently free pass.
fn duration_ms(start: u64, end: u64, period: f32) -> Option<f64> {
    if start == 0 || end == 0 || !period.is_finite() || period <= 0. {
        return None;
    }
    end.checked_sub(start)
        .map(|ticks| ticks as f64 * f64::from(period) / 1_000_000.)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn timestamp_units_and_invalid_samples() {
        assert_eq!(duration_ms(10, 500_010, 2.), Some(1.));
        assert_eq!(duration_ms(10, 10, 1.), Some(0.));
        for (start, end) in [(0, 0), (0, 42), (42, 0), (42, 40)] {
            assert_eq!(duration_ms(start, end, 1.), None);
        }
    }
    #[test]
    fn timestamp_readback_is_bounded_matches_frames_and_reuses_slots() -> Result<()> {
        let gpu = pollster::block_on(Gpu::request(
            &crate::instance(crate::Backend::native()),
            None,
            false,
        ))?;
        let target = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("profiler test target"),
            size: wgpu::Extent3d {
                width: 16,
                height: 16,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let view = target.create_view(&Default::default());
        let shader = gpu.device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("timestamp test triangle"),
            source: wgpu::ShaderSource::Wgsl("@vertex fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4f { let p = array<vec2f, 3>(vec2f(-1., -1.), vec2f(3., -1.), vec2f(-1., 3.)); return vec4f(p[i], 0., 1.); } @fragment fn fs() -> @location(0) vec4f { return vec4f(0., 1., 0., 1.); }".into()),
        });
        let pipeline = gpu
            .device
            .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("timestamp test pipeline"),
                layout: None,
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::TextureFormat::Rgba8Unorm.into())],
                }),
                primitive: Default::default(),
                depth_stencil: None,
                multisample: Default::default(),
                multiview_mask: None,
                cache: None,
            });
        let draw = |profiler: &mut GpuProfiler| {
            let mut encoder = profiler.encoder(&gpu);
            let id = encoder.frame;
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("Clear the test target"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        resolve_target: None,
                        depth_slice: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::GREEN),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    ..Default::default()
                });
                pass.set_pipeline(&pipeline);
                pass.draw(0..3, 0..1);
            }
            profiler.submit(&gpu, encoder);
            id
        };
        let mut profiler = GpuProfiler::default();
        draw(&mut profiler);
        assert!(
            profiler.slots.is_empty(),
            "disabled profiling allocates no GPU resources"
        );
        profiler.enabled = true;
        let ids: Vec<_> = (0..3).map(|_| draw(&mut profiler)).collect();
        draw(&mut profiler);
        if gpu
            .device
            .features()
            .contains(wgpu::Features::TIMESTAMP_QUERY)
        {
            assert_eq!(profiler.slots.len(), 3);
            assert_eq!(
                profiler.skipped, 1,
                "a fourth frame must not wait for a free slot"
            );
            gpu.wait()?;
            let completed = profiler.poll(&gpu)?;
            assert_eq!(completed.iter().map(|f| f.frame).collect::<Vec<_>>(), ids);
            for frame in completed {
                assert!(!frame.failed);
                assert_eq!(frame.passes.len(), 1);
                assert_eq!(frame.passes[0].name, "Clear the test target");
                if let Some(ms) = frame.passes[0].milliseconds {
                    assert!(ms.is_finite() && ms >= 0.);
                } else {
                    // macOS 26 can return a zero end sample even for real draws
                    // (wgpu #9414). Such samples must remain unavailable, not 0 ms.
                    assert_eq!(gpu.adapter.get_info().backend, wgpu::Backend::Metal);
                }
            }
            let next = draw(&mut profiler);
            gpu.wait()?;
            assert_eq!(profiler.poll(&gpu)?[0].frame, next);
        } else {
            assert!(profiler.slots.is_empty());
            assert!(profiler.poll(&gpu)?.is_empty());
        }
        // A device intentionally created without timestamp support follows the fallback,
        // even when the hardware itself supports it.
        let (device, queue) = pollster::block_on(
            gpu.adapter
                .request_device(&wgpu::DeviceDescriptor::default()),
        )?;
        let baseline = Gpu::from_device(gpu.adapter.clone(), device, queue);
        let mut unavailable = GpuProfiler {
            enabled: true,
            ..Default::default()
        };
        let encoder = unavailable.encoder(&baseline);
        assert!(encoder.active.is_none());
        unavailable.submit(&baseline, encoder);
        assert!(unavailable.slots.is_empty());
        Ok(())
    }
}
