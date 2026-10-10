use super::*;

/// Auxiliary data from the actual material shaders, with the same depth/coverage
/// as HDR. Four RGBA16F attachments total 32 bytes per sample on baseline devices.
pub(super) struct GeometryBuffers {
    pub normal: wgpu::TextureView,
    pub motion: wgpu::TextureView,
    pub specular: wgpu::TextureView,
}
impl GeometryBuffers {
    pub fn new(gpu: &Gpu, size: [u32; 2]) -> Self {
        Self {
            normal: gpu_util::color_texture(gpu, size, "surface normals and roughness"),
            motion: gpu_util::color_texture(
                gpu,
                size,
                "motion previous depth and reactive coverage",
            ),
            specular: gpu_util::color_texture(gpu, size, "surface Fresnel and occlusion"),
        }
    }
}
pub(crate) fn color_targets(
    format: wgpu::TextureFormat,
    transparent: bool,
    auxiliary: bool,
) -> [Option<wgpu::ColorTargetState>; 4] {
    color_targets_mask(format, transparent, if auxiliary { 7 } else { 0 })
}
pub(crate) fn color_targets_mask(
    format: wgpu::TextureFormat,
    transparent: bool,
    mask: u8,
) -> [Option<wgpu::ColorTargetState>; 4] {
    std::array::from_fn(|i| {
        if i > 0 && mask & (1 << (i - 1)) == 0 {
            return None;
        }
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

/// Only retain auxiliary render targets when a later pass consumes them. The
/// textures and shader outputs remain available when effects are enabled next frame.
pub(super) fn stores(scene: &RenderScene, raw: bool, caching: bool) -> [bool; 3] {
    if !caching {
        return [true; 3];
    }
    let temporal = !raw && (scene.display.temporal_aa.enabled || scene.display.motion_blur.enabled);
    let reflections =
        !raw && scene.display.reflections.enabled && scene.display.reflections.strength > 0.;
    [
        temporal || reflections,
        // The particle pass loads motion coverage, even when temporal effects are off.
        temporal || (!raw && !scene.particles.is_empty()),
        reflections,
    ]
}

pub(super) fn auxiliary_attachment(
    view: &wgpu::TextureView,
    store: bool,
) -> Option<wgpu::RenderPassColorAttachment<'_>> {
    let mut attachment = attachment(view, wgpu::Color::TRANSPARENT)?;
    attachment.ops.store = if store {
        wgpu::StoreOp::Store
    } else {
        wgpu::StoreOp::Discard
    };
    Some(attachment)
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
struct PendingFrame {
    previous: Option<PreviousFrame>,
    sample: u32,
    reset: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum MotionMesh {
    Sprite(u64),
    Quad,
    Cube,
    Sphere,
    Imported(String),
    ModelPart(String, usize),
    Text,
}
/// A borrowed motion key: compares and hashes without cloning asset IDs.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum MotionMeshRef<'a> {
    Sprite(u64),
    Quad,
    Cube,
    Sphere,
    Imported(&'a str),
    ModelPart(&'a str, usize),
    Text,
}
impl<'a> From<&'a MeshKind> for MotionMeshRef<'a> {
    fn from(mesh: &'a MeshKind) -> Self {
        match mesh {
            MeshKind::Quad => Self::Quad,
            MeshKind::Cube => Self::Cube,
            MeshKind::Sphere => Self::Sphere,
            MeshKind::Imported(id) => Self::Imported(id),
            MeshKind::ModelPart(id, part) => Self::ModelPart(id, *part),
            MeshKind::Sprite(sprite) => Self::Sprite(sprite.geometry.key()),
            MeshKind::Text(_) | MeshKind::SharedText(_) => Self::Text,
        }
    }
}
impl From<MotionMeshRef<'_>> for MotionMesh {
    fn from(mesh: MotionMeshRef<'_>) -> Self {
        match mesh {
            MotionMeshRef::Quad => Self::Quad,
            MotionMeshRef::Cube => Self::Cube,
            MotionMeshRef::Sphere => Self::Sphere,
            MotionMeshRef::Imported(id) => Self::Imported(id.to_owned()),
            MotionMeshRef::ModelPart(id, part) => Self::ModelPart(id.to_owned(), part),
            MotionMeshRef::Sprite(key) => Self::Sprite(key),
            MotionMeshRef::Text => Self::Text,
        }
    }
}
impl MotionMesh {
    fn as_ref(&self) -> MotionMeshRef<'_> {
        match self {
            Self::Quad => MotionMeshRef::Quad,
            Self::Cube => MotionMeshRef::Cube,
            Self::Sphere => MotionMeshRef::Sphere,
            Self::Imported(id) => MotionMeshRef::Imported(id),
            Self::ModelPart(id, part) => MotionMeshRef::ModelPart(id, *part),
            Self::Sprite(key) => MotionMeshRef::Sprite(*key),
            Self::Text => MotionMeshRef::Text,
        }
    }
}
fn motion_fingerprint(motion_id: u64, mesh: MotionMeshRef<'_>) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    motion_id.hash(&mut hash);
    mesh.hash(&mut hash);
    hash.finish()
}
/// Previous-frame poses retained by draw position. Stable draw order resolves
/// each lookup positionally; reordered or duplicated keys use a fingerprint
/// index that is rebuilt only when the key list changes. Duplicate keys keep
/// the former map semantics: the last occurrence wins.
#[derive(Default)]
struct Poses {
    keys: Vec<(u64, MotionMesh)>,
    models: Vec<Mat4>,
    index: HashMap<u64, u32>,
    duplicates: bool,
    collisions: bool,
    key_updates: usize,
}
impl Poses {
    fn clear(&mut self) {
        self.keys.clear();
        self.models.clear();
        self.index.clear();
        self.duplicates = false;
        self.collisions = false;
    }
    fn get(&self, index: usize, item: &DrawItem) -> Option<Mat4> {
        let mesh = MotionMeshRef::from(&item.mesh);
        let matches =
            |i: usize| self.keys[i].0 == item.motion_id && self.keys[i].1.as_ref() == mesh;
        if !self.duplicates && index < self.keys.len() && matches(index) {
            return Some(self.models[index]);
        }
        if self.collisions {
            return (0..self.keys.len())
                .rev()
                .find(|&i| matches(i))
                .map(|i| self.models[i]);
        }
        let candidate = *self.index.get(&motion_fingerprint(item.motion_id, mesh))? as usize;
        matches(candidate).then(|| self.models[candidate])
    }
    fn update(&mut self, draws: &[PreparedDraw]) {
        let mut changed = self.keys.len() != draws.len();
        self.key_updates = 0;
        for (index, draw) in draws.iter().enumerate() {
            let object = &draw.object;
            let mesh = MotionMeshRef::from(&object.mesh);
            if index == self.keys.len() {
                self.keys.push((object.motion_id, mesh.into()));
                self.models.push(object.model);
                self.key_updates += 1;
                continue;
            }
            let key = &mut self.keys[index];
            if key.0 != object.motion_id || key.1.as_ref() != mesh {
                *key = (object.motion_id, mesh.into());
                self.key_updates += 1;
                changed = true;
            }
            self.models[index] = object.model;
        }
        self.keys.truncate(draws.len());
        self.models.truncate(draws.len());
        if !changed && self.key_updates == 0 {
            return;
        }
        self.index.clear();
        self.duplicates = false;
        self.collisions = false;
        for (index, (motion_id, mesh)) in self.keys.iter().enumerate() {
            if *motion_id == 0 {
                continue;
            }
            let fingerprint = motion_fingerprint(*motion_id, mesh.as_ref());
            if let Some(previous) = self.index.insert(fingerprint, index as u32) {
                if self.keys[previous as usize] == self.keys[index] {
                    self.duplicates = true;
                } else {
                    self.collisions = true;
                }
            }
        }
    }
}
#[derive(Default)]
pub(super) struct MotionHistory {
    previous: Option<PreviousFrame>,
    poses: Poses,
    sample: u32,
    pending: Option<PendingFrame>,
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
            self.pending = Some(PendingFrame {
                previous: None,
                sample: 0,
                reset: true,
            });
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
        let mut sample = if frame.valid { self.sample } else { 0 };
        if !frame.repeated {
            sample = sample % 8 + 1;
        }
        frame.jitter = if scene.display.temporal_aa.enabled {
            [halton(sample, 2) - 0.5, halton(sample, 3) - 0.5]
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
        self.pending = Some(PendingFrame {
            previous: Some(PreviousFrame {
                vp: scene.view_projection,
                jittered,
                time,
                size,
                jitter: frame.jitter,
                signature,
                taa: scene.display.temporal_aa.enabled,
            }),
            sample,
            reset: !frame.valid,
        });
        (jittered, frame)
    }
    pub fn previous_model(&self, index: usize, item: &DrawItem) -> Option<Mat4> {
        if item.motion_id == 0 {
            Some(item.model)
        } else if self.pending.as_ref().is_some_and(|pending| pending.reset) {
            None
        } else {
            self.poses.get(index, item)
        }
    }
    /// Key rows rewritten by the last `finish`; zero for a stable draw order.
    pub fn key_updates(&self) -> usize {
        self.poses.key_updates
    }
    pub fn finish(&mut self, draws: &[PreparedDraw]) {
        let pending = self.pending.take().expect("frame history prepared");
        self.previous = pending.previous;
        self.sample = pending.sample;
        if self.previous.is_some() {
            self.poses.update(draws);
        } else {
            self.poses.clear();
            self.poses.key_updates = 0;
        }
    }
    pub fn abort(&mut self) {
        self.pending = None;
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
    struct HashWriter<'a, H>(&'a mut H);
    impl<H: Hasher> std::fmt::Write for HashWriter<'_, H> {
        fn write_str(&mut self, text: &str) -> std::fmt::Result {
            self.0.write(text.as_bytes());
            Ok(())
        }
    }
    let _ = std::fmt::Write::write_fmt(
        &mut HashWriter(&mut hash),
        format_args!(
            "{:?}{:?}{:?}{:?}{:?}",
            scene.display, scene.lighting, scene.fog, scene.environment, scene.lights
        ),
    );
    for item in &scene.items {
        item.motion_id.hash(&mut hash);
        MotionMeshRef::from(&item.mesh).hash(&mut hash);
        if let Some(text) = item.mesh.text() {
            text.text.hash(&mut hash);
            text.monospace.hash(&mut hash);
            std::mem::discriminant(&text.alignment).hash(&mut hash);
            text.max_width.map(f32::to_bits).hash(&mut hash);
            floats([text.font_size, text.opacity], &mut hash);
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    fn draw(motion_id: u64, mesh: MeshKind, model: Mat4) -> PreparedDraw {
        PreparedDraw {
            preparation: Default::default(),
            source_item: 0,
            deformation: 0,
            shared_geometry: None,
            world_geometry_units: None,
            pbr_override: [-1.; 2],
            shader: None,
            pbr: false,
            raster: 0,
            object: DrawItem {
                motion_id,
                model,
                mesh,
                material: Material {
                    metallic: None,
                    roughness: None,
                    tint: [1.; 3],
                    uv_scale: [1.; 2],
                    texture: TextureKind::White,
                    lit: true,
                    shader: None,
                    surface_overrides: Default::default(),
                },
            },
            opacity: 1.,
            cutoff: 0.,
            transparent: false,
            depth: 0.,
        }
    }
    #[test]
    #[ignore = "release-mode temporal history CPU profile; run explicitly"]
    fn temporal_history_benchmark() {
        let item = |i: usize| DrawItem {
            motion_id: i as u64 + 1,
            model: Mat4::from_translation(Vec3::new(i as f32, 0., -5.)),
            mesh: MeshKind::Imported(format!("mesh-{}", i % 64)),
            material: draw(0, MeshKind::Cube, Mat4::IDENTITY).object.material,
        };
        let particle = |i: usize| Particle {
            simulation: None,
            id: i as u64,
            position: Vec3::splat(i as f32),
            velocity: Vec3::Y,
            size: 0.3,
            rotation: 0.,
            color: [0.5; 3],
            opacity: 0.5,
            kind: ParticleKind::Smoke,
            softness: 0.2,
            trail_length: 0.,
            seed: 0.1,
        };
        let mut scene = RenderScene {
            skin_poses: Default::default(),
            particles: (0..16_384).map(particle).collect(),
            fog: Default::default(),
            gi: Some(IrradianceVolume {
                min: [0.; 3],
                max: [1.; 3],
                resolution: [16; 3],
                intensity: 1.,
                normal_bias: 0.,
                probes: std::sync::Arc::new(vec![[0.5; 4]; 16 * 16 * 16 * 41]),
            }),
            lights: Vec::new(),
            environment: Default::default(),
            display: Default::default(),
            lighting: Default::default(),
            view_projection: Mat4::IDENTITY,
            items: (0..4096).map(item).collect(),
            shader_time: 0.,
        };
        scene.display.temporal_aa.enabled = true;
        for (workload, advance) in [("play", true), ("paused", false)] {
            let mut history = MotionHistory::default();
            let mut samples = Vec::new();
            for frame in 0..70 {
                if advance {
                    scene.display.time_seconds = frame as f32 / 60.;
                }
                let start = std::time::Instant::now();
                let (_, temporal) = std::hint::black_box(history.begin(&scene, [640, 400], false));
                samples.push(start.elapsed().as_secs_f64() * 1000.);
                history.finish(&[]);
                if frame > 1 {
                    assert_eq!(temporal.repeated, !advance);
                }
            }
            samples.drain(..10);
            samples.sort_by(f64::total_cmp);
            println!(
                "temporal_history workload={workload} items=4096 particles=16384 gi_probes=4096 begin_median_ms={:.4}",
                samples[samples.len() / 2]
            );
        }
    }
    #[test]
    fn positional_history_matches_reference_map() {
        let mut seed = 0x2545_f491_u64;
        let mut next = |bound: u64| {
            seed = seed
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            (seed >> 33) % bound
        };
        let meshes = |pick: u64| match pick {
            0 => MeshKind::Cube,
            1 => MeshKind::Quad,
            2 => MeshKind::Imported("a".into()),
            3 => MeshKind::Imported("b".into()),
            4 => MeshKind::ModelPart("m".into(), 0),
            _ => MeshKind::ModelPart("m".into(), 1),
        };
        let mut poses = Poses::default();
        let mut reference = BTreeMap::new();
        let mut previous: Vec<PreparedDraw> = Vec::new();
        let mut stable_frames = 0;
        for frame in 0..400 {
            // Mostly stable order, with occasional reorder, insertion, removal and duplicates.
            let mut draws: Vec<PreparedDraw> = if frame % 5 != 0 && !previous.is_empty() {
                previous
                    .iter()
                    .map(|d| draw(d.object.motion_id, d.object.mesh.clone(), d.object.model))
                    .collect()
            } else {
                (0..next(14))
                    .map(|_| draw(next(6), meshes(next(6)), Mat4::IDENTITY))
                    .collect()
            };
            if frame % 7 == 3 && draws.len() > 1 {
                let a = next(draws.len() as u64) as usize;
                let b = next(draws.len() as u64) as usize;
                draws.swap(a, b);
            }
            for (i, d) in draws.iter_mut().enumerate() {
                d.object.model =
                    Mat4::from_translation(Vec3::new(frame as f32, i as f32, next(9) as f32));
            }
            for (index, d) in draws.iter().enumerate() {
                if d.object.motion_id == 0 {
                    continue;
                }
                let expected = reference
                    .get(&(
                        d.object.motion_id,
                        MotionMesh::from(MotionMeshRef::from(&d.object.mesh)),
                    ))
                    .copied();
                assert_eq!(
                    poses.get(index, &d.object),
                    expected,
                    "frame {frame} row {index}"
                );
            }
            poses.update(&draws);
            stable_frames += usize::from(poses.key_updates == 0);
            reference = draws
                .iter()
                .filter(|d| d.object.motion_id != 0)
                .map(|d| {
                    (
                        (
                            d.object.motion_id,
                            MotionMesh::from(MotionMeshRef::from(&d.object.mesh)),
                        ),
                        d.object.model,
                    )
                })
                .collect();
            previous = draws;
        }
        assert!(
            stable_frames > 200,
            "stable order rewrites no keys: {stable_frames}"
        );
    }
}
