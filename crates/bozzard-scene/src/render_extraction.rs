//! Immutable render payloads retained independently of the world and simulation.
use super::*;
use std::sync::{Arc, Mutex, MutexGuard};

/// Work performed by the most recent shared scene extraction. Reuse compares
/// values, so component writes that bypass ECS change tracking remain visible.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RenderExtractionStats {
    pub drawable_reuses: usize,
    pub drawable_rebuilds: usize,
    pub shader_graph_reuses: usize,
    pub shader_graph_rebuilds: usize,
}

#[derive(Default)]
pub(super) struct Cache(Mutex<CachedPayloads>);
impl Clone for Cache {
    fn clone(&self) -> Self {
        // Edit/Play and cloned instances never share mutable presentation state.
        Self::default()
    }
}
impl Cache {
    pub fn lock(&self) -> Result<MutexGuard<'_, CachedPayloads>> {
        self.0
            .lock()
            .map_err(|_| anyhow::anyhow!("render extraction cache lock poisoned"))
    }
}

#[derive(Default)]
pub(super) struct CachedPayloads {
    // Logic/HUD-only entities never allocate a payload slot. Entity generation
    // keys also preserve unaffected payloads when prefab removal compacts indices.
    slots: std::collections::HashMap<Entity, Slot>,
    pub render_entities: Vec<usize>,
    pub drawable_count: usize,
    pub light_entities: Vec<usize>,
    candidates: Vec<usize>,
    previous_candidates: Vec<usize>,
    light_candidates: Vec<usize>,
    previous_light_candidates: Vec<usize>,
    revision: Option<u64>,
    pub stats: RenderExtractionStats,
}
#[derive(Default)]
struct Slot {
    drawable: Option<DrawableEntry>,
    graph: Option<Arc<shader_graph::ShaderGraph>>,
}
struct DrawableEntry {
    source: Drawable,
    material: Option<Material>,
    mesh: Option<Mesh>,
    factor: f32,
    rendered: Arc<Drawable>,
}

fn compact_membership(items: &mut Vec<usize>, active: usize) {
    let retained = active.max(64);
    if items.capacity() > retained.saturating_mul(4) {
        items.shrink_to(retained);
    }
}

impl CachedPayloads {
    pub fn prepare(&mut self, scene: &SceneInstance, world: &World) {
        self.stats = Default::default();
        // Only hierarchy rebuilds change scene membership; component removal is live.
        let revised = self.revision != Some(scene.hierarchy_revision);
        self.slots.retain(|&entity, slot| {
            if (revised && !scene.object_indices.contains_key(&entity))
                || world.get::<Drawable>(entity).is_none()
            {
                return false;
            }
            if slot.graph.is_some() && world.get::<shader_graph::ShaderGraph>(entity).is_none() {
                slot.graph = None;
            }
            true
        });
        // Release oversized hash buckets after large additive worlds are unloaded.
        let retained = self.slots.len().max(64);
        if self.slots.capacity() > retained.saturating_mul(4) {
            self.slots.shrink_to(retained.saturating_mul(2));
        }
    }

    pub fn refresh_membership(&mut self, scene: &SceneInstance, world: &World, layer: Layer) {
        let revised = self.revision != Some(scene.hierarchy_revision);
        self.candidates.clear();
        self.candidates.extend(
            world
                .query::<Drawable>()
                .filter_map(|(entity, _)| scene.object_indices.get(&entity).copied()),
        );
        // Text-only rows participate in ordering, but cannot use mesh snapshot storage.
        self.drawable_count = self.candidates.len();
        self.candidates.extend(
            world
                .query::<TextRendering>()
                .filter_map(|(entity, _)| scene.object_indices.get(&entity).copied()),
        );
        let changed = revised || self.candidates != self.previous_candidates;
        if changed {
            self.render_entities.clear();
            self.render_entities.extend_from_slice(&self.candidates);
            self.render_entities.sort_unstable_by(|&a, &b| {
                scene.document.objects[a]
                    .id
                    .cmp(&scene.document.objects[b].id)
            });
            self.render_entities.dedup();
        }
        std::mem::swap(&mut self.candidates, &mut self.previous_candidates);
        if changed {
            let active = self.previous_candidates.len();
            let selected = self.render_entities.len();
            self.candidates.clear();
            compact_membership(&mut self.candidates, active);
            compact_membership(&mut self.previous_candidates, active);
            compact_membership(&mut self.render_entities, selected);
        }
        if layer == Layer::ThreeD {
            self.light_candidates.clear();
            self.light_candidates.extend(
                world
                    .query::<Light>()
                    .filter_map(|(entity, _)| scene.object_indices.get(&entity).copied()),
            );
            let changed = revised || self.light_candidates != self.previous_light_candidates;
            if changed {
                self.light_entities.clear();
                self.light_entities
                    .extend_from_slice(&self.light_candidates);
                self.light_entities.sort_unstable_by(|&a, &b| {
                    scene.document.objects[a]
                        .id
                        .cmp(&scene.document.objects[b].id)
                });
            }
            std::mem::swap(
                &mut self.light_candidates,
                &mut self.previous_light_candidates,
            );
            if changed {
                let active = self.previous_light_candidates.len();
                let selected = self.light_entities.len();
                self.light_candidates.clear();
                compact_membership(&mut self.light_candidates, active);
                compact_membership(&mut self.previous_light_candidates, active);
                compact_membership(&mut self.light_entities, selected);
            }
        } else if revised {
            // A 2D extraction can precede 3D after a membership change.
            self.light_entities.clear();
            self.light_candidates.clear();
            self.previous_light_candidates.clear();
            compact_membership(&mut self.light_entities, 0);
            compact_membership(&mut self.light_candidates, 0);
            compact_membership(&mut self.previous_light_candidates, 0);
        }
        self.revision = Some(scene.hierarchy_revision);
    }

    pub fn graph(
        &mut self,
        entity: Entity,
        graph: Option<&shader_graph::ShaderGraph>,
    ) -> Option<Arc<shader_graph::ShaderGraph>> {
        let slot = self
            .slots
            .get_mut(&entity)
            .expect("resolved drawable payload");
        match graph {
            Some(graph) => {
                if slot
                    .graph
                    .as_ref()
                    .is_some_and(|cached| graph_equal(cached, graph))
                {
                    self.stats.shader_graph_reuses += 1;
                } else {
                    slot.graph = Some(Arc::new(graph.clone()));
                    self.stats.shader_graph_rebuilds += 1;
                }
                slot.graph.clone()
            }
            None => {
                slot.graph = None;
                None
            }
        }
    }
}

pub(super) trait Payload: Sized {
    const SHARED: bool;
    fn resolve(cache: &mut CachedPayloads, input: DrawableInput<'_>) -> Self;
}

pub(super) struct DrawableInput<'a> {
    pub entity: Entity,
    pub source: &'a Drawable,
    pub material: Option<&'a Material>,
    pub mesh: Option<&'a Mesh>,
    pub factor: f32,
}

fn resolve_drawable(
    source: &Drawable,
    material: Option<&Material>,
    mesh: Option<&Mesh>,
    factor: f32,
) -> Drawable {
    let mut drawable = source.clone();
    if let Some(mesh) = mesh {
        // Surface overrides belong to the source model, not a replacement LOD.
        drawable.material_overrides.clear();
        drawable.mesh = mesh.clone();
    }
    if let Some(material) = material {
        material.apply(&mut drawable);
    }
    for channel in &mut drawable.color {
        *channel *= factor;
    }
    drawable
}

impl Payload for Drawable {
    const SHARED: bool = false;
    fn resolve(_: &mut CachedPayloads, input: DrawableInput<'_>) -> Self {
        resolve_drawable(input.source, input.material, input.mesh, input.factor)
    }
}

impl Payload for Arc<Drawable> {
    const SHARED: bool = true;
    fn resolve(cache: &mut CachedPayloads, input: DrawableInput<'_>) -> Self {
        let DrawableInput {
            entity,
            source,
            material,
            mesh,
            factor,
        } = input;
        let slot = cache.slots.entry(entity).or_default();
        if let Some(entry) = &slot.drawable
            && drawable_equal(&entry.source, source)
            && material_equal(entry.material.as_ref(), material)
            && entry.mesh.as_ref() == mesh
            && entry.factor.to_bits() == factor.to_bits()
        {
            cache.stats.drawable_reuses += 1;
            return entry.rendered.clone();
        }
        let rendered = Arc::new(resolve_drawable(source, material, mesh, factor));
        slot.drawable = Some(DrawableEntry {
            source: source.clone(),
            material: material.cloned(),
            mesh: mesh.cloned(),
            factor,
            rendered: rendered.clone(),
        });
        cache.stats.drawable_rebuilds += 1;
        rendered
    }
}

pub(super) fn floats_equal(a: &[f32], b: &[f32]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(a, b)| a.to_bits() == b.to_bits())
}
pub(super) fn transform_equal(a: Transform, b: Transform) -> bool {
    floats_equal(&a.translation, &b.translation)
        && floats_equal(&a.rotation_degrees, &b.rotation_degrees)
        && floats_equal(&a.scale, &b.scale)
}
fn optional_float_equal(a: Option<f32>, b: Option<f32>) -> bool {
    a.map(f32::to_bits) == b.map(f32::to_bits)
}
fn drawable_equal(a: &Drawable, b: &Drawable) -> bool {
    a.layer == b.layer
        && a.mesh == b.mesh
        && a.texture == b.texture
        && a.gi_static == b.gi_static
        && optional_float_equal(a.metallic, b.metallic)
        && optional_float_equal(a.roughness, b.roughness)
        && floats_equal(&a.color, &b.color)
        && floats_equal(&a.uv_scale, &b.uv_scale)
        && a.material_overrides.len() == b.material_overrides.len()
        && a.material_overrides
            .iter()
            .zip(&b.material_overrides)
            .all(|(a, b)| {
                a.surface == b.surface
                    && a.source == b.source
                    && a.texture == b.texture
                    && transform_equal(a.transform, b.transform)
                    && floats_equal(&a.uv_scale, &b.uv_scale)
                    && floats_equal(&a.tint, &b.tint)
                    && optional_float_equal(a.metallic, b.metallic)
                    && optional_float_equal(a.roughness, b.roughness)
            })
}
fn material_equal(a: Option<&Material>, b: Option<&Material>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => {
            match (&a.shared, &b.shared) {
                (None, None) => {}
                (Some(a), Some(b)) if Arc::ptr_eq(a, b) => {}
                _ => return false,
            }
            a.texture == b.texture
                && optional_float_equal(a.metallic, b.metallic)
                && optional_float_equal(a.roughness, b.roughness)
                && floats_equal(&a.color, &b.color)
                && floats_equal(&a.uv_scale, &b.uv_scale)
        }
        _ => false,
    }
}
fn graph_equal(a: &shader_graph::ShaderGraph, b: &shader_graph::ShaderGraph) -> bool {
    a.version == b.version
        && a.name == b.name
        && a.wires == b.wires
        && a.keywords == b.keywords
        && a.nodes.len() == b.nodes.len()
        && a.nodes.iter().zip(&b.nodes).all(|(a, b)| {
            a.id == b.id
                && a.kind == b.kind
                && a.slot == b.slot
                && a.keyword == b.keyword
                && floats_equal(&a.position, &b.position)
                && a.inputs.len() == b.inputs.len()
                && a.inputs.iter().zip(&b.inputs).all(|(a, b)| match (a, b) {
                    (shader_graph::Value::Float(a), shader_graph::Value::Float(b)) => {
                        a.to_bits() == b.to_bits()
                    }
                    (shader_graph::Value::Vector(a), shader_graph::Value::Vector(b)) => {
                        floats_equal(a, b)
                    }
                    _ => false,
                })
        })
}

impl SceneInstance {
    /// Counters from the last shared extraction; owned view queries leave these
    /// unchanged so a reference comparison does not erase the measured work.
    pub fn render_extraction_stats(&self) -> Result<RenderExtractionStats> {
        Ok(self.render_cache.lock()?.stats)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_only_rows_never_reserve_unused_mesh_snapshot_vectors() {
        fn assert_no_mesh_storage<D>(view: &RenderView<D>) {
            assert_eq!(view.objects.len(), 0);
            assert_eq!(view.objects.capacity(), 0);
            assert_eq!(view.object_ids.capacity(), 0);
            assert_eq!(view.shader_graphs.capacity(), 0);
            assert_eq!(view.material_instances.capacity(), 0);
        }
        let mut scene = Scene::from_json(r#"{
            "version":1,"name":"Text reservations","views":{"3d":"camera"},"objects":[
                {"id":"label","name":"Label","transform":{"translation":[0,0,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]}},
                {"id":"camera","name":"Camera","transform":{"translation":[0,0,10],"rotation_degrees":[0,0,0],"scale":[1,1,1]},
                 "camera":{"projection":"perspective","vertical_fov_degrees":60,"near":0.1,"far":100}}
            ]
        }"#).unwrap();
        let template = scene.objects[0].clone();
        scene.objects.remove(0);
        scene.objects.extend((0..1024).map(|index| {
            let mut object = template.clone();
            object.id = format!("label-{index}");
            object.text_rendering = Some(TextRendering {
                enabled: false,
                ..Default::default()
            });
            object
        }));
        let mut world = World::new();
        let instance = scene.spawn(&mut world).unwrap();
        assert_no_mesh_storage(&instance.view(&world, Layer::ThreeD, 1.).unwrap());
        assert_no_mesh_storage(
            &instance
                .view_shared_from_camera(&world, Layer::ThreeD, 1., None)
                .unwrap(),
        );
        let entity = instance.entity("label-0").unwrap();
        world
            .insert(
                entity,
                Drawable {
                    metallic: None,
                    roughness: None,
                    gi_static: true,
                    material_overrides: Vec::new(),
                    layer: Layer::ThreeD,
                    mesh: Mesh::Cube,
                    texture: Texture::White,
                    color: [1.; 3],
                    uv_scale: [1.; 2],
                },
            )
            .unwrap();
        assert_eq!(
            instance
                .view_shared_from_camera(&world, Layer::ThreeD, 1., None)
                .unwrap()
                .objects
                .len(),
            1
        );
        world.remove::<Drawable>(entity).unwrap();
        // The sorted render membership is unchanged: this object remains text-only.
        assert_no_mesh_storage(
            &instance
                .view_shared_from_camera(&world, Layer::ThreeD, 1., None)
                .unwrap(),
        );
        assert_no_mesh_storage(&instance.view(&world, Layer::ThreeD, 1.).unwrap());
    }

    #[test]
    fn unloading_a_large_additive_world_releases_payload_and_transform_capacity() {
        let mut scene = Scene::from_json(r#"{
            "version":1,"name":"Cache capacity","views":{"3d":"camera","2d":"camera"},"objects":[
                {"id":"child","name":"Child","transform":{"translation":[2,0,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]},
                 "drawable":{"layer":"3d","mesh":"cube","texture":"white","color":[1,1,1],"uv_scale":[1,1]}},
                {"id":"camera","name":"Camera","transform":{"translation":[0,0,10],"rotation_degrees":[0,0,0],"scale":[1,1,1]},
                 "camera":{"projection":"perspective","vertical_fov_degrees":60,"near":0.1,"far":100}}
            ]
        }"#).unwrap();
        let mut addition = scene.clone();
        addition.views.clear();
        addition.objects = (0..4096)
            .map(|index| {
                let mut object = scene.objects[0].clone();
                object.id = format!("object-{index}");
                object.text_rendering = Some(TextRendering {
                    enabled: false,
                    ..Default::default()
                });
                object
            })
            .collect();
        scene
            .runtime_scenes
            .insert("large".into(), Arc::new(addition));
        let mut world = World::new();
        let mut instance = scene.spawn(&mut world).unwrap();
        instance
            .load_runtime_scene(&mut world, "large", true)
            .unwrap();
        // Disabled runtime lights are legal extraction inputs and exercise the
        // sparse light buffers without exceeding the emitted-light budget.
        for object in &instance.document.objects {
            if object.id.starts_with("scene-1-") {
                world
                    .insert(
                        instance.entity(&object.id).unwrap(),
                        Light {
                            enabled: false,
                            ..Default::default()
                        },
                    )
                    .unwrap();
            }
        }
        instance.set_render_interpolation(&mut world, true).unwrap();
        let _ = instance
            .view_shared_from_camera(&world, Layer::ThreeD, 1., None)
            .unwrap();
        world.advance_change_tick();
        world
            .get_mut::<Transform>(instance.entity("child").unwrap())
            .unwrap()
            .translation[0] = 10.;
        instance.capture_render_transforms(&mut world).unwrap();
        let loaded = instance
            .view_shared_interpolated_from_camera(&world, Layer::ThreeD, 1., None, 0.5)
            .unwrap();
        assert_eq!(loaded.objects.len(), 4097);
        let retired = Arc::downgrade(&loaded.objects[1].1);
        assert!(
            instance.transform_cache.retained_capacities()[..6]
                .iter()
                .all(|&capacity| capacity >= 4096)
        );
        {
            let cache = instance.render_cache.lock().unwrap();
            assert!(
                [
                    cache.render_entities.capacity(),
                    cache.candidates.capacity(),
                    cache.previous_candidates.capacity(),
                    cache.light_entities.capacity(),
                    cache.light_candidates.capacity(),
                    cache.previous_light_candidates.capacity(),
                ]
                .iter()
                .all(|&capacity| capacity >= 4096)
            );
        }
        drop(loaded);
        instance
            .unload_runtime_scene(&mut world, "scene-1")
            .unwrap();
        // Editor layer order may extract 2D first after an unload.
        let _ = instance
            .view_shared_from_camera(&world, Layer::TwoD, 1., None)
            .unwrap();
        let remaining = instance
            .view_shared_from_camera(&world, Layer::ThreeD, 1., None)
            .unwrap();
        assert_eq!(remaining.objects.len(), 1);
        assert!(retired.upgrade().is_none());
        assert!(
            instance
                .transform_cache
                .retained_capacities()
                .iter()
                .all(|&capacity| capacity <= 64)
        );
        let cache = instance.render_cache.lock().unwrap();
        assert!(
            [
                cache.render_entities.capacity(),
                cache.candidates.capacity(),
                cache.previous_candidates.capacity(),
                cache.light_entities.capacity(),
                cache.light_candidates.capacity(),
                cache.previous_light_candidates.capacity(),
            ]
            .iter()
            .all(|&capacity| capacity <= 64)
        );
    }

    #[test]
    fn reparenting_rebuilds_dense_topology_without_rebuilding_immutable_payloads() {
        let scene = Scene::from_json(r#"{
            "version":1,"name":"Reparent cache","views":{"3d":"camera"},"objects":[
                {"id":"a","name":"A","transform":{"translation":[3,0,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]}},
                {"id":"b","name":"B","transform":{"translation":[20,0,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]}},
                {"id":"child","name":"Child","parent":"a","transform":{"translation":[2,0,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]},
                 "drawable":{"layer":"3d","mesh":"cube","texture":"white","color":[1,1,1],"uv_scale":[1,1]}},
                {"id":"camera","name":"Camera","transform":{"translation":[0,0,10],"rotation_degrees":[0,0,0],"scale":[1,1,1]},
                 "camera":{"projection":"perspective","vertical_fov_degrees":60,"near":0.1,"far":100}}
            ]
        }"#).unwrap();
        let mut world = World::new();
        let mut instance = scene.spawn(&mut world).unwrap();
        let before = instance
            .view_shared_from_camera(&world, Layer::ThreeD, 1., None)
            .unwrap();
        assert_eq!(before.objects[0].0.transform_point3(Vec3::ZERO).x, 5.);
        instance.document.objects[2].parent = Some("b".into());
        instance.order = instance.document.order().unwrap();
        instance.rebuild_hierarchy_index();
        let after = instance
            .view_shared_from_camera(&world, Layer::ThreeD, 1., None)
            .unwrap();
        assert_eq!(after.objects[0].0.transform_point3(Vec3::ZERO).x, 22.);
        assert!(Arc::ptr_eq(&before.objects[0].1, &after.objects[0].1));
        assert_eq!(
            instance.global_transforms(&world).unwrap()["child"],
            after.objects[0].0
        );
        // Failure releases the transform lock and cannot hide a later repair.
        let transform = world
            .remove::<Transform>(instance.entity("b").unwrap())
            .unwrap()
            .unwrap();
        assert!(
            instance
                .view_shared_from_camera(&world, Layer::ThreeD, 1., None)
                .is_err()
        );
        world
            .insert(instance.entity("b").unwrap(), transform)
            .unwrap();
        assert_eq!(
            instance
                .view_shared_from_camera(&world, Layer::ThreeD, 1., None)
                .unwrap()
                .objects[0]
                .0,
            after.objects[0].0
        );
    }
}
