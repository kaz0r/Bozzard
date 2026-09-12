use bozzard_demo::SceneDemo;
use bozzard_editor::Editor;
use bozzard_scene::{Blueprint, GameplayInput, Layer, Transform};
use std::path::PathBuf;

fn step(demo: &mut SceneDemo, ticks: usize) {
    for _ in 0..ticks {
        demo.app.step();
        demo.check_simulation().unwrap();
    }
}
fn place_player(demo: &mut SceneDemo, position: [f32; 3]) {
    let entity = demo.instance().entity("player").unwrap();
    demo.app
        .world
        .get_mut::<Transform>(entity)
        .unwrap()
        .translation = position;
}

#[test]
fn gold_yard_pickup_mouse_look_physics_pad_cleanup_and_restart() {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/demo/scenes/gold-yard.json");
    let mut editor = Editor::open(&path).unwrap();
    editor.assets.require_ready().unwrap();
    let authored = editor.scene().clone();
    for (id, file) in [("gold", "yard-mouse"), ("drop-pad", "yard-drop")] {
        let export = Blueprint::from_json(
            &std::fs::read_to_string(
                path.parent()
                    .unwrap()
                    .join(format!("assets/Blueprints/{file}.blueprint.json")),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(
            authored
                .objects
                .iter()
                .find(|o| o.id == id)
                .unwrap()
                .blueprints[0]
                .graph,
            export
        );
    }
    let prefab = bozzard_scene::Prefab::from_json(include_str!(
        "../../../examples/demo/scenes/assets/gold-yard/drop-block.prefab.json"
    ))
    .unwrap();
    assert_eq!(
        prefab.objects[0].blueprints[0].graph,
        Blueprint::from_json(include_str!(
            "../../../examples/demo/scenes/assets/Blueprints/yard-lifetime.blueprint.json"
        ))
        .unwrap()
    );
    editor.selected = Some("gold".into()); // Controller movement does not depend on selection.
    editor.start_play().unwrap();
    let demo = editor.play.as_mut().unwrap();
    let initial_visible = demo
        .instance()
        .view(&demo.app.world, Layer::ThreeD, 1.)
        .unwrap()
        .objects
        .len();
    let gold = demo.instance().entity("gold").unwrap();
    demo.set_gameplay_input(GameplayInput {
        orbit: [20., -10.],
        ..Default::default()
    });
    step(demo, 1);
    assert!((demo.gameplay().unwrap().yaw - 356.).abs() < 0.001);
    assert_eq!(
        demo.app
            .world
            .get::<Transform>(gold)
            .unwrap()
            .rotation_degrees,
        [9., 36., 0.]
    );
    step(demo, 3);
    assert_eq!(
        demo.app
            .world
            .get::<Transform>(gold)
            .unwrap()
            .rotation_degrees,
        [9., 36., 0.]
    );
    demo.set_gameplay_input(GameplayInput {
        orbit: [-20., 10.],
        ..Default::default()
    });
    step(demo, 1);
    demo.set_gameplay_input(GameplayInput {
        movement: [0., 1.],
        ..Default::default()
    });
    step(demo, 60);
    assert_eq!(demo.gameplay().unwrap().collected.len(), 1);
    assert!(!demo.gameplay().unwrap().won);
    assert_eq!(
        demo.instance()
            .view(&demo.app.world, Layer::ThreeD, 1.)
            .unwrap()
            .objects
            .len(),
        initial_visible - 1
    );
    let base_bodies = demo.instance().physics_body_count(&demo.app.world);
    demo.set_gameplay_input(GameplayInput {
        jump: true,
        ..Default::default()
    });
    step(demo, 10);
    assert!(
        demo.app
            .world
            .get::<Transform>(demo.instance().entity("player").unwrap())
            .unwrap()
            .translation[1]
            > 1.
    );
    step(demo, 90);
    demo.set_gameplay_input(GameplayInput {
        movement: [0., 1.],
        ..Default::default()
    });
    step(demo, 40); // Walk onto the pad; no scripted teleport needed to start the toy.
    demo.clear_gameplay_input();
    assert_eq!(demo.instance().document().prefabs.len(), 1);
    let first_id = demo
        .instance()
        .document()
        .prefabs
        .keys()
        .next()
        .unwrap()
        .clone();
    let first_entity = demo.instance().entity(&first_id).unwrap();
    let first_pose = *demo.app.world.get::<Transform>(first_entity).unwrap();
    // Re-entering immediately must not spam bodies.
    place_player(demo, [0., 0.65, 2.5]);
    step(demo, 1);
    place_player(demo, [0., 0.65, -1.]);
    step(demo, 1);
    assert_eq!(demo.instance().document().prefabs.len(), 1);
    step(demo, 120);
    let fallen = demo.app.world.get::<Transform>(first_entity).unwrap();
    assert!(fallen.translation[1] < first_pose.translation[1] - 1.);
    assert_ne!(fallen.rotation_degrees, first_pose.rotation_degrees);
    // Repeated player entries exercise actual spawn/destroy and solver resource ownership.
    for _ in 0..16 {
        place_player(demo, [0., 0.65, 2.5]);
        step(demo, 65);
        place_player(demo, [0., 0.65, -1.]);
        step(demo, 2);
        let live = demo.instance().document().prefabs.len();
        assert!(live > 0 && live <= 13);
        assert_eq!(demo.app.world.len(), authored.objects.len() + live);
        assert!(demo.instance().physics_body_count(&demo.app.world) <= base_bodies + 13);
    }
    assert!(!demo.app.world.contains(first_entity));
    assert!(demo.instance().entity(&first_id).is_none());
    place_player(demo, [0., 0.65, 2.5]);
    step(demo, 750);
    assert!(demo.instance().document().prefabs.is_empty());
    assert_eq!(demo.app.world.len(), authored.objects.len());
    assert_eq!(
        demo.instance().physics_body_count(&demo.app.world),
        base_bodies
    );
    // Falling off the plane recovers; collecting gold doesn't lock exploration.
    place_player(demo, [20., -10., 0.]);
    step(demo, 1);
    assert_eq!(demo.gameplay().unwrap().respawns, 1);
    assert_eq!(demo.gameplay().unwrap().collected.len(), 1);
    place_player(demo, [-8., 0.65, -7.]);
    step(demo, 1);
    assert!(demo.gameplay().unwrap().won);
    editor.stop_play();
    assert_eq!(editor.scene(), &authored);
    editor.start_play().unwrap();
    let fresh = editor.play.as_ref().unwrap();
    assert!(fresh.gameplay().unwrap().collected.is_empty());
    assert!(!fresh.gameplay().unwrap().won);
    assert_eq!(fresh.instance().physics_body_count(&fresh.app.world), 0);
    assert_eq!(
        fresh.instance().capture(&fresh.app.world).unwrap(),
        authored
    );
}
