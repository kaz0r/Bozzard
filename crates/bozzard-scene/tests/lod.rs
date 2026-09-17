use bozzard_ecs::World;
use bozzard_scene::{AssetKind, AssetSource, Layer, Lod, LodLevel, Mesh, Object, Scene, Transform};
use std::collections::BTreeMap;

fn scene() -> Scene {
    Scene::from_json(r#"{"version":1,"name":"LOD","views":{"3d":"camera"},"objects":[
    {"id":"camera","name":"Camera","transform":{"translation":[0,0,3],"rotation_degrees":[0,0,0],"scale":[1,1,1]},"camera":{"projection":"perspective","vertical_fov_degrees":60,"near":0.1,"far":1000}},
    {"id":"far","name":"Far","transform":{"translation":[0,0,-40],"rotation_degrees":[0,0,0],"scale":[1,1,1]},
     "drawable":{"layer":"3d","mesh":"cube","texture":"white","color":[1,1,1],"uv_scale":[1,1]},
     "lod":{"levels":[{"switch":40,"mesh":"quad"},{"switch":60,"mesh":null}]}},
    {"id":"near","name":"Near","transform":{"translation":[0,0,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]},
     "drawable":{"layer":"3d","mesh":"cube","texture":"white","color":[1,1,1],"uv_scale":[1,1]},
     "lod":{"levels":[{"switch":40,"mesh":"quad"},{"switch":60,"mesh":null}]}}
    ]}"#).unwrap()
}

#[test]
fn lod_swaps_culls_reappears_and_preserves_world_and_capture() {
    let scene = scene();
    assert_eq!(scene, Scene::from_json(&scene.to_json().unwrap()).unwrap());
    let mut world = World::default();
    let instance = scene.spawn(&mut world).unwrap();
    let meshes = |world: &World| {
        let view = instance.view(world, Layer::ThreeD, 1.).unwrap();
        assert_eq!(view.objects.len(), view.object_ids.len());
        assert_eq!(view.objects.len(), view.shader_graphs.len());
        view.objects
            .into_iter()
            .map(|(_, d)| d.mesh)
            .collect::<Vec<_>>()
    };
    assert_eq!(meshes(&world), [Mesh::Quad, Mesh::Cube]); // 43 and 3 units
    let camera = instance.entity("camera").unwrap();
    world.get_mut::<Transform>(camera).unwrap().translation = [0., 0., 20.];
    assert_eq!(meshes(&world), [Mesh::Cube]); // far exactly at cull threshold
    world.get_mut::<Transform>(camera).unwrap().translation = [0., 0., 1000.];
    assert!(meshes(&world).is_empty()); // last level remains active
    world.get_mut::<Transform>(camera).unwrap().translation = [0., 0., 0.];
    assert_eq!(meshes(&world), [Mesh::Quad, Mesh::Cube]); // exact swap threshold
    world.get_mut::<Transform>(camera).unwrap().translation = [0., 0., -20.];
    assert_eq!(meshes(&world), [Mesh::Cube, Mesh::Cube]); // both reappear at full detail
    world.get_mut::<Transform>(camera).unwrap().translation = [0., 0., 3.];
    assert_eq!(instance.capture(&world).unwrap(), scene); // LOD never edits the base mesh
}

#[test]
fn lod_uses_composed_world_origins_not_camera_rotation_or_scale() {
    let mut scene = scene();
    scene.objects.push(Object {
        id: "parent".into(),
        name: "Parent".into(),
        transform: Transform {
            translation: [0., 0., 100.],
            ..Default::default()
        },
        ..Default::default()
    });
    scene.objects[0].parent = Some("parent".into());
    scene.objects[0].transform.rotation_degrees = [0., 90., 0.];
    scene.objects[0].transform.scale = [2.; 3];
    scene.objects[1].parent = Some("parent".into());
    let mut world = World::default();
    let instance = scene.spawn(&mut world).unwrap();
    let view = instance.view(&world, Layer::ThreeD, 1.).unwrap();
    assert_eq!(view.objects.len(), 1); // near is now 103 units from the camera
    assert_eq!(view.objects[0].1.mesh, Mesh::Quad); // far is still 43 units away
}

#[test]
fn replacement_clears_source_overrides_then_applies_object_material() {
    use bozzard_scene::{Drawable, Material, SurfaceMaterialOverride, Texture};
    let scene = scene();
    let mut world = World::default();
    let instance = scene.spawn(&mut world).unwrap();
    let entity = instance.entity("far").unwrap();
    let old = SurfaceMaterialOverride::inherited(0, "0000000000000000".into());
    world
        .get_mut::<Drawable>(entity)
        .unwrap()
        .material_overrides
        .push(old.clone());
    world.get_mut::<Lod>(entity).unwrap().levels[0].mesh = Some(Mesh::Surface {
        asset: "low".into(),
        index: 1,
        source: "1111111111111111".into(),
    });
    world
        .insert(
            entity,
            Material {
                metallic: Some(0.7),
                roughness: None,
                texture: Some(Texture::Checker),
                color: [0.2, 0.3, 0.4],
                uv_scale: [2.; 2],
            },
        )
        .unwrap();
    let view = instance.view(&world, Layer::ThreeD, 1.).unwrap();
    let drawable = &view.objects[0].1;
    assert_eq!(drawable.color, [0.2, 0.3, 0.4]);
    assert_eq!(drawable.material_overrides.len(), 1);
    assert_eq!(drawable.material_overrides[0].surface, 1);
    assert_eq!(drawable.material_overrides[0].source, "1111111111111111");
    assert_eq!(drawable.material_overrides[0].metallic, Some(0.7));
    assert_eq!(
        world.get::<Drawable>(entity).unwrap().material_overrides,
        [old]
    );
}

#[test]
fn skinned_objects_keep_base_mesh_and_palette() {
    use bozzard_scene::middleware::animation::{
        Animator,
        data::{Binding, Joint, Rig},
    };
    let scene = scene();
    let mut world = World::default();
    let instance = scene.spawn(&mut world).unwrap();
    world
        .insert(
            instance.entity("far").unwrap(),
            Animator {
                rig: std::sync::Arc::new(Rig {
                    nodes: vec![Joint {
                        name: "root".into(),
                        parent: None,
                        rest: Default::default(),
                    }],
                    bindings: vec![Binding {
                        node: 0,
                        inverse_bind: glam::Mat4::IDENTITY.to_cols_array(),
                    }],
                    clips: vec![],
                }),
                ..Default::default()
            },
        )
        .unwrap();
    let view = instance.view(&world, Layer::ThreeD, 1.).unwrap();
    assert_eq!(view.objects[0].1.mesh, Mesh::Cube);
    assert_eq!(view.skin_poses.len(), 1);
    let camera = instance.entity("camera").unwrap();
    world.get_mut::<Transform>(camera).unwrap().translation = [0., 0., 1000.];
    let view = instance.view(&world, Layer::ThreeD, 1.).unwrap();
    assert_eq!(view.objects.len(), 1);
    assert_eq!(view.objects[0].1.mesh, Mesh::Cube);
}

#[test]
fn lod_validates_and_tracks_imported_mesh_dependencies() {
    let mut scene = scene();
    for distances in [
        vec![0.],
        vec![-1.],
        vec![f32::NAN],
        vec![f32::INFINITY],
        vec![10., 10.],
        vec![20., 10.],
        vec![1.; 33],
    ] {
        let lod = Lod {
            levels: distances
                .into_iter()
                .map(|switch| LodLevel { switch, mesh: None })
                .collect(),
        };
        assert!(lod.validate().is_err());
    }
    assert!(Lod::default().validate().is_ok());
    scene.objects[1].lod.as_mut().unwrap().levels[0].mesh = Some(Mesh::Asset("low".into()));
    assert!(scene.validate().is_err());
    scene.assets.insert(
        "low".into(),
        AssetSource {
            kind: AssetKind::Image,
            path: "low.png".into(),
        },
    );
    assert!(scene.validate().is_err());
    scene.assets.get_mut("low").unwrap().kind = AssetKind::Mesh;
    scene.validate().unwrap();
    assert_eq!(scene.asset_users()["low"], ["far"]);
    let object = &mut scene.objects[1];
    object.lod.as_mut().unwrap().levels[1].mesh = Some(Mesh::Surface {
        asset: "low".into(),
        index: 0,
        source: "source".into(),
    });
    object.remap_assets(&BTreeMap::from([("low".into(), "renamed".into())]));
    assert_eq!(
        object.asset_dependencies(),
        [("renamed", AssetKind::Mesh), ("renamed", AssetKind::Mesh)]
    );
    object.drawable = None;
    assert!(scene.validate().is_err());
}
