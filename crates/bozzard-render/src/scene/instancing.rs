use super::*;
use std::collections::HashMap;
mod arena;
mod depth;
mod diagnostics;
mod graphs;
mod reuse;
pub use diagnostics::BatchingStats;

// Fits the downlevel 16 KiB uniform-binding limit without storage-buffer features.
pub(super) const MAX_INSTANCES: usize = 64;
pub(super) const BUFFER_BYTES: usize = OBJECT_UNIFORM_BYTES * MAX_INSTANCES;
pub(super) const SHADOW_UNIFORM_BYTES: usize = 96;
pub(super) const MAX_SHADOW_INSTANCES: usize = 170;
pub(super) const SHADOW_BUFFER_BYTES: usize = SHADOW_UNIFORM_BYTES * MAX_SHADOW_INSTANCES;
/// Exact physical streams consumed by an animated instanced draw. Independent
/// motion histories remain separate when their previous streams differ.
#[derive(Clone, PartialEq, Eq, Hash)]
pub(super) struct GeometryKey {
    pub vertices: wgpu::Buffer,
    pub previous: Option<wgpu::Buffer>,
    pub tangents: Option<wgpu::Buffer>,
}
// Construction must remain bounded even for coincident or unbounded surfaces.
const MAX_PLAN_EDGES: usize = 65_536;
const MAX_PLAN_CANDIDATES: usize = 524_288;

pub(super) struct Pipelines {
    pub pipelines: [[Option<wgpu::RenderPipeline>; 2]; 2],
}
pub(super) struct InstanceBinding {
    buffer: wgpu::Buffer,
    texture: TextureKind,
    pub binding: wgpu::BindGroup,
    bytes: Vec<u8>,
    revisions: Vec<u64>,
    parameter_buffer: Option<wgpu::Buffer>,
    parameter_bytes: Vec<u8>,
    parameter_revisions: Vec<u64>,
    first_instance: u32,
}
pub(super) struct Instancing {
    enabled: bool,
    global: bool,
    incremental: bool,
    graph_enabled: bool,
    pub shadow_batches_enabled: bool,
    plan: Option<Plan>,
    pub frame_batches: Vec<Batch>,
    pub(super) layout: wgpu::BindGroupLayout,
    pub pipelines: Option<Pipelines>,
    pub shadow_pipelines: Option<[wgpu::RenderPipeline; 2]>,
    shadow_pipeline_compact: bool,
    pub bindings: Vec<InstanceBinding>,
    pub shadow_bindings: Vec<InstanceBinding>,
    pub shadow_layout: Option<wgpu::BindGroupLayout>,
    pub shadow_frame_batches: Vec<Batch>,
    shadow_plan: Option<depth::Plan>,
    diagnostics_visible: Vec<bool>,
    diagnostics: Option<BatchingStats>,
    binding_reserved: Vec<bool>,
    transparent_runs: bool,
    baseline_layout: wgpu::BindGroupLayout,
    native_layout: Option<wgpu::BindGroupLayout>,
    native_requested: bool,
    native_mode: bool,
    arena: arena::Arena,
    text_bytes_limit: usize,
    superset_policy: SupersetPolicy,
}
#[derive(Clone)]
pub(super) struct Batch {
    pub indices: Vec<usize>,
    pub slot: Option<usize>,
    pub first_instance: u32,
}
impl Instancing {
    pub(super) fn set_transparent_runs_allowed(&mut self, allowed: bool) {
        if self.transparent_runs != allowed {
            self.transparent_runs = allowed;
            self.plan = None;
        }
    }
    pub(super) fn clear_depth_plan(&mut self) {
        self.shadow_plan = None;
        self.shadow_frame_batches.clear();
    }
    pub(super) fn shadow_batching(&self) -> bool {
        self.enabled && self.shadow_batches_enabled
    }
    pub(super) fn arena_enabled(&self) -> bool {
        self.native_mode
    }
    pub(super) fn object_slot(&self, draw_index: usize) -> u32 {
        self.arena.object_slot(draw_index)
    }
    pub(super) fn arena_buffers(&self) -> (&wgpu::Buffer, &wgpu::Buffer) {
        self.arena.buffers()
    }
    fn capacity(&self) -> usize {
        if self.native_mode {
            arena::MAX_NATIVE_INSTANCES
        } else {
            MAX_INSTANCES
        }
    }
    pub fn new(
        baseline_layout: wgpu::BindGroupLayout,
        native_layout: Option<wgpu::BindGroupLayout>,
    ) -> Self {
        let native_mode = native_layout.is_some();
        let layout = native_layout.as_ref().unwrap_or(&baseline_layout).clone();
        Self {
            enabled: true,
            global: true,
            incremental: true,
            graph_enabled: true,
            shadow_batches_enabled: true,
            plan: None,
            frame_batches: Vec::new(),
            layout,
            pipelines: None,
            shadow_pipelines: None,
            shadow_pipeline_compact: false,
            bindings: Vec::new(),
            shadow_bindings: Vec::new(),
            shadow_layout: None,
            shadow_frame_batches: Vec::new(),
            shadow_plan: None,
            diagnostics_visible: Vec::new(),
            diagnostics: None,
            binding_reserved: Vec::new(),
            transparent_runs: false,
            baseline_layout,
            native_layout,
            native_requested: true,
            native_mode,
            arena: Default::default(),
            text_bytes_limit: 16 * 1024,
            superset_policy: Default::default(),
        }
    }
}

/// Use the stock shader unchanged apart from selecting its per-invocation object.
/// Private variables are invocation-local in both vertex and fragment stages.
#[cfg(test)]
pub(super) fn module_text(pbr: bool) -> String {
    instance_module_text(host_text(pbr))
}

#[cfg(test)]
pub(super) fn instance_module_text(source: String) -> String {
    instance_module_text_for(source, false)
}

pub(super) fn instance_module_text_for(source: String, native: bool) -> String {
    let declaration = if native {
        "@group(0) @binding(0) var<storage, read> objects: array<ObjectUniform>;\n@group(0) @binding(13) var<storage, read> instance_ids: array<u32>;\nvar<private> object: ObjectUniform;".to_owned()
    } else {
        format!(
            "@group(0) @binding(0) var<uniform> objects: array<ObjectUniform, {MAX_INSTANCES}>;\nvar<private> object: ObjectUniform;"
        )
    };
    let source = if native {
        source.replace(
            "@group(0) @binding(12) var<uniform> graph_parameters: array<vec4<f32>, 1024>;",
            "@group(0) @binding(12) var<storage, read> graph_parameters: array<vec4<f32>>;",
        )
    } else {
        source
    };
    let vertex_selection = if native {
        "    let object_slot = instance_ids[instance];\n    object = objects[object_slot];\n    graph_instance = object_slot;\n    var out: VertexOutput;\n    out.instance = object_slot;"
    } else {
        "    object = objects[instance];\n    graph_instance = instance;\n    var out: VertexOutput;\n    out.instance = instance;"
    };
    source
        .replacen(
            "@group(0) @binding(0) var<uniform> object: ObjectUniform;",
            &declaration,
            1,
        )
        .replacen("struct VertexOutput {", "struct VertexOutput {\n    @location(9) @interpolate(flat) instance: u32,", 1)
        .replacen("fn vs_main(", "fn vs_main(@builtin(instance_index) instance: u32, ", 1)
        .replacen("    var out: VertexOutput;", vertex_selection, 1)
        .replacen("-> SurfaceOutput {", "-> SurfaceOutput {\n    object = objects[in.instance];\n    graph_instance = in.instance;", 1)
}

pub(super) fn world_units(draw: &PreparedDraw) -> Option<usize> {
    if draw.shader.is_some() || draw.pbr || draw.deformation != 0 {
        return None;
    }
    match &draw.object.mesh {
        MeshKind::Text(text) if text.screen.is_none() => draw.world_geometry_units,
        MeshKind::SharedText(text) if text.screen.is_none() => draw.world_geometry_units,
        MeshKind::Sprite(sprite) if sprite.screen.is_none() => draw.world_geometry_units,
        _ => None,
    }
}
fn world_run_fits(
    first: &PreparedDraw,
    draw: &PreparedDraw,
    units: usize,
    heterogeneous: bool,
    limit: usize,
) -> bool {
    let Some(next) = world_units(draw) else {
        return true;
    };
    // Large homogeneous runs can still use ordinary instancing without copying
    // glyph/sprite geometry. Mixed runs must fit the per-vertex ID/copy budget.
    units.saturating_add(next) <= limit || !heterogeneous && first.object.mesh == draw.object.mesh
}
fn compatible(a: &PreparedDraw, b: &PreparedDraw, graphs: bool, heterogeneous_text: bool) -> bool {
    a.transparent == b.transparent
        && a.shader == b.shader
        && (graphs || a.shader.is_none())
        && (a.deformation == 0 || a.shared_geometry.is_some())
        && (b.deformation == 0 || b.shared_geometry.is_some())
        && a.shared_geometry == b.shared_geometry
        && (a.object.mesh == b.object.mesh
            || heterogeneous_text
                && a.transparent
                && world_units(a).is_some()
                && world_units(b).is_some())
        && a.object.material.texture == b.object.material.texture
        && a.object.material.lit == b.object.material.lit
        && a.pbr == b.pbr
        && a.raster == b.raster
}

#[cfg(test)]
fn batches(
    draws: &[PreparedDraw],
    visible: &[bool],
    enabled: bool,
    graphs: bool,
    transparent_runs: bool,
) -> Vec<Batch> {
    batches_with_capacity(
        draws,
        visible,
        enabled,
        graphs,
        transparent_runs,
        MAX_INSTANCES,
        0,
    )
}

fn batches_with_capacity(
    draws: &[PreparedDraw],
    visible: &[bool],
    enabled: bool,
    graphs: bool,
    transparent_runs: bool,
    capacity: usize,
    text_limit: usize,
) -> Vec<Batch> {
    let mut result: Vec<Batch> = Vec::new();
    let mut text_units = 0usize;
    let mut heterogeneous = false;
    for (index, draw) in draws.iter().enumerate().filter(|(i, _)| visible[*i]) {
        if enabled
            && (!draw.transparent || transparent_runs)
            && let Some(last) = result.last_mut()
            && last.indices.last() == index.checked_sub(1).as_ref()
            && last.indices.len() < capacity
            && compatible(
                &draws[last.indices[0]],
                draw,
                graphs,
                capacity > MAX_INSTANCES,
            )
            && (capacity <= MAX_INSTANCES
                || world_run_fits(
                    &draws[last.indices[0]],
                    draw,
                    text_units,
                    heterogeneous,
                    text_limit,
                ))
        {
            heterogeneous |= draws[last.indices[0]].object.mesh != draw.object.mesh;
            text_units = text_units.saturating_add(world_units(draw).unwrap_or(0));
            last.indices.push(index);
        } else {
            text_units = world_units(draw).unwrap_or(0);
            heterogeneous = false;
            result.push(Batch {
                indices: vec![index],
                slot: None,
                first_instance: 0,
            });
        }
    }
    result
}

#[derive(Clone, PartialEq, Eq, Hash)]
enum MeshKey<'a> {
    Quad,
    Cube,
    Sphere,
    Imported(&'a str),
    Part(&'a str, usize),
}
#[derive(PartialEq, Eq, Hash)]
// Uniform shadow eligibility prevents one unlit member from forcing a large
// otherwise reusable color batch back to individual shadow draws.
struct Key<'a>(
    MeshKey<'a>,
    &'a TextureKind,
    bool,
    bool,
    Option<u64>,
    u8,
    Option<&'a GeometryKey>,
);
fn key(draw: &PreparedDraw, graphs: bool) -> Option<Key<'_>> {
    if draw.transparent
        || (!graphs && draw.shader.is_some())
        || (draw.deformation != 0 && draw.shared_geometry.is_none())
    {
        return None;
    }
    let mesh = match &draw.object.mesh {
        MeshKind::Quad => MeshKey::Quad,
        MeshKind::Cube => MeshKey::Cube,
        MeshKind::Sphere => MeshKey::Sphere,
        MeshKind::Imported(id) => MeshKey::Imported(id),
        MeshKind::ModelPart(id, part) => MeshKey::Part(id, *part),
        _ => return None,
    };
    Some(Key(
        mesh,
        &draw.object.material.texture,
        draw.pbr,
        draw.object.material.lit,
        draw.shader,
        draw.raster,
        draw.shared_geometry.as_ref(),
    ))
}

struct Input {
    mesh: MeshKind,
    texture: TextureKind,
    model: Mat4,
    bounds: [Vec3; 2],
    shader: Option<u64>,
    deformation: u64,
    shared_geometry: Option<GeometryKey>,
    world_geometry_units: Option<usize>,
    pbr: bool,
    lit: bool,
    transparent: bool,
    visible: bool,
    raster: u8,
}
impl Input {
    fn matches_metadata(&self, draw: &PreparedDraw, bounds: [Vec3; 2], visible: bool) -> bool {
        self.visible == visible
            && self.bounds == bounds
            && self.mesh == draw.object.mesh
            && self.texture == draw.object.material.texture
            && self.shader == draw.shader
            && self.shared_geometry == draw.shared_geometry
            && self.world_geometry_units == draw.world_geometry_units
            && (self.shared_geometry.is_some() || self.deformation == draw.deformation)
            && self.pbr == draw.pbr
            && self.lit == draw.object.material.lit
            && self.transparent == draw.transparent
            && self.raster == draw.raster
    }
}
struct Plan {
    camera: Mat4,
    // Orthographic plans include hidden surfaces so camera-frustum churn only
    // filters the output. Their ordering certificate covers that full superset.
    all_surfaces: bool,
    inputs: Vec<Input>,
    batches: Vec<Batch>,
    ordering: std::result::Result<reuse::Ordering, BatchPlanRebuildReason>,
    changed: Vec<usize>,
    batch_of: Vec<usize>,
}

#[derive(Default)]
struct SupersetPolicy {
    rejected: bool,
}
impl SupersetPolicy {
    fn request(&mut self, previous: Option<&Plan>, reason: BatchPlanRebuildReason) -> bool {
        // Reconsider admission only when population/compatibility/geometry changes.
        // Camera/frustum churn must not retry a known unproductive hidden graph.
        if matches!(
            reason,
            BatchPlanRebuildReason::Cold
                | BatchPlanRebuildReason::Membership
                | BatchPlanRebuildReason::Metadata
                | BatchPlanRebuildReason::Bounds
                | BatchPlanRebuildReason::UnsupportedProjection
        ) {
            self.rejected = false;
        }
        previous.is_some_and(|plan| plan.all_surfaces)
            || !self.rejected && reason == BatchPlanRebuildReason::Visibility
    }
}

#[derive(Clone, Copy)]
struct PlanOptions {
    graphs: bool,
    transparent_runs: bool,
    capacity: usize,
    text_limit: usize,
    incremental: bool,
    all_surfaces: bool,
}
struct PlanBuild {
    plan: Plan,
    construction_limited: bool,
    rejected_superset: bool,
}

// An opaque reorder can change an equal-depth winner. Retain original order for
// intersecting projected bounds (including their depth intervals). Disjoint
// bounds cannot cover the same sample at equal depth. Near-plane crossings use
// unbounded boxes, so uncertain projections always retain their dependencies.
fn projected_bounds(bounds: [Vec3; 2], matrix: Mat4) -> [Vec3; 2] {
    let mut result = [Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)];
    for corner in 0..8 {
        let point = matrix
            * Vec3::new(
                bounds[(corner & 1) as usize].x,
                bounds[((corner >> 1) & 1) as usize].y,
                bounds[((corner >> 2) & 1) as usize].z,
            )
            .extend(1.);
        if !point.is_finite() || point.w <= 1e-6 {
            return [Vec3::splat(f32::NEG_INFINITY), Vec3::splat(f32::INFINITY)];
        }
        let point = point.truncate() / point.w;
        result[0] = result[0].min(point);
        result[1] = result[1].max(point);
    }
    // Cover projection roundoff and Depth32Float quantization conservatively.
    let padding = Vec3::splat(2e-5);
    [result[0] - padding, result[1] + padding]
}

// Choose the axis with the most separation relative to total projected width.
// Long thin bounds sharing X can still be separated cheaply along Y or depth.
fn sweep_axis(indices: &[usize], boxes: &[[Vec3; 2]]) -> usize {
    let mut low = [f64::INFINITY; 3];
    let mut high = [f64::NEG_INFINITY; 3];
    let mut widths = [0.; 3];
    for &index in indices {
        for axis in 0..3 {
            low[axis] = low[axis].min(f64::from(boxes[index][0][axis]));
            high[axis] = high[axis].max(f64::from(boxes[index][1][axis]));
            widths[axis] += f64::from(boxes[index][1][axis] - boxes[index][0][axis]);
        }
    }
    let score = |axis: usize| {
        let score = (high[axis] - low[axis]) / widths[axis].max(1e-20);
        if score.is_finite() { score } else { 0. }
    };
    (0..3)
        .max_by(|&a, &b| score(a).total_cmp(&score(b)).then(b.cmp(&a)))
        .unwrap_or(0)
}

fn enqueue(
    index: usize,
    group: usize,
    queues: &mut [BTreeSet<usize>],
    ready: &mut BTreeSet<(usize, usize)>,
) {
    if let Some(&first) = queues[group].first() {
        ready.remove(&(first, group));
    }
    queues[group].insert(index);
    ready.insert((*queues[group].first().unwrap(), group));
}

fn next_ready_group(
    ready: &BTreeSet<(usize, usize)>,
    queues: &[BTreeSet<usize>],
    followers: &[Vec<usize>],
    group_has_followers: &[bool],
    pending: &[usize],
    group_of: &[usize],
    capacity: usize,
) -> Option<usize> {
    let mut best = None;
    // Keep scheduling work bounded when a scene has many unique keys. A ready
    // group has one entry, and all examined alternatives preserve DAG edges.
    for &(first, group) in ready.iter().take(64) {
        let mut score = queues[group].len().min(capacity);
        let mut examined = 0;
        if group_has_followers[group] {
            'members: for &index in queues[group].iter().take(capacity) {
                for &next in &followers[index] {
                    if examined == 256 {
                        break 'members;
                    }
                    examined += 1;
                    if pending[next] == 1 && !queues[group_of[next]].is_empty() {
                        // Unlocking a peer of an already ready surface can eliminate
                        // a tail, e.g. A0,B1,A2 with only B1 -> A2.
                        score += 1;
                    }
                }
            }
        }
        let candidate = (score, std::cmp::Reverse(first), group);
        if best.is_none_or(|old| candidate > old) {
            best = Some(candidate);
        }
    }
    best.map(|(_, _, group)| group)
}

#[cfg(test)]
fn global_batches(
    draws: &[PreparedDraw],
    inputs: &[Input],
    camera: Mat4,
    graphs: bool,
    transparent_runs: bool,
) -> (Vec<Batch>, Vec<[Vec3; 2]>, bool) {
    global_batches_with_capacity(
        draws,
        inputs,
        camera,
        graphs,
        transparent_runs,
        MAX_INSTANCES,
        0,
    )
}

fn global_batches_with_capacity(
    draws: &[PreparedDraw],
    inputs: &[Input],
    camera: Mat4,
    graphs: bool,
    transparent_runs: bool,
    capacity: usize,
    text_limit: usize,
) -> (Vec<Batch>, Vec<[Vec3; 2]>, bool) {
    // wgpu Buffer Eq/Hash use immutable handle identity, never storage contents.
    #[allow(clippy::mutable_key_type)]
    let mut groups = HashMap::new();
    let mut group_of = vec![0; draws.len()];
    let mut queues: Vec<BTreeSet<usize>> = Vec::new();
    let mut sweep = Vec::new();
    let mut group_counts = Vec::new();
    for (index, (draw, input)) in draws.iter().zip(inputs).enumerate() {
        if !input.visible || draw.transparent {
            continue;
        }
        let group = key(draw, graphs).and_then(|key| groups.get(&key).copied());
        group_of[index] = group.unwrap_or_else(|| {
            let group = queues.len();
            queues.push(BTreeSet::new());
            group_counts.push(0usize);
            if let Some(key) = key(draw, graphs) {
                groups.insert(key, group);
            }
            group
        });
        group_counts[group_of[index]] += 1;
        sweep.push(index);
    }
    let original = original_batches(
        draws,
        inputs,
        &group_of,
        graphs,
        transparent_runs,
        capacity,
        text_limit,
    );
    let lower_bound = group_counts
        .iter()
        .map(|count| count.div_ceil(capacity))
        .sum::<usize>()
        + original
            .iter()
            .filter(|batch| draws[batch.indices[0]].transparent)
            .count();
    // No ordering can improve a plan already at the compatibility/capacity lower
    // bound. In particular, coincident homogeneous copies need no quadratic DAG.
    if original.len() == lower_bound {
        return (original, Vec::new(), false);
    }
    let mut boxes = vec![[Vec3::ZERO; 2]; draws.len()];
    let mut world_boxes = vec![[Vec3::ZERO; 2]; draws.len()];
    // In an orthographic view, coincident screen/depth samples map to the same
    // world point. Disjoint world bounds can therefore remove false projected
    // overlaps. Expand by the inverse camera's NDC-roundoff footprint; retain
    // projected-only ordering for perspective cameras and uncertain inverses.
    let inverse = camera.inverse();
    let world_padding = (camera.x_axis.w == 0.
        && camera.y_axis.w == 0.
        && camera.z_axis.w == 0.
        && inverse.is_finite()
        && inverse.w_axis.w.abs() > 1e-6)
        .then(|| {
            (inverse.x_axis.truncate().abs()
                + inverse.y_axis.truncate().abs()
                + inverse.z_axis.truncate().abs())
                * (2e-5 / inverse.w_axis.w.abs())
        });
    for (index, (draw, input)) in draws.iter().zip(inputs).enumerate() {
        if !input.visible || draw.transparent {
            continue;
        }
        boxes[index] = projected_bounds(input.bounds, camera * input.model);
        if let Some(padding) = world_padding {
            let bounds = projected_bounds(input.bounds, input.model);
            world_boxes[index] = [bounds[0] - padding, bounds[1] + padding];
        }
    }
    let axis = sweep_axis(&sweep, &boxes);
    sweep.sort_by(|&a, &b| {
        boxes[a][0][axis]
            .total_cmp(&boxes[b][0][axis])
            .then(a.cmp(&b))
    });
    let mut followers = vec![Vec::new(); draws.len()];
    let mut group_has_followers = vec![false; queues.len()];
    let mut pending = vec![0usize; draws.len()];
    let mut active: Vec<usize> = Vec::new();
    let mut edge_count = 0;
    let mut candidates = 0;
    let edge_budget = draws.len().saturating_mul(16).min(MAX_PLAN_EDGES);
    let candidate_budget = draws.len().saturating_mul(64).min(MAX_PLAN_CANDIDATES);
    for &index in &sweep {
        let bounds = boxes[index];
        active.retain(|&other| boxes[other][1][axis] >= bounds[0][axis]);
        for &other in &active {
            candidates += 1;
            if candidates > candidate_budget {
                return (original, Vec::new(), true);
            }
            let previous = boxes[other];
            if previous[1].cmplt(bounds[0]).any() || bounds[1].cmplt(previous[0]).any() {
                continue;
            }
            if world_padding.is_some() {
                let a = world_boxes[index];
                let b = world_boxes[other];
                if a[1].cmplt(b[0]).any() || b[1].cmplt(a[0]).any() {
                    continue;
                }
            }
            let (before, after) = (index.min(other), index.max(other));
            edge_count += 1;
            if edge_count > edge_budget {
                return (original, Vec::new(), true);
            }
            followers[before].push(after);
            group_has_followers[group_of[before]] = true;
            pending[after] += 1;
        }
        active.push(index);
    }
    let mut ready = BTreeSet::new();
    for &index in &sweep {
        if pending[index] == 0 {
            enqueue(index, group_of[index], &mut queues, &mut ready);
        }
    }
    let mut result = Vec::new();
    while let Some(group) = next_ready_group(
        &ready,
        &queues,
        &followers,
        &group_has_followers,
        &pending,
        &group_of,
        capacity,
    ) {
        let mut indices = Vec::new();
        while indices.len() < capacity {
            let Some(index) = queues[group].pop_first() else {
                break;
            };
            ready.remove(&(index, group));
            if let Some(&next) = queues[group].first() {
                ready.insert((next, group));
            }
            indices.push(index);
            for &next in &followers[index] {
                pending[next] -= 1;
                if pending[next] == 0 {
                    enqueue(next, group_of[next], &mut queues, &mut ready);
                }
            }
        }
        result.push(Batch {
            indices,
            slot: None,
            first_instance: 0,
        });
    }
    append_transparent(
        draws,
        inputs,
        &mut result,
        graphs,
        transparent_runs,
        capacity,
        text_limit,
    );
    (result, boxes, false)
}

fn original_batches(
    draws: &[PreparedDraw],
    inputs: &[Input],
    group_of: &[usize],
    graphs: bool,
    transparent_runs: bool,
    capacity: usize,
    text_limit: usize,
) -> Vec<Batch> {
    let mut result: Vec<Batch> = Vec::new();
    for (index, draw) in draws.iter().enumerate() {
        if !inputs[index].visible || draw.transparent {
            continue;
        }
        if let Some(last) = result.last_mut()
            && last.indices.len() < capacity
            && group_of[last.indices[0]] == group_of[index]
        {
            last.indices.push(index);
        } else {
            result.push(Batch {
                indices: vec![index],
                slot: None,
                first_instance: 0,
            });
        }
    }
    append_transparent(
        draws,
        inputs,
        &mut result,
        graphs,
        transparent_runs,
        capacity,
        text_limit,
    );
    result
}
fn append_transparent(
    draws: &[PreparedDraw],
    inputs: &[Input],
    output: &mut Vec<Batch>,
    graphs: bool,
    enabled: bool,
    capacity: usize,
    text_limit: usize,
) {
    let mut text_units = 0usize;
    let mut heterogeneous = false;
    for (index, draw) in draws.iter().enumerate() {
        if !inputs[index].visible || !draw.transparent {
            continue;
        }
        if enabled
            && let Some(last) = output.last_mut()
            && last.indices.len() < capacity
            && compatible(
                &draws[last.indices[0]],
                draw,
                graphs,
                capacity > MAX_INSTANCES,
            )
            && (capacity <= MAX_INSTANCES
                || world_run_fits(
                    &draws[last.indices[0]],
                    draw,
                    text_units,
                    heterogeneous,
                    text_limit,
                ))
        {
            heterogeneous |= draws[last.indices[0]].object.mesh != draw.object.mesh;
            text_units = text_units.saturating_add(world_units(draw).unwrap_or(0));
            last.indices.push(index);
        } else {
            text_units = world_units(draw).unwrap_or(0);
            heterogeneous = false;
            output.push(Batch {
                indices: vec![index],
                slot: None,
                first_instance: 0,
            });
        }
    }
}
fn batch_membership(count: usize, batches: &[Batch]) -> Vec<usize> {
    let mut result = vec![usize::MAX; count];
    for (batch, members) in batches.iter().enumerate() {
        for &index in &members.indices {
            result[index] = batch;
        }
    }
    result
}

// Keep each frame's mutable buffer slots separate from the certified ordering.
// Reuse the index allocations rather than deep-cloning every cached batch.
fn visible_batches(plan: &Plan, visible: &[bool], mut output: Vec<Batch>) -> Vec<Batch> {
    let mut count = 0;
    for batch in &plan.batches {
        if count == output.len() {
            output.push(Batch {
                indices: Vec::new(),
                slot: None,
                first_instance: 0,
            });
        }
        let current = &mut output[count];
        current.slot = batch.slot;
        current.first_instance = 0;
        current.indices.clear();
        current.indices.extend(
            batch
                .indices
                .iter()
                .copied()
                .filter(|&index| visible[index]),
        );
        if current.indices.len() < 2 {
            current.slot = None;
        }
        count += usize::from(!current.indices.is_empty());
    }
    output.truncate(count);
    if output.capacity() > 256 && output.capacity() > plan.batches.len().saturating_mul(4) {
        output.shrink_to(plan.batches.len().max(64));
    }
    output
}

fn build_plan(
    draws: &[PreparedDraw],
    bounds: &[[Vec3; 2]],
    visible: &[bool],
    camera: Mat4,
    options: PlanOptions,
) -> PlanBuild {
    let candidate = |all_surfaces| {
        let inputs = draws
            .iter()
            .zip(visible)
            .zip(bounds)
            .map(|((draw, &visible), &bounds)| Input {
                mesh: draw.object.mesh.clone(),
                texture: draw.object.material.texture.clone(),
                model: draw.object.model,
                bounds,
                shader: draw.shader,
                deformation: draw.deformation,
                shared_geometry: draw.shared_geometry.clone(),
                world_geometry_units: draw.world_geometry_units,
                pbr: draw.pbr,
                lit: draw.object.material.lit,
                transparent: draw.transparent,
                visible: all_surfaces || visible,
                raster: draw.raster,
            })
            .collect::<Vec<_>>();
        let (batches, projected, construction_limited) = global_batches_with_capacity(
            draws,
            &inputs,
            camera,
            options.graphs,
            options.transparent_runs,
            options.capacity,
            options.text_limit,
        );
        let ordering = if options.incremental {
            reuse::Ordering::new(&inputs, &batches, camera, projected)
        } else {
            Err(BatchPlanRebuildReason::IncrementalDisabled)
        };
        let batch_of = batch_membership(draws.len(), &batches);
        PlanBuild {
            plan: Plan {
                camera,
                all_surfaces,
                inputs,
                batches,
                ordering,
                changed: Vec::new(),
                batch_of,
            },
            construction_limited,
            rejected_superset: false,
        }
    };
    let result = candidate(options.all_surfaces);
    if !options.all_surfaces {
        return result;
    }
    // Original order certifies even a construction-budget fallback, but can
    // leave thousands of compatible visible peers as permanently split draws.
    // A certificate proves correctness, not that hidden-scene admission helps.
    let mut visible_result = candidate(false);
    let filtered_draws = result
        .plan
        .batches
        .iter()
        .filter(|batch| batch.indices.iter().any(|&index| visible[index]))
        .count();
    if !result.construction_limited
        && result.plan.ordering.is_ok()
        && filtered_draws <= visible_result.plan.batches.len()
    {
        result
    } else {
        visible_result.rejected_superset = true;
        visible_result
    }
}

pub(super) fn native_arena_supported(gpu: &Gpu) -> bool {
    arena::supported(gpu)
}

impl SceneRenderer {
    pub(super) fn native_text_binding(
        &self,
        gpu: &Gpu,
        texture: &TextureKind,
        vertex_ids: &wgpu::Buffer,
    ) -> Result<wgpu::BindGroup> {
        let (objects, parameters) = self.instancing.arena.buffers();
        self.texture_binding(
            gpu,
            texture,
            objects,
            &self.instancing.layout,
            Some(parameters),
            Some(vertex_ids),
        )
    }
    /// Force the portable 64-record path for comparison on capable devices.
    pub fn set_native_instance_arena_enabled(&mut self, enabled: bool) {
        self.instancing.native_requested = enabled;
    }
    pub(super) fn configure_arena(&mut self, gpu: &Gpu, draws: usize) {
        self.instancing.text_bytes_limit = (gpu.device.limits().max_buffer_size / 128)
            .min(gpu.device.limits().max_storage_buffer_binding_size / 24)
            .min(16 * 1024) as usize;
        let native = self.instancing.native_requested
            && self.instancing.shadow_batches_enabled
            && self.instancing.native_layout.is_some()
            && arena::fits(gpu, draws);
        if native == self.instancing.native_mode {
            return;
        }
        self.instancing.native_mode = native;
        self.instancing.layout = if native {
            self.instancing.native_layout.as_ref().unwrap()
        } else {
            &self.instancing.baseline_layout
        }
        .clone();
        self.instancing.plan = None;
        self.instancing.bindings.clear();
        self.instancing.pipelines = None;
        self.instancing.shadow_pipelines = None;
        self.instancing.arena = Default::default();
        self.surface_variants = Default::default();
        for graph in self.graphs.values_mut() {
            std::sync::Arc::make_mut(graph).instanced = [None, None];
        }
        self.occlusion.invalidate();
        self.shadow_frame = None;
    }
    /// Compare the same ordered surfaces against the single-object reference path.
    pub fn set_instancing_enabled(&mut self, enabled: bool) {
        self.instancing.enabled = enabled;
        if !enabled {
            self.instancing.plan = None;
            self.instancing.frame_batches.clear();
            self.instancing.bindings.clear();
            self.instancing.shadow_bindings.clear();
        }
    }

    /// Compare scene-wide grouping with the former consecutive-run batcher.
    pub fn set_global_batching_enabled(&mut self, enabled: bool) {
        self.instancing.global = enabled;
        self.instancing.plan = None;
    }

    /// Compare incremental ordering checks with rebuilding changed batch plans.
    pub fn set_incremental_batch_planning_enabled(&mut self, enabled: bool) {
        self.instancing.incremental = enabled;
        self.instancing.plan = None;
    }

    pub(super) fn prepare_instances(
        &mut self,
        gpu: &Gpu,
        draws: &[PreparedDraw],
        bounds: &[[Vec3; 2]],
        visible: &[bool],
        camera: Mat4,
        output_mask: u8,
    ) -> Result<Vec<Batch>> {
        self.configure_arena(gpu, draws.len());
        self.stats.native_instance_arena = self.instancing.arena_enabled();
        let capacity = self.instancing.capacity();
        let planning_started = std::time::Instant::now();
        let mut batches = if self.instancing.enabled && self.instancing.global {
            let mut previous = self.instancing.plan.take();
            let checks = previous
                .as_mut()
                .ok_or(BatchPlanRebuildReason::Cold)
                .and_then(|plan| {
                    reuse::retain(
                        plan,
                        draws,
                        visible,
                        camera,
                        self.instancing.incremental,
                        bounds,
                    )
                });
            if let Ok(checks) = checks {
                self.stats.batch_plan_reused = true;
                self.stats.batch_bounds_updates = checks.bounds;
                self.stats.batch_order_checks = checks.pairs;
                self.stats.batch_plan_recertifications = usize::from(checks.recertified);
                self.instancing.plan = previous;
            } else {
                // Promote after actual frustum churn, not on a cold/static
                // frame. Cache rejected promotions across camera/visibility
                // churn so a dense hidden population is considered only once.
                let reason = checks.err().unwrap();
                let wants_superset = self
                    .instancing
                    .superset_policy
                    .request(previous.as_ref(), reason);
                // Do not build a huge hidden-scene graph for a tiny viewport.
                // Once certified, a superset can still survive an empty frame.
                let visible_count = visible.iter().filter(|v| **v).count();
                let all_surfaces = wants_superset
                    && self.instancing.incremental
                    && reuse::orthographic(camera)
                    && draws.len() <= visible_count.saturating_mul(4).max(256);
                let built = build_plan(
                    draws,
                    bounds,
                    visible,
                    camera,
                    PlanOptions {
                        graphs: self.instancing.graph_enabled,
                        transparent_runs: self.instancing.transparent_runs,
                        capacity,
                        text_limit: self.instancing.text_bytes_limit,
                        incremental: self.instancing.incremental,
                        all_surfaces,
                    },
                );
                if built.rejected_superset {
                    self.instancing.superset_policy.rejected = true;
                }
                self.instancing.plan = Some(built.plan);
                self.stats.batch_plan_rebuilds = 1;
                self.stats.batch_plan_rebuild_reason = if built.construction_limited {
                    Some(BatchPlanRebuildReason::ConstructionCapacity)
                } else {
                    Some(reason)
                };
            }
            let output = std::mem::take(&mut self.instancing.frame_batches);
            visible_batches(self.instancing.plan.as_ref().unwrap(), visible, output)
        } else {
            batches_with_capacity(
                draws,
                visible,
                self.instancing.enabled,
                self.instancing.graph_enabled,
                self.instancing.transparent_runs,
                capacity,
                self.instancing.text_bytes_limit,
            )
        };
        self.stats.batching = if self.stats.batch_plan_reused
            && self.instancing.diagnostics_visible == visible
            && let Some(stats) = self.instancing.diagnostics
        {
            self.stats.batch_diagnostics_reused = true;
            stats
        } else {
            let stats = diagnostics::collect(
                draws,
                &batches,
                self.instancing.enabled,
                self.instancing.graph_enabled,
            );
            self.instancing.diagnostics_visible.clear();
            self.instancing
                .diagnostics_visible
                .extend_from_slice(visible);
            self.instancing.diagnostics = Some(stats);
            stats
        };
        self.stats.batch_plan_ms = planning_started.elapsed().as_secs_f64() * 1000.;
        let count = batches.iter().filter(|b| b.indices.len() > 1).count();
        // Retain a bounded set of spare allocations through temporary culling or
        // removals. Explicit disable/asset invalidation still releases everything.
        let retained_slots = self.instancing.plan.as_ref().map_or(0, |p| {
            p.batches
                .iter()
                .filter_map(|b| b.slot)
                .max()
                .map_or(0, |s| s + 1)
        });
        self.instancing
            .bindings
            .truncate((count + 8).max(retained_slots));
        if count == 0 {
            return Ok(batches);
        }
        let mut used = [false; 2];
        for batch in batches.iter().filter(|b| b.indices.len() > 1) {
            let draw = &draws[batch.indices[0]];
            if draw.shader.is_none()
                && self
                    .variant_key(draw, batch.indices.len() as u32, output_mask)
                    .is_none()
            {
                used[usize::from(draw.pbr)] = true;
            }
        }
        let auxiliary = usize::from(output_mask != 0);
        for (pbr, needed) in used.into_iter().enumerate() {
            if !needed {
                continue;
            }
            self.instancing.pipelines.get_or_insert_with(|| Pipelines {
                pipelines: std::array::from_fn(|_| [None, None]),
            });
            if self.instancing.pipelines.as_ref().unwrap().pipelines[auxiliary][pbr].is_some() {
                continue;
            }
            let layout = gpu
                .device
                .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: Some("instanced scene pipeline layout"),
                    bind_group_layouts: &[
                        Some(&self.instancing.layout),
                        (pbr == 1).then(|| self.pbr.material_layout()),
                        Some(&self.shadows.sample_layout),
                        Some(&self.environment.layout),
                    ],
                    immediate_size: 0,
                });
            let module = gpu
                .device
                .create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some("instanced scene shader"),
                    source: wgpu::ShaderSource::Wgsl(
                        instance_module_text_for(
                            host_text(pbr == 1),
                            self.instancing.arena_enabled(),
                        )
                        .into(),
                    ),
                });
            let pipeline = scene_pipeline(
                gpu,
                "instanced scene pipeline",
                &layout,
                &module,
                pbr == 1,
                false,
                auxiliary == 1,
            );
            self.instancing.pipelines.as_mut().unwrap().pipelines[auxiliary][pbr] = Some(pipeline);
        }
        let mut bindings = std::mem::take(&mut self.instancing.bindings);
        self.instancing.binding_reserved.clear();
        self.instancing
            .binding_reserved
            .resize(bindings.len() + count, false);
        if let Some(plan) = &self.instancing.plan {
            for slot in plan.batches.iter().filter_map(|batch| batch.slot) {
                self.instancing.binding_reserved[slot] = true;
            }
        }
        let mut next = 0;
        for batch in batches.iter_mut().filter(|b| b.indices.len() > 1) {
            if batch.slot.is_none() {
                while self.instancing.binding_reserved[next] {
                    next += 1;
                }
                batch.slot = Some(next);
                self.instancing.binding_reserved[next] = true;
            }
        }
        let result = if self.instancing.arena_enabled() {
            arena::prepare(self, gpu, draws, &mut batches, &mut bindings)
        } else {
            self.prepare_instance_bindings(gpu, draws, &mut batches, &mut bindings)
        };
        self.instancing.bindings = bindings;
        let (bytes, allocations, parameter_bytes) = result?;
        self.stats.instance_uniform_bytes += bytes;
        self.stats.instance_buffer_allocations += allocations;
        self.stats.graph_parameter_bytes += parameter_bytes;
        if let Some(plan) = &mut self.instancing.plan {
            for batch in &batches {
                if let Some(slot) = batch.slot {
                    let index = plan.batch_of[batch.indices[0]];
                    if index != usize::MAX {
                        plan.batches[index].slot = Some(slot);
                    }
                }
            }
        }
        Ok(batches)
    }

    pub(super) fn prepare_shadow_instances(
        &mut self,
        gpu: &Gpu,
        draws: &[PreparedDraw],
    ) -> Result<Vec<Batch>> {
        depth::prepare(self, gpu, draws)
    }

    fn prepare_instance_bindings(
        &self,
        gpu: &Gpu,
        draws: &[PreparedDraw],
        batches: &mut [Batch],
        bindings: &mut Vec<InstanceBinding>,
    ) -> Result<(usize, usize, usize)> {
        let mut bytes = 0;
        let mut allocations = 0;
        let mut parameter_bytes = 0;
        for batch in batches.iter_mut().filter(|b| b.indices.len() > 1) {
            let slot = batch.slot.unwrap();
            let texture = &draws[batch.indices[0]].object.material.texture;
            let has_parameters = batch
                .indices
                .iter()
                .any(|&index| !self.objects[index].numeric_parameters.is_empty());
            let parameter_buffer = || {
                gpu.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("instanced graph numeric parameters"),
                    size: GRAPH_PARAMETER_BUFFER_BYTES as u64,
                    usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                })
            };
            if slot == bindings.len() {
                let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("instanced object uniforms"),
                    size: BUFFER_BYTES as u64,
                    usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                let parameters = has_parameters.then(parameter_buffer);
                let binding = self.texture_binding(
                    gpu,
                    texture,
                    &buffer,
                    &self.instancing.layout,
                    parameters.as_ref(),
                    None,
                )?;
                let value = InstanceBinding {
                    buffer,
                    binding,
                    texture: texture.clone(),
                    bytes: Vec::with_capacity(BUFFER_BYTES),
                    revisions: Vec::with_capacity(MAX_INSTANCES),
                    parameter_buffer: parameters,
                    parameter_bytes: Vec::new(),
                    parameter_revisions: Vec::new(),
                    first_instance: 0,
                };
                bindings.push(value);
                allocations += 1;
            } else if bindings[slot].texture != *texture
                || has_parameters && bindings[slot].parameter_buffer.is_none()
            {
                if has_parameters && bindings[slot].parameter_buffer.is_none() {
                    bindings[slot].parameter_buffer = Some(parameter_buffer());
                }
                let binding = self.texture_binding(
                    gpu,
                    texture,
                    &bindings[slot].buffer,
                    &self.instancing.layout,
                    bindings[slot].parameter_buffer.as_ref(),
                    None,
                )?;
                bindings[slot].binding = binding;
                bindings[slot].texture = texture.clone();
            }
            let binding = &mut bindings[slot];
            let old_len = binding.bytes.len();
            binding
                .bytes
                .resize(batch.indices.len() * OBJECT_UNIFORM_BYTES, 0);
            binding.revisions.resize(batch.indices.len(), 0);
            let mut changed_start = None;
            // Merge adjacent edits into one write; leave unchanged instance ranges
            // resident. New/expanded records are always uploaded, even if all zero.
            for (instance, &index) in batch.indices.iter().enumerate() {
                let start = instance * OBJECT_UNIFORM_BYTES;
                let end = start + OBJECT_UNIFORM_BYTES;
                let uniform = self.objects[index].uniform.as_ref().unwrap();
                let revision = self.objects[index].uniform_revision;
                let revision_changed = binding.revisions[instance] != revision;
                // A rebuilt plan may restore an identical row whose CPU object
                // record was retired and received a new revision. Compare bytes
                // only on that cold path; reused plans retain revision-only checks.
                let changed = !self.state_caching
                    || start >= old_len
                    || revision_changed
                        && (self.stats.batch_plan_reused || binding.bytes[start..end] != *uniform);
                if revision_changed {
                    binding.revisions[instance] = revision;
                }
                if changed {
                    binding.bytes[start..end].copy_from_slice(uniform);
                    changed_start.get_or_insert(start);
                } else if let Some(first) = changed_start.take() {
                    gpu.queue.write_buffer(
                        &binding.buffer,
                        (u64::from(batch.first_instance) * OBJECT_UNIFORM_BYTES as u64)
                            + first as u64,
                        &binding.bytes[first..start],
                    );
                    bytes += start - first;
                }
            }
            if let Some(first) = changed_start {
                gpu.queue.write_buffer(
                    &binding.buffer,
                    u64::from(batch.first_instance) * OBJECT_UNIFORM_BYTES as u64 + first as u64,
                    &binding.bytes[first..],
                );
                bytes += binding.bytes.len() - first;
            }
            if let Some(buffer) = &binding.parameter_buffer {
                let old_len = binding.parameter_bytes.len();
                binding
                    .parameter_bytes
                    .resize(batch.indices.len() * GRAPH_PARAMETER_RECORD_BYTES, 0);
                binding.parameter_revisions.resize(batch.indices.len(), 0);
                let mut changed_start = None;
                for (instance, &index) in batch.indices.iter().enumerate() {
                    let start = instance * GRAPH_PARAMETER_RECORD_BYTES;
                    let end = start + GRAPH_PARAMETER_RECORD_BYTES;
                    let object = &self.objects[index];
                    let revision_changed =
                        binding.parameter_revisions[instance] != object.parameter_revision;
                    let changed = if !self.state_caching || start >= old_len || revision_changed {
                        let parameters = graph_parameter_bytes(&object.numeric_parameters);
                        let changed = !self.state_caching
                            || start >= old_len
                            || self.stats.batch_plan_reused
                            || binding.parameter_bytes[start..end] != parameters;
                        binding.parameter_revisions[instance] = object.parameter_revision;
                        if changed {
                            binding.parameter_bytes[start..end].copy_from_slice(&parameters);
                        }
                        changed
                    } else {
                        false
                    };
                    if changed {
                        changed_start.get_or_insert(start);
                    } else if let Some(first) = changed_start.take() {
                        gpu.queue.write_buffer(
                            buffer,
                            u64::from(batch.first_instance) * GRAPH_PARAMETER_RECORD_BYTES as u64
                                + first as u64,
                            &binding.parameter_bytes[first..start],
                        );
                        parameter_bytes += start - first;
                    }
                }
                if let Some(first) = changed_start {
                    gpu.queue.write_buffer(
                        buffer,
                        u64::from(batch.first_instance) * GRAPH_PARAMETER_RECORD_BYTES as u64
                            + first as u64,
                        &binding.parameter_bytes[first..],
                    );
                    parameter_bytes += binding.parameter_bytes.len() - first;
                }
            }
            batch.slot = Some(slot);
        }
        Ok((bytes, allocations, parameter_bytes))
    }

    pub(super) fn prepare_instanced_shadows(&mut self, gpu: &Gpu) {
        let compact = self.instancing.shadow_batching();
        if self.instancing.shadow_pipelines.is_none()
            || self.instancing.shadow_pipeline_compact != compact
        {
            self.instancing.shadow_pipelines = Some(std::array::from_fn(|point| {
                shadows::pipeline(
                    gpu,
                    if self.instancing.shadow_batching() {
                        self.instancing.shadow_layout.as_ref().unwrap()
                    } else {
                        &self.instancing.layout
                    },
                    &self.shadows.caster_layout,
                    true,
                    point == 1,
                    compact,
                )
            }));
            self.instancing.shadow_pipeline_compact = compact;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn runs_split_at_limits_culling_and_incompatible_surfaces() {
        let draw = || PreparedDraw {
            preparation: Default::default(),
            source_item: 0,
            deformation: 0,
            shared_geometry: None,
            world_geometry_units: None,
            pbr_override: [-1.; 2],
            shader: None,
            pbr: false,
            raster: 0,
            opacity: 1.,
            cutoff: 0.,
            transparent: false,
            depth: 0.,
            object: DrawItem {
                motion_id: 1,
                model: Mat4::IDENTITY,
                mesh: MeshKind::Cube,
                material: Material {
                    metallic: None,
                    roughness: None,
                    surface_overrides: Default::default(),
                    tint: [1.; 3],
                    uv_scale: [1.; 2],
                    texture: TextureKind::White,
                    lit: false,
                    shader: None,
                },
            },
        };
        let mut draws: Vec<_> = (0..67).map(|_| draw()).collect();
        let mut visible = vec![true; draws.len()];
        let sizes = |draws: &[PreparedDraw], visible: &[bool], enabled| {
            batches(draws, visible, enabled, true, false)
                .iter()
                .map(|b| b.indices.len())
                .collect::<Vec<_>>()
        };
        assert_eq!(sizes(&draws, &visible, true), [64, 3]);
        assert_eq!(sizes(&draws, &visible, false), vec![1; 67]);
        visible[32] = false;
        assert_eq!(sizes(&draws, &visible, true), [32, 34]);
        for kind in 0..4 {
            let mut b = draw();
            match kind {
                0 => b.deformation = 1,
                1 => b.shader = Some(1),
                2 => b.transparent = true,
                _ => b.object.material.texture = TextureKind::Checker,
            }
            assert!(!compatible(&draws[0], &b, true, false));
            assert!(!compatible(&b, &draws[0], true, false));
        }
        draws[1].deformation = 1;
        assert_eq!(&sizes(&draws, &visible, true)[..3], &[1, 1, 30]);
    }
    #[test]
    fn stock_instanced_shaders_validate_on_baseline_capabilities() {
        for pbr in [false, true] {
            let source = module_text(pbr);
            let module = wgpu::naga::front::wgsl::parse_str(&source)
                .unwrap_or_else(|e| panic!("{}", e.emit_to_string(&source)));
            wgpu::naga::valid::Validator::new(
                wgpu::naga::valid::ValidationFlags::all(),
                wgpu::naga::valid::Capabilities::empty(),
            )
            .validate(&module)
            .unwrap();
            for (name, expected) in [
                ("ObjectUniform", OBJECT_UNIFORM_BYTES),
                ("FrameUniform", FRAME_UNIFORM_BYTES),
            ] {
                let ty = &module
                    .types
                    .iter()
                    .find(|(_, ty)| ty.name.as_deref() == Some(name))
                    .unwrap()
                    .1;
                let wgpu::naga::TypeInner::Struct { span, .. } = ty.inner else {
                    panic!("uniform struct")
                };
                assert_eq!(span as usize, expected, "{name} packing");
            }
            const { assert!(BUFFER_BYTES <= 16 * 1024) };
        }
    }
}
