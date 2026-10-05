//! Hierarchical depth culling with asynchronous visibility readback. Reuse is
//! allowed only for identical depth inputs and query bounds; changed views use
//! current-frame GPU visibility, never a previous frame's approximate results.
use super::*;
mod gpu;

const MIN_SURFACES: usize = 64;
const MAX_BATCHES: usize = 16_384;
const MAX_OCCLUDERS: usize = 32;
const MAX_DEPTH_TRIANGLES: u64 = 100_000;

#[derive(Clone, Copy, Debug, Default, serde::Serialize)]
pub struct OcclusionResult {
    pub frame_id: u64,
    pub tested_batches: usize,
    pub culled_batches: usize,
    pub culled_surfaces: usize,
    pub skipped_triangles: u64,
}
#[derive(Clone, Copy, Debug)]
struct Projection {
    rectangle: [f32; 4], // pixel edges, expanded for raster/projection roundoff
    nearest: f32,
}
#[derive(Clone)]
struct CachedProjection {
    model: Mat4,
    bounds: [Vec3; 2],
    projection: Option<Projection>,
}
#[derive(Clone, PartialEq)]
struct DepthInput {
    mesh: MeshKind,
    model: Mat4,
    double_sided: bool,
}
struct Snapshot {
    camera: (Mat4, [u32; 2]),
    depth: Vec<DepthInput>,
    candidates: Vec<u8>,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Mode {
    Disabled,
    Indirect,
    Cached,
}
pub(super) struct Occlusion {
    enabled: bool,
    camera: Option<(Mat4, [u32; 2])>,
    projections: Vec<CachedProjection>,
    resources: Option<gpu::Resources>,
    candidates: Vec<u8>,
    occluders: Vec<(usize, Projection)>,
    snapshot: Option<Snapshot>,
    generation: u64,
    adaptive: bool,
    last_result_frame: Option<u64>,
    unproductive_results: u32,
    cooldown: u32,
    instance_enabled: bool,
    instance_resources: Option<gpu::Resources>,
    instance_candidates: Vec<u8>,
    instance_snapshot: Option<Snapshot>,
    instance_generation: u64,
    instance_applied: bool,
    instance_queries_ready: bool,
    refreshed: bool,
}
impl Default for Occlusion {
    fn default() -> Self {
        Self {
            enabled: true,
            camera: None,
            projections: Vec::new(),
            resources: None,
            candidates: Vec::new(),
            occluders: Vec::new(),
            snapshot: None,
            generation: 0,
            adaptive: true,
            last_result_frame: None,
            unproductive_results: 0,
            cooldown: 0,
            instance_enabled: true,
            instance_resources: None,
            instance_candidates: Vec::new(),
            instance_snapshot: None,
            instance_generation: 0,
            instance_applied: false,
            instance_queries_ready: false,
            refreshed: false,
        }
    }
}
fn double_sided(renderer: &SceneRenderer, object: &DrawItem) -> bool {
    match &object.mesh {
        MeshKind::ModelPart(id, index) => renderer.models[id][*index]
            .shading
            .as_ref()
            .is_none_or(|s| s.double_sided),
        _ => true,
    }
}

fn project(bounds: [Vec3; 2], mvp: Mat4, size: [u32; 2]) -> Option<Projection> {
    let mut min = Vec3::splat(f32::INFINITY);
    let mut max = Vec3::splat(f32::NEG_INFINITY);
    for i in 0..8 {
        let q = mvp
            * Vec3::new(
                bounds[i & 1].x,
                bounds[(i >> 1) & 1].y,
                bounds[(i >> 2) & 1].z,
            )
            .extend(1.);
        // Perspective extrema lie at vertices only when the entire box is in
        // front of the near plane. Crossing boxes conservatively remain visible.
        if !q.is_finite() || q.w <= 1e-5 || q.z <= 1e-5 {
            return None;
        }
        let p = q.truncate() / q.w;
        min = min.min(p);
        max = max.max(p);
    }
    let width = size[0] as f32;
    let height = size[1] as f32;
    let rectangle = [
        ((min.x * 0.5 + 0.5) * width - 2.).clamp(0., width - 1.),
        ((-max.y * 0.5 + 0.5) * height - 2.).clamp(0., height - 1.),
        ((max.x * 0.5 + 0.5) * width + 2.).clamp(0., width - 1.),
        ((-min.y * 0.5 + 0.5) * height + 2.).clamp(0., height - 1.),
    ];
    Some(Projection {
        rectangle,
        nearest: min.z,
    })
}
pub(super) fn opaque(renderer: &SceneRenderer, draw: &PreparedDraw) -> bool {
    if draw.transparent
        || draw.shader.is_some()
        || draw.opacity < 1.
        || draw.cutoff > 0.
        || draw.deformation != 0
    {
        return false;
    }
    if matches!(
        draw.object.mesh,
        MeshKind::Text(_) | MeshKind::SharedText(_) | MeshKind::Sprite(_)
    ) {
        return false;
    }
    // Alpha-masked materials can be classified opaque by the main pass. Test
    // their actual texture coverage before allowing a texture-free depth pass.
    match &draw.object.material.texture {
        TextureKind::Generated(_) | TextureKind::Text => false,
        TextureKind::Imported(id) => !renderer.transparent_textures.contains(id),
        TextureKind::ModelPart(id, index) => {
            let part = &renderer.models[id][*index];
            !part.translucent
                && part
                    .shading
                    .as_ref()
                    .is_none_or(|shading| shading.base_color_opaque_addressing)
        }
        _ => true,
    }
}

fn candidate(bytes: &mut Vec<u8>, rectangle: [f32; 4], nearest: f32, indices: u32, instances: u32) {
    for edge in rectangle {
        bytes.extend_from_slice(&((edge.max(0.) / 8.) as u32).to_le_bytes());
    }
    bytes.extend_from_slice(&nearest.to_le_bytes());
    bytes.extend_from_slice(&indices.to_le_bytes());
    bytes.extend_from_slice(&instances.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
}
fn snapshot_matches(
    previous: Option<&Snapshot>,
    renderer: &SceneRenderer,
    vp: Mat4,
    size: [u32; 2],
    draws: &[PreparedDraw],
    occluders: &[(usize, Projection)],
    candidates: &[u8],
) -> bool {
    previous.is_some_and(|old| {
        old.camera == (vp, size)
            && old.candidates == candidates
            && old.depth.len() == occluders.len()
            && old.depth.iter().zip(occluders).all(|(prior, (index, _))| {
                let object = &draws[*index].object;
                prior.mesh == object.mesh
                    && prior.model == object.model
                    && prior.double_sided == double_sided(renderer, object)
            })
    })
}
fn snapshot(
    renderer: &SceneRenderer,
    vp: Mat4,
    size: [u32; 2],
    draws: &[PreparedDraw],
    occluders: &[(usize, Projection)],
    candidates: &[u8],
) -> Snapshot {
    Snapshot {
        camera: (vp, size),
        candidates: candidates.to_vec(),
        depth: occluders
            .iter()
            .map(|(i, _)| {
                let object = &draws[*i].object;
                DepthInput {
                    mesh: object.mesh.clone(),
                    model: object.model,
                    double_sided: double_sided(renderer, object),
                }
            })
            .collect(),
    }
}
impl SceneRenderer {
    /// Reference switch: disabled draws the same surfaces without depth/compute
    /// culling. Small scenes and scenes without useful opaque occluders skip it.
    pub fn set_occlusion_enabled(&mut self, enabled: bool) {
        self.occlusion.enabled = enabled;
    }
    pub fn occlusion_result(&self) -> Option<OcclusionResult> {
        self.occlusion.resources.as_ref().and_then(|r| r.result)
    }
    /// Diagnostic reference retains whole-batch indirect occlusion.
    pub fn set_instance_occlusion_enabled(&mut self, enabled: bool) {
        self.occlusion.instance_enabled = enabled;
        self.occlusion.instance_applied = false;
        self.occlusion.instance_snapshot = None;
    }
    /// Call before instance packing; the mask is an ordered subsequence of the
    /// original frustum-visible list. Retained GPU results require exact inputs.
    pub(super) fn apply_cached_instance_occlusion(
        &mut self,
        gpu: &Gpu,
        draws: &[PreparedDraw],
        view_projection: Mat4,
        size: [u32; 2],
        visible: &mut [bool],
    ) {
        let mut state = std::mem::take(&mut self.occlusion);
        state.apply_instances(self, gpu, view_projection, size, draws, visible);
        self.occlusion = state;
    }
    /// Compare adaptive pass scheduling with always testing eligible views.
    /// Bypassing a test draws all frustum-visible surfaces; it never reuses
    /// approximate visibility from a different camera or scene.
    pub fn set_adaptive_occlusion_enabled(&mut self, enabled: bool) {
        self.occlusion.adaptive = enabled;
        self.occlusion.unproductive_results = 0;
        self.occlusion.cooldown = 0;
    }
    #[allow(clippy::too_many_arguments)]
    pub(super) fn prepare_occlusion(
        &mut self,
        gpu: &Gpu,
        encoder: &mut crate::profiling::Encoder,
        view_projection: Mat4,
        size: [u32; 2],
        draws: &[PreparedDraw],
        visible: &[bool],
        batches: &[instancing::Batch],
    ) -> Mode {
        let mut state = std::mem::take(&mut self.occlusion);
        let active = state.prepare(
            self,
            gpu,
            encoder,
            view_projection,
            size,
            draws,
            visible,
            batches,
        );
        self.occlusion = state;
        active
    }
}
impl Occlusion {
    pub(super) fn invalidate(&mut self) {
        // Asset replacement can change depth coverage without changing IDs or
        // bounds. All publication/removal paths invalidate object bindings.
        self.snapshot = None;
        self.instance_snapshot = None;
    }
    pub(super) fn batch_visible(&self, batch: usize) -> bool {
        self.resources.as_ref().unwrap().visibility[batch]
    }
    pub(super) fn arguments(&self) -> &wgpu::Buffer {
        &self.resources.as_ref().unwrap().arguments
    }
    pub(super) fn submitted(&mut self) {
        if let Some(resources) = &mut self.resources {
            resources.submitted();
        }
        if let Some(resources) = &mut self.instance_resources {
            resources.submitted();
        }
    }
    fn observe_result(&mut self, result: OcclusionResult) {
        if self.last_result_frame == Some(result.frame_id) {
            return;
        }
        self.last_result_frame = Some(result.frame_id);
        if result.tested_batches > 0 && result.skipped_triangles == 0 {
            self.unproductive_results += 1;
            if self.unproductive_results >= 3 {
                self.cooldown = 30;
                self.unproductive_results = 0;
            }
        } else {
            self.unproductive_results = 0;
            self.cooldown = 0;
        }
    }
    fn refresh_inputs(
        &mut self,
        renderer: &SceneRenderer,
        vp: Mat4,
        size: [u32; 2],
        draws: &[PreparedDraw],
        visible: &[bool],
    ) {
        let camera_changed = self.camera != Some((vp, size));
        self.camera = Some((vp, size));
        self.projections.truncate(draws.len());
        self.occluders.clear();
        for (index, draw) in draws.iter().enumerate() {
            let bounds = renderer.mesh_for(&draw.object).bounds;
            if index == self.projections.len() {
                self.projections.push(CachedProjection {
                    model: draw.object.model,
                    bounds,
                    projection: project(bounds, vp * draw.object.model, size),
                });
            } else {
                let previous = &mut self.projections[index];
                if camera_changed
                    || previous.model != draw.object.model
                    || previous.bounds != bounds
                {
                    *previous = CachedProjection {
                        model: draw.object.model,
                        bounds,
                        projection: project(bounds, vp * draw.object.model, size),
                    };
                }
            }
            if !visible[index] || !opaque(renderer, draw) {
                continue;
            }
            let Some(p) = self.projections[index].projection else {
                continue;
            };
            let r = p.rectangle;
            let area = (r[2] - r[0]) * (r[3] - r[1]);
            if area < size[0] as f32 * size[1] as f32 * 0.02
                || renderer.mesh_for(&draw.object).count as u64 / 3 > MAX_DEPTH_TRIANGLES
            {
                continue;
            }
            self.occluders.push((index, p));
        }
        // Favor nearby large surfaces; retain at most 32 without sorting the
        // scene's full draw list or changing the color pass's tie ordering.
        let order = |a: &(usize, Projection), b: &(usize, Projection)| {
            a.1.nearest.total_cmp(&b.1.nearest).then(a.0.cmp(&b.0))
        };
        if self.occluders.len() > MAX_OCCLUDERS {
            self.occluders
                .select_nth_unstable_by(MAX_OCCLUDERS - 1, order);
            self.occluders.truncate(MAX_OCCLUDERS);
        }
        self.occluders.sort_unstable_by(order);
        let mut triangles = 0;
        self.occluders.retain(|(i, _)| {
            let count = u64::from(renderer.mesh_for(&draws[*i].object).count / 3);
            if triangles + count > MAX_DEPTH_TRIANGLES {
                return false;
            }
            triangles += count;
            true
        });
    }
    fn apply_instances(
        &mut self,
        renderer: &mut SceneRenderer,
        gpu: &Gpu,
        vp: Mat4,
        size: [u32; 2],
        draws: &[PreparedDraw],
        visible: &mut [bool],
    ) {
        self.instance_applied = false;
        self.instance_queries_ready = false;
        self.refreshed = false;
        if let Some(resources) = &mut self.instance_resources {
            resources.poll(gpu);
        }
        if !self.enabled
            || !self.instance_enabled
            || !renderer.culling
            || !renderer.state_caching
            || draws.len() > MAX_BATCHES
            || visible.iter().filter(|v| **v).count() < MIN_SURFACES
        {
            return;
        }
        self.refresh_inputs(renderer, vp, size, draws, visible);
        self.refreshed = true;
        if self.occluders.is_empty() {
            return;
        }
        self.instance_candidates.clear();
        for (index, draw) in draws.iter().enumerate() {
            let projection = self.projections[index]
                .projection
                .filter(|_| visible[index] && !draw.transparent && draw.deformation == 0);
            let (rectangle, nearest) =
                projection.map_or(([0.; 4], -1.), |p| (p.rectangle, p.nearest));
            candidate(
                &mut self.instance_candidates,
                rectangle,
                nearest,
                renderer.mesh_for(&draw.object).count,
                u32::from(visible[index]),
            );
        }
        self.instance_queries_ready = true;
        let unchanged = snapshot_matches(
            self.instance_snapshot.as_ref(),
            renderer,
            vp,
            size,
            draws,
            &self.occluders,
            &self.instance_candidates,
        );
        if !unchanged {
            self.instance_generation = self.instance_generation.wrapping_add(1);
            self.instance_snapshot = Some(snapshot(
                renderer,
                vp,
                size,
                draws,
                &self.occluders,
                &self.instance_candidates,
            ));
        }
        if let Some(resources) = &self.instance_resources
            && resources.visibility_generation == Some(self.instance_generation)
            && resources.visibility.len() == visible.len()
        {
            for (visible, retained) in visible.iter_mut().zip(&resources.visibility) {
                *visible &= *retained;
            }
            self.instance_applied = true;
            renderer.stats.occlusion_result = resources.result;
            renderer.stats.occlusion_candidates = resources.result.map_or(0, |r| r.tested_batches);
            renderer.stats.occlusion_bytes =
                self.resources.as_ref().map_or(0, |r| r.bytes()) + resources.bytes();
            if resources.result.is_some_and(|r| r.skipped_triangles > 0) {
                self.cooldown = 0;
                self.unproductive_results = 0;
            }
        }
    }
    #[allow(clippy::too_many_arguments)]
    fn prepare(
        &mut self,
        renderer: &mut SceneRenderer,
        gpu: &Gpu,
        encoder: &mut crate::profiling::Encoder,
        vp: Mat4,
        size: [u32; 2],
        draws: &[PreparedDraw],
        visible: &[bool],
        batches: &[instancing::Batch],
    ) -> Mode {
        if let Some(resources) = &mut self.resources {
            resources.poll(gpu);
            renderer.stats.occlusion_result = resources.result;
            renderer.stats.occlusion_bytes = resources.bytes();
            if self.adaptive
                && renderer.state_caching
                && let Some(result) = resources.result
            {
                self.observe_result(result);
            }
        }
        if self.instance_applied {
            renderer.stats.occlusion_result =
                self.instance_resources.as_ref().and_then(|r| r.result);
            renderer.stats.occlusion_cache_hit = true;
            self.refreshed = false;
            return Mode::Disabled;
        }
        if !self.enabled
            || !renderer.culling
            || visible.iter().filter(|v| **v).count() < MIN_SURFACES
            || batches.len() > MAX_BATCHES
            || batches.len() < 4
        {
            return Mode::Disabled;
        }
        if self.adaptive && renderer.state_caching && self.cooldown > 0 {
            self.cooldown -= 1;
            return Mode::Disabled;
        }
        if !self.refreshed {
            self.refresh_inputs(renderer, vp, size, draws, visible);
        }
        self.refreshed = false;
        if self.occluders.is_empty() {
            return Mode::Disabled;
        }
        let triangles = self
            .occluders
            .iter()
            .map(|(i, _)| u64::from(renderer.mesh_for(&draws[*i].object).count / 3))
            .sum();
        self.candidates.clear();
        let mut tested = 0;
        for batch in batches {
            let mut rectangle = [
                f32::INFINITY,
                f32::INFINITY,
                f32::NEG_INFINITY,
                f32::NEG_INFINITY,
            ];
            let mut nearest = 1.0_f32;
            for &index in &batch.indices {
                let Some(p) = self.projections[index]
                    .projection
                    .filter(|_| !draws[index].transparent && draws[index].deformation == 0)
                else {
                    nearest = -1.;
                    break;
                };
                rectangle[0] = rectangle[0].min(p.rectangle[0]);
                rectangle[1] = rectangle[1].min(p.rectangle[1]);
                rectangle[2] = rectangle[2].max(p.rectangle[2]);
                rectangle[3] = rectangle[3].max(p.rectangle[3]);
                nearest = nearest.min(p.nearest);
            }
            tested += usize::from(nearest >= 0.);
            for edge in rectangle {
                self.candidates
                    .extend_from_slice(&((edge.max(0.) / 8.) as u32).to_le_bytes());
            }
            self.candidates.extend_from_slice(&nearest.to_le_bytes());
            self.candidates.extend_from_slice(
                &renderer
                    .mesh_for(&draws[batch.indices[0]].object)
                    .count
                    .to_le_bytes(),
            );
            self.candidates
                .extend_from_slice(&(batch.indices.len() as u32).to_le_bytes());
            self.candidates
                .extend_from_slice(&batch.first_instance.to_le_bytes());
        }
        let unchanged = self.snapshot.as_ref().is_some_and(|previous| {
            previous.camera == (vp, size)
                && previous.candidates == self.candidates
                && previous.depth.len() == self.occluders.len()
                && previous
                    .depth
                    .iter()
                    .zip(&self.occluders)
                    .all(|(old, (i, _))| {
                        let object = &draws[*i].object;
                        old.mesh == object.mesh
                            && old.model == object.model
                            && old.double_sided == double_sided(renderer, object)
                    })
        });
        if !unchanged {
            self.generation = self.generation.wrapping_add(1);
            let snapshot = self.snapshot.get_or_insert_with(|| Snapshot {
                camera: (vp, size),
                depth: Vec::with_capacity(MAX_OCCLUDERS),
                candidates: Vec::new(),
            });
            snapshot.camera = (vp, size);
            snapshot.candidates.clone_from(&self.candidates);
            snapshot.depth.clear();
            snapshot.depth.extend(self.occluders.iter().map(|(i, _)| {
                let object = &draws[*i].object;
                DepthInput {
                    mesh: object.mesh.clone(),
                    model: object.model,
                    double_sided: double_sided(renderer, object),
                }
            }));
        }
        renderer.stats.occlusion_candidates = tested;
        let resources = self
            .resources
            .get_or_insert_with(|| gpu::Resources::new(gpu));
        if renderer.state_caching && resources.visibility_generation == Some(self.generation) {
            if self.instance_queries_ready {
                let queries = self
                    .instance_resources
                    .get_or_insert_with(|| gpu::Resources::new_queries(gpu, resources));
                queries.prepare_queries(gpu, size, &self.instance_candidates, resources);
                let tested = self
                    .instance_candidates
                    .chunks_exact(32)
                    .filter(|c| f32::from_le_bytes(c[16..20].try_into().unwrap()) >= 0.)
                    .count();
                queries.encode(
                    encoder,
                    draws.len(),
                    tested,
                    &self.instance_candidates,
                    self.instance_generation,
                    false,
                );
            }
            renderer.stats.occlusion_cache_hit = true;
            return Mode::Cached;
        }
        resources.prepare(gpu, size, &self.candidates);
        resources.encode_depth(renderer, gpu, encoder, vp, draws, &self.occluders);
        resources.encode(
            encoder,
            batches.len(),
            tested,
            &self.candidates,
            self.generation,
            true,
        );
        if self.instance_queries_ready {
            let queries = self
                .instance_resources
                .get_or_insert_with(|| gpu::Resources::new_queries(gpu, resources));
            queries.prepare_queries(gpu, size, &self.instance_candidates, resources);
            let tested = self
                .instance_candidates
                .chunks_exact(32)
                .filter(|c| f32::from_le_bytes(c[16..20].try_into().unwrap()) >= 0.)
                .count();
            queries.encode(
                encoder,
                draws.len(),
                tested,
                &self.instance_candidates,
                self.instance_generation,
                false,
            );
        }
        renderer.stats.occlusion_depth_draws = self.occluders.len();
        renderer.stats.occlusion_depth_triangles = triangles;
        renderer.stats.occlusion_bytes =
            resources.bytes() + self.instance_resources.as_ref().map_or(0, |r| r.bytes());
        Mode::Indirect
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn adaptive_policy_requires_fresh_unproductive_samples_and_resets_on_savings() {
        let mut state = Occlusion::default();
        for frame in 1..=3 {
            let result = OcclusionResult {
                frame_id: frame,
                tested_batches: 16,
                ..Default::default()
            };
            state.observe_result(result);
            state.observe_result(result);
            assert_eq!(state.cooldown, if frame == 3 { 30 } else { 0 });
        }
        state.observe_result(OcclusionResult {
            frame_id: 4,
            tested_batches: 16,
            skipped_triangles: 100,
            ..Default::default()
        });
        assert_eq!(state.cooldown, 0);
        assert_eq!(state.unproductive_results, 0);
    }
    #[test]
    fn cached_instance_queries_compact_mixed_batches_and_invalidate_exactly() -> Result<()> {
        let gpu = pollster::block_on(Gpu::request(
            &crate::instance(crate::Backend::native()),
            None,
            false,
        ))?;
        let mut optimized = SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm);
        let mut reference = SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm);
        reference.set_occlusion_enabled(false);
        optimized.set_adaptive_occlusion_enabled(false);
        let material = |texture| Material {
            metallic: None,
            roughness: None,
            surface_overrides: Default::default(),
            tint: [0.4, 0.8, 0.3],
            uv_scale: [1.; 2],
            texture,
            lit: false,
            shader: None,
        };
        let mut scene = RenderScene {
            skin_poses: Default::default(),
            shader_time: 0.,
            particles: vec![],
            fog: Default::default(),
            gi: None,
            lights: vec![],
            environment: EnvironmentSettings::disabled(),
            display: DisplaySettings {
                tone_mapping: false,
                ..Default::default()
            },
            lighting: Lighting {
                shadows: false,
                ..Default::default()
            },
            view_projection: glam::camera::rh::proj::directx::orthographic(
                -2., 2., -2., 2., 0.1, 20.,
            ),
            items: vec![DrawItem {
                motion_id: 1,
                model: Mat4::from_translation(Vec3::new(0., 0., -2.))
                    * Mat4::from_scale(Vec3::new(2., 2., 1.)),
                mesh: MeshKind::Quad,
                material: material(TextureKind::White),
            }],
        };
        let graph=std::sync::Arc::new(ShaderSource {id:99887,opaque_sort_id:99887,numeric_parameters:std::sync::Arc::from([]),surface:"fn graph_material_surface(uv:vec2<f32>,normal_uv:vec2<f32>,mr_uv:vec2<f32>,ao_uv:vec2<f32>,emissive_uv:vec2<f32>,world_normal:vec3<f32>,tangent:vec4<f32>,world:vec3<f32>,view:vec3<f32>,front:bool,time:f32)->SurfaceParams { return default_material_surface(uv,normal_uv,mr_uv,ao_uv,emissive_uv,world_normal,tangent,world,view,front,time); }".into()});
        for group in 0..4 {
            for index in 0..24 {
                let mut item = DrawItem {
                    motion_id: 2 + group * 24 + index,
                    model: Mat4::from_translation(Vec3::new(
                        if index == 0 {
                            1.5
                        } else {
                            (index % 5) as f32 * 0.15 - 0.3
                        },
                        if index == 0 {
                            group as f32 * 0.2 - 0.3
                        } else {
                            (index / 5) as f32 * 0.15 - 0.3
                        },
                        -4.,
                    )) * Mat4::from_scale(Vec3::splat(0.1)),
                    mesh: MeshKind::Cube,
                    material: material(match group {
                        0 => TextureKind::Checker,
                        1 => TextureKind::Normals,
                        2 => TextureKind::Toon,
                        _ => TextureKind::White,
                    }),
                };
                if group == 3 {
                    item.material.shader = Some(graph.clone());
                }
                scene.items.push(item);
            }
        }
        let capture = |renderer: &mut SceneRenderer, scene: &RenderScene| {
            crate::capture_offscreen(&gpu, 256, 256, |target| {
                renderer.draw_linear(&gpu, target, [256; 2], scene)
            })
        };
        let expected = capture(&mut reference, &scene)?;
        for _ in 0..4 {
            assert_eq!(capture(&mut optimized, &scene)?.rgba, expected.rgba);
            gpu.wait()?;
        }
        assert!(optimized.stats.occlusion_cache_hit);
        assert!(
            optimized.stats.color_triangles < reference.stats.color_triangles / 4,
            "individual hidden members of visible union batches must compact"
        );
        assert!(
            optimized
                .occlusion
                .instance_resources
                .as_ref()
                .unwrap()
                .result
                .unwrap()
                .culled_surfaces
                >= 80
        );
        let saved = optimized.stats.color_triangles;
        // Changed camera and bounds reject old masks before packing; strict
        // current-frame GPU queries still produce exact reference pixels.
        scene.items[9].model =
            Mat4::from_translation(Vec3::new(-1.4, 0.8, -4.)) * Mat4::from_scale(Vec3::splat(0.2));
        scene.view_projection *= Mat4::from_translation(Vec3::new(0.02, 0., 0.));
        let expected = capture(&mut reference, &scene)?;
        assert_eq!(capture(&mut optimized, &scene)?.rgba, expected.rgba);
        assert!(!optimized.occlusion.instance_applied);
        for _ in 0..3 {
            assert_eq!(capture(&mut optimized, &scene)?.rgba, expected.rgba);
            gpu.wait()?;
        }
        // Removing the occluder must immediately restore every former member.
        scene.items.remove(0);
        assert_eq!(
            capture(&mut optimized, &scene)?.rgba,
            capture(&mut reference, &scene)?.rgba
        );
        assert!(!optimized.occlusion.instance_applied);
        assert_eq!(
            optimized.stats.color_triangles,
            reference.stats.color_triangles
        );
        // A moving open view with an occluder behind all queried surfaces
        // yields three fresh zero-savings readbacks, then bypasses the GPU test.
        optimized.set_instance_occlusion_enabled(false);
        optimized.set_adaptive_occlusion_enabled(true);
        scene.items.insert(
            0,
            DrawItem {
                motion_id: 1,
                model: Mat4::from_translation(Vec3::new(0., 0., -8.))
                    * Mat4::from_scale(Vec3::new(2., 2., 1.)),
                mesh: MeshKind::Quad,
                material: material(TextureKind::White),
            },
        );
        for tick in 0..8 {
            scene.view_projection =
                glam::camera::rh::proj::directx::orthographic(-2., 2., -2., 2., 0.1, 20.)
                    * Mat4::from_translation(Vec3::new(tick as f32 * 0.001, 0., 0.));
            assert_eq!(
                capture(&mut optimized, &scene)?.rgba,
                capture(&mut reference, &scene)?.rgba,
                "adaptive bypass pixels tick{tick}"
            );
            gpu.wait()?;
        }
        assert!(optimized.occlusion.cooldown > 0);
        assert_eq!(optimized.stats.occlusion_depth_draws, 0);
        println!(
            "instance_occlusion_proof graph_candidates mixed_union_batch {}->{}triangles exact_camera_bounds_occluder_invalidation",
            reference.stats.color_triangles, saved
        );
        Ok(())
    }
    #[test]
    fn depth_and_compute_shaders_validate_with_baseline_capabilities() {
        for source in [
            include_str!("occlusion/depth.wgsl"),
            include_str!("occlusion/tiles.wgsl"),
            include_str!("occlusion/reduce.wgsl"),
            include_str!("occlusion/cull.wgsl"),
        ] {
            let module = wgpu::naga::front::wgsl::parse_str(source)
                .unwrap_or_else(|e| panic!("{}", e.emit_to_string(source)));
            wgpu::naga::valid::Validator::new(
                wgpu::naga::valid::ValidationFlags::all(),
                wgpu::naga::valid::Capabilities::empty(),
            )
            .validate(&module)
            .unwrap();
        }
    }
    #[test]
    fn projected_bounds_enclose_vertices_and_keep_near_crossings_visible() {
        let bounds = [Vec3::splat(-0.5), Vec3::splat(0.5)];
        let camera = glam::camera::rh::proj::directx::perspective(1., 1.4, 0.1, 100.);
        for position in [Vec3::new(0., 0., -0.2), Vec3::new(0., 0., 1.)] {
            assert!(
                project(
                    bounds,
                    camera * Mat4::from_translation(position),
                    [157, 139]
                )
                .is_none()
            );
        }
        let size = [157, 139];
        let mvp = camera
            * Mat4::from_translation(Vec3::new(0.2, -0.1, -4.))
            * Mat4::from_rotation_y(0.7)
            * Mat4::from_scale(Vec3::new(-2., 0.8, 0.4));
        let p = project(bounds, mvp, size).unwrap();
        for i in 0..8 {
            let q = mvp.project_point3(Vec3::new(
                bounds[i & 1].x,
                bounds[(i >> 1) & 1].y,
                bounds[(i >> 2) & 1].z,
            ));
            let xy = [
                (q.x * 0.5 + 0.5) * size[0] as f32,
                (-q.y * 0.5 + 0.5) * size[1] as f32,
            ];
            assert!(xy[0] >= p.rectangle[0] && xy[0] <= p.rectangle[2]);
            assert!(xy[1] >= p.rectangle[1] && xy[1] <= p.rectangle[3]);
            assert!(q.z >= p.nearest);
        }
    }
}
