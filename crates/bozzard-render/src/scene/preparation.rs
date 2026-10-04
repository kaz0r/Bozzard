//! Retain static surface expansion while refreshing transforms, skin bounds and sort depth.
use super::*;
use std::{cmp::Ordering, mem, sync::Arc};

#[derive(Default)]
pub(super) struct SurfacePreparation {
    pub draws: Vec<PreparedDraw>,
    sources: Vec<DrawItem>,
    changed: Vec<bool>,
    models_changed: Vec<bool>,
    view_projection: Option<Mat4>,
}
impl SurfacePreparation {
    pub fn clear(&mut self) {
        *self = Self::default();
    }
    fn compact(&mut self) {
        compact(&mut self.sources);
        compact(&mut self.changed);
        compact(&mut self.models_changed);
    }
    fn capacity_bytes(&self, draw_capacity: usize) -> usize {
        draw_capacity * mem::size_of::<PreparedDraw>()
            + self.sources.capacity() * mem::size_of::<DrawItem>()
            + self.changed.capacity() * mem::size_of::<bool>()
            + self.models_changed.capacity() * mem::size_of::<bool>()
    }
}
fn compact<T>(items: &mut Vec<T>) {
    // Keep normal frame-to-frame churn allocation-free without retaining a former large scene.
    if items.capacity() > 256 && items.capacity() > items.len().saturating_mul(4) {
        items.shrink_to(items.len().max(64));
    }
}
#[derive(Default)]
pub(super) struct DrawPreparation {
    surface_order: usize,
    skinned: bool,
    base_center: Vec3,
    center: Vec3,
    // Resource publication clears the entire retained surface set. Static source edits
    // rebuild a draw, so this certificate belongs to its immutable resolved mesh identity.
    pub(super) resources: Option<ResourceMetadata>,
    // Keep these separate: regrouping matrix products changes rounding and signed zero.
    origin: Option<Mat4>,
    override_transform: Option<Mat4>,
}
pub(super) struct ResourceMetadata {
    pub bounds: [Vec3; 2],
    pub double_sided: bool,
}
impl DrawPreparation {
    fn cacheable_resources(&self, mesh: &MeshKind) -> bool {
        !self.skinned && !matches!(mesh, MeshKind::Text(_) | MeshKind::Sprite(_))
    }
}

pub(super) fn double_sided(models: &BTreeMap<String, Vec<UploadedPart>>, mesh: &MeshKind) -> bool {
    match mesh {
        MeshKind::ModelPart(id, index) => models[id][*index]
            .shading
            .as_ref()
            .is_none_or(|s| s.double_sided),
        _ => true,
    }
}

fn bits_eq<const N: usize>(a: [f32; N], b: [f32; N]) -> bool {
    a.map(f32::to_bits) == b.map(f32::to_bits)
}
fn matrix_eq(a: Mat4, b: Mat4) -> bool {
    bits_eq(a.to_cols_array(), b.to_cols_array())
}
fn option_float_eq(a: Option<f32>, b: Option<f32>) -> bool {
    a.map(f32::to_bits) == b.map(f32::to_bits)
}
fn option_array_eq<const N: usize>(a: Option<[f32; N]>, b: Option<[f32; N]>) -> bool {
    a.map(|v| v.map(f32::to_bits)) == b.map(|v| v.map(f32::to_bits))
}
fn screen_eq(a: Option<ScreenText>, b: Option<ScreenText>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => bits_eq(a.anchor, b.anchor) && bits_eq(a.offset, b.offset),
        _ => false,
    }
}
fn mesh_eq(a: &MeshKind, b: &MeshKind) -> bool {
    match (a, b) {
        (MeshKind::Sprite(a), MeshKind::Sprite(b)) => {
            (Arc::ptr_eq(&a.geometry, &b.geometry)
                || (a.geometry.quads().len() == b.geometry.quads().len()
                    && a.geometry
                        .quads()
                        .iter()
                        .zip(b.geometry.quads())
                        .all(|(a, b)| bits_eq(*a, *b))))
                && screen_eq(a.screen, b.screen)
                && option_array_eq(a.clip, b.clip)
                && a.opacity.to_bits() == b.opacity.to_bits()
        }
        (MeshKind::Text(a), MeshKind::Text(b)) => {
            a.text == b.text
                && a.monospace == b.monospace
                && a.custom_font == b.custom_font
                && a.alignment == b.alignment
                && screen_eq(a.screen, b.screen)
                && option_array_eq(a.clip, b.clip)
                && a.font_size.to_bits() == b.font_size.to_bits()
                && option_float_eq(a.max_width, b.max_width)
                && a.opacity.to_bits() == b.opacity.to_bits()
        }
        _ => a == b,
    }
}
fn override_eq(a: &SurfaceMaterialOverride, b: &SurfaceMaterialOverride) -> bool {
    a.surface == b.surface
        && a.source == b.source
        && matrix_eq(a.transform, b.transform)
        && a.texture == b.texture
        && bits_eq(a.uv_scale, b.uv_scale)
        && bits_eq(a.tint, b.tint)
        && option_float_eq(a.metallic, b.metallic)
        && option_float_eq(a.roughness, b.roughness)
}
fn material_eq(a: &Material, b: &Material) -> bool {
    a.texture == b.texture
        && a.lit == b.lit
        && bits_eq(a.tint, b.tint)
        && bits_eq(a.uv_scale, b.uv_scale)
        && option_float_eq(a.metallic, b.metallic)
        && option_float_eq(a.roughness, b.roughness)
        && (Arc::ptr_eq(&a.surface_overrides, &b.surface_overrides)
            || (a.surface_overrides.len() == b.surface_overrides.len()
                && a.surface_overrides
                    .iter()
                    .zip(b.surface_overrides.iter())
                    .all(|(a, b)| override_eq(a, b))))
        && match (&a.shader, &b.shader) {
            (None, None) => true,
            (Some(a), Some(b)) => Arc::ptr_eq(a, b) || (a.id == b.id && a.surface == b.surface),
            _ => false,
        }
}
fn static_eq(a: &DrawItem, b: &DrawItem) -> bool {
    mesh_eq(&a.mesh, &b.mesh) && material_eq(&a.material, &b.material)
}
fn order(a: &PreparedDraw, b: &PreparedDraw) -> Ordering {
    a.transparent.cmp(&b.transparent).then_with(|| {
        if a.transparent {
            b.depth.total_cmp(&a.depth)
        } else {
            a.shader.cmp(&b.shader).then_with(|| a.pbr.cmp(&b.pbr))
        }
    })
}
fn retained_order(a: &PreparedDraw, b: &PreparedDraw) -> Ordering {
    order(a, b)
        .then_with(|| a.source_item.cmp(&b.source_item))
        .then_with(|| {
            a.preparation
                .surface_order
                .cmp(&b.preparation.surface_order)
        })
}
struct DynamicInputs {
    view_projection: Mat4,
    model_changed: bool,
    camera_changed: bool,
    center: Vec3,
    deformation: u64,
}
fn refresh_draw(
    draw: &mut PreparedDraw,
    source: &DrawItem,
    inputs: DynamicInputs,
    stats: &mut FrameStats,
) -> bool {
    let DynamicInputs {
        view_projection,
        model_changed,
        camera_changed,
        center,
        deformation,
    } = inputs;
    if model_changed {
        let mut model = source.model;
        if let Some(origin) = draw.preparation.origin {
            model *= origin;
        }
        if let Some(transform) = draw.preparation.override_transform {
            model *= transform;
        }
        draw.object.model = model;
        stats.surface_model_updates += 1;
    }
    draw.object.motion_id = source.motion_id;
    draw.deformation = deformation;
    let center_changed = !bits_eq(draw.preparation.center.to_array(), center.to_array());
    if model_changed || camera_changed || center_changed {
        draw.preparation.center = center;
        let depth = view_projection
            .project_point3(draw.object.model.transform_point3(center))
            .z;
        let changed = draw.depth.to_bits() != depth.to_bits();
        draw.depth = depth;
        stats.surface_depth_updates += 1;
        return changed && draw.transparent;
    }
    false
}

impl SceneRenderer {
    /// Retain validated static mesh bounds and sidedness with prepared surfaces.
    /// Requires state and surface preparation caching; dynamic geometry always bypasses it.
    /// Disabling clears certificates but preserves other preparation/batching caches.
    /// Existing FrameStats remain the last completed frame snapshot until the next draw.
    pub fn set_resource_metadata_caching_enabled(&mut self, enabled: bool) {
        self.resource_metadata_caching = enabled;
        if !enabled {
            for draw in &mut self.surface_preparation.draws {
                draw.preparation.resources = None;
            }
        }
    }

    pub(super) fn prepare_bounds(&mut self, draws: &mut [PreparedDraw]) -> Vec<[Vec3; 2]> {
        let retain = self.resource_metadata_caching
            && self.state_caching
            && self.surface_preparation_caching;
        let bounds = draws
            .iter_mut()
            .map(|draw| {
                if let Some(resources) = &draw.preparation.resources {
                    self.stats.resource_metadata_hits += 1;
                    return resources.bounds;
                }
                self.stats.resource_bounds_lookups += 1;
                // Called only after the original ordered binding/resource validation pass.
                let bounds = self.mesh_for(&draw.object).bounds;
                if retain && draw.preparation.cacheable_resources(&draw.object.mesh) {
                    draw.preparation.resources = Some(ResourceMetadata {
                        bounds,
                        double_sided: double_sided(&self.models, &draw.object.mesh),
                    });
                    self.stats.resource_metadata_builds += 1;
                } else {
                    self.stats.resource_metadata_bypasses += 1;
                }
                bounds
            })
            .collect();
        self.stats.resource_metadata_bytes = (self.stats.resource_metadata_hits
            + self.stats.resource_metadata_builds)
            * mem::size_of::<ResourceMetadata>();
        bounds
    }

    /// Retain surface expansion between frames. Disable for the original per-frame reference.
    /// Global state caching must also be enabled; disabling either releases retained records.
    /// Existing FrameStats remain the last frame snapshot until the next draw.
    pub fn set_surface_preparation_caching_enabled(&mut self, enabled: bool) {
        self.surface_preparation_caching = enabled;
        if !enabled {
            self.surface_preparation.clear();
        }
    }
    pub(super) fn prepare(&mut self, scene: &RenderScene) -> Vec<PreparedDraw> {
        let started = std::time::Instant::now();
        if !self.state_caching || !self.surface_preparation_caching {
            self.surface_preparation.clear();
            let draws = self.prepare_reference(scene);
            self.stats.surface_items_rebuilt = scene.items.len();
            self.stats.surface_records_built = draws.len();
            self.stats.surface_prepare_ms = started.elapsed().as_secs_f64() * 1000.;
            return draws;
        }
        let mut cache = mem::take(&mut self.surface_preparation);
        cache.changed.resize(scene.items.len(), false);
        cache.models_changed.resize(scene.items.len(), false);
        let retired = cache.sources.len() > scene.items.len();
        cache.sources.truncate(scene.items.len());
        for (index, source) in scene.items.iter().enumerate() {
            self.stats.surface_source_checks += 1;
            let changed = cache
                .sources
                .get(index)
                .is_none_or(|old| !static_eq(old, source));
            cache.changed[index] = changed;
            cache.models_changed[index] = cache
                .sources
                .get(index)
                .is_none_or(|old| !matrix_eq(old.model, source.model));
            if changed {
                self.stats.surface_items_rebuilt += 1;
                if let Some(old) = cache.sources.get_mut(index) {
                    *old = source.clone();
                } else {
                    cache.sources.push(source.clone());
                }
            } else {
                self.stats.surface_items_reused += 1;
                cache.sources[index].model = source.model;
                cache.sources[index].motion_id = source.motion_id;
            }
        }
        let camera_changed = cache
            .view_projection
            .is_none_or(|old| !matrix_eq(old, scene.view_projection));
        cache.view_projection = Some(scene.view_projection);
        let mut draws = mem::take(&mut cache.draws);
        let previous_len = draws.len();
        if retired || self.stats.surface_items_rebuilt != 0 {
            draws.retain(|draw| {
                draw.source_item < scene.items.len() && !cache.changed[draw.source_item]
            });
        }
        let mut sort = previous_len != draws.len();
        self.stats.surface_records_reused = draws.len();
        for draw in &mut draws {
            let source = &scene.items[draw.source_item];
            let motion_changed = draw.object.motion_id != source.motion_id;
            draw.object.motion_id = source.motion_id;
            let deformation = if draw.preparation.skinned {
                self.skinning.revision(&draw.object)
            } else {
                0
            };
            let center = if deformation != draw.deformation || motion_changed {
                self.skinning
                    .mesh(&draw.object)
                    .map_or(draw.preparation.base_center, |mesh| {
                        (mesh.bounds[0] + mesh.bounds[1]) * 0.5
                    })
            } else {
                draw.preparation.center
            };
            sort |= refresh_draw(
                draw,
                source,
                DynamicInputs {
                    view_projection: scene.view_projection,
                    model_changed: cache.models_changed[draw.source_item],
                    camera_changed,
                    center,
                    deformation,
                },
                &mut self.stats,
            );
        }
        if self.stats.surface_items_rebuilt != 0 {
            for (source_item, &changed) in cache.changed.iter().enumerate() {
                if changed {
                    let before = draws.len();
                    self.prepare_item(scene, source_item, &mut draws);
                    self.stats.surface_records_built += draws.len() - before;
                    sort |= draws.len() != before;
                }
            }
        }
        if sort {
            // Explicit source order restores the reference stable ties even after camera sorting.
            draws.sort_unstable_by(retained_order);
        }
        self.stats.surface_order_reused = !sort;
        compact(&mut draws);
        cache.compact();
        self.stats.surface_preparation_bytes = cache.capacity_bytes(draws.capacity());
        self.surface_preparation = cache;
        self.stats.surface_prepare_ms = started.elapsed().as_secs_f64() * 1000.;
        draws
    }
    fn prepare_reference(&self, scene: &RenderScene) -> Vec<PreparedDraw> {
        let mut draws = Vec::new();
        for source_item in 0..scene.items.len() {
            self.prepare_item(scene, source_item, &mut draws);
        }
        draws.sort_by(order);
        draws
    }
    fn prepare_item(&self, scene: &RenderScene, source_item: usize, draws: &mut Vec<PreparedDraw>) {
        let mut surface_order = 0;
        let mut add = |source_item: usize,
                       object: DrawItem,
                       opacity: f32,
                       cutoff: Option<f32>,
                       translucent: bool,
                       center: Vec3,
                       pbr_override: [f32; 2],
                       pbr: bool,
                       mut preparation: DrawPreparation| {
            preparation.surface_order = surface_order;
            surface_order += 1;
            let depth = scene
                .view_projection
                .project_point3(object.model.transform_point3(center))
                .z;
            preparation.center = center;
            preparation.skinned = matches!(&object.mesh, MeshKind::ModelPart(asset, _) if self.skinning.sources.contains_key(asset));
            draws.push(PreparedDraw {
                preparation,
                source_item,
                deformation: self.skinning.revision(&object),
                pbr_override,
                pbr,
                shader: object.material.shader.as_ref().map(|s| s.id),
                object,
                opacity,
                cutoff: cutoff.unwrap_or(0.0),
                transparent: cutoff.is_none() && translucent,
                depth,
            });
        };
        let object = &scene.items[source_item];
        if let MeshKind::Sprite(sprite) = &object.mesh {
            if sprite.screen.is_none()
                && let Some(mesh) = self.sprites.mesh(sprite)
            {
                add(
                    source_item,
                    object.clone(),
                    sprite.opacity,
                    None,
                    true,
                    (mesh.bounds[0] + mesh.bounds[1]) * 0.5,
                    [-1.; 2],
                    false,
                    DrawPreparation {
                        base_center: (mesh.bounds[0] + mesh.bounds[1]) * 0.5,
                        ..Default::default()
                    },
                );
            }
            return;
        }
        if let MeshKind::Text(text) = &object.mesh {
            if text.screen.is_some() {
                return;
            }
            if let Some(mesh) = self.text.as_ref().and_then(|t| t.mesh(text)) {
                add(
                    source_item,
                    object.clone(),
                    text.opacity,
                    None,
                    true,
                    (mesh.bounds[0] + mesh.bounds[1]) * 0.5,
                    [-1.; 2],
                    false,
                    DrawPreparation {
                        base_center: (mesh.bounds[0] + mesh.bounds[1]) * 0.5,
                        ..Default::default()
                    },
                );
            }
            return;
        }
        if let MeshKind::Imported(id) | MeshKind::ModelPart(id, _) = &object.mesh
            && let Some(parts) = self.models.get(id)
        {
            let overrides: BTreeMap<_, _> = object
                .material
                .surface_overrides
                .iter()
                .map(|v| (v.surface as usize, v))
                .collect();
            let (start, count) = match &object.mesh {
                MeshKind::ModelPart(_, index) => (*index, 1),
                _ => (0, parts.len()),
            };
            // Expanded surface entities already identify their part. Slice iterators
            // skip directly to it instead of scanning every sibling for every entity.
            for (index, part) in parts.iter().enumerate().skip(start).take(count) {
                let mut item = object.clone();
                if matches!(object.mesh, MeshKind::ModelPart(..)) {
                    item.model *= Mat4::from_translation(-part.center);
                }
                item.mesh = MeshKind::ModelPart(id.clone(), index);
                for (tint, color) in item.material.tint.iter_mut().zip(part.color) {
                    *tint *= color;
                }
                let override_value = overrides
                    .get(&index)
                    .filter(|value| value.source == part.source_key);
                if let Some(value) = override_value {
                    item.model *= Mat4::from_translation(part.center)
                        * value.transform
                        * Mat4::from_translation(-part.center);
                    for (uv, scale) in item.material.uv_scale.iter_mut().zip(value.uv_scale) {
                        *uv *= scale;
                    }
                    if let Some(texture) = &value.texture {
                        item.material.texture = texture.clone();
                    }
                }
                let translucent = if item.material.texture == TextureKind::White
                    && override_value.is_none_or(|v| v.texture.is_none())
                {
                    if part.texture.is_some() {
                        item.material.texture = TextureKind::ModelPart(id.clone(), index);
                    }
                    part.translucent
                } else {
                    part.color[3] < 1.0
                        || matches!(&item.material.texture, TextureKind::Imported(id) if self.transparent_textures.contains(id))
                        || matches!(&item.material.texture, TextureKind::Generated(_))
                };
                if let Some(value) = override_value {
                    for (tint, multiplier) in item.material.tint.iter_mut().zip(value.tint) {
                        *tint *= multiplier;
                    }
                }
                let factors = [
                    override_value
                        .and_then(|v| v.metallic)
                        .or(item.material.metallic)
                        .unwrap_or(-1.),
                    override_value
                        .and_then(|v| v.roughness)
                        .or(item.material.roughness)
                        .unwrap_or(-1.),
                ];
                let center = self
                    .skinning
                    .mesh(&item)
                    .map_or(part.center, |mesh| (mesh.bounds[0] + mesh.bounds[1]) * 0.5);
                add(
                    source_item,
                    item,
                    part.color[3],
                    part.cutoff,
                    translucent,
                    center,
                    factors,
                    part.shading.is_some(),
                    DrawPreparation {
                        base_center: part.center,
                        origin: matches!(object.mesh, MeshKind::ModelPart(..))
                            .then(|| Mat4::from_translation(-part.center)),
                        override_transform: override_value.map(|value| {
                            Mat4::from_translation(part.center)
                                * value.transform
                                * Mat4::from_translation(-part.center)
                        }),
                        ..Default::default()
                    },
                );
            }
        } else {
            let transparent = matches!(&object.material.texture, TextureKind::Imported(id) if self.transparent_textures.contains(id))
                || matches!(&object.material.texture, TextureKind::Generated(_));
            add(
                source_item,
                object.clone(),
                1.0,
                None,
                transparent,
                Vec3::ZERO,
                [
                    object.material.metallic.unwrap_or(-1.),
                    object.material.roughness.unwrap_or(-1.),
                ],
                false,
                DrawPreparation::default(),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn source() -> DrawItem {
        DrawItem {
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
        }
    }
    fn draw(index: usize, transparent: bool, depth: f32) -> PreparedDraw {
        PreparedDraw {
            preparation: Default::default(),
            source_item: index,
            deformation: 0,
            pbr_override: [-1.; 2],
            shader: None,
            pbr: false,
            object: source(),
            opacity: 1.,
            cutoff: 0.,
            transparent,
            depth,
        }
    }
    #[test]
    fn static_keys_ignore_runtime_matrices_but_preserve_float_bits_and_graph_content() {
        let original = source();
        let mut edited = original.clone();
        edited.model = Mat4::from_translation(Vec3::new(3., -0., 7.));
        edited.motion_id = 12;
        assert!(static_eq(&original, &edited));
        edited.material.roughness = Some(0.);
        let mut positive = edited.clone();
        edited.material.roughness = Some(-0.);
        assert!(!static_eq(&positive, &edited));
        positive.material.shader = Some(Arc::new(ShaderSource {
            id: 7,
            surface: "first".into(),
        }));
        edited = positive.clone();
        edited.material.shader = Some(Arc::new(ShaderSource {
            id: 7,
            surface: "second".into(),
        }));
        assert!(!static_eq(&positive, &edited));
        positive.material.surface_overrides = Arc::from([SurfaceMaterialOverride {
            surface: 0,
            source: "0000000000000000".into(),
            transform: Mat4::IDENTITY,
            texture: None,
            uv_scale: [1.; 2],
            tint: [0., 1., 1.],
            metallic: None,
            roughness: None,
        }]);
        edited = positive.clone();
        Arc::make_mut(&mut edited.material.surface_overrides)[0].tint[0] = -0.;
        assert!(!static_eq(&positive, &edited));
        positive.mesh = MeshKind::Text(TextMesh::default());
        edited = positive.clone();
        if let MeshKind::Text(text) = &mut edited.mesh {
            text.opacity = -0.;
        }
        if let MeshKind::Text(text) = &mut positive.mesh {
            text.opacity = 0.;
        }
        assert!(!static_eq(&positive, &edited));
    }
    #[test]
    fn retained_sort_restores_canonical_ties_after_transparency_reorders() {
        let inputs = [
            (false, 3.),
            (true, -0.),
            (true, 0.),
            (true, f32::NAN),
            (true, 7.),
            (true, 7.),
            (false, 0.),
        ];
        let mut reference: Vec<_> = inputs
            .iter()
            .enumerate()
            .map(|(i, &(t, d))| draw(i, t, d))
            .collect();
        let mut retained: Vec<_> = inputs
            .iter()
            .enumerate()
            .rev()
            .map(|(i, &(t, d))| draw(i, t, d))
            .collect();
        reference.sort_by(order);
        retained.sort_unstable_by(retained_order);
        assert_eq!(
            reference.iter().map(|d| d.source_item).collect::<Vec<_>>(),
            retained.iter().map(|d| d.source_item).collect::<Vec<_>>()
        );
        // Several previously reversed objects now occupy the same projected center.
        for d in &mut reference {
            d.depth = 2.;
        }
        for d in &mut retained {
            d.depth = 2.;
        }
        reference.sort_by_key(|d| d.source_item);
        reference.sort_by(order);
        retained.sort_unstable_by(retained_order);
        assert_eq!(
            reference.iter().map(|d| d.source_item).collect::<Vec<_>>(),
            retained.iter().map(|d| d.source_item).collect::<Vec<_>>()
        );
    }
    #[test]
    fn dynamic_refresh_matches_sequential_surface_transforms_and_skin_centers() {
        let mut source = source();
        source.model = Mat4::from_rotation_y(0.73) * Mat4::from_scale(Vec3::new(-1.3, 2.7, 0.9));
        let center = Vec3::new(0.31, -0.57, 0.83);
        let origin = Mat4::from_translation(-center);
        let adjustment = Mat4::from_translation(center)
            * Mat4::from_rotation_z(1.1)
            * Mat4::from_translation(-center);
        let mut retained = draw(0, true, 0.);
        retained.preparation.origin = Some(origin);
        retained.preparation.override_transform = Some(adjustment);
        let mut expected = source.model;
        expected *= origin;
        expected *= adjustment;
        let mut stats = FrameStats::default();
        assert!(refresh_draw(
            &mut retained,
            &source,
            DynamicInputs {
                view_projection: Mat4::IDENTITY,
                model_changed: true,
                camera_changed: false,
                center,
                deformation: 3
            },
            &mut stats
        ));
        assert!(matrix_eq(expected, retained.object.model));
        assert_eq!(
            retained.depth.to_bits(),
            expected.transform_point3(center).z.to_bits()
        );
        assert_eq!(stats.surface_model_updates, 1);
        let moved_center = center + Vec3::Z;
        assert!(refresh_draw(
            &mut retained,
            &source,
            DynamicInputs {
                view_projection: Mat4::IDENTITY,
                model_changed: false,
                camera_changed: false,
                center: moved_center,
                deformation: 4
            },
            &mut stats
        ));
        assert_eq!(retained.deformation, 4);
        assert_eq!(stats.surface_model_updates, 1);
        assert_eq!(stats.surface_depth_updates, 2);
        assert!(!refresh_draw(
            &mut retained,
            &source,
            DynamicInputs {
                view_projection: Mat4::IDENTITY,
                model_changed: false,
                camera_changed: false,
                center: moved_center,
                deformation: 4
            },
            &mut stats
        ));
        assert_eq!(stats.surface_depth_updates, 2);
    }
    #[test]
    fn removing_a_large_scene_releases_excess_capacity() {
        let mut cache = SurfacePreparation {
            sources: (0..4096).map(|_| source()).collect(),
            ..Default::default()
        };
        cache.changed.resize(4096, true);
        cache.models_changed.resize(4096, true);
        cache.sources.clear();
        cache.changed.clear();
        cache.models_changed.clear();
        cache.compact();
        assert!(cache.capacity_bytes(0) < 64 * mem::size_of::<DrawItem>() + 129);
        cache.clear();
        assert_eq!(cache.capacity_bytes(0), 0);
    }
}
