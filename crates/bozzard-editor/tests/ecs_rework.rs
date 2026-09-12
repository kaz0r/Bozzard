use bozzard_editor::Editor;
use bozzard_scene::{
    AssetKind, AssetSource, Blueprint, BlueprintAttachment, BoxCollider, Gravity, Layer, Material,
    Mesh, Prefab, Scene, Texture, Transform,
    blueprint::{Node, NodeKind as K, ObjectRef, Socket, Value, Wire},
};
use std::{collections::BTreeMap, path::PathBuf};

struct Temp(PathBuf);
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn wire(from: u32, out: usize, to: u32, input: usize) -> Wire {
    Wire {
        from: Socket {
            node: from,
            port: out,
        },
        to: Socket {
            node: to,
            port: input,
        },
    }
}

#[test]
fn spawned_graphs_rebase_prefab_dependencies_and_preload_cycles_once() {
    let temp =
        Temp(std::env::temp_dir().join(format!("bozzard-prefab-chain-{}", std::process::id())));
    std::fs::create_dir(&temp.0).unwrap();
    let path = temp.0.join("scene.json");
    let mut editor = Editor::new(bozzard_demo::scene_document().unwrap(), &path).unwrap();
    editor.create(Mesh::Cube, Layer::ThreeD).unwrap();
    let graph = |asset: &str, enabled| {
        let mut spawn = Node::new(2, K::SpawnPrefab, [0.; 2]);
        spawn.prefab = asset.into();
        BlueprintAttachment {
            enabled,
            graph: Blueprint {
                nodes: vec![Node::new(1, K::Start, [0.; 2]), spawn],
                wires: vec![wire(1, 0, 2, 0)],
                ..Default::default()
            },
        }
    };
    for (name, next, enabled) in [("parent", "child", true), ("child", "parent", false)] {
        let mut object = editor.selected_object().unwrap().clone();
        object.blueprints = vec![graph("next", enabled)];
        let prefab = Prefab {
            version: 1,
            name: name.into(),
            root: object.id.clone(),
            objects: vec![object],
            assets: BTreeMap::from([(
                "next".into(),
                AssetSource {
                    kind: AssetKind::Prefab,
                    path: format!("{next}.prefab.json"),
                },
            )]),
        };
        std::fs::write(
            temp.0.join(format!("{name}.prefab.json")),
            prefab.to_json().unwrap(),
        )
        .unwrap();
    }
    let mut scene = editor.scene().clone();
    scene.assets.insert(
        "parent".into(),
        AssetSource {
            kind: AssetKind::Prefab,
            path: "parent.prefab.json".into(),
        },
    );
    scene.objects.last_mut().unwrap().blueprints = vec![graph("parent", true)];
    let count = scene.objects.len();
    let mut play = bozzard_demo::SceneDemo::new_with_prefabs(&scene, Some(&path)).unwrap();
    assert_eq!(
        play.instance()
            .document()
            .assets
            .values()
            .filter(|a| a.kind == AssetKind::Prefab)
            .count(),
        2
    );
    for spawned in [1, 2, 2] {
        play.app.step();
        play.check_simulation().unwrap();
        assert_eq!(play.app.world.len(), count + spawned);
    }
    let child = play.instance().document().objects.last().unwrap();
    assert_eq!(child.blueprints[0].graph.nodes[1].prefab, "parent");
    std::fs::remove_file(temp.0.join("child.prefab.json")).unwrap();
    assert!(bozzard_demo::SceneDemo::new_with_prefabs(&scene, Some(&path)).is_err());
    scene.assets.clear();
    assert!(bozzard_demo::SceneDemo::new_with_prefabs(&scene, Some(&path)).is_err());
}

#[test]
fn optional_material_and_blueprint_prefab_lifecycle_survive_history_play_and_save() {
    let temp =
        Temp(std::env::temp_dir().join(format!("bozzard-ecs-rework-{}", std::process::id())));
    std::fs::create_dir(&temp.0).unwrap();
    std::fs::create_dir(temp.0.join("prefabs")).unwrap();
    std::fs::create_dir(temp.0.join("models")).unwrap();
    std::fs::write(
        temp.0.join("models/shape.obj"),
        "v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n",
    )
    .unwrap();
    std::fs::copy(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/demo/scenes/assets/palette.png"),
        temp.0.join("paint.png"),
    )
    .unwrap();
    let mut scene = bozzard_demo::scene_document().unwrap();
    for object in &mut scene.objects {
        object.blueprints.clear();
        object.spin = None;
    }
    let mut editor = Editor::new(scene, &temp.0.join("scene.json")).unwrap();
    editor.create(Mesh::Cube, Layer::ThreeD).unwrap();
    let owner = editor.selected.clone().unwrap();
    let fresh = editor.selected_object().unwrap().clone();
    assert!(fresh.drawable.is_some()); // Visible source geometry, no optional components.
    assert!(
        fresh.material.is_none()
            && fresh.collider.is_none()
            && fresh.gravity.is_none()
            && fresh.blueprints.is_empty()
    );
    let mut scene = editor.scene().clone();
    let material = Material {
        metallic: None,
        roughness: None,
        texture: None,
        color: [0.8, 0.2, 0.1],
        uv_scale: [2.; 2],
    };
    scene
        .objects
        .iter_mut()
        .find(|o| o.id == owner)
        .unwrap()
        .material = Some(material.clone());
    editor.apply("Add Material", scene).unwrap();
    editor.undo().unwrap();
    assert!(editor.selected_object().unwrap().material.is_none());
    editor.redo().unwrap();
    assert_eq!(editor.selected_object().unwrap().material, Some(material));
    editor.duplicate().unwrap();
    let copy = editor.selected.clone().unwrap();
    let mut scene = editor.scene().clone();
    scene
        .objects
        .iter_mut()
        .find(|o| o.id == copy)
        .unwrap()
        .material = None;
    editor.apply("Remove copy Material", scene).unwrap();
    assert!(
        editor
            .scene()
            .objects
            .iter()
            .find(|o| o.id == owner)
            .unwrap()
            .material
            .is_some()
    );
    editor.selected = Some(owner.clone());

    let mut root = fresh;
    root.id = "body".into();
    root.gravity = Some(Gravity::default());
    root.collider = Some(BoxCollider::default());
    root.drawable.as_mut().unwrap().mesh = Mesh::Asset("shape".into());
    root.material = Some(Material {
        metallic: None,
        roughness: None,
        texture: Some(Texture::Asset("paint".into())),
        color: [0.4, 0.7, 0.2],
        uv_scale: [1.; 2],
    });
    let mut child = root.clone();
    child.id = "child".into();
    child.parent = Some(root.id.clone());
    child.transform.translation = [1., 0., 0.];
    child.gravity = None;
    child.collider = None;
    child.material = None;
    let mut spin = Blueprint::spinning();
    spin.nodes
        .iter_mut()
        .find(|n| n.kind == K::Rotate)
        .unwrap()
        .inputs[2] = Value::Object(ObjectRef::Id(child.id.clone()));
    root.blueprints.push(BlueprintAttachment {
        enabled: true,
        graph: spin,
    });
    let prefab = Prefab {
        version: 1,
        name: "Body".into(),
        root: root.id.clone(),
        objects: vec![root, child],
        assets: BTreeMap::from([
            (
                "shape".into(),
                AssetSource {
                    kind: AssetKind::Mesh,
                    path: "../models/shape.obj".into(),
                },
            ),
            (
                "paint".into(),
                AssetSource {
                    kind: AssetKind::Image,
                    path: "../paint.png".into(),
                },
            ),
        ]),
    };
    std::fs::write(
        temp.0.join("prefabs/body.prefab.json"),
        prefab.to_json().unwrap(),
    )
    .unwrap();
    let mut scene = editor.scene().clone();
    scene.assets.insert(
        "body-prefab".into(),
        AssetSource {
            kind: AssetKind::Prefab,
            path: "prefabs/body.prefab.json".into(),
        },
    );
    editor.apply("Link prefab", scene).unwrap();
    let mut graph = Blueprint {
        nodes: vec![
            Node::new(1, K::Start, [0.; 2]),
            Node::new(2, K::SpawnPrefab, [0.; 2]),
            Node::new(3, K::DestroyPrefab, [0.; 2]),
            Node::new(4, K::InputPressed, [0.; 2]),
        ],
        wires: vec![wire(1, 0, 2, 0), wire(4, 0, 3, 0), wire(2, 1, 3, 1)],
        ..Blueprint::default()
    };
    graph.nodes[1].prefab = "body-prefab".into();
    graph.nodes[1].inputs[1] = Value::Vector([10., 4., 0.]);
    editor
        .set_blueprints(
            &owner,
            vec![BlueprintAttachment {
                enabled: true,
                graph,
            }],
        )
        .unwrap();
    assert!(editor.remove_asset("body-prefab").is_err());
    let authored = editor.scene().clone();
    let count = authored.objects.len();
    assert!(
        authored.prefabs.is_empty(),
        "source need not be placed before spawning"
    );
    editor.save(&temp.0.join("scene.json")).unwrap();
    editor = Editor::open(&temp.0.join("scene.json")).unwrap();
    editor.start_play().unwrap();
    assert!(editor.assets.require_ready().is_ok());
    let play = editor.play.as_mut().unwrap();
    play.app.step();
    play.check_simulation().unwrap();
    assert_eq!(play.instance().document().objects.len(), count + 2);
    let (spawned, link) = play.instance().document().prefabs.iter().next().unwrap();
    let spawned = spawned.clone();
    let child = link.members["child"].clone();
    let entity = play.instance().entity(&spawned).unwrap();
    let child_entity = play.instance().entity(&child).unwrap();
    assert_eq!(
        play.app.world.get::<Transform>(entity).unwrap().translation,
        [10., 4., 0.]
    );
    assert_eq!(
        play.app
            .world
            .get::<Transform>(child_entity)
            .unwrap()
            .rotation_degrees,
        [0.; 3]
    );
    assert!(play.app.world.get::<Material>(entity).is_some());
    assert!(
        play.instance()
            .collisions(&play.app.world)
            .unwrap()
            .boxes
            .iter()
            .any(|b| b.id == spawned)
    );
    let view = play
        .instance()
        .view(&play.app.world, Layer::ThreeD, 1.)
        .unwrap();
    assert!(
        view.objects
            .iter()
            .any(|(_, d)| d.color == [0.4, 0.7, 0.2] && matches!(d.texture, Texture::Asset(_)))
    );
    play.app.step();
    play.check_simulation().unwrap();
    assert!(play.app.world.get::<Transform>(entity).unwrap().translation[1] < 4.);
    assert!(
        play.app
            .world
            .get::<Transform>(child_entity)
            .unwrap()
            .rotation_degrees[1]
            > 0.
    );
    let captured = play.instance().capture(&play.app.world).unwrap();
    assert_eq!(
        Scene::from_json(&captured.to_json().unwrap()).unwrap(),
        captured
    );
    let before = play.app.world.len();
    assert!(
        play.with_instance(|instance, world| instance.spawn_prefab(
            world,
            "body-prefab",
            [f32::NAN; 3]
        ))
        .is_err()
    );
    assert!(
        play.with_instance(|instance, world| instance.destroy_prefab(world, "camera-3d"))
            .is_err()
    );
    assert_eq!(play.app.world.len(), before);
    play.set_gameplay_input(bozzard_scene::GameplayInput {
        jump: true,
        ..Default::default()
    });
    play.app.step();
    play.check_simulation().unwrap();
    assert_eq!(play.app.world.len(), count);
    assert!(play.instance().entity(&spawned).is_none());
    assert!(!play.app.world.contains(entity) && !play.app.world.contains(child_entity));
    play.instance()
        .capture(&play.app.world)
        .unwrap()
        .validate()
        .unwrap();
    let next = play
        .with_instance(|instance, world| instance.spawn_prefab(world, "body-prefab", [20., 4., 0.]))
        .unwrap();
    assert_ne!(next, spawned);
    let runtime_assets = editor.assets.entries().count();
    editor.save(&temp.0.join("saved/scene.json")).unwrap();
    assert_eq!(
        editor.assets.entries().count(),
        runtime_assets,
        "saving during Play must keep spawned mesh dependencies resident"
    );
    editor.render(Layer::ThreeD, 1.).unwrap();
    editor.stop_play();
    assert_eq!(editor.assets.entries().count(), editor.scene().assets.len());
    assert_eq!(editor.scene().objects, authored.objects);
    editor.start_play().unwrap();
    assert_eq!(editor.play.as_ref().unwrap().app.world.len(), count);
}
