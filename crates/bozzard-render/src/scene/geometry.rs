use super::*;

/// Auxiliary data from the actual material shaders, with the same depth/coverage
/// as HDR. Four RGBA16F attachments total 32 bytes per sample on baseline devices.
pub(super) struct GeometryBuffers {
    pub normal: wgpu::TextureView,
    pub motion: wgpu::TextureView,
    pub specular: wgpu::TextureView,
}
pub(super) fn color_texture(gpu: &Gpu, size: [u32; 2], label: &str) -> wgpu::TextureView {
    gpu.device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width: size[0],
                height: size[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba16Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        })
        .create_view(&Default::default())
}
impl GeometryBuffers {
    pub fn new(gpu: &Gpu, size: [u32; 2]) -> Self {
        Self {
            normal: color_texture(gpu, size, "surface normals and roughness"),
            motion: color_texture(gpu, size, "motion previous depth and reactive coverage"),
            specular: color_texture(gpu, size, "surface Fresnel and occlusion"),
        }
    }
}
pub(crate) fn color_targets(
    format: wgpu::TextureFormat,
    transparent: bool,
) -> [Option<wgpu::ColorTargetState>; 4] {
    std::array::from_fn(|i| {
        Some(wgpu::ColorTargetState {
            format: if i == 0 {
                format
            } else {
                wgpu::TextureFormat::Rgba16Float
            },
            blend: if i == 0 && transparent {
                Some(wgpu::BlendState::ALPHA_BLENDING)
            } else if i == 2 && transparent {
                Some(wgpu::BlendState {
                    color: wgpu::BlendComponent {
                        src_factor: wgpu::BlendFactor::One,
                        dst_factor: wgpu::BlendFactor::One,
                        operation: wgpu::BlendOperation::Max,
                    },
                    alpha: wgpu::BlendComponent {
                        src_factor: wgpu::BlendFactor::One,
                        dst_factor: wgpu::BlendFactor::One,
                        operation: wgpu::BlendOperation::Max,
                    },
                })
            } else {
                None
            },
            write_mask: if !transparent || i == 0 {
                wgpu::ColorWrites::ALL
            } else if i == 2 {
                wgpu::ColorWrites::ALPHA
            } else {
                wgpu::ColorWrites::empty()
            },
        })
    })
}
pub(super) fn attachment(
    view: &wgpu::TextureView,
    color: wgpu::Color,
) -> Option<wgpu::RenderPassColorAttachment<'_>> {
    Some(wgpu::RenderPassColorAttachment {
        view,
        depth_slice: None,
        resolve_target: None,
        ops: wgpu::Operations {
            load: wgpu::LoadOp::Clear(color),
            store: wgpu::StoreOp::Store,
        },
    })
}

#[derive(Clone, Copy, Debug)]
pub(super) struct TemporalFrame {
    pub previous_vp: Mat4,
    pub jitter: [f32; 2],
    pub previous_jitter: [f32; 2],
    pub valid: bool,
    pub repeated: bool,
    pub motion_scale: f32,
}
impl Default for TemporalFrame {
    fn default() -> Self {
        Self {
            previous_vp: Mat4::IDENTITY,
            jitter: [0.; 2],
            previous_jitter: [0.; 2],
            valid: false,
            repeated: false,
            motion_scale: 0.,
        }
    }
}
struct PreviousFrame {
    vp: Mat4,
    jittered: Mat4,
    time: f32,
    size: [u32; 2],
    jitter: [f32; 2],
    signature: u64,
    taa: bool,
}
#[derive(Default)]
pub(super) struct MotionHistory {
    previous: Option<PreviousFrame>,
    poses: BTreeMap<(u64, MeshKind), Mat4>,
    sample: u32,
}
fn halton(mut index: u32, base: u32) -> f32 {
    let mut weight = 1.;
    let mut result = 0.;
    while index > 0 {
        weight /= base as f32;
        result += weight * (index % base) as f32;
        index /= base;
    }
    result
}
impl MotionHistory {
    pub fn reset(&mut self) {
        *self = Self::default();
    }
    pub fn begin(
        &mut self,
        scene: &RenderScene,
        size: [u32; 2],
        raw: bool,
    ) -> (Mat4, TemporalFrame) {
        let active =
            !raw && (scene.display.temporal_aa.enabled || scene.display.motion_blur.enabled);
        if !active {
            self.reset();
            return (scene.view_projection, TemporalFrame::default());
        }
        let signature = frame_signature(scene);
        let time = scene.display.time_seconds;
        let mut frame = TemporalFrame::default();
        if let Some(PreviousFrame {
            vp: old,
            jittered: old_jittered,
            time: old_time,
            size: old_size,
            jitter,
            signature: old_signature,
            taa: old_taa,
        }) = self.previous
        {
            let inverse = scene.view_projection.inverse();
            let previous_inverse = old.inverse();
            let origin = inverse.project_point3(Vec3::ZERO);
            let previous_origin = previous_inverse.project_point3(Vec3::ZERO);
            let forward = (inverse.project_point3(Vec3::Z) - origin).normalize();
            let old_forward =
                (previous_inverse.project_point3(Vec3::Z) - previous_origin).normalize();
            let delta = time - old_time;
            frame.valid = size == old_size
                && (0. ..=0.25).contains(&delta)
                && origin.distance(previous_origin) < 3.
                && forward.dot(old_forward) > 0.65
                && old_taa == scene.display.temporal_aa.enabled;
            frame.repeated = frame.valid && signature == old_signature;
            frame.previous_vp = old_jittered;
            frame.previous_jitter = jitter;
            frame.motion_scale = if frame.valid && delta > 0.00001 {
                (1. / 60. / delta).clamp(0., 4.)
            } else {
                0.
            };
        }
        if !frame.valid {
            self.sample = 0;
            self.poses.clear();
        }
        if !frame.repeated {
            self.sample = self.sample % 8 + 1;
        }
        frame.jitter = if scene.display.temporal_aa.enabled {
            [halton(self.sample, 2) - 0.5, halton(self.sample, 3) - 0.5]
        } else {
            [0.; 2]
        };
        let jittered = Mat4::from_translation(Vec3::new(
            frame.jitter[0] * 2. / size[0] as f32,
            -frame.jitter[1] * 2. / size[1] as f32,
            0.,
        )) * scene.view_projection;
        if !frame.valid {
            frame.previous_vp = jittered;
            frame.previous_jitter = frame.jitter;
        }
        self.previous = Some(PreviousFrame {
            vp: scene.view_projection,
            jittered,
            time,
            size,
            jitter: frame.jitter,
            signature,
            taa: scene.display.temporal_aa.enabled,
        });
        (jittered, frame)
    }
    pub fn previous_model(&self, item: &DrawItem) -> Option<Mat4> {
        if item.motion_id == 0 {
            Some(item.model)
        } else {
            self.poses
                .get(&(item.motion_id, item.mesh.clone()))
                .copied()
        }
    }
    pub fn finish(&mut self, draws: &[PreparedDraw]) {
        if self.previous.is_some() {
            self.poses = draws
                .iter()
                .filter(|d| d.object.motion_id != 0)
                .map(|d| ((d.object.motion_id, d.object.mesh.clone()), d.object.model))
                .collect();
        }
    }
}

fn frame_signature(scene: &RenderScene) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    fn floats(values: impl IntoIterator<Item = f32>, hash: &mut impl Hasher) {
        for value in values {
            value.to_bits().hash(hash);
        }
    }
    floats(scene.view_projection.to_cols_array(), &mut hash);
    // Only small settings use Debug; geometry and potentially large GI grids
    // are hashed directly, with no per-frame scene serialization/allocation.
    format!(
        "{:?}{:?}{:?}{:?}{:?}",
        scene.display, scene.lighting, scene.fog, scene.environment, scene.lights
    )
    .hash(&mut hash);
    for item in &scene.items {
        item.motion_id.hash(&mut hash);
        item.mesh.hash(&mut hash);
        item.material.texture.hash(&mut hash);
        item.material.lit.hash(&mut hash);
        floats(
            item.model
                .to_cols_array()
                .into_iter()
                .chain(item.material.tint)
                .chain(item.material.uv_scale)
                .chain(item.material.metallic)
                .chain(item.material.roughness),
            &mut hash,
        );
        for surface in item.material.surface_overrides.iter() {
            surface.surface.hash(&mut hash);
            surface.source.hash(&mut hash);
            surface.texture.hash(&mut hash);
            floats(
                surface
                    .transform
                    .to_cols_array()
                    .into_iter()
                    .chain(surface.tint)
                    .chain(surface.uv_scale)
                    .chain(surface.metallic)
                    .chain(surface.roughness),
                &mut hash,
            );
        }
    }
    for p in &scene.particles {
        p.id.hash(&mut hash);
        p.kind.hash(&mut hash);
        floats(
            p.position
                .to_array()
                .into_iter()
                .chain(p.velocity.to_array())
                .chain(p.color)
                .chain([
                    p.size,
                    p.rotation,
                    p.opacity,
                    p.softness,
                    p.trail_length,
                    p.seed,
                ]),
            &mut hash,
        );
    }
    if let Some(gi) = &scene.gi {
        gi.resolution.hash(&mut hash);
        floats(
            gi.min
                .into_iter()
                .chain(gi.max)
                .chain([gi.intensity, gi.normal_bias])
                .chain(gi.probes.iter().flatten().copied()),
            &mut hash,
        );
    }
    hash.finish()
}
