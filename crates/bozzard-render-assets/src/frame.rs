//! Owned native frames whose immutable object payloads return to a bounded pool.
use anyhow::Result;
use bozzard_assets::{AssetData, AssetStore};
use bozzard_render::{DrawItem, IrradianceVolume, Material, MeshKind, RenderScene, TextureKind};
use bozzard_scene::{
    Drawable, Layer, Mesh, RenderView, SceneView, SharedSceneView, Texture,
    material_asset::MaterialInstance, shader_graph::ShaderGraph,
};
use glam::Mat4;
use std::{
    ops::Deref,
    sync::{Arc, Mutex, Weak},
    time::Instant,
};

#[derive(Clone, Copy, Debug, Default)]
pub struct RenderSceneStats {
    /// Adapter wall time, excluding the producer's scene extraction.
    pub adapter_ms: f64,
    pub asset_scan_ms: f64,
    pub material_prepare_ms: f64,
    pub material_reuses: usize,
    pub material_rebuilds: usize,
    pub pooled_items: usize,
    pub assets_changed: usize,
    pub asset_catalog_reads: usize,
    pub retained_frames: usize,
    pub text_descriptor_reuses: usize,
    pub text_descriptor_rebuilds: usize,
}

struct Input {
    motion_id: u64,
    drawable: Arc<Drawable>,
    shader: Option<Arc<ShaderGraph>>,
    binding: Option<Arc<MaterialInstance>>,
    generated: Option<bozzard_scene::compute::Handle>,
    // The host/catalog owns snapshots. Frozen draw frames keep only guards:
    // their metadata must not extend asset lifetime after the host closes.
    dependencies: Vec<(String, Option<Weak<AssetData>>)>,
}
impl Input {
    fn matches(
        &self,
        drawable: &Arc<Drawable>,
        shader: &Option<Arc<ShaderGraph>>,
        binding: &Option<Arc<MaterialInstance>>,
        generated: Option<bozzard_scene::compute::Handle>,
    ) -> bool {
        Arc::ptr_eq(&self.drawable, drawable)
            && same_arc(&self.shader, shader)
            && same_arc(&self.binding, binding)
            && self.generated == generated
    }
    fn assets_match(&self, assets: &AssetStore) -> bool {
        self.dependencies.iter().all(|(id, previous)| {
            let next = assets
                .handle(id)
                .and_then(|handle| assets.get(handle))
                .and_then(|entry| entry.data());
            match previous {
                None => next.is_none(),
                Some(previous) => next.is_some_and(|next| {
                    previous
                        .upgrade()
                        .is_some_and(|previous| std::ptr::eq(previous.as_ref(), next))
                }),
            }
        })
    }
}

fn dependencies(
    drawable: &Drawable,
    binding: Option<&MaterialInstance>,
    item: &DrawItem,
    assets: &AssetStore,
) -> Vec<(String, Option<Weak<AssetData>>)> {
    let mut names = std::collections::BTreeSet::new();
    if let Mesh::Asset(id) | Mesh::Surface { asset: id, .. } = &drawable.mesh {
        names.insert(id.as_str());
    }
    if let Texture::Asset(id) = &drawable.texture {
        names.insert(id.as_str());
    }
    for item in &drawable.material_overrides {
        if let Some(Texture::Asset(id)) = &item.texture {
            names.insert(id.as_str());
        }
    }
    if let Some(binding) = binding {
        names.insert(binding.asset.as_str());
    }
    for texture in std::iter::once(&item.material.texture).chain(
        item.material
            .surface_overrides
            .iter()
            .filter_map(|item| item.texture.as_ref()),
    ) {
        if let TextureKind::Imported(id) | TextureKind::ModelPart(id, _) = texture {
            names.insert(id.as_str());
        }
    }
    names
        .into_iter()
        .map(|id| {
            (
                id.to_owned(),
                assets
                    .handle(id)
                    .and_then(|handle| assets.get(handle))
                    .and_then(|entry| entry.shared_data())
                    .map(|data| Arc::downgrade(&data)),
            )
        })
        .collect()
}
fn same_arc<T>(a: &Option<Arc<T>>, b: &Option<Arc<T>>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => Arc::ptr_eq(a, b),
        (None, None) => true,
        _ => false,
    }
}

fn same_asset_data(a: Option<&AssetData>, b: Option<&AssetData>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => std::ptr::eq(a, b),
        (None, None) => true,
        _ => false,
    }
}

#[derive(Default)]
struct Buffer {
    items: Vec<DrawItem>,
    inputs: Vec<Input>,
    assets: Option<Arc<()>>,
}
struct TextPayload {
    source: Arc<bozzard_scene::TextRendering>,
    dependencies: Vec<(String, Option<Arc<AssetData>>)>,
    mesh: Arc<bozzard_render::TextMesh>,
    used: u64,
}

#[derive(Default)]
struct State {
    disabled: bool,
    assets: Vec<(String, Option<Arc<AssetData>>)>,
    asset_epoch: Arc<()>,
    publication: Option<Arc<()>>,
    buffers: [Vec<Buffer>; 2],
    item_limits: [usize; 2],
    stats: RenderSceneStats,
    numeric_mode: Option<bool>,
    text_payloads: std::collections::HashMap<usize, TextPayload>,
    text_generation: u64,
    text_reuses: usize,
    text_rebuilds: usize,
}
impl State {
    fn shared_text(
        &mut self,
        model: Mat4,
        source: Arc<bozzard_scene::TextRendering>,
        assets: &AssetStore,
    ) -> Result<DrawItem> {
        let key = Arc::as_ptr(&source) as usize;
        let mesh = self
            .text_payloads
            .get_mut(&key)
            .filter(|entry| {
                Arc::ptr_eq(&entry.source, &source)
                    && entry.dependencies.iter().all(|(id, previous)| {
                        let current = assets
                            .handle(id)
                            .and_then(|h| assets.get(h))
                            .and_then(|e| e.data());
                        same_asset_data(previous.as_deref(), current)
                    })
            })
            .map(|entry| {
                entry.used = self.text_generation;
                entry.mesh.clone()
            });
        let mesh = match mesh {
            Some(mesh) => {
                self.text_reuses += 1;
                mesh
            }
            None => {
                self.text_rebuilds += 1;
                let mesh = Arc::new(crate::text_mesh(&source, assets)?);
                let mut ids = std::collections::BTreeSet::new();
                if let bozzard_scene::TextFont::Custom(id) = &source.font {
                    ids.insert(id.as_str());
                }
                ids.extend(source.font_fallbacks.iter().map(String::as_str));
                let dependencies = ids
                    .into_iter()
                    .map(|id| {
                        (
                            id.to_owned(),
                            assets
                                .handle(id)
                                .and_then(|h| assets.get(h))
                                .and_then(|e| e.shared_data()),
                        )
                    })
                    .collect();
                self.text_payloads.insert(
                    key,
                    TextPayload {
                        source: source.clone(),
                        dependencies,
                        mesh: mesh.clone(),
                        used: self.text_generation,
                    },
                );
                mesh
            }
        };
        Ok(DrawItem {
            motion_id: 0,
            model,
            mesh: MeshKind::SharedText(mesh),
            material: Material {
                metallic: None,
                roughness: None,
                surface_overrides: Default::default(),
                tint: [source.color[0], source.color[1], source.color[2]],
                uv_scale: [1.; 2],
                texture: TextureKind::Text,
                lit: false,
                shader: None,
            },
        })
    }
    fn update_assets(&mut self, assets: &AssetStore) -> (usize, usize) {
        if self
            .publication
            .as_ref()
            .is_some_and(|previous| Arc::ptr_eq(previous, assets.publication_identity()))
        {
            return (0, 0);
        }
        let mut changed = 0;
        let mut count = 0;
        for (index, entry) in assets.entries().enumerate() {
            count += 1;
            match self.assets.get_mut(index) {
                Some((id, previous))
                    if *id == entry.id && same_asset_data(previous.as_deref(), entry.data()) => {}
                Some(slot) => {
                    *slot = (entry.id.clone(), entry.shared_data());
                    changed += 1;
                }
                None => {
                    self.assets.push((entry.id.clone(), entry.shared_data()));
                    changed += 1;
                }
            }
        }
        changed += self.assets.len().saturating_sub(count);
        self.assets.truncate(count);
        if changed != 0 {
            // Holding the previous token prevents pointer reuse while old frames live.
            // Cloned asset stores may diverge with identical numeric revisions.
            self.asset_epoch = Arc::new(());
        }
        self.publication = Some(assets.publication_identity().clone());
        if self.assets.capacity() > count.saturating_mul(4).max(64) {
            self.assets.shrink_to_fit();
        }
        (changed, count)
    }
}

/// A native host retains this cache; a frozen frame never borrows a live world.
#[derive(Default)]
pub struct RenderSceneCache(Arc<Mutex<State>>);
impl RenderSceneCache {
    /// Release pooled payloads and invalidate frames that are still in flight.
    pub fn clear(&self) {
        let mut state = self.0.lock().unwrap_or_else(|error| error.into_inner());
        state.buffers = Default::default();
        state.text_payloads.clear();
        state.assets = Vec::new();
        state.publication = None;
        state.asset_epoch = Arc::new(());
        state.stats = Default::default();
    }
    pub fn set_enabled(&self, enabled: bool) {
        let mut state = self.0.lock().unwrap_or_else(|error| error.into_inner());
        state.disabled = !enabled;
        if !enabled {
            state.buffers = Default::default();
            state.text_payloads.clear();
            state.assets = Vec::new();
            state.publication = None;
            state.asset_epoch = Arc::new(());
        }
    }
    pub fn enabled(&self) -> bool {
        !self
            .0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .disabled
    }
    pub fn stats(&self) -> RenderSceneStats {
        self.0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .stats
    }
    pub fn extract(
        &self,
        view: SharedSceneView,
        assets: &AssetStore,
        layer: Layer,
        gi: Option<IrradianceVolume>,
    ) -> Result<RenderFrame> {
        let started = Instant::now();
        let slot = layer_index(layer);
        let mut state = self.0.lock().unwrap_or_else(|error| error.into_inner());
        let reuse = !state.disabled;
        state.text_generation = state
            .text_generation
            .checked_add(1)
            .expect("text frame generation exhausted");
        state.text_reuses = 0;
        state.text_rebuilds = 0;
        let numeric_mode = crate::shaders::parameterization_enabled();
        if state.numeric_mode != Some(numeric_mode) {
            state.buffers = Default::default();
            state.asset_epoch = Arc::new(());
            state.numeric_mode = Some(numeric_mode);
        }
        let scan = Instant::now();
        let (assets_changed, asset_catalog_reads) = if reuse {
            state.update_assets(assets)
        } else {
            (0, 0)
        };
        let mut stats = RenderSceneStats {
            assets_changed,
            asset_catalog_reads,
            ..Default::default()
        };
        stats.asset_scan_ms = scan.elapsed().as_secs_f64() * 1000.;
        let mut buffer = if !reuse {
            Buffer::default()
        } else {
            state.buffers[slot].pop().unwrap_or_default()
        };
        let valid_assets = buffer
            .assets
            .as_ref()
            .is_some_and(|epoch| Arc::ptr_eq(epoch, &state.asset_epoch));
        let materials = Instant::now();
        let mut count = 0;
        // Allocate remapping only after an actual immutable-source mismatch.
        // Moving/inserted rows keep existing material payloads rather than
        // cascading every later positional cache entry into a rebuild.
        let mut remap: Option<Option<std::collections::HashMap<u64, usize>>> = None;
        for ((((model, drawable), motion_id), shader), binding) in view
            .objects
            .iter()
            .zip(&view.object_ids)
            .zip(&view.shader_graphs)
            .zip(&view.material_instances)
        {
            if matches!(drawable.mesh, Mesh::Surface { .. })
                && assets.mesh_surface_binding(&drawable.mesh).is_none()
            {
                continue;
            }
            let generated = view.compute_textures.get(motion_id).copied();
            let prefix_matches = buffer.inputs.get(count).is_some_and(|previous| {
                previous.matches(drawable, shader, binding, generated)
                    && (valid_assets || previous.assets_match(assets))
            });
            if reuse && !prefix_matches && !buffer.inputs.is_empty() {
                let mapping = remap.get_or_insert_with(|| {
                    let mut mapping = std::collections::HashMap::with_capacity(buffer.inputs.len());
                    for (index, input) in buffer.inputs.iter().enumerate() {
                        if input.motion_id == 0 || mapping.insert(input.motion_id, index).is_some()
                        {
                            return None;
                        }
                    }
                    let mut ids = std::collections::HashSet::with_capacity(view.object_ids.len());
                    if view
                        .object_ids
                        .iter()
                        .any(|id| *id == 0 || !ids.insert(*id))
                    {
                        return None;
                    }
                    Some(mapping)
                });
                if let Some(mapping) = mapping
                    && let Some(&previous_index) = mapping.get(motion_id)
                    && previous_index > count
                    && buffer.inputs[previous_index].matches(drawable, shader, binding, generated)
                    && (valid_assets || buffer.inputs[previous_index].assets_match(assets))
                {
                    buffer.inputs.swap(count, previous_index);
                    buffer.items.swap(count, previous_index);
                    mapping.insert(buffer.inputs[count].motion_id, count);
                    mapping.insert(buffer.inputs[previous_index].motion_id, previous_index);
                }
            }
            if prefix_matches
                || buffer.inputs.get(count).is_some_and(|previous| {
                    previous.matches(drawable, shader, binding, generated)
                        && (valid_assets || previous.assets_match(assets))
                })
            {
                let item = &mut buffer.items[count];
                item.model = *model;
                item.motion_id = *motion_id;
                buffer.inputs[count].motion_id = *motion_id;
                stats.material_reuses += 1;
            } else {
                let item = drawable_item(
                    *model,
                    *motion_id,
                    drawable.as_ref().clone(),
                    MaterialInputs {
                        shader: shader.as_deref(),
                        binding: binding.as_deref(),
                        generated,
                    },
                    assets,
                    layer,
                )?;
                // Reserve once for a cold buffer, after an eligible row succeeds.
                // Invalid Surface rows can leave the frame empty; they must not
                // allocate and then compact a full-size vector every frame.
                if buffer.items.capacity() == 0 {
                    buffer.items.reserve(view.objects.len());
                    if reuse {
                        buffer.inputs.reserve(view.objects.len());
                    }
                }
                let displaced = remap.as_ref().is_some_and(|mapping| mapping.is_some())
                    && count < buffer.inputs.len();
                if displaced {
                    buffer.items.push(item);
                    let last = buffer.items.len() - 1;
                    buffer.items.swap(count, last);
                } else if count < buffer.items.len() {
                    buffer.items[count] = item;
                } else {
                    buffer.items.push(item);
                }
                if reuse {
                    let dependencies =
                        dependencies(drawable, binding.as_deref(), &buffer.items[count], assets);
                    let input = Input {
                        motion_id: *motion_id,
                        drawable: drawable.clone(),
                        shader: shader.clone(),
                        binding: binding.clone(),
                        generated,
                        dependencies,
                    };
                    if displaced {
                        buffer.inputs.push(input);
                        let last = buffer.inputs.len() - 1;
                        buffer.inputs.swap(count, last);
                        if let Some(Some(mapping)) = &mut remap {
                            mapping.insert(buffer.inputs[last].motion_id, last);
                            mapping.insert(*motion_id, count);
                        }
                    } else if count < buffer.inputs.len() {
                        buffer.inputs[count] = input;
                    } else {
                        buffer.inputs.push(input);
                    }
                }
                stats.material_rebuilds += 1;
            }
            count += 1;
        }
        buffer.items.truncate(count);
        buffer.inputs.truncate(count);
        let limit = count.saturating_mul(4).max(64);
        state.item_limits[slot] = limit;
        state.buffers[slot].retain(|buffer| buffer.inputs.len() <= limit);
        stats.material_prepare_ms = materials.elapsed().as_secs_f64() * 1000.;
        stats.pooled_items = stats.material_reuses;
        buffer.assets = reuse.then(|| state.asset_epoch.clone());
        let scene = frame_settings(
            view,
            layer,
            gi,
            std::mem::take(&mut buffer.items),
            assets,
            reuse.then_some(&mut *state),
        )?;
        stats.text_descriptor_reuses = state.text_reuses;
        stats.text_descriptor_rebuilds = state.text_rebuilds;
        let generation = state.text_generation;
        state
            .text_payloads
            .retain(|_, entry| entry.used >= generation.saturating_sub(1));
        let text_limit = state.text_payloads.len().max(64);
        if state.text_payloads.capacity() > text_limit.saturating_mul(4) {
            state.text_payloads.shrink_to(text_limit.saturating_mul(2));
        }
        stats.adapter_ms = started.elapsed().as_secs_f64() * 1000.;
        stats.retained_frames = state.buffers.iter().map(Vec::len).sum();
        state.stats = stats;
        let owner = reuse.then(|| Arc::downgrade(&self.0));
        drop(state);
        Ok(RenderFrame {
            scene: Some(scene),
            buffer,
            owner,
            layer: slot,
            stats,
        })
    }
    pub fn reference(
        &self,
        view: SceneView,
        assets: &AssetStore,
        layer: Layer,
        gi: Option<IrradianceVolume>,
    ) -> Result<RenderFrame> {
        let started = Instant::now();
        let scene = render_scene(view, assets, layer, gi)?;
        let stats = RenderSceneStats {
            adapter_ms: started.elapsed().as_secs_f64() * 1000.,
            material_rebuilds: scene.items.len(),
            ..Default::default()
        };
        self.0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .stats = stats;
        // Reference frames do not populate the pool.
        Ok(RenderFrame {
            scene: Some(scene),
            buffer: Buffer::default(),
            owner: None,
            layer: layer_index(layer),
            stats,
        })
    }
}

/// Immutable object payloads stay owned until this frame finishes. Dropping it
/// recycles its drawable prefix; temporary sprites, world text, and HUD items drop.
pub struct RenderFrame {
    scene: Option<RenderScene>,
    buffer: Buffer,
    owner: Option<Weak<Mutex<State>>>,
    layer: usize,
    stats: RenderSceneStats,
}
impl Deref for RenderFrame {
    type Target = RenderScene;
    fn deref(&self) -> &RenderScene {
        self.scene.as_ref().expect("live render frame")
    }
}
impl RenderFrame {
    pub fn stats(&self) -> RenderSceneStats {
        self.stats
    }
    /// Transient overlays append after the reusable immutable drawable prefix.
    pub fn append_items(&mut self, items: impl IntoIterator<Item = DrawItem>) {
        self.scene
            .as_mut()
            .expect("live render frame")
            .items
            .extend(items);
    }
    pub fn clear_gi(&mut self) {
        self.scene.as_mut().expect("live render frame").gi = None;
    }
    pub fn set_view_projection(&mut self, projection: Mat4) {
        self.scene
            .as_mut()
            .expect("live render frame")
            .view_projection = projection;
    }
    pub fn set_display(&mut self, display: bozzard_render::DisplaySettings) {
        self.scene.as_mut().expect("live render frame").display = display;
    }
    pub fn bypass_effects(&mut self) {
        let scene = self.scene.as_mut().expect("live render frame");
        scene.display = Default::default();
        scene.particles.clear();
        scene.fog.enabled = false;
    }
    pub fn set_shader_time(&mut self, time: f32) {
        self.scene.as_mut().expect("live render frame").shader_time = time;
    }
    /// Consume a frame for callers needing unrestricted mutation. Its storage is not recycled.
    pub fn into_scene(mut self) -> RenderScene {
        self.owner = None;
        self.scene.take().expect("live render frame")
    }
}
impl Drop for RenderFrame {
    fn drop(&mut self) {
        // Frames own their contents; a closed host has no useful pool to retain.
        let Some(owner) = self.owner.take().and_then(|owner| owner.upgrade()) else {
            return;
        };
        let mut state = owner.lock().unwrap_or_else(|error| error.into_inner());
        if state.disabled
            || state.buffers[self.layer].len() >= 2
            || self.buffer.inputs.len() > state.item_limits[self.layer]
            || !self
                .buffer
                .assets
                .as_ref()
                .is_some_and(|epoch| Arc::ptr_eq(epoch, &state.asset_epoch))
        {
            return;
        }
        let mut scene = self.scene.take().expect("live render frame");
        scene.items.truncate(self.buffer.inputs.len());
        self.buffer.items = std::mem::take(&mut scene.items);
        // Large streamed-out scenes must not retain their former frame capacity.
        let capacity = self.buffer.items.len().saturating_mul(2).max(64);
        if self.buffer.items.capacity() > capacity {
            self.buffer.items.shrink_to_fit();
        }
        if self.buffer.inputs.capacity() > capacity {
            self.buffer.inputs.shrink_to_fit();
        }
        state.buffers[self.layer].push(std::mem::take(&mut self.buffer));
    }
}
fn layer_index(layer: Layer) -> usize {
    match layer {
        Layer::TwoD => 0,
        Layer::ThreeD => 1,
    }
}
fn texture(texture: Texture) -> TextureKind {
    match texture {
        Texture::White => TextureKind::White,
        Texture::Checker => TextureKind::Checker,
        Texture::Normals => TextureKind::Normals,
        Texture::ProceduralChecker => TextureKind::ProceduralChecker,
        Texture::Toon => TextureKind::Toon,
        Texture::Asset(id) => TextureKind::Imported(id),
    }
}
struct MaterialInputs<'a> {
    shader: Option<&'a ShaderGraph>,
    binding: Option<&'a MaterialInstance>,
    generated: Option<bozzard_scene::compute::Handle>,
}
fn drawable_item(
    model: Mat4,
    motion_id: u64,
    mut drawable: Drawable,
    inputs: MaterialInputs<'_>,
    assets: &AssetStore,
    layer: Layer,
) -> Result<DrawItem> {
    let shader = crate::material_binding(&mut drawable, inputs.binding, inputs.shader, assets)?;
    let canonical_texture = |value: Texture| match value {
        Texture::Asset(id) => TextureKind::Imported(assets.canonical_asset_id(&id).to_owned()),
        other => texture(other),
    };
    Ok(DrawItem {
        motion_id,
        model,
        mesh: match drawable.mesh {
            Mesh::Quad => MeshKind::Quad,
            Mesh::Cube => MeshKind::Cube,
            Mesh::Asset(id) => MeshKind::Imported(assets.canonical_asset_id(&id).to_owned()),
            Mesh::Surface { asset, index, .. } => {
                MeshKind::ModelPart(assets.canonical_asset_id(&asset).to_owned(), index as usize)
            }
        },
        material: Material {
            metallic: drawable.metallic,
            roughness: drawable.roughness,
            surface_overrides: drawable
                .material_overrides
                .into_iter()
                .map(|value| bozzard_render::SurfaceMaterialOverride {
                    surface: value.surface,
                    source: value.source,
                    transform: value.transform.matrix(),
                    texture: value.texture.map(canonical_texture),
                    uv_scale: value.uv_scale,
                    tint: value.tint,
                    metallic: value.metallic,
                    roughness: value.roughness,
                })
                .collect(),
            tint: drawable.color,
            uv_scale: drawable.uv_scale,
            texture: inputs.generated.map_or_else(
                || canonical_texture(drawable.texture),
                TextureKind::Generated,
            ),
            lit: layer == Layer::ThreeD,
            shader,
        },
    })
}

/// Owned conversion remains available as an independent cache-free reference.
pub fn render_scene(
    mut view: SceneView,
    assets: &AssetStore,
    layer: Layer,
    gi: Option<IrradianceVolume>,
) -> Result<RenderScene> {
    let mut items = Vec::with_capacity(view.objects.len());
    for ((((model, drawable), motion_id), shader), binding) in std::mem::take(&mut view.objects)
        .into_iter()
        .zip(std::mem::take(&mut view.object_ids))
        .zip(std::mem::take(&mut view.shader_graphs))
        .zip(std::mem::take(&mut view.material_instances))
    {
        if matches!(drawable.mesh, Mesh::Surface { .. })
            && assets.mesh_surface_binding(&drawable.mesh).is_none()
        {
            continue;
        }
        items.push(drawable_item(
            model,
            motion_id,
            drawable,
            MaterialInputs {
                shader: shader.as_deref(),
                binding: binding.as_deref(),
                generated: view.compute_textures.get(&motion_id).copied(),
            },
            assets,
            layer,
        )?);
    }
    frame_settings(view, layer, gi, items, assets, None)
}

fn frame_settings<D>(
    view: RenderView<D>,
    layer: Layer,
    gi: Option<IrradianceVolume>,
    items: Vec<DrawItem>,
    assets: &AssetStore,
    mut cache: Option<&mut State>,
) -> Result<RenderScene> {
    let mut items = items;
    items.extend(crate::sprite_items_resolved(&view.sprites, |id| {
        assets.canonical_asset_id(id).to_owned()
    })?);
    for (model, text) in view.texts {
        items.push(crate::text_item(model, &text, assets)?);
    }
    for (model, text) in view.shared_texts {
        items.push(match cache.as_deref_mut() {
            Some(cache) => cache.shared_text(model, text, assets)?,
            None => crate::text_item(model, &text, assets)?,
        });
    }
    Ok(RenderScene {
        skin_poses: crate::skin_poses(&view.skin_poses),
        particles: crate::particle_frame(&view.particles),
        fog: bozzard_render::FogSettings {
            enabled: layer == Layer::ThreeD && view.fog.enabled,
            color: view.fog.color,
            distance_density: view.fog.distance_density,
            start_distance: view.fog.start_distance,
            height_density: view.fog.height_density,
            base_height: view.fog.base_height,
            height_falloff: view.fog.height_falloff,
        },
        gi,
        lights: view
            .lights
            .iter()
            .map(|world| bozzard_render::LocalLight {
                directional: world.light.kind == bozzard_scene::LightKind::Directional,
                shadows: world.light.requests_shadow_map().then_some(
                    bozzard_render::LocalShadowSettings {
                        bias: world.light.shadow_bias,
                        normal_bias: world.light.shadow_normal_bias,
                    },
                ),
                position: world.position,
                direction: world.direction,
                color: world.light.color,
                intensity: world.light.intensity,
                range: world.light.range,
                spot_angles: (world.light.kind == bozzard_scene::LightKind::Spot).then_some([
                    world.light.inner_angle_degrees,
                    world.light.outer_angle_degrees,
                ]),
            })
            .collect(),
        environment: bozzard_render::EnvironmentSettings {
            zenith: view.environment.zenith,
            horizon: view.environment.horizon,
            ground: view.environment.ground,
            star_intensity: view.environment.star_intensity,
            intensity: if layer == Layer::ThreeD {
                view.environment.intensity
            } else {
                0.
            },
            background: layer == Layer::ThreeD && view.environment.background,
        },
        display: crate::display_settings(view.display, layer, view.display_time),
        lighting: bozzard_render::Lighting {
            shadows: view.lighting.shadows,
            shadow_resolution: view.lighting.shadow_resolution,
            shadow_bias: view.lighting.shadow_bias,
            shadow_normal_bias: view.lighting.shadow_normal_bias,
            sun_direction: view.lighting.sun_direction,
            sun_color: view.lighting.sun_color,
            sun_intensity: view.lighting.sun_intensity,
            ambient_color: view.lighting.ambient_color,
            ambient_intensity: view.lighting.ambient_intensity,
        },
        view_projection: view.view_projection,
        items,
        shader_time: view.display_time,
    })
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GiFreshnessStats {
    pub reuses: usize,
    pub rebuilds: usize,
}
pub fn gi_freshness_stats() -> GiFreshnessStats {
    GI_STATS.with(|stats| *stats.borrow())
}
struct GiFreshnessEntry {
    identity: Arc<()>,
    world: (u64, u64),
    publication: Arc<()>,
    authored_revision: u64,
    current: bool,
}
thread_local! {
    static GI_STATS: std::cell::RefCell<GiFreshnessStats> = const { std::cell::RefCell::new(GiFreshnessStats { reuses: 0, rebuilds: 0 }) };
    static GI_FRESHNESS: std::cell::RefCell<Vec<GiFreshnessEntry>> = const { std::cell::RefCell::new(Vec::new()) };
}
fn live_gi_current(
    instance: &bozzard_scene::SceneInstance,
    world: &bozzard_ecs::World,
    assets: &AssetStore,
) -> Result<bool> {
    GI_FRESHNESS.with(|cache| {
        let mut cache = cache.borrow_mut();
        let identity = instance.render_cache_identity();
        let revision = world.component_mutation_revision();
        if let Some(entry) = cache.iter().find(|entry| {
            Arc::ptr_eq(&entry.identity, identity)
                && entry.world == revision
                && Arc::ptr_eq(&entry.publication, assets.publication_identity())
                && entry.authored_revision == instance.authored_revision()
        }) {
            GI_STATS.with(|stats| stats.borrow_mut().reuses += 1);
            return Ok(entry.current);
        }
        // Capture remains the independent oracle, including validation. Errors
        // never replace a last-good bookmark or conceal a later repair.
        let current =
            bozzard_assets::gi::is_current(&instance.capture(world)?, assets).unwrap_or(false);
        GI_STATS.with(|stats| stats.borrow_mut().rebuilds += 1);
        cache.retain(|entry| !Arc::ptr_eq(&entry.identity, identity));
        if cache.len() == 8 {
            cache.remove(0);
        }
        cache.push(GiFreshnessEntry {
            identity: identity.clone(),
            world: revision,
            publication: assets.publication_identity().clone(),
            authored_revision: instance.authored_revision(),
            current,
        });
        Ok(current)
    })
}

/// Preserve authored/Edit and live/Play GI freshness rules before preparing a frame.
pub fn irradiance_volume(
    instance: &bozzard_scene::SceneInstance,
    world: &bozzard_ecs::World,
    assets: &AssetStore,
    layer: Layer,
    authored_current: Option<bool>,
) -> Result<Option<IrradianceVolume>> {
    let document = instance.document();
    if layer != Layer::ThreeD || !document.gi.enabled || document.gi.baked.is_none() {
        return Ok(None);
    }
    let current = match authored_current {
        Some(current) => current,
        None => live_gi_current(instance, world, assets)?,
    };
    if !current {
        return Ok(None);
    }
    let baked = document.gi.baked.as_ref().expect("baked GI was checked");
    Ok(Some(IrradianceVolume {
        min: baked.volume.min,
        max: baked.volume.max,
        resolution: baked.volume.resolution,
        intensity: document.gi.intensity,
        normal_bias: document.gi.normal_bias,
        probes: baked.probes.clone(),
    }))
}
