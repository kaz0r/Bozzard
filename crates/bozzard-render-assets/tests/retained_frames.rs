//! CPU contracts for owned, recycled native frames; no graphics adapter is needed.
use anyhow::Result;
use bozzard_assets::{AssetData, AssetStore, LoadState};
use bozzard_ecs::World;
use bozzard_render::{IrradianceVolume, MeshKind, RenderScene, TextureKind};
use bozzard_render_assets::{RenderFrame, RenderSceneCache, render_scene};
use bozzard_scene::{
    AssetKind, AssetSource, Drawable, Layer, Material, Mesh, RenderView, Scene, SceneInstance,
    SceneView, SharedSceneView, SurfaceMaterialOverride, TextFont, TextRendering, Transform,
    material_asset::{MaterialAsset, MaterialInstance},
    middleware::{animation::Palette, sprite::Visual},
    shader_graph::ShaderGraph,
};
use glam::{Mat4, Vec3};
use std::{
    collections::BTreeMap,
    fs,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

fn setup() -> (SceneInstance, World) {
    let scene = Scene::from_json(r#"{
        "version":1,"name":"Retained adapter contracts",
        "views":{"3d":"camera","2d":"camera2"},
        "assets":{"model":{"kind":"mesh","path":"model.obj"},"shared":{"kind":"material","path":"shared.material.json"}},
        "objects":[
            {"id":"root","name":"Root","transform":{"translation":[0,0,0],"rotation_degrees":[0,0,0],"scale":[2,1,0.5]}},
            {"id":"z-cube","name":"Cube","parent":"root","transform":{"translation":[1,0,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]},
             "drawable":{"layer":"3d","mesh":"cube","texture":"white","color":[1,0.5,0.2],"uv_scale":[1,1]}},
            {"id":"a-cube","name":"Other","transform":{"translation":[-2,0,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]},
             "drawable":{"layer":"3d","mesh":"cube","texture":"white","color":[0.2,0.6,0.8],"uv_scale":[1,1]}},
            {"id":"quad","name":"Quad","transform":{"translation":[0,0,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]},
             "drawable":{"layer":"2d","mesh":"quad","texture":"checker","color":[0.3,0.4,0.5],"uv_scale":[2,3]}},
            {"id":"text","name":"Text","parent":"z-cube","transform":{"translation":[0,1,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]}},
            {"id":"lamp","name":"Lamp","parent":"z-cube","transform":{"translation":[0,0,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]},"light":{"kind":"point"}},
            {"id":"camera","name":"Camera","transform":{"translation":[0,0,10],"rotation_degrees":[0,0,0],"scale":[1,1,1]},
             "camera":{"projection":"perspective","vertical_fov_degrees":60,"near":0.1,"far":100}},
            {"id":"camera2","name":"Camera2","transform":{"translation":[0,0,10],"rotation_degrees":[0,0,0],"scale":[1,1,1]},
             "camera":{"projection":"orthographic","vertical_size":10,"near":0.1,"far":100}}
        ]
    }"#).unwrap();
    let mut world = World::new();
    let instance = scene.spawn(&mut world).unwrap();
    world
        .insert(instance.entity("z-cube").unwrap(), ShaderGraph::default())
        .unwrap();
    world
        .insert(instance.entity("text").unwrap(), TextRendering::default())
        .unwrap();
    (instance, world)
}

fn empty_assets() -> AssetStore {
    AssetStore::new(std::path::Path::new("."), &BTreeMap::new()).unwrap()
}

fn shared(instance: &SceneInstance, world: &World, layer: Layer) -> SharedSceneView {
    instance
        .view_shared_from_camera(world, layer, 1.6, None)
        .unwrap()
}

fn owned(view: &SharedSceneView) -> SceneView {
    SceneView {
        objects: view
            .objects
            .iter()
            .map(|(model, drawable)| (*model, drawable.as_ref().clone()))
            .collect(),
        sprites: view.sprites.clone(),
        skin_poses: view.skin_poses.clone(),
        object_ids: view.object_ids.clone(),
        compute_textures: view.compute_textures.clone(),
        shader_graphs: view.shader_graphs.clone(),
        material_instances: view.material_instances.clone(),
        particles: view.particles.clone(),
        display_time: view.display_time,
        texts: view.texts.clone(),
        fog: view.fog,
        lights: view.lights.clone(),
        environment: view.environment,
        display: view.display,
        lighting: view.lighting,
        view_projection: view.view_projection,
    }
}

fn assert_scene_equal(a: &RenderScene, b: &RenderScene) {
    assert_eq!(
        a.view_projection.to_cols_array().map(f32::to_bits),
        b.view_projection.to_cols_array().map(f32::to_bits)
    );
    // Includes every setting and complete item/material/mesh payload, including
    // shader source, overrides, text, sprite geometry, particles, skin and GI.
    assert_eq!(format!("{a:?}"), format!("{b:?}"));
}

fn extract(
    cache: &RenderSceneCache,
    instance: &SceneInstance,
    world: &World,
    assets: &AssetStore,
    layer: Layer,
) -> RenderFrame {
    cache
        .extract(shared(instance, world, layer), assets, layer, None)
        .unwrap()
}

fn decorate<D>(mut view: RenderView<D>) -> RenderView<D> {
    view.display_time = 2.75;
    view.fog.enabled = true;
    view.fog.distance_density = 0.03;
    view.environment.intensity = 0.7;
    view.environment.star_intensity = 0.2;
    view.sprites.push(Visual {
        motion_id: 99,
        model: Mat4::from_translation(Vec3::new(1., 2., 3.)),
        image: "atlas".into(),
        color: [0.2, 0.4, 0.6, 0.8],
        quads: Arc::from([[0., 1., 2., 3., 0., 0., 1., 1.]]),
    });
    view.skin_poses.insert(
        88,
        Palette {
            signature: 42,
            matrices: Arc::new(vec![Mat4::IDENTITY.to_cols_array()]),
        },
    );
    view.particles.push(bozzard_scene::Particle {
        simulation: None,
        id: 7,
        position: Vec3::new(1., 2., 3.),
        velocity: Vec3::Y,
        size: 0.5,
        rotation: 0.2,
        color: [0.3, 0.4, 0.5],
        opacity: 0.6,
        kind: bozzard_scene::ParticleKind::Smoke,
        softness: 0.1,
        trail_length: 0.2,
        seed: 0.3,
    });
    view
}

#[test]
fn retained_and_reference_frames_preserve_all_fields_and_transient_order() -> Result<()> {
    let (instance, world) = setup();
    let assets = empty_assets();
    let cache = RenderSceneCache::default();
    for layer in [Layer::ThreeD, Layer::TwoD] {
        let view = decorate(shared(&instance, &world, layer));
        let gi = (layer == Layer::ThreeD).then(|| IrradianceVolume {
            min: [0.; 3],
            max: [2.; 3],
            resolution: [2; 3],
            intensity: 0.8,
            normal_bias: 0.02,
            probes: Arc::new(vec![
                [0.1, 0.2, 0.3, 0.4];
                8 * bozzard_scene::GI_PROBE_STRIDE
            ]),
        });
        let reference = render_scene(owned(&view), &assets, layer, gi.clone())?;
        let frame = cache.extract(view, &assets, layer, gi)?;
        assert_scene_equal(&reference, &frame);
        assert_eq!(frame.fog.enabled, layer == Layer::ThreeD);
        assert_eq!(
            frame.environment.intensity,
            if layer == Layer::ThreeD { 0.7 } else { 0. }
        );
        assert_eq!(frame.shader_time, 2.75);
        assert!(
            frame
                .items
                .iter()
                .filter(|item| !matches!(item.mesh, MeshKind::Sprite(_) | MeshKind::Text(_)))
                .all(|item| item.material.lit == (layer == Layer::ThreeD))
        );
        let sprite = frame
            .items
            .iter()
            .position(|item| matches!(item.mesh, MeshKind::Sprite(_)))
            .unwrap();
        assert!(
            frame.items[..sprite]
                .iter()
                .all(|item| !matches!(item.mesh, MeshKind::Text(_)))
        );
        assert!(
            frame.items[sprite + 1..]
                .iter()
                .all(|item| matches!(item.mesh, MeshKind::Text(_)))
        );
    }
    Ok(())
}

#[test]
fn warmed_frames_move_static_payloads_and_remove_appended_overlays() {
    let (instance, mut world) = setup();
    let a = instance.entity("a-cube").unwrap();
    {
        let mut drawable = world.get_mut::<Drawable>(a).unwrap();
        drawable.mesh = Mesh::Asset("model".into());
        drawable
            .material_overrides
            .push(SurfaceMaterialOverride::inherited(
                0,
                "0123456789abcdef".into(),
            ));
    }
    let assets = empty_assets();
    let cache = RenderSceneCache::default();
    let mut first = extract(&cache, &instance, &world, &assets, Layer::ThreeD);
    let expected_len = first.items.len();
    let mesh_pointer = match &first.items[0].mesh {
        MeshKind::Imported(id) => id.as_ptr(),
        _ => panic!("expected imported item"),
    };
    let overrides = first.items[0].material.surface_overrides.clone();
    let overlay = first.items[0].clone();
    first.append_items([overlay]);
    first.set_shader_time(99.);
    first.set_view_projection(Mat4::IDENTITY);
    first.bypass_effects();
    drop(first);
    world.get_mut::<Transform>(a).unwrap().translation[0] += 1.;
    let view = shared(&instance, &world, Layer::ThreeD);
    let expected = render_scene(owned(&view), &assets, Layer::ThreeD, None).unwrap();
    let next = cache.extract(view, &assets, Layer::ThreeD, None).unwrap();
    assert_eq!(next.items.len(), expected_len);
    assert_eq!(next.stats().material_rebuilds, 0);
    assert_eq!(next.stats().material_reuses, 2);
    assert!(Arc::ptr_eq(
        &overrides,
        &next.items[0].material.surface_overrides
    ));
    match &next.items[0].mesh {
        MeshKind::Imported(id) => assert_eq!(mesh_pointer, id.as_ptr()),
        _ => panic!("expected imported item"),
    }
    assert_scene_equal(&expected, &next);
}

#[test]
fn frozen_frames_survive_same_tick_and_bypassed_world_edits() {
    let (instance, mut world) = setup();
    let assets = empty_assets();
    let cache = RenderSceneCache::default();
    let frozen = extract(&cache, &instance, &world, &assets, Layer::ThreeD);
    let frozen_dump = format!("{:?}", &*frozen);
    let a = instance.entity("a-cube").unwrap();
    let tick = world.change_tick();
    world.get_mut::<Transform>(a).unwrap().translation[0] = 9.;
    world
        .get_mut::<Drawable>(a)
        .unwrap()
        .bypass_change_detection()
        .color = [0.7, 0.8, 0.9];
    let view = shared(&instance, &world, Layer::ThreeD);
    let expected = render_scene(owned(&view), &assets, Layer::ThreeD, None).unwrap();
    let next = cache.extract(view, &assets, Layer::ThreeD, None).unwrap();
    assert_eq!(world.change_tick(), tick);
    assert_scene_equal(&expected, &next);
    assert_ne!(frozen.items[0].model, next.items[0].model);
    assert_ne!(frozen.items[0].material.tint, next.items[0].material.tint);
    assert_eq!(format!("{:?}", &*frozen), frozen_dump);
}

struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "bozzard-retained-frames-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        fs::write(
            path.join("model.obj"),
            include_bytes!("../../../examples/demo/scenes/assets/quad.obj"),
        )
        .unwrap();
        let temp = Self(path);
        temp.material([0.2, 0.3, 0.4]);
        temp
    }
    fn material(&self, color: [f32; 3]) {
        let mut material = MaterialAsset::default();
        material.properties.color = Some(color);
        fs::write(
            self.0.join("shared.material.json"),
            material.to_json().unwrap(),
        )
        .unwrap();
    }
    fn store(&self) -> AssetStore {
        let sources = BTreeMap::from([
            (
                "shared".into(),
                AssetSource {
                    kind: AssetKind::Material,
                    path: "shared.material.json".into(),
                },
            ),
            (
                "model".into(),
                AssetSource {
                    kind: AssetKind::Mesh,
                    path: "model.obj".into(),
                },
            ),
        ]);
        let mut store = AssetStore::new(&self.0, &sources).unwrap();
        store.load_pending().unwrap();
        store
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn bind_material(instance: &SceneInstance, world: &mut World) {
    let a = instance.entity("a-cube").unwrap();
    let drawable = world.get::<Drawable>(a).unwrap();
    let mut material = Material::from_drawable(drawable);
    material.shared = Some(Arc::new(MaterialInstance::new("shared".into())));
    world.insert(a, material).unwrap();
}

#[test]
fn frozen_frames_release_host_assets_and_unused_pool_when_cache_drops() {
    let temp = Temp::new();
    let assets = temp.store();
    let published = assets
        .entries()
        .map(|entry| {
            (
                entry.id.clone(),
                Arc::downgrade(&entry.shared_data().unwrap()),
            )
        })
        .collect::<Vec<_>>();
    let (instance, mut world) = setup();
    bind_material(&instance, &mut world);
    let cache = RenderSceneCache::default();
    let view = decorate(shared(&instance, &world, Layer::ThreeD));
    let expected = render_scene(owned(&view), &assets, Layer::ThreeD, None).unwrap();
    let frozen = cache.extract(view, &assets, Layer::ThreeD, None).unwrap();
    assert_eq!(frozen.items[0].material.tint, [0.2, 0.3, 0.4]);
    assert_scene_equal(&expected, &frozen);

    // A distinct source guard belongs only to this second, unused frame buffer.
    // It proves that a live frozen frame does not keep the host's pool alive.
    let mut other = shared(&instance, &world, Layer::ThreeD);
    let source = Arc::new(other.objects[0].1.as_ref().clone());
    let pooled_source = Arc::downgrade(&source);
    other.objects[0].1 = source;
    drop(cache.extract(other, &assets, Layer::ThreeD, None).unwrap());
    assert!(pooled_source.upgrade().is_some());

    drop(assets);
    assert!(published.iter().all(|(_, data)| data.upgrade().is_some()));
    drop(cache);
    for (id, data) in published {
        assert!(data.upgrade().is_none(), "closed host retained asset {id}");
    }
    assert!(pooled_source.upgrade().is_none());
    assert_scene_equal(&expected, &frozen);
    assert_eq!(frozen.shader_time, 2.75);
    assert!(frozen.fog.enabled);
    assert_eq!(frozen.environment.intensity, 0.7);
    drop(frozen);
}

#[test]
fn asset_arc_identity_handles_divergent_clones_replacements_and_failed_reload() {
    let temp = Temp::new();
    let mut base = temp.store();
    let mut fork = base.clone();
    let handle = base.handle("shared").unwrap();
    temp.material([0.4, 0.5, 0.6]);
    base.refresh();
    temp.material([0.8, 0.7, 0.6]);
    fork.refresh();
    assert_eq!(
        base.get(handle).unwrap().revision(),
        fork.get(handle).unwrap().revision()
    );
    assert!(!Arc::ptr_eq(
        &base.get(handle).unwrap().shared_data().unwrap(),
        &fork.get(handle).unwrap().shared_data().unwrap()
    ));
    let (instance, mut world) = setup();
    bind_material(&instance, &mut world);
    let cache = RenderSceneCache::default();
    let frozen = extract(&cache, &instance, &world, &base, Layer::ThreeD);
    assert_eq!(frozen.items[0].material.tint, [0.4, 0.5, 0.6]);
    let next = extract(&cache, &instance, &world, &fork, Layer::ThreeD);
    assert_eq!(next.items[0].material.tint, [0.8, 0.7, 0.6]);
    assert!(next.stats().assets_changed > 0);
    drop(next);
    fs::write(temp.0.join("shared.material.json"), b"invalid material").unwrap();
    fork.refresh();
    assert!(matches!(
        fork.get(handle).unwrap().state(),
        LoadState::Failed(_)
    ));
    let failed = extract(&cache, &instance, &world, &fork, Layer::ThreeD);
    assert_eq!(failed.items[0].material.tint, [0.8, 0.7, 0.6]);
    assert_eq!(failed.stats().assets_changed, 0);
    assert_eq!(failed.stats().material_rebuilds, 0);
    drop(failed);
    // A separately constructed store can also reach the exact same revision.
    temp.material([0.6, 0.5, 0.4]);
    let mut replacement = temp.store();
    temp.material([0.9, 0.8, 0.7]);
    replacement.refresh();
    let replacement_handle = replacement.handle("shared").unwrap();
    assert_ne!(handle, replacement_handle);
    assert_eq!(
        replacement.get(replacement_handle).unwrap().revision(),
        fork.get(handle).unwrap().revision()
    );
    let fresh = extract(&cache, &instance, &world, &replacement, Layer::ThreeD);
    assert_eq!(fresh.items[0].material.tint, [0.9, 0.8, 0.7]);
    drop(fresh);
    assert_eq!(frozen.items[0].material.tint, [0.4, 0.5, 0.6]);
    drop(frozen);
    let warm = extract(&cache, &instance, &world, &replacement, Layer::ThreeD);
    assert_eq!(
        warm.stats().material_rebuilds,
        0,
        "an obsolete frame must not replace the current pool entry"
    );
}

#[test]
fn surface_source_filtering_membership_order_and_layers_track_current_inputs() {
    let temp = Temp::new();
    // Plain OBJ geometry has no selectable parts. A material assignment makes
    // this fixture exercise the real imported-surface source-signature path.
    fs::write(temp.0.join("model.mtl"), "newmtl surface\nKd 1 1 1\n").unwrap();
    fs::write(
        temp.0.join("model.obj"),
        format!(
            "mtllib model.mtl\nusemtl surface\n{}",
            include_str!("../../../examples/demo/scenes/assets/quad.obj")
        ),
    )
    .unwrap();
    let mut assets = temp.store();
    let entry = assets.get(assets.handle("model").unwrap()).unwrap();
    let AssetData::Mesh(model) = entry.data().unwrap() else {
        panic!("expected model")
    };
    assert_eq!(
        model.parts.len(),
        1,
        "fixture must expose one source surface"
    );
    let source = model.parts[0].source_key.clone();
    let (instance, mut world) = setup();
    let a = instance.entity("a-cube").unwrap();
    let z = instance.entity("z-cube").unwrap();
    world.get_mut::<Drawable>(a).unwrap().mesh = Mesh::Surface {
        asset: "model".into(),
        index: 0,
        source,
    };
    let cache = RenderSceneCache::default();
    let initial = extract(&cache, &instance, &world, &assets, Layer::ThreeD);
    assert!(matches!(initial.items[0].mesh, MeshKind::ModelPart(_, 0)));
    let a_motion = initial.items[0].motion_id;
    let z_motion = initial.items[1].motion_id;
    drop(initial);
    let two_d = extract(&cache, &instance, &world, &assets, Layer::TwoD);
    assert_eq!(two_d.items.len(), 1);
    assert!(!two_d.items[0].material.lit);
    drop(two_d);
    let mut reordered = shared(&instance, &world, Layer::ThreeD);
    reordered.objects.swap(0, 1);
    reordered.object_ids.swap(0, 1);
    reordered.shader_graphs.swap(0, 1);
    reordered.material_instances.swap(0, 1);
    let expected = render_scene(owned(&reordered), &assets, Layer::ThreeD, None).unwrap();
    let frame = cache
        .extract(reordered, &assets, Layer::ThreeD, None)
        .unwrap();
    assert_eq!(frame.items[0].motion_id, z_motion);
    assert_eq!(frame.items[1].motion_id, a_motion);
    assert_scene_equal(&expected, &frame);
    drop(frame);
    let model = fs::read_to_string(temp.0.join("model.obj"))
        .unwrap()
        .replace("v 0.5 0.5 0", "v 0.7 0.5 0");
    fs::write(temp.0.join("model.obj"), model).unwrap();
    assets.refresh();
    let changed = extract(&cache, &instance, &world, &assets, Layer::ThreeD);
    assert!(!changed.items.iter().any(|item| item.motion_id == a_motion));
    assert_eq!(changed.items[0].motion_id, z_motion);
    drop(changed);
    let removed = world.remove::<Drawable>(z).unwrap().unwrap();
    let without = extract(&cache, &instance, &world, &assets, Layer::ThreeD);
    assert!(
        without
            .items
            .iter()
            .all(|item| matches!(item.mesh, MeshKind::Text(_)))
    );
    drop(without);
    world.insert(z, removed).unwrap();
    let mut added = world.get::<Drawable>(z).unwrap().clone();
    added.color = [0.1, 0.2, 0.3];
    world
        .insert(instance.entity("root").unwrap(), added)
        .unwrap();
    let view = shared(&instance, &world, Layer::ThreeD);
    let expected = render_scene(owned(&view), &assets, Layer::ThreeD, None).unwrap();
    let frame = cache.extract(view, &assets, Layer::ThreeD, None).unwrap();
    assert_scene_equal(&expected, &frame);
    assert_eq!(frame.items[1].motion_id, z_motion);
    assert_eq!(frame.items[0].material.tint, [0.1, 0.2, 0.3]);
    drop(frame);
    let two_d = extract(&cache, &instance, &world, &assets, Layer::TwoD);
    assert_eq!(
        two_d.stats().material_rebuilds,
        1,
        "asset reload invalidates both layer pools"
    );
    assert!(!two_d.items[0].material.lit);
}

#[test]
fn conversion_errors_discard_candidates_and_retry_returns_current_data() {
    let (instance, world) = setup();
    let assets = empty_assets();
    let cache = RenderSceneCache::default();
    drop(extract(&cache, &instance, &world, &assets, Layer::ThreeD));
    let mut bad = shared(&instance, &world, Layer::ThreeD);
    bad.material_instances[0] = Some(Arc::new(MaterialInstance::new("missing".into())));
    assert!(cache.extract(bad, &assets, Layer::ThreeD, None).is_err());
    let mut bad_text = shared(&instance, &world, Layer::ThreeD);
    bad_text.texts[0].1.font = TextFont::Custom("missing".into());
    assert!(
        cache
            .extract(bad_text, &assets, Layer::ThreeD, None)
            .is_err()
    );
    let mut bad_graph = shared(&instance, &world, Layer::ThreeD);
    let mut graph = ShaderGraph::default();
    graph.nodes[0].position[0] = f32::NAN;
    bad_graph.shader_graphs[0] = Some(Arc::new(graph));
    assert!(
        cache
            .extract(bad_graph, &assets, Layer::ThreeD, None)
            .is_err()
    );
    let view = shared(&instance, &world, Layer::ThreeD);
    let expected = render_scene(owned(&view), &assets, Layer::ThreeD, None).unwrap();
    let restored = cache.extract(view, &assets, Layer::ThreeD, None).unwrap();
    assert_scene_equal(&expected, &restored);
    drop(restored);
    assert_eq!(
        extract(&cache, &instance, &world, &assets, Layer::ThreeD)
            .stats()
            .material_rebuilds,
        0
    );
}

#[test]
fn late_drops_after_clear_or_disable_never_repopulate_obsolete_buffers() {
    let (instance, world) = setup();
    let assets = empty_assets();
    let cache = RenderSceneCache::default();
    for disable in [false, true] {
        let old = extract(&cache, &instance, &world, &assets, Layer::ThreeD);
        if disable {
            cache.set_enabled(false);
            let reference_view = shared(&instance, &world, Layer::ThreeD);
            let reference = cache
                .reference(owned(&reference_view), &assets, Layer::ThreeD, None)
                .unwrap();
            assert_scene_equal(&old, &reference);
            drop(reference);
            cache.set_enabled(true);
        } else {
            cache.clear();
        }
        drop(extract(&cache, &instance, &world, &assets, Layer::ThreeD));
        drop(old);
        let next = extract(&cache, &instance, &world, &assets, Layer::ThreeD);
        assert_eq!(next.stats().material_rebuilds, 0);
        assert_eq!(next.stats().material_reuses, 2);
    }
}

#[test]
fn consumed_frames_allow_mutation_without_corrupting_future_reuse() {
    let (instance, world) = setup();
    let assets = empty_assets();
    let cache = RenderSceneCache::default();
    let mut unrestricted = extract(&cache, &instance, &world, &assets, Layer::ThreeD).into_scene();
    unrestricted.items[0].mesh = MeshKind::Sphere;
    unrestricted.items[0].material.tint = [1., 0., 0.];
    unrestricted.items[0].material.texture = TextureKind::Checker;
    let view = shared(&instance, &world, Layer::ThreeD);
    let expected = render_scene(owned(&view), &assets, Layer::ThreeD, None).unwrap();
    let next = cache.extract(view, &assets, Layer::ThreeD, None).unwrap();
    assert_scene_equal(&expected, &next);
    assert_ne!(next.items[0].mesh, unrestricted.items[0].mesh);
}

fn large_view(instance: &SceneInstance, world: &World, source: &Arc<Drawable>) -> SharedSceneView {
    let mut view = shared(instance, world, Layer::ThreeD);
    view.objects = (0..512).map(|_| (Mat4::IDENTITY, source.clone())).collect();
    view.object_ids = (1..=512).collect();
    view.shader_graphs = vec![None; 512];
    view.material_instances = vec![None; 512];
    view
}

#[test]
fn large_old_frames_release_payloads_after_the_current_workload_shrinks() {
    let (instance, world) = setup();
    let assets = empty_assets();
    let cache = RenderSceneCache::default();
    let source = Arc::new(
        world
            .get::<Drawable>(instance.entity("a-cube").unwrap())
            .unwrap()
            .clone(),
    );
    let weak = Arc::downgrade(&source);
    let old = cache
        .extract(
            large_view(&instance, &world, &source),
            &assets,
            Layer::ThreeD,
            None,
        )
        .unwrap();
    // Two already pooled large frames plus one still live cover both pruning
    // unused pool entries and rejecting a late drop after the workload shrinks.
    let pooled_a = cache
        .extract(
            large_view(&instance, &world, &source),
            &assets,
            Layer::ThreeD,
            None,
        )
        .unwrap();
    let pooled_b = cache
        .extract(
            large_view(&instance, &world, &source),
            &assets,
            Layer::ThreeD,
            None,
        )
        .unwrap();
    drop(pooled_a);
    drop(pooled_b);
    drop(source);
    drop(extract(&cache, &instance, &world, &assets, Layer::ThreeD));
    drop(old);
    assert!(
        weak.upgrade().is_none(),
        "a late large frame must not stay in the small scene's pool"
    );
    let next = extract(&cache, &instance, &world, &assets, Layer::ThreeD);
    assert_eq!(next.stats().material_rebuilds, 0);
}
