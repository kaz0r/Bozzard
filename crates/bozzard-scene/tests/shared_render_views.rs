use bozzard_ecs::World;
use bozzard_scene::{
    Drawable, Layer, Lod, LodLevel, Material, Mesh, Prefab, Scene, SceneInstance, SceneView,
    SharedSceneView, SurfaceMaterialOverride, TextRendering, Texture, Transform,
    middleware::{registry, sprite::Sprite},
    shader_graph::{ShaderGraph, Value},
};
use glam::{Mat4, Vec3};
use std::sync::Arc;

fn scene() -> Scene {
    let mut scene = Scene::from_json(r#"{
        "version":1,"name":"Retained views","views":{"3d":"camera","2d":"camera2"},
        "assets":{"atlas":{"kind":"image","path":"atlas.png"},"model":{"kind":"mesh","path":"model.gltf"}},
        "objects":[
            {"id":"root","name":"Root","transform":{"translation":[0,0,0],"rotation_degrees":[0,0,0],"scale":[2,1,0.5]}},
            {"id":"z-cube","name":"Cube","parent":"root","transform":{"translation":[1,0,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]},
             "drawable":{"layer":"3d","mesh":"cube","texture":"white","color":[1,0.5,0.2],"uv_scale":[1,1]}},
            {"id":"a-cube","name":"Other","transform":{"translation":[-2,0,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]},
             "drawable":{"layer":"3d","mesh":"cube","texture":"white","color":[0,1,1],"uv_scale":[1,1]}},
            {"id":"text","name":"Text","parent":"z-cube","transform":{"translation":[0,1,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]}},
            {"id":"sprite","name":"Sprite","transform":{"translation":[0,0,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]}},
            {"id":"lamp","name":"Lamp","parent":"z-cube","transform":{"translation":[0,0,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]},"light":{"kind":"point"}},
            {"id":"camera","name":"Camera","parent":"root","transform":{"translation":[0,0,10],"rotation_degrees":[0,0,0],"scale":[1,1,1]},
             "camera":{"projection":"perspective","vertical_fov_degrees":60,"near":0.1,"far":100}},
            {"id":"camera2","name":"Camera2","transform":{"translation":[0,0,10],"rotation_degrees":[0,0,0],"scale":[1,1,1]},
             "camera":{"projection":"orthographic","vertical_size":10,"near":0.1,"far":100}}
        ]
    }"#).unwrap();
    scene.objects[1].shader_graph = Some(ShaderGraph::default());
    scene.objects[3].text_rendering = Some(TextRendering::default());
    registry::set(
        &mut scene.objects[4],
        &Sprite {
            image: "atlas".into(),
            ..Default::default()
        },
    )
    .unwrap();
    scene
}

fn setup() -> (SceneInstance, World) {
    let mut world = World::new();
    let instance = scene().spawn(&mut world).unwrap();
    (instance, world)
}

fn shared(instance: &SceneInstance, world: &World) -> SharedSceneView {
    instance
        .view_shared_from_camera(world, Layer::ThreeD, 1., None)
        .unwrap()
}

fn assert_views(owned: &SceneView, shared: &SharedSceneView) {
    assert_eq!(owned.view_projection, shared.view_projection);
    assert_eq!(owned.object_ids, shared.object_ids);
    assert_eq!(owned.objects.len(), shared.objects.len());
    for ((a_matrix, a), (b_matrix, b)) in owned.objects.iter().zip(&shared.objects) {
        assert_eq!(a_matrix, b_matrix);
        assert_eq!(a, b.as_ref());
    }
    assert_eq!(owned.shader_graphs, shared.shader_graphs);
    assert_eq!(owned.material_instances, shared.material_instances);
    assert_eq!(owned.compute_textures, shared.compute_textures);
    assert_eq!(
        owned.texts,
        shared
            .texts
            .iter()
            .cloned()
            .chain(
                shared
                    .shared_texts
                    .iter()
                    .map(|(model, text)| (*model, text.as_ref().clone()))
            )
            .collect::<Vec<_>>()
    );
    assert_eq!(owned.particles, shared.particles);
    assert_eq!(owned.display, shared.display);
    assert_eq!(owned.display_time, shared.display_time);
    assert_eq!(owned.environment, shared.environment);
    assert_eq!(owned.fog, shared.fog);
    assert_eq!(owned.lighting, shared.lighting);
    assert_eq!(owned.lights.len(), shared.lights.len());
    for (a, b) in owned.lights.iter().zip(&shared.lights) {
        assert_eq!(a.light, b.light);
        assert_eq!(a.position, b.position);
        assert_eq!(a.direction, b.direction);
    }
    assert_eq!(owned.skin_poses.len(), shared.skin_poses.len());
    for (id, a) in &owned.skin_poses {
        let b = &shared.skin_poses[id];
        assert_eq!(a.signature, b.signature);
        assert_eq!(a.matrices, b.matrices);
    }
    assert_eq!(owned.sprites.len(), shared.sprites.len());
    for (a, b) in owned.sprites.iter().zip(&shared.sprites) {
        assert_eq!(a.motion_id, b.motion_id);
        assert_eq!(a.model, b.model);
        assert_eq!(a.image, b.image);
        assert_eq!(a.color, b.color);
        assert_eq!(a.quads, b.quads);
    }
}

#[test]
fn shared_snapshots_match_owned_views_and_reuse_payloads_when_poses_change() {
    let (instance, mut world) = setup();
    let initial = shared(&instance, &world);
    // Membership remains sorted by persistent ID, independent of document/ECS order.
    assert_eq!(initial.objects[0].1.color, [0., 1., 1.]);
    assert_eq!(
        instance
            .render_extraction_stats()
            .unwrap()
            .drawable_rebuilds,
        2
    );
    assert_eq!(
        instance
            .render_extraction_stats()
            .unwrap()
            .shader_graph_rebuilds,
        1
    );
    let saved = instance.capture(&world).unwrap();
    let checkpoint = instance.save_game_json(&world).unwrap();
    world
        .get_mut::<Transform>(instance.entity("root").unwrap())
        .unwrap()
        .translation[0] = 10.;
    world
        .get_mut::<Transform>(instance.entity("camera").unwrap())
        .unwrap()
        .translation[1] = 2.;
    let next = shared(&instance, &world);
    assert_views(&instance.view(&world, Layer::ThreeD, 1.).unwrap(), &next);
    assert_ne!(initial.objects[1].0, next.objects[1].0);
    for (a, b) in initial.objects.iter().zip(&next.objects) {
        assert!(Arc::ptr_eq(&a.1, &b.1));
    }
    assert!(Arc::ptr_eq(
        initial.shader_graphs[1].as_ref().unwrap(),
        next.shader_graphs[1].as_ref().unwrap()
    ));
    let stats = instance.render_extraction_stats().unwrap();
    assert_eq!(stats.drawable_reuses, 2);
    assert_eq!(stats.drawable_rebuilds, 0);
    assert_eq!(stats.shader_graph_reuses, 1);
    // Extraction must not overwrite authored state, the checkpoint, or held frames.
    assert_eq!(initial.objects[1].0.transform_point3(Vec3::ZERO).x, 2.);
    assert_ne!(instance.capture(&world).unwrap(), saved);
    assert_ne!(instance.save_game_json(&world).unwrap(), checkpoint);
    let before = instance.capture(&world).unwrap();
    let before_checkpoint = instance.save_game_json(&world).unwrap();
    let _ = shared(&instance, &world);
    assert_eq!(instance.capture(&world).unwrap(), before);
    assert_eq!(instance.save_game_json(&world).unwrap(), before_checkpoint);
}

#[test]
fn same_tick_and_bypassed_edits_refresh_only_affected_payloads() {
    let (instance, mut world) = setup();
    let cube = instance.entity("z-cube").unwrap();
    let initial = shared(&instance, &world);
    let tick = world.change_tick();
    world
        .get_mut::<Drawable>(cube)
        .unwrap()
        .bypass_change_detection()
        .color = [0.25, 0.5, 0.75];
    let edited = shared(&instance, &world);
    assert_eq!(world.change_tick(), tick);
    assert!(Arc::ptr_eq(&initial.objects[0].1, &edited.objects[0].1));
    assert!(!Arc::ptr_eq(&initial.objects[1].1, &edited.objects[1].1));
    assert_eq!(initial.objects[1].1.color, [1., 0.5, 0.2]);
    assert_views(&instance.view(&world, Layer::ThreeD, 1.).unwrap(), &edited);
    world
        .insert(
            cube,
            Material {
                shared: None,
                metallic: Some(0.3),
                roughness: None,
                texture: Some(Texture::Checker),
                color: [0.1, 0.2, 0.3],
                uv_scale: [2., 3.],
            },
        )
        .unwrap();
    let material = shared(&instance, &world);
    assert_eq!(material.objects[1].1.texture, Texture::Checker);
    assert_eq!(material.objects[1].1.color, [0.1, 0.2, 0.3]);
    world
        .get_mut::<Material>(cube)
        .unwrap()
        .bypass_change_detection()
        .roughness = Some(0.9);
    let override_edit = shared(&instance, &world);
    assert_eq!(override_edit.objects[1].1.roughness, Some(0.9));
    world.remove::<Material>(cube).unwrap();
    let removed = shared(&instance, &world);
    assert_eq!(removed.objects[1].1.texture, Texture::White);
    assert_eq!(removed.objects[1].1.color, [0.25, 0.5, 0.75]);
    world
        .get_mut::<ShaderGraph>(cube)
        .unwrap()
        .bypass_change_detection()
        .name = "Changed graph".into();
    let graph_edit = shared(&instance, &world);
    assert!(!Arc::ptr_eq(
        initial.shader_graphs[1].as_ref().unwrap(),
        graph_edit.shader_graphs[1].as_ref().unwrap()
    ));
    assert_eq!(
        graph_edit.shader_graphs[1].as_ref().unwrap().name,
        "Changed graph"
    );
    world.remove::<ShaderGraph>(cube).unwrap();
    assert!(shared(&instance, &world).shader_graphs[1].is_none());
    world.remove::<Drawable>(cube).unwrap();
    assert_eq!(shared(&instance, &world).objects.len(), 1);
    world
        .insert(cube, initial.objects[1].1.as_ref().clone())
        .unwrap();
    let reinserted = shared(&instance, &world);
    assert_views(
        &instance.view(&world, Layer::ThreeD, 1.).unwrap(),
        &reinserted,
    );
    assert_eq!(reinserted.objects.len(), 2);
}

#[test]
fn signed_zero_changes_are_preserved_in_drawable_material_and_graph_payloads() {
    let (instance, mut world) = setup();
    let entity = instance.entity("a-cube").unwrap();
    let before = shared(&instance, &world);
    world.get_mut::<Drawable>(entity).unwrap().color[0] = -0.;
    let after = shared(&instance, &world);
    assert!(!Arc::ptr_eq(&before.objects[0].1, &after.objects[0].1));
    assert_eq!(after.objects[0].1.color[0].to_bits(), (-0_f32).to_bits());
    world
        .insert(
            entity,
            Material::from_drawable(world.get::<Drawable>(entity).unwrap()),
        )
        .unwrap();
    let material_before = shared(&instance, &world);
    world.get_mut::<Material>(entity).unwrap().color[0] = 0.;
    let material_after = shared(&instance, &world);
    assert!(!Arc::ptr_eq(
        &material_before.objects[0].1,
        &material_after.objects[0].1
    ));
    assert_eq!(
        material_after.objects[0].1.color[0].to_bits(),
        0_f32.to_bits()
    );
    let graph_entity = instance.entity("z-cube").unwrap();
    let graph_before = shared(&instance, &world);
    let input = graph_before.shader_graphs[1].as_ref().unwrap().nodes[0]
        .inputs
        .iter()
        .position(|value| matches!(value, Value::Float(_)))
        .unwrap();
    {
        let mut graph = world.get_mut::<ShaderGraph>(graph_entity).unwrap();
        graph.nodes[0].inputs[input] = Value::Float(-0.);
    }
    let graph_after = shared(&instance, &world);
    assert!(!Arc::ptr_eq(
        graph_before.shader_graphs[1].as_ref().unwrap(),
        graph_after.shader_graphs[1].as_ref().unwrap()
    ));
    assert_eq!(
        graph_after.shader_graphs[1].as_ref().unwrap().nodes[0].inputs[input],
        Value::Float(-0.)
    );
}

#[test]
fn inspection_lod_culling_and_material_surface_changes_match_owned_extraction() {
    let (instance, mut world) = setup();
    let entity = instance.entity("z-cube").unwrap();
    world
        .insert(
            entity,
            Lod {
                levels: vec![
                    LodLevel {
                        switch: 10.,
                        mesh: Some(Mesh::Quad),
                    },
                    LodLevel {
                        switch: 30.,
                        mesh: None,
                    },
                ],
                hysteresis: 0.,
            },
        )
        .unwrap();
    for distance in [2., 20., 40., 2.] {
        let inspection = Some(Mat4::from_translation(Vec3::new(2., 0., distance)));
        let retained = instance
            .view_shared_from_camera(&world, Layer::ThreeD, 1., inspection)
            .unwrap();
        assert_views(
            &instance
                .view_from_camera(&world, Layer::ThreeD, 1., inspection)
                .unwrap(),
            &retained,
        );
        match distance {
            20. => assert_eq!(retained.objects[1].1.mesh, Mesh::Quad),
            40. => assert_eq!(retained.objects.len(), 1),
            _ => assert_eq!(retained.objects[1].1.mesh, Mesh::Cube),
        }
    }
    // Runtime imported-surface data can be edited even when no change tick advances.
    world.get_mut::<Drawable>(entity).unwrap().mesh = Mesh::Asset("model".into());
    world
        .get_mut::<Drawable>(entity)
        .unwrap()
        .material_overrides
        .push(SurfaceMaterialOverride::inherited(
            0,
            "0123456789abcdef".into(),
        ));
    let first = shared(&instance, &world);
    world
        .get_mut::<Drawable>(entity)
        .unwrap()
        .bypass_change_detection()
        .material_overrides[0]
        .tint[0] = 0.25;
    let second = shared(&instance, &world);
    assert!(!Arc::ptr_eq(&first.objects[1].1, &second.objects[1].1));
    assert_views(&instance.view(&world, Layer::ThreeD, 1.).unwrap(), &second);
}

#[test]
fn interpolation_resets_text_light_and_sprite_views_remain_consistent() {
    let (instance, mut world) = setup();
    instance.set_render_interpolation(&mut world, true).unwrap();
    world.advance_change_tick();
    world
        .get_mut::<Transform>(instance.entity("root").unwrap())
        .unwrap()
        .translation[0] = 10.;
    world
        .get_mut::<Transform>(instance.entity("sprite").unwrap())
        .unwrap()
        .translation[0] = 8.;
    instance.capture_render_transforms(&mut world).unwrap();
    let mut previous = None;
    for alpha in [0., 0.25, 0.5, 1.] {
        for layer in [Layer::ThreeD, Layer::TwoD] {
            let retained = instance
                .view_shared_interpolated_from_camera(&world, layer, 1., None, alpha)
                .unwrap();
            assert_views(
                &instance
                    .view_interpolated_from_camera(&world, layer, 1., None, alpha)
                    .unwrap(),
                &retained,
            );
            if layer == Layer::ThreeD {
                assert_eq!(
                    retained.objects[1].0.transform_point3(Vec3::ZERO).x,
                    2. + 10. * alpha
                );
                if let Some(ref previous) = previous {
                    assert!(Arc::ptr_eq(previous, &retained.objects[1].1));
                }
                previous = Some(retained.objects[1].1.clone());
            } else {
                assert_eq!(
                    retained.sprites[0].model.transform_point3(Vec3::ZERO).x,
                    8. * alpha
                );
            }
        }
    }
    instance
        .reset_render_interpolation(&mut world, "z-cube")
        .unwrap();
    let retained = instance
        .view_shared_interpolated_from_camera(&world, Layer::ThreeD, 1., None, 0.5)
        .unwrap();
    assert_eq!(retained.objects[1].0.transform_point3(Vec3::ZERO).x, 12.);
    assert_views(
        &instance
            .view_interpolated_from_camera(&world, Layer::ThreeD, 1., None, 0.5)
            .unwrap(),
        &retained,
    );
    for invalid in [f32::NAN, -0.1, 1.1] {
        assert!(
            instance
                .view_shared_interpolated_from_camera(&world, Layer::ThreeD, 1., None, invalid)
                .is_err()
        );
    }
}

#[test]
fn removed_payloads_are_released_and_clones_have_independent_caches() {
    let (instance, mut world) = setup();
    let initial = shared(&instance, &world);
    let weak = Arc::downgrade(&initial.objects[1].1);
    let graph_weak = Arc::downgrade(initial.shader_graphs[1].as_ref().unwrap());
    let clone = instance.clone();
    let cloned = shared(&clone, &world);
    assert!(!Arc::ptr_eq(&initial.objects[1].1, &cloned.objects[1].1));
    assert!(!Arc::ptr_eq(
        initial.shader_graphs[1].as_ref().unwrap(),
        cloned.shader_graphs[1].as_ref().unwrap()
    ));
    drop(initial);
    world
        .remove::<Drawable>(instance.entity("z-cube").unwrap())
        .unwrap();
    let _ = shared(&instance, &world);
    assert!(weak.upgrade().is_none());
    assert!(graph_weak.upgrade().is_none());
    // The other cache/snapshot is independently owned and remains intact.
    assert_eq!(cloned.objects[1].1.color, [1., 0.5, 0.2]);
}

#[test]
fn hidden_graph_and_drawable_removal_release_warm_cache_payloads() {
    let (instance, mut world) = setup();
    let entity = instance.entity("z-cube").unwrap();
    let initial = shared(&instance, &world);
    let drawable = Arc::downgrade(&initial.objects[1].1);
    let graph = Arc::downgrade(initial.shader_graphs[1].as_ref().unwrap());
    drop(initial);
    let tick = world.change_tick();
    world
        .insert(entity, bozzard_scene::BlueprintHidden(true))
        .unwrap();
    world.remove::<ShaderGraph>(entity).unwrap();
    assert_eq!(shared(&instance, &world).objects.len(), 1);
    // Hidden objects skip graph(), so cache preparation must release removed graphs.
    assert!(graph.upgrade().is_none());
    assert!(drawable.upgrade().is_some());
    world.remove::<Drawable>(entity).unwrap();
    assert_eq!(shared(&instance, &world).objects.len(), 1);
    assert!(drawable.upgrade().is_none());
    assert_eq!(world.change_tick(), tick);
}

#[test]
fn prefab_despawn_generation_reuse_and_additive_loads_never_replay_cached_payloads() {
    let mut source = scene();
    source.assets.insert(
        "cube".into(),
        bozzard_scene::AssetSource {
            kind: bozzard_scene::AssetKind::Prefab,
            path: "cube.prefab.json".into(),
        },
    );
    let mut addition = scene();
    addition.objects[1].drawable.as_mut().unwrap().color = [0.2, 0.3, 0.4];
    source
        .runtime_scenes
        .insert("addition".into(), Arc::new(addition));
    let mut world = World::new();
    let mut instance = source.spawn(&mut world).unwrap();
    let mut objects: Vec<_> = source
        .objects
        .iter()
        .filter(|object| object.id == "z-cube" || object.parent.as_deref() == Some("z-cube"))
        .cloned()
        .collect();
    objects[0].parent = None;
    let prefab = Prefab {
        version: 1,
        name: "Cube".into(),
        root: "z-cube".into(),
        objects,
        assets: source.assets.clone(),
        nested: Default::default(),
        base: None,
    };
    instance.register_prefab("cube".into(), prefab).unwrap();
    let authored_revision = instance.authored_revision();
    let root = instance
        .spawn_prefab(&mut world, "cube", [30., 0., 0.])
        .unwrap();
    assert!(instance.authored_revision() > authored_revision);
    let first = shared(&instance, &world);
    assert_eq!(first.objects.len(), 3);
    let old = instance.entity(&root).unwrap();
    let authored_revision = instance.authored_revision();
    instance.destroy_prefab(&mut world, &root).unwrap();
    assert!(instance.authored_revision() > authored_revision);
    assert_eq!(shared(&instance, &world).objects.len(), 2);
    let new_root = instance
        .spawn_prefab(&mut world, "cube", [40., 0., 0.])
        .unwrap();
    assert_ne!(instance.entity(&new_root).unwrap(), old);
    world
        .get_mut::<Drawable>(instance.entity(&new_root).unwrap())
        .unwrap()
        .color = [0.6, 0.7, 0.8];
    let spawned = shared(&instance, &world);
    assert_views(&instance.view(&world, Layer::ThreeD, 1.).unwrap(), &spawned);
    assert!(
        spawned
            .objects
            .iter()
            .any(|(_, drawable)| drawable.color == [0.6, 0.7, 0.8])
    );
    let authored_revision = instance.authored_revision();
    instance
        .load_runtime_scene(&mut world, "addition", true)
        .unwrap();
    assert!(instance.authored_revision() > authored_revision);
    let loaded = shared(&instance, &world);
    assert_eq!(loaded.objects.len(), 5);
    assert_views(&instance.view(&world, Layer::ThreeD, 1.).unwrap(), &loaded);
    let authored_revision = instance.authored_revision();
    instance
        .unload_runtime_scene(&mut world, "scene-1")
        .unwrap();
    assert!(instance.authored_revision() > authored_revision);
    let unloaded = shared(&instance, &world);
    assert_eq!(unloaded.objects.len(), 3);
    assert_views(
        &instance.view(&world, Layer::ThreeD, 1.).unwrap(),
        &unloaded,
    );
    // Held snapshots retain their original scene membership and render data.
    assert_eq!(loaded.objects.len(), 5);
    assert_eq!(first.objects.len(), 3);
}

#[test]
fn sparse_transform_closure_reads_only_changed_subtrees_and_matches_full_scan_oracle() {
    let (instance, mut world) = setup();
    let first = shared(&instance, &world);
    let warm = shared(&instance, &world);
    assert_eq!(instance.render_transform_stats().unwrap().source_reads, 0);
    assert_eq!(instance.render_transform_stats().unwrap().topology_reads, 0);
    assert_eq!(
        instance
            .render_extraction_stats()
            .unwrap()
            .motion_id_rebuilds,
        0
    );
    assert_eq!(
        instance.render_extraction_stats().unwrap().motion_id_reuses,
        warm.objects.len()
    );
    assert_eq!(
        instance.render_transform_stats().unwrap().topology_rebuilds,
        0
    );
    assert_views(&instance.view(&world, Layer::ThreeD, 1.).unwrap(), &warm);
    let root = instance.entity("root").unwrap();
    world
        .get_mut::<Transform>(root)
        .unwrap()
        .bypass_change_detection()
        .translation[0] = 7.;
    let changed = shared(&instance, &world);
    let sparse = instance.render_transform_stats().unwrap();
    // Root, cube, text, lamp and camera inherit this changed parent; unrelated
    // other-cube, sprite and orthographic camera retain their validated matrices.
    assert_eq!(sparse.source_reads, 5);
    assert_eq!(sparse.topology_reads, 0);
    instance
        .set_sparse_render_extraction_enabled(false)
        .unwrap();
    let reference = instance.view(&world, Layer::ThreeD, 1.).unwrap();
    assert_eq!(instance.render_transform_stats().unwrap().source_reads, 8);
    assert_eq!(instance.render_transform_stats().unwrap().topology_reads, 8);
    assert_views(&reference, &changed);
    instance.set_sparse_render_extraction_enabled(true).unwrap();
    let repeated = shared(&instance, &world);
    assert_views(&reference, &repeated);
    assert_eq!(first.objects[1].0.transform_point3(Vec3::ZERO).x, 2.);
    assert_eq!(changed.objects[1].0.transform_point3(Vec3::ZERO).x, 9.);
    let camera = instance.entity("camera2").unwrap();
    let removed = world.remove::<Transform>(camera).unwrap().unwrap();
    assert!(
        instance
            .view_shared_from_camera(&world, Layer::ThreeD, 1., None)
            .is_err()
    );
    world.insert(camera, removed).unwrap();
    assert_views(&reference, &shared(&instance, &world));
}
