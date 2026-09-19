use bozzard_editor::Editor;
use bozzard_scene::{GameplayInput, Scene, TextRendering};
use std::path::Path;

fn editor() -> Editor {
    Editor::open(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/demo/scenes/hud-lab.json"),
    )
    .unwrap()
}
#[test]
fn hud_counter_roundtrips_updates_in_play_and_restores_authored_text() {
    let mut editor = editor();
    let scene = editor.scene().clone();
    assert_eq!(Scene::from_json(&scene.to_json().unwrap()).unwrap(), scene);
    editor.start_play().unwrap();
    let play = editor.play.as_mut().unwrap();
    let id = play.instance().entity("counter").unwrap();
    for count in 1..=12 {
        play.set_gameplay_input(GameplayInput {
            jump: true,
            ..Default::default()
        });
        play.app.step();
        play.check_simulation().unwrap();
        assert_eq!(
            play.app.world.get::<TextRendering>(id).unwrap().text,
            format!("Taps: {count}")
        );
    }
    let captured = play.instance().capture(&play.app.world).unwrap();
    assert_eq!(
        captured
            .objects
            .iter()
            .find(|o| o.id == "counter")
            .unwrap()
            .text_rendering
            .as_ref()
            .unwrap()
            .text,
        "Taps: 12"
    );
    editor.stop_play();
    assert_eq!(editor.scene(), &scene);
    assert!(!editor.dirty());
    editor.start_play().unwrap();
    let play = editor.play.as_mut().unwrap();
    play.app.step();
    play.check_simulation().unwrap();
    assert_eq!(
        play.app
            .world
            .get::<TextRendering>(play.instance().entity("counter").unwrap())
            .unwrap()
            .text,
        "Taps: 0"
    );
}
#[test]
fn invalid_text_output_does_not_replace_the_label() {
    let mut editor = editor();
    let mut scene = editor.scene().clone();
    let counter = scene
        .objects
        .iter_mut()
        .find(|o| o.id == "counter")
        .unwrap();
    let join = counter.blueprints[0]
        .graph
        .nodes
        .iter_mut()
        .find(|n| n.kind == bozzard_scene::blueprint::NodeKind::JoinText)
        .unwrap();
    join.inputs[0] = bozzard_scene::blueprint::Value::Text("x".repeat(4096));
    editor.apply("Long prefix", scene).unwrap();
    editor.start_play().unwrap();
    let play = editor.play.as_mut().unwrap();
    play.app.step();
    assert!(play.check_simulation().is_err());
    assert_eq!(
        play.app
            .world
            .get::<TextRendering>(play.instance().entity("counter").unwrap())
            .unwrap()
            .text,
        "Taps: 0"
    );
}
#[test]
fn hud_does_not_pollute_world_bounds_and_positions_validate() {
    let mut editor = editor();
    let original = editor.scene().clone();
    let before = editor
        .frame_bounds(bozzard_scene::Layer::ThreeD, None)
        .unwrap();
    let mut changed = original.clone();
    let counter = changed
        .objects
        .iter_mut()
        .find(|o| o.id == "counter")
        .unwrap();
    counter.transform.translation = [5000.; 3];
    editor.apply("Move HUD entity", changed.clone()).unwrap();
    assert_eq!(
        editor
            .frame_bounds(bozzard_scene::Layer::ThreeD, None)
            .unwrap(),
        before
    );
    changed
        .objects
        .iter_mut()
        .find(|o| o.id == "counter")
        .unwrap()
        .text_rendering
        .as_mut()
        .unwrap()
        .screen
        .as_mut()
        .unwrap()
        .anchor[0] = 2.;
    assert!(editor.apply("Invalid anchor", changed).is_err());
    editor.undo().unwrap();
    assert_eq!(editor.scene(), &original);
}

#[test]
fn hud_picking_matches_anchor_at_two_display_scales_without_editing() {
    let mut editor = editor();
    let scene = editor.scene().clone();
    let source = scene
        .objects
        .iter()
        .find(|o| o.id == "counter")
        .unwrap()
        .text_rendering
        .as_ref()
        .unwrap();
    let mesh = bozzard_render_assets::text_mesh(source, &editor.assets).unwrap();
    let [min, max] = bozzard_render::text_bounds(&mesh).unwrap().unwrap();
    for (size, scale) in [([800, 600], 1.), ([1600, 1200], 2.)] {
        let point = mesh
            .screen
            .unwrap()
            .matrix(size, scale)
            .transform_point3((min + max) * 0.5);
        let picked = editor
            .pick_hud(
                bozzard_scene::Layer::ThreeD,
                size,
                scale,
                [point.x, point.y],
            )
            .unwrap();
        assert_eq!(picked.as_deref(), Some("counter"));
        editor.select_object(picked);
        assert_eq!(editor.scene(), &scene);
        assert!(
            editor
                .pick_hud(bozzard_scene::Layer::TwoD, size, scale, [point.x, point.y])
                .unwrap()
                .is_none()
        );
    }
}
