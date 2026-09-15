//! A prefab member may carry scripts of its own. Their catalog lives in the prefab file, so the
//! loader has to merge that catalog before it reads script sources — reading sources first left the
//! spawned member pointing at a script nobody had compiled, and the simulation stopped mid-run.
use bozzard_demo::SceneDemo;
use bozzard_scene::{GameAction, GameplayInput, Transform};
use std::path::PathBuf;

const SCENE: &str = r#"{"version":1,"name":"prefab scripts","views":{},
  "assets":{"twin":{"kind":"prefab","path":"assets/twin.prefab.json"}},
  "objects":[{"id":"spawner","name":"spawner",
    "transform":{"translation":[0,0,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]},
    "blueprints":[{"enabled":true,"graph":{"version":1,"name":"spawn","nodes":[
      {"id":1,"position":[0,0],"kind":"start","inputs":[]},
      {"id":2,"position":[0,0],"kind":"spawn_prefab","prefab":"twin","inputs":["exec",{"vector":[0,0,0]}]}],
      "wires":[{"from":{"node":1,"port":0},"to":{"node":2,"port":0}}]}}]}]}"#;

const PREFAB: &str = r#"{"version":1,"name":"twin","root":"root",
  "assets":{"twin-spin":{"kind":"script","path":"spin.rs"}},
  "objects":[{"id":"root","name":"root",
    "transform":{"translation":[0,0,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]},
    "script_manager":{"scripts":[{"enabled":true,"script":"twin-spin"}]}}]}"#;

const SCRIPT: &str = "fn on_start(me) { rotate(me, [0.0, 90.0, 0.0]); }";

#[test]
fn a_spawned_prefab_member_runs_the_script_from_the_prefab_catalog() {
    let root = std::env::temp_dir().join(format!("bozzard-prefab-scripts-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("assets")).unwrap();
    std::fs::write(root.join("scene.json"), SCENE).unwrap();
    std::fs::write(root.join("assets/twin.prefab.json"), PREFAB).unwrap();
    std::fs::write(root.join("assets/spin.rs"), SCRIPT).unwrap();

    let path = PathBuf::from(&root).join("scene.json");
    let document = bozzard_demo::load_document(Some(&path)).unwrap();
    let mut demo = SceneDemo::new_with_prefabs(&document, Some(&path)).unwrap();
    demo.game_action(GameAction::Start).unwrap();
    for _ in 0..2 {
        demo.set_gameplay_input(GameplayInput::default());
        demo.app.step();
        demo.check_simulation()
            .expect("the spawned member's script must have been compiled at load");
    }
    let spawned = demo
        .instance()
        .document()
        .prefabs
        .keys()
        .next()
        .expect("the graph spawns the prefab")
        .clone();
    let entity = demo.instance().entity(&spawned).unwrap();
    assert_eq!(
        demo.app
            .world
            .get::<Transform>(entity)
            .unwrap()
            .rotation_degrees,
        [0.0, 90.0, 0.0],
        "the prefab member's on_start hook must have run"
    );
    std::fs::remove_dir_all(&root).unwrap();
}
