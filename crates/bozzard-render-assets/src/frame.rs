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
    pub retained_frames: usize,
}

struct Input {
    drawable: Arc<Drawable>,
    shader: Option<Arc<ShaderGraph>>,
    binding: Option<Arc<MaterialInstance>>,
    generated: Option<bozzard_scene::compute::Handle>,
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
#[derive(Default)]
struct State {
    disabled: bool,
    assets: Vec<(String, Option<Arc<AssetData>>)>,
    asset_epoch: Arc<()>,
    buffers: [Vec<Buffer>; 2],
    item_limits: [usize; 2],
    stats: RenderSceneStats,
}
impl State {
    fn update_assets(&mut self, assets: &AssetStore) -> usize {
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
            self.buffers = Default::default();
        }
        if self.assets.capacity() > count.saturating_mul(4).max(64) {
            self.assets.shrink_to_fit();
        }
        changed
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
        state.assets = Vec::new();
        state.asset_epoch = Arc::new(());
        state.stats = Default::default();
    }
    pub fn set_enabled(&self, enabled: bool) {
        let mut state = self.0.lock().unwrap_or_else(|error| error.into_inner());
        state.disabled = !enabled;
        if !enabled {
            state.buffers = Default::default();
            state.assets = Vec::new();
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
        let scan = Instant::now();
        let mut stats = RenderSceneStats {
            assets_changed: if reuse {
                state.update_assets(assets)
            } else {
                0
            },
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
        for ((((model, drawable), motion_id), shader), binding) in view
            .objects
            .iter()
            .zip(&view.object_ids)
            .zip(&view.shader_graphs)
            .zip(&view.material_instances)
        {
            if matches!(drawable.mesh, Mesh::Surface { .. })
                && assets.mesh_surface(&drawable.mesh).is_none()
            {
                continue;
            }
            let generated = view.compute_textures.get(motion_id).copied();
            if valid_assets
                && buffer
                    .inputs
                    .get(count)
                    .is_some_and(|previous| previous.matches(drawable, shader, binding, generated))
            {
                let item = &mut buffer.items[count];
                item.model = *model;
                item.motion_id = *motion_id;
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
                if count < buffer.items.len() {
                    buffer.items[count] = item;
                } else {
                    buffer.items.push(item);
                }
                if reuse {
                    let input = Input {
                        drawable: drawable.clone(),
                        shader: shader.clone(),
                        binding: binding.clone(),
                        generated,
                    };
                    if count < buffer.inputs.len() {
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
        let scene = frame_settings(view, layer, gi, std::mem::take(&mut buffer.items), assets)?;
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
    Ok(DrawItem {
        motion_id,
        model,
        mesh: match drawable.mesh {
            Mesh::Quad => MeshKind::Quad,
            Mesh::Cube => MeshKind::Cube,
            Mesh::Asset(id) => MeshKind::Imported(id),
            Mesh::Surface { asset, index, .. } => MeshKind::ModelPart(asset, index as usize),
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
                    texture: value.texture.map(texture),
                    uv_scale: value.uv_scale,
                    tint: value.tint,
                    metallic: value.metallic,
                    roughness: value.roughness,
                })
                .collect(),
            tint: drawable.color,
            uv_scale: drawable.uv_scale,
            texture: inputs
                .generated
                .map_or_else(|| texture(drawable.texture), TextureKind::Generated),
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
            && assets.mesh_surface(&drawable.mesh).is_none()
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
    frame_settings(view, layer, gi, items, assets)
}

fn frame_settings<D>(
    view: RenderView<D>,
    layer: Layer,
    gi: Option<IrradianceVolume>,
    items: Vec<DrawItem>,
    assets: &AssetStore,
) -> Result<RenderScene> {
    let mut items = items;
    items.extend(crate::sprite_items(&view.sprites)?);
    for (model, text) in view.texts {
        items.push(crate::text_item(model, &text, assets)?);
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
        None => bozzard_assets::gi::is_current(&instance.capture(world)?, assets).unwrap_or(false),
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
