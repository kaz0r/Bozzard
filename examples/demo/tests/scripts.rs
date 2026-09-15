//! End-to-end checks for the Script Lab scene and for how script failures surface.
//!
//! Scripts are the coding half of the gameplay path: they drive the same actions a graph does and
//! share the scene blackboard with it, so one scene can use both.
use bozzard_demo::SceneDemo;
use bozzard_scene::{GameplayInput, Light, TextRendering, Transform};
use std::path::{Path, PathBuf};

fn scene_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scenes/script-lab.json")
}

fn demo() -> SceneDemo {
    let path = scene_path();
    let scene = bozzard_demo::load_document(Some(&path)).unwrap();
    SceneDemo::new_with_prefabs(&scene, Some(&path)).unwrap()
}

fn tick(demo: &mut SceneDemo) {
    demo.set_gameplay_input(GameplayInput::default());
    demo.app.step();
    demo.check_simulation().unwrap();
}

fn text(demo: &SceneDemo, id: &str) -> String {
    demo.app
        .world
        .get::<TextRendering>(demo.instance().entity(id).unwrap())
        .unwrap()
        .text
        .clone()
}

fn translation(demo: &SceneDemo, id: &str) -> [f32; 3] {
    demo.app
        .world
        .get::<Transform>(demo.instance().entity(id).unwrap())
        .unwrap()
        .translation
}

#[test]
fn scripts_run_from_a_scene_catalog_and_share_state_with_a_graph() {
    let mut demo = demo();
    assert!(
        demo.instance().has_scripts() && demo.instance().has_blueprints(),
        "the scene mixes both authoring paths"
    );
    for _ in 0..180 {
        tick(&mut demo);
    }
    // spin.rs rotates its object with the same degrees delta the Rotate node applies.
    let rotation = demo
        .app
        .world
        .get::<Transform>(demo.instance().entity("spinner").unwrap())
        .unwrap()
        .rotation_degrees;
    assert!(
        rotation[1] > 1.0,
        "spin.rs must have turned the cube, got {rotation:?}"
    );

    // orbit.rs keeps its phase in a scene variable and circles the origin.
    let orbiter = translation(&demo, "orbiter");
    let radius = (orbiter[0] * orbiter[0] + orbiter[2] * orbiter[2]).sqrt();
    assert!(
        (radius - 3.0).abs() < 0.05,
        "orbit.rs must stay on its radius, got {radius} at {orbiter:?}"
    );

    // pulse.rs fades the lamp through `sin`.
    let intensity = demo
        .app
        .world
        .get::<Light>(demo.instance().entity("lamp").unwrap())
        .unwrap()
        .intensity;
    assert!(
        (1.0..=7.0).contains(&intensity),
        "pulse.rs must keep the light inside its swing, got {intensity}"
    );

    // gate.rs counts what the orbiter passes through and writes the HUD text object.
    let script_line = text(&demo, "hud-status");
    assert!(
        script_line.starts_with("PASSES 1"),
        "the gate script must have counted the orbiter, got {script_line:?}"
    );

    // The graph on the neighbouring text object reads the same scene variable the script wrote.
    assert_eq!(text(&demo, "hud-graph"), "GRAPH PASSES 1");

    // drop.rs bounces the rigidbody off the floor, so it never comes to rest on it.
    let dropper = translation(&demo, "dropper").get(1).copied().unwrap();
    assert!(
        dropper > 0.4,
        "drop.rs must have bounced the body clear of the floor, got y {dropper}"
    );
}

/// A throwaway project directory: a scene and its scripts, laid out like a real one.
fn project(script: &str) -> (PathBuf, PathBuf) {
    // A fresh directory per call: tests in one binary run in parallel.
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let serial = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let root =
        std::env::temp_dir().join(format!("bozzard-scripts-{}-{serial}", std::process::id()));
    std::fs::create_dir_all(root.join("scripts")).unwrap();
    std::fs::write(root.join("scripts/broken.rs"), script).unwrap();
    let scene = root.join("scene.json");
    std::fs::write(
        &scene,
        r#"{"version":1,"name":"broken","views":{},
            "blackboard":{},
            "assets":{"broken":{"kind":"script","path":"scripts/broken.rs"}},
            "objects":[{"id":"thing","name":"thing",
                "transform":{"translation":[0,0,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]},
                "script_manager":{"scripts":[{"enabled":true,"script":"broken"}]}}]}"#,
    )
    .unwrap();
    (scene, root)
}

fn open(scene: &Path) -> anyhow::Result<SceneDemo> {
    let document = bozzard_demo::load_document(Some(scene))?;
    SceneDemo::new_with_prefabs(&document, Some(scene))
}

#[test]
fn a_syntax_error_fails_when_the_scene_opens_not_on_the_first_tick() {
    let (scene, root) = project("fn on_update(me, dt) {\n    let x = ;\n}\n");
    let error = match open(&scene) {
        Ok(_) => panic!("a syntax error must fail when the scene opens"),
        Err(error) => error,
    };
    assert!(
        format!("{error:#}").contains("script 'broken'"),
        "{error:#}"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn a_throwing_script_stops_the_simulation_and_names_its_hook() {
    let (scene, root) =
        project("fn on_update(me, dt) {\n    set_position(\"nowhere\", [1.0, 1.0, 1.0]);\n}\n");
    let mut demo = open(&scene).unwrap();
    demo.set_gameplay_input(GameplayInput::default());
    demo.app.step();
    let error = demo.check_simulation().unwrap_err();
    let message = format!("{error:#}");
    assert!(
        message.contains("on_update") && message.contains("nowhere"),
        "{message}"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn script_console_preserves_owner_and_severity() {
    use bozzard_diagnostics::{Diagnostics, Level};
    let (path, root) = project(
        r#"fn on_start(me) { print("hello"); log_warning("careful"); log_error("failed"); }"#,
    );
    let mut demo = open(&path).unwrap();
    tick(&mut demo);
    let events = &demo
        .app
        .world
        .resource::<Diagnostics>()
        .unwrap()
        .console
        .events;
    assert_eq!(events.len(), 3);
    assert_eq!(
        events.iter().map(|e| e.level).collect::<Vec<_>>(),
        vec![Level::Info, Level::Warning, Level::Error]
    );
    assert!(
        events
            .iter()
            .all(|e| e.source == "Script" && e.location.object.as_deref() == Some("thing"))
    );
    let _ = std::fs::remove_dir_all(root);
}
