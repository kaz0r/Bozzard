use bozzard_editor::Editor;
use bozzard_scene::{
    Blueprint, BlueprintAttachment, Scene,
    blueprint::{BlackboardValue as B, Node, NodeKind, PinType, Value, VariableScope},
};
#[test]
fn blackboards_and_graph_edits_share_history_and_play_isolation() {
    let mut scene=Scene::from_json(r#"{"version":1,"name":"scope edit","views":{},"objects":[{"id":"owner","name":"Owner","transform":{"translation":[0,0,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]}}]}"#).unwrap();
    scene.objects[0]
        .blackboard
        .insert("score".into(), B::Scalar(Value::Number(5.)));
    let mut get = Node::new(3, NodeKind::GetVariable, [0.; 2]);
    get.scope = VariableScope::Object;
    get.variable = "score".into();
    get.value_type = PinType::Number;
    let mut graph = Blueprint::default();
    graph.nodes.push(get);
    scene.objects[0].blueprints.push(BlueprintAttachment {
        enabled: true,
        graph,
    });
    let root = std::env::temp_dir().join(format!("bozzard-depth-editor-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let mut e = Editor::new(scene.clone(), &root.join("scene.json")).unwrap();
    let mut next = scene.clone();
    next.objects[0]
        .blackboard
        .insert("score".into(), B::Scalar(Value::Number(9.)));
    e.apply("Change shared default", next.clone()).unwrap();
    e.undo().unwrap();
    assert_eq!(e.scene(), &scene);
    e.redo().unwrap();
    assert_eq!(e.scene(), &next);
    e.start_play().unwrap();
    e.advance(std::time::Duration::from_millis(30));
    assert_eq!(e.scene(), &next);
    e.stop_play();
    assert_eq!(e.scene(), &next);
    let mut invalid = next.clone();
    invalid.objects[0].blackboard.clear();
    assert!(e.apply("Remove used variable", invalid).is_err());
    assert_eq!(e.scene(), &next);
    let path = root.join("level.json");
    std::fs::write(&path, scene.to_json().unwrap()).unwrap();
    e.import_runtime_scene("next", &path).unwrap();
    assert!(e.scene().runtime_scenes.contains_key("next"));
    e.undo().unwrap();
    assert!(e.scene().runtime_scenes.is_empty());
    std::fs::remove_dir_all(root).unwrap();
}
