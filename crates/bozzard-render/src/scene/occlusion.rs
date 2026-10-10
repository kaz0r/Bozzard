//! Hierarchical depth culling with asynchronous visibility readback. Reuse is
//! allowed only for identical depth inputs and query bounds; changed views use
//! current-frame GPU visibility, never a previous frame's approximate results.
use super::*;
mod gpu;

const MIN_SURFACES: usize = 64;
const MAX_BATCHES: usize = 16_384;
const MAX_OCCLUDERS: usize = 32;
const MAX_DEPTH_TRIANGLES: u64 = 100_000;
const UNPRODUCTIVE_COOLDOWN: u32 = 30;

#[derive(Clone, Copy, Debug, Default, serde::Serialize)]
pub struct OcclusionResult {
    pub frame_id: u64,
    pub tested_batches: usize,
    pub culled_batches: usize,
    pub culled_surfaces: usize,
    pub skipped_triangles: u64,
}
#[derive(Clone, Copy, Debug, PartialEq)]
struct Projection {
    rectangle: [f32; 4], // pixel edges, expanded for raster/projection roundoff
    nearest: f32,
}
#[derive(Clone, Debug, PartialEq)]
struct CachedProjection {
    model: Mat4,
    bounds: [Vec3; 2],
    /// Camera generation the projection was computed for; stale entries are
    /// refreshed only when the frame has at least one qualifying occluder.
    camera: u64,
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
    frame_bypassed: bool,
    instance_enabled: bool,
    instance_resources: Option<gpu::Resources>,
    instance_candidates: Vec<u8>,
    instance_snapshot: Option<Snapshot>,
    instance_generation: u64,
    instance_input_visible: Vec<bool>,
    instance_inputs_valid: bool,
    instance_input_reused: bool,
    instance_applied: bool,
    instance_queries_ready: bool,
    refreshed: bool,
    bound_prepass: bool,
    camera_generation: u64,
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
            frame_bypassed: false,
            instance_enabled: true,
            instance_resources: None,
            instance_candidates: Vec::new(),
            instance_snapshot: None,
            instance_generation: 0,
            instance_input_visible: Vec::new(),
            instance_inputs_valid: false,
            instance_input_reused: false,
            instance_applied: false,
            instance_queries_ready: false,
            refreshed: false,
            bound_prepass: true,
            camera_generation: 0,
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
/// Conservative upper bound on `project`'s clamped rectangle area, from a
/// world-space sphere around the box. `None` when the sphere reaches the camera
/// plane, where only the exact corner projection can decide.
fn projected_area_bound(bounds: [Vec3; 2], model: Mat4, vp: Mat4, size: [u32; 2]) -> Option<f32> {
    let half = (bounds[1] - bounds[0]) * 0.5;
    let linear = glam::Mat3::from_mat4(model);
    // The Frobenius norm bounds the largest stretch of any rotation/scale/shear.
    let stretch = (linear.x_axis.length_squared()
        + linear.y_axis.length_squared()
        + linear.z_axis.length_squared())
    .sqrt();
    let radius = half.length() * stretch * 1.001 + 1e-4;
    let center = model.transform_point3(bounds[0] + half).extend(1.);
    let [x, y, _, w] = [0, 1, 2, 3].map(|row| {
        let row = vp.row(row);
        (row.dot(center), row.truncate().length() * radius)
    });
    let near = w.0 - w.1;
    if !near.is_finite() || near <= 1e-5 || !x.0.is_finite() || !y.0.is_finite() {
        return None;
    }
    // For w > 0, x/w is monotone in x and in w, so extremes lie at the corners.
    let extent = |(value, spread): (f32, f32)| {
        let quotients = [
            (value - spread) / (w.0 - w.1),
            (value - spread) / (w.0 + w.1),
            (value + spread) / (w.0 - w.1),
            (value + spread) / (w.0 + w.1),
        ];
        let low = quotients.into_iter().fold(f32::INFINITY, f32::min);
        let high = quotients.into_iter().fold(f32::NEG_INFINITY, f32::max);
        high - low
    };
    let width = size[0] as f32;
    let height = size[1] as f32;
    // `project` pads by two pixels per edge; keep one more pixel and 1% slack.
    let span = |ndc: f32, pixels: f32| ((ndc * 0.5 * pixels + 6.) * 1.01).min(pixels - 1.).max(0.);
    let area = span(extent(x), width) * span(extent(y), height);
    area.is_finite().then_some(area)
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
        if self.occlusion.enabled != enabled {
            self.occlusion.invalidate();
        }
        self.occlusion.enabled = enabled;
    }
    /// Reference switch: disabled projects every surface before selecting
    /// occluders; enabled first rejects surfaces whose conservative screen
    /// bound cannot reach the occluder threshold.
    pub fn set_occlusion_bound_prepass_enabled(&mut self, enabled: bool) {
        self.occlusion.bound_prepass = enabled;
    }
    pub fn occlusion_result(&self) -> Option<OcclusionResult> {
        self.occlusion.resources.as_ref().and_then(|r| r.result)
    }
    /// Diagnostic reference retains whole-batch indirect occlusion.
    pub fn set_instance_occlusion_enabled(&mut self, enabled: bool) {
        self.occlusion.instance_enabled = enabled;
        self.occlusion.instance_applied = false;
        self.occlusion.instance_snapshot = None;
        self.occlusion.reset_adaptive_history();
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
        let started = std::time::Instant::now();
        let mut state = std::mem::take(&mut self.occlusion);
        state.apply_instances(self, gpu, view_projection, size, draws, visible);
        self.occlusion = state;
        self.stats.occlusion_prepare_ms += started.elapsed().as_secs_f64() * 1000.;
    }
    /// Compare adaptive pass scheduling with always testing eligible views.
    /// Bypassing a test draws all frustum-visible surfaces; it never reuses
    /// approximate visibility from a different camera or scene.
    pub fn set_adaptive_occlusion_enabled(&mut self, enabled: bool) {
        self.occlusion.adaptive = enabled;
        self.occlusion.reset_adaptive_history();
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
        let started = std::time::Instant::now();
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
        self.stats.occlusion_prepare_ms += started.elapsed().as_secs_f64() * 1000.;
        active
    }
}
impl Occlusion {
    pub(super) fn invalidate(&mut self) {
        // Asset replacement can change depth coverage without changing IDs or
        // bounds. Retiring resident geometry always invalidates this snapshot.
        self.snapshot = None;
        self.instance_snapshot = None;
        self.instance_inputs_valid = false;
        self.reset_adaptive_history();
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
        if let Some(frame) = self.last_result_frame {
            if frame > result.frame_id {
                return;
            }
            if frame == result.frame_id {
                // Separate readbacks can complete on different CPU frames.
                // A late productive instance result upgrades its earlier
                // zero-savings batch result without counting the frame twice.
                if result.skipped_triangles > 0 {
                    self.reset_adaptive_history();
                }
                return;
            }
        }
        self.last_result_frame = Some(result.frame_id);
        if result.tested_batches > 0 && result.skipped_triangles == 0 {
            self.unproductive_results += 1;
            if self.unproductive_results >= 3 {
                self.cooldown = UNPRODUCTIVE_COOLDOWN;
                self.unproductive_results = 0;
            }
        } else {
            self.unproductive_results = 0;
            self.cooldown = 0;
        }
    }
    fn reset_adaptive_history(&mut self) {
        self.unproductive_results = 0;
        self.cooldown = 0;
        self.frame_bypassed = false;
    }
    fn observe_frame_results(
        &mut self,
        batches: Option<OcclusionResult>,
        instances: Option<OcclusionResult>,
    ) {
        // Both queries refer to the same submitted frame. Count it once;
        // per-instance savings can make an unproductive union batch useful.
        if let Some(result) = batches
            .into_iter()
            .chain(instances)
            .max_by_key(|r| (r.frame_id, r.skipped_triangles))
        {
            self.observe_result(result);
        }
    }
    fn advance_cooldown(&mut self, active: bool) {
        // Called once, before both preparation stages. The last bypass frame
        // remains bypassed even after its decrement reaches zero.
        self.frame_bypassed = active && self.cooldown > 0;
        if self.frame_bypassed {
            self.cooldown -= 1;
        }
    }
    fn retained_instance_inputs_match(
        &self,
        stats: &FrameStats,
        vp: Mat4,
        size: [u32; 2],
        draws: &[PreparedDraw],
        visible: &[bool],
    ) -> bool {
        // Surface preparation already compares exact source mesh/material
        // content and matrix bits. Publication invalidates these snapshots.
        // Its unchanged order/records/model certificate lets us reuse bounds,
        // occluder selection and query bytes without repeating their expansion.
        // Posed bounds have a separate lifecycle, so keep deformation on the
        // full validation path even when the source records were retained.
        self.instance_inputs_valid
            && stats.surface_order_reused
            && stats.surface_records_built == 0
            && stats.surface_model_updates == 0
            && self
                .instance_snapshot
                .as_ref()
                .is_some_and(|snapshot| snapshot.camera == (vp, size))
            && self.instance_input_visible.as_slice() == visible
            && self.projections.len() == draws.len()
            && self.instance_candidates.len() == draws.len() * 32
            && draws.iter().all(|draw| draw.deformation == 0)
    }
    fn refresh_inputs(
        &mut self,
        renderer: &mut SceneRenderer,
        vp: Mat4,
        size: [u32; 2],
        draws: &[PreparedDraw],
        visible: &[bool],
    ) {
        if self.camera != Some((vp, size)) {
            self.camera = Some((vp, size));
            self.camera_generation = self.camera_generation.wrapping_add(1);
        }
        let generation = self.camera_generation;
        let minimum_area = size[0] as f32 * size[1] as f32 * 0.02;
        self.projections.truncate(draws.len());
        self.occluders.clear();
        let mut projections = 0;
        let mut rejections = 0;
        // Selection: cheap predicates first, then a conservative screen bound,
        // and an exact projection only where the bound can reach the threshold.
        for (index, draw) in draws.iter().enumerate() {
            let bounds = renderer.mesh_for(&draw.object).bounds;
            if index == self.projections.len() {
                self.projections.push(CachedProjection {
                    model: draw.object.model,
                    bounds,
                    camera: generation.wrapping_sub(1),
                    projection: None,
                });
            }
            let fresh = {
                let cached = &self.projections[index];
                cached.camera == generation
                    && cached.model == draw.object.model
                    && cached.bounds == bounds
            };
            let qualifies = visible[index]
                && renderer.mesh_for(&draw.object).count as u64 / 3 <= MAX_DEPTH_TRIANGLES
                && opaque(renderer, draw);
            if !qualifies && self.bound_prepass {
                continue;
            }
            if !fresh {
                if self.bound_prepass
                    && projected_area_bound(bounds, draw.object.model, vp, size)
                        .is_some_and(|bound| bound < minimum_area)
                {
                    rejections += 1;
                    continue;
                }
                projections += 1;
                self.projections[index] = CachedProjection {
                    model: draw.object.model,
                    bounds,
                    camera: generation,
                    projection: project(bounds, vp * draw.object.model, size),
                };
            }
            if !qualifies {
                continue;
            }
            let Some(p) = self.projections[index].projection else {
                continue;
            };
            let r = p.rectangle;
            let area = (r[2] - r[0]) * (r[3] - r[1]);
            if area < minimum_area {
                continue;
            }
            self.occluders.push((index, p));
        }
        // Candidate rectangles need every projection, but only when depth exists.
        if !self.occluders.is_empty() {
            for (index, draw) in draws.iter().enumerate() {
                let bounds = renderer.mesh_for(&draw.object).bounds;
                let cached = &mut self.projections[index];
                if cached.camera != generation
                    || cached.model != draw.object.model
                    || cached.bounds != bounds
                {
                    projections += 1;
                    *cached = CachedProjection {
                        model: draw.object.model,
                        bounds,
                        camera: generation,
                        projection: project(bounds, vp * draw.object.model, size),
                    };
                }
            }
        }
        renderer.stats.occlusion_projections += projections;
        renderer.stats.occlusion_bound_rejections += rejections;
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
        self.instance_input_reused = false;
        self.instance_queries_ready = false;
        self.refreshed = false;
        if let Some(resources) = &mut self.resources {
            resources.poll(gpu);
            renderer.stats.occlusion_result = resources.result;
        }
        if let Some(resources) = &mut self.instance_resources {
            resources.poll(gpu);
        }
        renderer.stats.occlusion_bytes = self.resources.as_ref().map_or(0, |r| r.bytes())
            + self.instance_resources.as_ref().map_or(0, |r| r.bytes());
        let adaptive = self.enabled && self.adaptive && renderer.culling && renderer.state_caching;
        if adaptive {
            self.observe_frame_results(
                self.resources.as_ref().and_then(|r| r.result),
                self.instance_resources
                    .as_ref()
                    .filter(|_| self.instance_enabled)
                    .and_then(|r| r.result),
            );
        }
        self.advance_cooldown(adaptive);
        if !self.enabled
            || self.frame_bypassed
            || !self.instance_enabled
            || !renderer.culling
            || !renderer.state_caching
            || draws.len() > MAX_BATCHES
            || visible.iter().filter(|v| **v).count() < MIN_SURFACES
        {
            return;
        }
        self.instance_input_reused =
            self.retained_instance_inputs_match(&renderer.stats, vp, size, draws, visible);
        if !self.instance_input_reused {
            // A failed/no-occluder refresh must not leave the preceding input
            // certificate valid, even if a later frame reverts its source data.
            self.instance_inputs_valid = false;
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
            self.instance_input_visible.clear();
            self.instance_input_visible.extend_from_slice(visible);
            self.instance_inputs_valid = true;
        }
        self.refreshed = true;
        self.instance_queries_ready = true;
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
        if self.instance_applied {
            renderer.stats.occlusion_result =
                self.instance_resources.as_ref().and_then(|r| r.result);
            renderer.stats.occlusion_cache_hit = true;
            self.refreshed = false;
            return Mode::Disabled;
        }
        if !self.enabled
            || self.frame_bypassed
            || !renderer.culling
            || visible.iter().filter(|v| **v).count() < MIN_SURFACES
            || batches.len() > MAX_BATCHES
            // A few large instance batches can still hide many individual
            // surfaces. Keep this whole-batch efficiency threshold only when
            // there are no eligible per-instance queries to submit.
            || (batches.len() < 4 && !self.instance_queries_ready)
        {
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
    fn projected_area_bound_never_undercuts_the_exact_rectangle() {
        let mut seed = 0x9e37_79b9_u64;
        let mut next = || {
            seed = seed
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            ((seed >> 40) as f32 / (1u64 << 24) as f32) * 2. - 1.
        };
        let size = [640, 360];
        let mut bounded = 0;
        for _ in 0..10_000 {
            let vp = glam::camera::rh::proj::directx::perspective(
                0.6 + next().abs() * 1.2,
                16. / 9.,
                0.1,
                400.,
            ) * glam::camera::rh::view::look_at_mat4(
                Vec3::new(next() * 30., 5. + next() * 20., next() * 30.),
                Vec3::new(next() * 5., next() * 5., next() * 5.),
                Vec3::Y,
            );
            let low = Vec3::new(next(), next(), next()) * 2.;
            let bounds = [
                low,
                low + Vec3::new(next().abs(), next().abs(), next().abs()) * 3.,
            ];
            let model = Mat4::from_scale_rotation_translation(
                Vec3::new(
                    0.2 + next().abs() * 6.,
                    0.2 + next().abs(),
                    0.2 + next().abs() * 3.,
                ),
                glam::Quat::from_euler(glam::EulerRot::YXZ, next() * 3., next() * 3., next() * 3.),
                Vec3::new(next() * 60., next() * 20., next() * 60.),
            ) * Mat4::from_cols_array(&[
                1.,
                0.,
                0.,
                0.,
                next() * 0.5,
                1.,
                0.,
                0.,
                0.,
                0.,
                1.,
                0.,
                0.,
                0.,
                0.,
                1.,
            ]);
            let Some(bound) = projected_area_bound(bounds, model, vp, size) else {
                continue;
            };
            bounded += 1;
            if let Some(exact) = project(bounds, vp * model, size) {
                let r = exact.rectangle;
                let area = (r[2] - r[0]) * (r[3] - r[1]);
                assert!(bound >= area, "bound {bound} below exact {area}");
            }
        }
        assert!(bounded > 5_000, "bounds decided most cases: {bounded}");
    }
    #[test]
    fn retained_instance_input_certificate_rejects_every_changed_dependency() {
        let camera = Mat4::IDENTITY;
        let size = [128; 2];
        let mut draw = PreparedDraw {
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
                motion_id: 1,
                model: Mat4::IDENTITY,
                mesh: MeshKind::Cube,
                material: Material {
                    metallic: None,
                    roughness: None,
                    tint: [1.; 3],
                    uv_scale: [1.; 2],
                    texture: TextureKind::White,
                    lit: false,
                    shader: None,
                    surface_overrides: Default::default(),
                },
            },
            opacity: 1.,
            cutoff: 0.,
            transparent: false,
            depth: 0.,
        };
        let mut state = Occlusion {
            instance_inputs_valid: true,
            instance_input_visible: vec![true],
            instance_candidates: vec![0; 32],
            projections: vec![CachedProjection {
                model: Mat4::IDENTITY,
                bounds: [Vec3::ZERO; 2],
                camera: 0,
                projection: None,
            }],
            instance_snapshot: Some(Snapshot {
                camera: (camera, size),
                depth: vec![],
                candidates: vec![0; 32],
            }),
            ..Default::default()
        };
        let mut stats = FrameStats {
            surface_order_reused: true,
            ..Default::default()
        };
        let matches = |state: &Occlusion, stats: &FrameStats, draw: &PreparedDraw| {
            state.retained_instance_inputs_match(
                stats,
                camera,
                size,
                std::slice::from_ref(draw),
                &[true],
            )
        };
        assert!(matches(&state, &stats, &draw));
        stats.surface_order_reused = false;
        assert!(
            !matches(&state, &stats, &draw),
            "membership or order changed"
        );
        stats.surface_order_reused = true;
        stats.surface_records_built = 1;
        assert!(
            !matches(&state, &stats, &draw),
            "mesh/material/occluder source changed"
        );
        stats.surface_records_built = 0;
        stats.surface_model_updates = 1;
        assert!(!matches(&state, &stats, &draw), "matrix bits changed");
        stats.surface_model_updates = 0;
        draw.deformation = 1;
        assert!(
            !matches(&state, &stats, &draw),
            "posed bounds require full validation"
        );
        draw.deformation = 0;
        assert!(!state.retained_instance_inputs_match(
            &stats,
            camera,
            [129; 2],
            std::slice::from_ref(&draw),
            &[true]
        ));
        assert!(!state.retained_instance_inputs_match(
            &stats,
            camera * Mat4::from_rotation_y(0.01),
            size,
            std::slice::from_ref(&draw),
            &[true]
        ));
        assert!(!state.retained_instance_inputs_match(
            &stats,
            camera,
            size,
            std::slice::from_ref(&draw),
            &[false]
        ));
        assert!(!state.retained_instance_inputs_match(&stats, camera, size, &[], &[]));
        state.invalidate();
        assert!(
            !matches(&state, &stats, &draw),
            "asset bounds/coverage publication invalidated"
        );
    }
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
    fn adaptive_cooldown_advances_once_before_both_stages_and_keeps_instance_savings() {
        let mut state = Occlusion::default();
        let empty = |frame| OcclusionResult {
            frame_id: frame,
            tested_batches: 16,
            ..Default::default()
        };
        // Batch and individual queries from one frame are one observation.
        for frame in 1..=2 {
            state.observe_frame_results(Some(empty(frame)), Some(empty(frame)));
            assert_eq!(state.unproductive_results, frame as u32);
        }
        state.observe_frame_results(
            Some(empty(3)),
            Some(OcclusionResult {
                skipped_triangles: 12,
                ..empty(3)
            }),
        );
        assert_eq!(state.cooldown, 0);
        assert_eq!(state.unproductive_results, 0);
        // A delayed older readback cannot erase newer productive evidence.
        state.observe_frame_results(Some(empty(2)), None);
        assert_eq!(state.unproductive_results, 0);
        for frame in 4..=6 {
            state.observe_frame_results(Some(empty(frame)), Some(empty(frame)));
        }
        assert_eq!(state.cooldown, UNPRODUCTIVE_COOLDOWN);
        for remaining in (0..UNPRODUCTIVE_COOLDOWN).rev() {
            state.advance_cooldown(true);
            assert!(state.frame_bypassed);
            assert_eq!(state.cooldown, remaining);
            // The late preparation stage reads this flag without consuming a
            // second frame, including the final decrement from one to zero.
            state.observe_frame_results(Some(empty(6)), Some(empty(6)));
            assert_eq!(state.cooldown, remaining);
        }
        state.advance_cooldown(true);
        assert!(!state.frame_bypassed);
        state.observe_result(OcclusionResult {
            skipped_triangles: 24,
            ..empty(7)
        });
        assert_eq!(state.cooldown, 0);
        for frame in 8..=10 {
            state.observe_frame_results(Some(empty(frame)), None);
        }
        assert_eq!(state.cooldown, UNPRODUCTIVE_COOLDOWN);
        state.observe_frame_results(
            Some(empty(10)),
            Some(OcclusionResult {
                skipped_triangles: 48,
                ..empty(10)
            }),
        );
        assert_eq!(
            state.cooldown, 0,
            "late same-frame instance savings reactivate tests"
        );
        state.cooldown = 20;
        state.invalidate();
        state.advance_cooldown(true);
        assert!(
            !state.frame_bypassed,
            "asset publication restarts fresh queries"
        );
        assert_eq!(state.unproductive_results, 0);
    }
    #[test]
    fn adaptive_instance_cooldown_skips_preparation_and_resumes_with_exact_moving_pixels()
    -> Result<()> {
        let gpu = pollster::block_on(Gpu::request(
            &crate::instance(crate::Backend::native()),
            None,
            false,
        ))?;
        let mut optimized = SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm);
        let mut reference = SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm);
        reference.set_occlusion_enabled(false);
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
            view_projection: Mat4::IDENTITY,
            items: vec![DrawItem {
                motion_id: 1,
                // A large surface behind the queries makes the test eligible,
                // while every tested object remains visible.
                model: Mat4::from_translation(Vec3::new(0., 0., -8.))
                    * Mat4::from_scale(Vec3::new(2., 2., 1.)),
                mesh: MeshKind::Quad,
                material: material(TextureKind::White),
            }],
        };
        for group in 0..4 {
            for index in 0..24 {
                scene.items.push(DrawItem {
                    motion_id: 2 + group * 24 + index,
                    model: Mat4::from_translation(Vec3::new(
                        (index % 6) as f32 * 0.15 - 0.375,
                        (index / 6) as f32 * 0.15 - 0.225,
                        -4.,
                    )) * Mat4::from_scale(Vec3::splat(0.08)),
                    mesh: MeshKind::Cube,
                    material: material(match group {
                        0 => TextureKind::Checker,
                        1 => TextureKind::Normals,
                        2 => TextureKind::Toon,
                        _ => TextureKind::White,
                    }),
                });
            }
        }
        let camera = |frame: usize| {
            glam::camera::rh::proj::directx::orthographic(-2., 2., -2., 2., 0.1, 20.)
                * Mat4::from_translation(Vec3::new(frame as f32 * 0.001, 0., 0.))
        };
        let compare = |optimized: &mut SceneRenderer,
                       reference: &mut SceneRenderer,
                       scene: &RenderScene|
         -> Result<()> {
            let capture = |renderer: &mut SceneRenderer| {
                crate::capture_offscreen(&gpu, 128, 128, |target| {
                    renderer.draw_linear(&gpu, target, [128; 2], scene)
                })
            };
            assert_eq!(capture(optimized)?.rgba, capture(reference)?.rgba);
            gpu.wait()?;
            Ok(())
        };
        let mut frame = 0;
        while frame < 12 {
            scene.view_projection = camera(frame);
            compare(&mut optimized, &mut reference, &scene)?;
            frame += 1;
            if optimized.occlusion.frame_bypassed {
                break;
            }
        }
        assert!(optimized.occlusion.frame_bypassed);
        assert_eq!(optimized.occlusion.cooldown, UNPRODUCTIVE_COOLDOWN - 1);
        let prepared_camera = optimized.occlusion.camera;
        let projections = optimized.occlusion.projections.clone();
        let queries = optimized.occlusion.instance_candidates.clone();
        let generation = optimized.occlusion.instance_generation;
        let mut bypass_frames = 1;
        for _ in 0..UNPRODUCTIVE_COOLDOWN - 1 {
            scene.view_projection = camera(frame);
            frame += 1;
            compare(&mut optimized, &mut reference, &scene)?;
            assert!(optimized.occlusion.frame_bypassed);
            assert!(!optimized.occlusion.refreshed);
            assert!(!optimized.occlusion.instance_queries_ready);
            assert!(!optimized.occlusion.instance_applied);
            assert_eq!(optimized.occlusion.camera, prepared_camera);
            assert_eq!(optimized.occlusion.projections, projections);
            assert_eq!(optimized.occlusion.instance_candidates, queries);
            assert_eq!(optimized.occlusion.instance_generation, generation);
            assert_eq!(optimized.stats.occlusion_candidates, 0);
            assert_eq!(optimized.stats.occlusion_depth_draws, 0);
            assert_eq!(
                optimized.stats.color_triangles,
                reference.stats.color_triangles
            );
            bypass_frames += 1;
        }
        assert_eq!(optimized.occlusion.cooldown, 0);
        scene.view_projection = camera(frame);
        compare(&mut optimized, &mut reference, &scene)?;
        assert!(!optimized.occlusion.frame_bypassed);
        assert_eq!(
            optimized.occlusion.camera,
            Some((scene.view_projection, [128; 2]))
        );
        assert!(optimized.occlusion.instance_queries_ready);
        assert!(optimized.stats.occlusion_depth_draws > 0);
        assert!(optimized.stats.occlusion_prepare_ms > 0.);
        // An unchanged open view produces no additional fresh results; reuse
        // its exact CPU inputs instead of repacking zero-savings queries forever.
        let prepared = optimized.occlusion.projections.clone();
        let rows = optimized.occlusion.instance_candidates.clone();
        for _ in 0..3 {
            compare(&mut optimized, &mut reference, &scene)?;
            assert!(optimized.occlusion.instance_input_reused);
            assert_eq!(optimized.occlusion.projections, prepared);
            assert_eq!(optimized.occlusion.instance_candidates, rows);
        }
        // A changed asset restarts testing immediately, and subsequent fresh
        // productive samples must retain exact per-instance compaction.
        optimized.occlusion.invalidate();
        scene.items[0].model = Mat4::from_translation(Vec3::new(0., 0., -2.))
            * Mat4::from_scale(Vec3::new(2., 2., 1.));
        for _ in 0..5 {
            compare(&mut optimized, &mut reference, &scene)?;
        }
        assert_eq!(optimized.occlusion.cooldown, 0);
        assert!(optimized.occlusion.instance_applied);
        assert!(optimized.stats.color_triangles < reference.stats.color_triangles / 4);
        println!(
            "adaptive_instance_prepare_proof bypass_frames={bypass_frames} projection_refreshes=0 packed_query_rows=0 exact_moving_pixels=true bounded_resume=true productive_cached_compaction=true warm_zero_savings_input_reuse=true"
        );
        Ok(())
    }
    #[test]
    fn compact_native_batches_submit_instance_queries_and_retain_exact_visibility() -> Result<()> {
        let gpu = pollster::block_on(Gpu::request(
            &crate::instance(crate::Backend::native()),
            None,
            false,
        ))?;
        let material = Material {
            metallic: None,
            roughness: None,
            surface_overrides: Default::default(),
            tint: [0.3, 0.65, 0.8],
            uv_scale: [1.; 2],
            texture: TextureKind::White,
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
                    * Mat4::from_scale(Vec3::new(3.2, 3.2, 1.)),
                mesh: MeshKind::Quad,
                material: material.clone(),
            }],
        };
        for index in 0..128 {
            scene.items.push(DrawItem {
                motion_id: index + 2,
                model: Mat4::from_translation(Vec3::new(
                    (index % 16) as f32 * 0.12 - 0.9,
                    (index / 16) as f32 * 0.12 - 0.42,
                    -4.,
                )) * Mat4::from_scale(Vec3::splat(0.08)),
                mesh: MeshKind::Cube,
                material: material.clone(),
            });
        }
        let capture = |renderer: &mut SceneRenderer| {
            crate::capture_offscreen(&gpu, 128, 128, |target| {
                renderer.draw_linear(&gpu, target, [128; 2], &scene)
            })
        };
        // The native arena makes two batches; the portable 64-record path
        // makes three. Both must admit the 129 individual visibility queries.
        for native in [true, false] {
            let mut reference = SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm);
            reference.set_native_instance_arena_enabled(native);
            reference.set_occlusion_enabled(false);
            let expected = capture(&mut reference)?;
            let expected_draws = if reference.stats.native_instance_arena {
                2
            } else {
                3
            };
            assert_eq!(reference.stats.color_draws, expected_draws);
            let mut optimized = SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm);
            optimized.set_native_instance_arena_enabled(native);
            assert_eq!(capture(&mut optimized)?.rgba, expected.rgba);
            assert_eq!(optimized.instancing.frame_batches.len(), expected_draws);
            assert_eq!(optimized.stats.occlusion_depth_draws, 1);
            assert!(optimized.occlusion.instance_resources.is_some());
            for _ in 0..4 {
                gpu.wait()?;
                assert_eq!(capture(&mut optimized)?.rgba, expected.rgba);
            }
            let result = optimized
                .occlusion
                .instance_resources
                .as_ref()
                .unwrap()
                .result
                .unwrap();
            assert_eq!(result.culled_surfaces, 128);
            assert_eq!(
                result.skipped_triangles,
                reference.stats.color_triangles - 2
            );
            assert!(optimized.stats.occlusion_cache_hit);
            assert!(optimized.occlusion.instance_input_reused);
            assert!(optimized.occlusion.instance_applied);
            assert_eq!(optimized.stats.color_draws, 1);
            assert_eq!(optimized.stats.color_triangles, 2);
            assert_eq!(optimized.stats.occlusion_depth_draws, 0);
            // The reference switch still skips depth/query work for fewer
            // than four whole batches; it restores every color command.
            optimized.set_instance_occlusion_enabled(false);
            for _ in 0..2 {
                assert_eq!(capture(&mut optimized)?.rgba, expected.rgba);
                assert!(!optimized.stats.occlusion_cache_hit);
                assert!(!optimized.occlusion.instance_queries_ready);
                assert_eq!(optimized.stats.occlusion_depth_draws, 0);
                assert_eq!(optimized.stats.color_draws, expected_draws);
                assert_eq!(
                    optimized.stats.color_triangles,
                    reference.stats.color_triangles
                );
            }
        }
        Ok(())
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
        let generation = optimized.occlusion.instance_generation;
        let projections = optimized.occlusion.projections.clone();
        let rows = optimized.occlusion.instance_candidates.clone();
        for _ in 0..4 {
            assert_eq!(capture(&mut optimized, &scene)?.rgba, expected.rgba);
            assert!(optimized.occlusion.instance_input_reused);
            assert!(optimized.occlusion.instance_applied);
            assert_eq!(optimized.occlusion.instance_generation, generation);
            assert_eq!(optimized.occlusion.projections, projections);
            assert_eq!(optimized.occlusion.instance_candidates, rows);
        }
        // The early stage packs changed inputs, then singular normal-matrix
        // validation fails before GPU submission. Reverting must not mistake
        // the failed CPU snapshot for the old submitted visibility result.
        let original_model = scene.items[9].model;
        scene.items[9].model =
            Mat4::from_translation(Vec3::new(1.5, 1., -4.)) * Mat4::from_scale(Vec3::ZERO);
        assert!(capture(&mut optimized, &scene).is_err());
        assert!(optimized.occlusion.instance_generation > generation);
        assert!(!optimized.occlusion.instance_input_reused);
        scene.items[9].model = original_model;
        assert_eq!(capture(&mut optimized, &scene)?.rgba, expected.rgba);
        assert!(!optimized.occlusion.instance_input_reused);
        assert!(!optimized.occlusion.instance_applied);
        for _ in 0..3 {
            assert_eq!(capture(&mut optimized, &scene)?.rgba, expected.rgba);
        }
        assert!(optimized.occlusion.instance_input_reused);
        // Changed camera and bounds reject old masks before packing; strict
        // current-frame GPU queries still produce exact reference pixels.
        // These bounds also put neighboring queries on different mip levels;
        // identical right-hand bounds must keep the same background coverage.
        scene.items[9].model =
            Mat4::from_translation(Vec3::new(-1.4, 0.8, -4.)) * Mat4::from_scale(Vec3::splat(0.2));
        scene.view_projection *= Mat4::from_translation(Vec3::new(0.02, 0., 0.));
        let expected = capture(&mut reference, &scene)?;
        let actual = capture(&mut optimized, &scene)?;
        if actual.rgba != expected.rgba {
            // Full byte arrays can truncate the CI log before any useful
            // evidence. Keep this an exact oracle and report bounded details.
            let changed_pixels = actual
                .rgba
                .chunks_exact(4)
                .zip(expected.rgba.chunks_exact(4))
                .filter(|(a, b)| a != b)
                .count();
            let differences: Vec<_> = actual
                .rgba
                .iter()
                .zip(&expected.rgba)
                .enumerate()
                .filter(|(_, (a, b))| a != b)
                .collect();
            let max_delta = differences
                .iter()
                .map(|(_, (a, b))| a.abs_diff(**b))
                .max()
                .unwrap();
            let first: Vec<_> = differences
                .iter()
                .take(16)
                .map(|(index, (a, b))| {
                    (
                        index / 4 % actual.width as usize,
                        index / 4 / actual.width as usize,
                        index % 4,
                        **a,
                        **b,
                    )
                })
                .collect();
            let batches = |renderer: &SceneRenderer| {
                renderer
                    .instancing
                    .frame_batches
                    .iter()
                    .map(|batch| (batch.indices.clone(), batch.slot, batch.first_instance))
                    .collect::<Vec<_>>()
            };
            panic!(
                "changed-camera exact RGBA mismatch: {changed_pixels} pixels, {} channels, \
                 max delta {max_delta}; first (x,y,channel,actual,expected)={first:?}; \
                 optimized stats={}; reference stats={}; optimized batches={:?}; reference batches={:?}",
                differences.len(),
                serde_json::to_string(&optimized.stats).unwrap(),
                serde_json::to_string(&reference.stats).unwrap(),
                batches(&optimized),
                batches(&reference),
            );
        }
        assert!(!optimized.occlusion.instance_applied);
        assert!(!optimized.occlusion.instance_input_reused);
        for _ in 0..3 {
            assert_eq!(capture(&mut optimized, &scene)?.rgba, expected.rgba);
            gpu.wait()?;
        }
        // A large surface becoming a graph changes its eligibility as a depth
        // occluder even though its geometry and model stay identical.
        scene.items[0].material.shader = Some(graph.clone());
        assert_eq!(
            capture(&mut optimized, &scene)?.rgba,
            capture(&mut reference, &scene)?.rgba
        );
        assert!(!optimized.occlusion.instance_input_reused);
        assert!(!optimized.occlusion.instance_applied);
        assert!(!optimized.occlusion.instance_inputs_valid);
        scene.items[0].material.shader = None;
        for _ in 0..3 {
            assert_eq!(
                capture(&mut optimized, &scene)?.rgba,
                capture(&mut reference, &scene)?.rgba
            );
        }
        assert!(optimized.occlusion.instance_input_reused);
        let quad = |half: f32| {
            [
                [-half, -half, 0., 0., 0., 1., 0., 0.],
                [half, -half, 0., 0., 0., 1., 1., 0.],
                [half, half, 0., 0., 0., 1., 1., 1.],
                [-half, half, 0., 0., 0., 1., 0., 1.],
            ]
        };
        for renderer in [&mut optimized, &mut reference] {
            renderer.upload_mesh(
                &gpu,
                "retained-query-occluder",
                &quad(0.5),
                &[0, 1, 2, 0, 2, 3],
            )?;
        }
        scene.items[0].mesh = MeshKind::Imported("retained-query-occluder".into());
        for _ in 0..3 {
            assert_eq!(
                capture(&mut optimized, &scene)?.rgba,
                capture(&mut reference, &scene)?.rgba
            );
        }
        assert!(optimized.occlusion.instance_input_reused);
        // The source ID remains identical, but publication changes its actual
        // bounds/depth coverage and invalidates the retained certificate.
        for renderer in [&mut optimized, &mut reference] {
            renderer.upload_mesh(
                &gpu,
                "retained-query-occluder",
                &quad(0.3),
                &[0, 1, 2, 0, 2, 3],
            )?;
        }
        assert_eq!(
            capture(&mut optimized, &scene)?.rgba,
            capture(&mut reference, &scene)?.rgba
        );
        assert!(!optimized.occlusion.instance_input_reused);
        assert!(!optimized.occlusion.instance_applied);
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
            "instance_occlusion_proof graph_candidates mixed_union_batch {}->{}triangles exact_camera_bounds_occluder_invalidation warm_input_reuse=true failed_reverted_retry=true asset_bounds_invalidation=true",
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
