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
fn opaque(renderer: &SceneRenderer, draw: &PreparedDraw) -> bool {
    if draw.transparent
        || draw.shader.is_some()
        || draw.opacity < 1.
        || draw.cutoff > 0.
        || draw.deformation != 0
    {
        return false;
    }
    if matches!(draw.object.mesh, MeshKind::Text(_) | MeshKind::Sprite(_)) {
        return false;
    }
    // Alpha-masked materials can be classified opaque by the main pass. Test
    // their actual texture coverage before allowing a texture-free depth pass.
    match &draw.object.material.texture {
        TextureKind::Generated(_) | TextureKind::Text => false,
        TextureKind::Imported(id) => !renderer.transparent_textures.contains(id),
        TextureKind::ModelPart(id, index) => !renderer.models[id][*index].translucent,
        _ => true,
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
        }
        if !self.enabled
            || !renderer.culling
            || visible.iter().filter(|v| **v).count() < MIN_SURFACES
            || batches.len() > MAX_BATCHES
            || batches.len() < 4
        {
            return Mode::Disabled;
        }
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
        if self.occluders.is_empty() {
            return Mode::Disabled;
        }
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
            for index in batch.range.clone() {
                let Some(p) = self.projections[index].projection.filter(|_| {
                    !draws[index].transparent
                        && draws[index].deformation == 0
                        && draws[index].shader.is_none()
                }) else {
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
                    .mesh_for(&draws[batch.range.start].object)
                    .count
                    .to_le_bytes(),
            );
            self.candidates
                .extend_from_slice(&(batch.range.len() as u32).to_le_bytes());
            self.candidates.extend_from_slice(&0u32.to_le_bytes());
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
        );
        renderer.stats.occlusion_depth_draws = self.occluders.len();
        renderer.stats.occlusion_depth_triangles = triangles;
        renderer.stats.occlusion_bytes = resources.bytes();
        Mode::Indirect
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
