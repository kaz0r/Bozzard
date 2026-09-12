use bozzard_editor::Editor;
use bozzard_scene::{AssetKind, AssetSource, Layer, Prefab, Scene, TextRendering, Transform};
use glam::Vec3;
use std::path::PathBuf;

fn path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/demo/scenes/text-lab.json")
}

#[test]
fn empty_objects_have_only_transform_and_support_history() {
    let mut editor = Editor::open(&path()).unwrap();
    let before = editor.scene().clone();
    editor.create_empty().unwrap();
    let object = editor.selected_object().unwrap().clone();
    assert_eq!(
        object,
        bozzard_scene::Object {
            id: object.id.clone(),
            name: "Empty Object".into(),
            ..Default::default()
        }
    );
    assert_eq!(
        serde_json::to_value(&object)
            .unwrap()
            .as_object()
            .unwrap()
            .len(),
        3
    );
    let created = editor.scene().clone();
    assert_eq!(
        Scene::from_json(&created.to_json().unwrap()).unwrap(),
        created
    );
    editor.undo().unwrap();
    assert_eq!(editor.scene(), &before);
    editor.redo().unwrap();
    assert_eq!(editor.scene(), &created);
    editor.create_empty().unwrap();
    assert_ne!(editor.selected_object().unwrap().id, object.id);
}

#[test]
fn text_component_roundtrip_layers_bounds_picking_and_play_isolation() {
    let mut editor = Editor::open(&path()).unwrap();
    let original = editor.scene().clone();
    assert_eq!(
        Scene::from_json(&original.to_json().unwrap()).unwrap(),
        original
    );
    for (layer, count) in [(Layer::ThreeD, 5), (Layer::TwoD, 4)] {
        assert_eq!(
            editor
                .render(layer, 1.)
                .unwrap()
                .items
                .iter()
                .filter(|i| matches!(i.mesh, bozzard_render::MeshKind::Text(_)))
                .count(),
            count
        );
    }
    let mut scene = original.clone();
    scene
        .objects
        .retain(|o| o.camera.is_some() || o.id == "screen-heading");
    let mut parent = scene
        .objects
        .iter()
        .find(|o| o.id == "screen-heading")
        .unwrap()
        .clone();
    parent.id = "parent".into();
    parent.text_rendering = None;
    parent.transform = Transform {
        translation: [2., -1., -2.],
        rotation_degrees: [12., 0., 25.],
        scale: [-1.5, 0.6, 1.],
    };
    let label = scene
        .objects
        .iter_mut()
        .find(|o| o.id == "screen-heading")
        .unwrap();
    label.parent = Some("parent".into());
    label.transform = Transform::default();
    let local = bozzard_render::text_bounds(&bozzard_render_assets::text_mesh(
        label.text_rendering.as_ref().unwrap(),
    ))
    .unwrap()
    .unwrap();
    scene.objects.push(parent);
    editor.apply("Parent text", scene.clone()).unwrap();
    let matrix = scene.global_transforms().unwrap()["screen-heading"];
    let mut expected = [Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)];
    for x in [local[0].x, local[1].x] {
        for y in [local[0].y, local[1].y] {
            let p = matrix.transform_point3(Vec3::new(x, y, 0.));
            expected[0] = expected[0].min(p);
            expected[1] = expected[1].max(p);
        }
    }
    assert_eq!(
        editor
            .frame_bounds(Layer::TwoD, Some("parent"))
            .unwrap()
            .unwrap(),
        expected
    );
    let center = matrix.transform_point3((local[0] + local[1]) * 0.5);
    let projection = glam::camera::rh::proj::directx::orthographic(-4., 4., -4., 4., 0.1, 100.)
        * glam::camera::rh::view::look_at_mat4(center + Vec3::Z * 10., center, Vec3::Y);
    assert_eq!(
        editor
            .pick_with_projection(Layer::TwoD, projection, [0., 0.])
            .unwrap()
            .as_deref(),
        Some("screen-heading")
    );
    assert!(
        editor
            .pick_with_projection(Layer::ThreeD, projection, [0., 0.])
            .unwrap()
            .is_none()
    );
    assert!(
        editor
            .pick_with_projection(Layer::TwoD, projection, [0.99, 0.99])
            .unwrap()
            .is_none()
    );
    let mut invalid = scene.clone();
    invalid
        .objects
        .iter_mut()
        .find(|o| o.id == "screen-heading")
        .unwrap()
        .text_rendering
        .as_mut()
        .unwrap()
        .font_size = f32::NAN;
    assert!(editor.apply("Invalid text", invalid).is_err());
    assert_eq!(editor.scene(), &scene);
    editor.start_play().unwrap();
    let play = editor.play.as_mut().unwrap();
    let entity = play.instance().entity("screen-heading").unwrap();
    play.app
        .world
        .get_mut::<TextRendering>(entity)
        .unwrap()
        .text = "Runtime only".into();
    assert_eq!(
        play.instance()
            .capture(&play.app.world)
            .unwrap()
            .objects
            .iter()
            .find(|o| o.id == "screen-heading")
            .unwrap()
            .text_rendering
            .as_ref()
            .unwrap()
            .text,
        "Runtime only"
    );
    play.app.world.remove::<TextRendering>(entity).unwrap();
    assert!(
        play.instance()
            .view(&play.app.world, Layer::TwoD, 1.)
            .unwrap()
            .texts
            .is_empty()
    );
    editor.stop_play();
    assert_eq!(editor.scene(), &scene);
    let mut removed = scene.clone();
    removed
        .objects
        .iter_mut()
        .find(|o| o.id == "screen-heading")
        .unwrap()
        .text_rendering = None;
    editor.apply("Remove text", removed.clone()).unwrap();
    assert!(editor.render(Layer::TwoD, 1.).unwrap().items.is_empty());
    editor.undo().unwrap();
    assert_eq!(editor.scene(), &scene);
    editor.redo().unwrap();
    assert_eq!(editor.scene(), &removed);
}

#[test]
fn text_prefabs_are_independent_and_blueprint_color_visibility_work_headlessly() {
    let mut scene = Scene::from_json(&std::fs::read_to_string(path()).unwrap()).unwrap();
    let mut label = scene
        .objects
        .iter()
        .find(|o| o.id == "world-heading")
        .unwrap()
        .clone();
    label.transform = Transform::default();
    label.blueprints.push(bozzard_scene::BlueprintAttachment {
        enabled: true,
        graph: serde_json::from_value(serde_json::json!({"version":1,"name":"Text effects","variables":{},"nodes":[
            {"id":1,"kind":"start","position":[0,0],"inputs":[]},
            {"id":2,"kind":"set_color","position":[250,0],"inputs":["exec",{"vector":[1,0,0]},{"object":"self_object"}]},
            {"id":3,"kind":"set_visible","position":[500,0],"inputs":["exec",{"bool":false},{"object":"self_object"}]}
        ],"wires":[{"from":{"node":1,"port":0},"to":{"node":2,"port":0}},{"from":{"node":2,"port":0},"to":{"node":3,"port":0}}]})).unwrap(),
    });
    let prefab = Prefab {
        version: 1,
        name: "Text label".into(),
        root: label.id.clone(),
        objects: vec![label],
        assets: Default::default(),
    };
    scene.assets.insert(
        "label".into(),
        AssetSource {
            kind: AssetKind::Prefab,
            path: "label.prefab.json".into(),
        },
    );
    let mut demo = bozzard_demo::SceneDemo::new(&scene).unwrap();
    demo.with_instance(|i, _| i.register_prefab("label".into(), prefab))
        .unwrap();
    let a = demo
        .with_instance(|i, w| i.spawn_prefab(w, "label", [0., 0., 0.]))
        .unwrap();
    let b = demo
        .with_instance(|i, w| i.spawn_prefab(w, "label", [1., 0., 0.]))
        .unwrap();
    let ea = demo.instance().entity(&a).unwrap();
    let eb = demo.instance().entity(&b).unwrap();
    demo.app.world.get_mut::<TextRendering>(ea).unwrap().text = "Independent".into();
    assert_ne!(
        demo.app.world.get::<TextRendering>(ea),
        demo.app.world.get::<TextRendering>(eb)
    );
    demo.app.step();
    demo.check_simulation().unwrap();
    assert_eq!(
        &demo.app.world.get::<TextRendering>(ea).unwrap().color[..3],
        &[1., 0., 0.]
    );
    assert_eq!(
        demo.instance()
            .view(&demo.app.world, Layer::ThreeD, 1.)
            .unwrap()
            .texts
            .len(),
        5
    );
    demo.with_instance(|i, w| i.destroy_prefab(w, &a)).unwrap();
    assert!(demo.app.world.get::<TextRendering>(ea).is_none());
    assert!(demo.app.world.get::<TextRendering>(eb).is_some());
    demo.with_instance(|i, w| i.destroy_prefab(w, &b)).unwrap();
    assert_eq!(demo.app.world.len(), scene.objects.len());
}
