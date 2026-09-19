//! Portable material authoring fixture with two variants and an instance override.
use anyhow::{Context, Result, ensure};
use bozzard_editor::Editor;
use bozzard_scene::{
    Layer, Scene,
    material_asset::{MaterialAsset, MaterialShader, MaterialTexture},
    shader_graph::{Node, NodeKind, ShaderGraph, Socket, Value, Wire},
};
use std::{fs, path::PathBuf};
fn main() -> Result<()> {
    let root = PathBuf::from(
        std::env::args()
            .nth(1)
            .context("usage: shared_materials OUTPUT_DIRECTORY")?,
    );
    ensure!(!root.exists(), "choose a new fixture directory");
    fs::create_dir_all(&root)?;
    let mut scene = Scene::from_json(
        r#"{"version":1,"name":"Shared Material Workshop","views":{"3d":"camera"},"objects":[
      {"id":"camera","name":"Camera","transform":{"translation":[0,2,8],"rotation_degrees":[-12,0,0],"scale":[1,1,1]},"camera":{"projection":"perspective","vertical_fov_degrees":48,"near":0.1,"far":100}},
      {"id":"base-a","name":"Base A","transform":{"translation":[-2.4,0,0],"rotation_degrees":[10,25,0],"scale":[1.3,1.3,1.3]},"drawable":{"layer":"3d","mesh":"cube","texture":"white","color":[1,1,1],"uv_scale":[1,1]}},
      {"id":"base-b","name":"Base B","transform":{"translation":[-0.8,0,0],"rotation_degrees":[10,25,0],"scale":[1.3,1.3,1.3]},"drawable":{"layer":"3d","mesh":"cube","texture":"white","color":[1,1,1],"uv_scale":[1,1]}},
      {"id":"variant","name":"Matte Variant","transform":{"translation":[0.8,0,0],"rotation_degrees":[10,25,0],"scale":[1.3,1.3,1.3]},"drawable":{"layer":"3d","mesh":"cube","texture":"white","color":[1,1,1],"uv_scale":[1,1]}},
      {"id":"instance","name":"Tint Override","transform":{"translation":[2.4,0,0],"rotation_degrees":[10,25,0],"scale":[1.3,1.3,1.3]},"drawable":{"layer":"3d","mesh":"cube","texture":"white","color":[1,1,1],"uv_scale":[1,1]}}
    ]}"#,
    )?;
    scene.lighting.ambient_intensity = 0.3;
    let path = root.join("scene.json");
    let mut editor = Editor::new(scene, &path)?;
    let base = editor.create_material(None)?;
    fs::copy(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/demo/scenes/assets/palette.png"),
        root.join("assets/Materials/map.png"),
    )?;
    let saved = editor.material_source(&base)?;
    let mut definition = MaterialAsset::from_json(&saved)?;
    definition.name = "Workshop Finish".into();
    definition.texture = Some(MaterialTexture::Image("map.png".into()));
    definition.properties.color = Some([0.9, 0.8, 0.5]);
    definition.properties.metallic = Some(0.1);
    let mut graph = ShaderGraph {
        name: "Finish".into(),
        ..Default::default()
    };
    graph.keywords.insert("MATTE".into(), false);
    let mut node = Node::new(2, NodeKind::StaticSwitch, [20., 100.]);
    node.keyword = "MATTE".into();
    node.inputs = vec![Value::Float(0.12), Value::Float(0.85)];
    graph.nodes.push(node);
    graph.nodes[0].position = [320., 100.];
    graph.connect(Wire {
        from: Socket { node: 2, port: 0 },
        to: Socket { node: 1, port: 2 },
    })?;
    definition.shader = MaterialShader::Graph(graph);
    editor.save_material(&base, &saved, &definition)?;
    let variant = editor.create_material(Some(&base))?;
    let saved = editor.material_source(&variant)?;
    let mut definition = MaterialAsset::from_json(&saved)?;
    definition.name = "Matte Finish".into();
    definition.keywords.insert("MATTE".into(), true);
    editor.save_material(&variant, &saved, &definition)?;
    for (id, asset) in [
        ("base-a", &base),
        ("base-b", &base),
        ("variant", &variant),
        ("instance", &base),
    ] {
        editor.select_object(Some(id.into()));
        editor.assign_asset_to_selected(asset)?;
    }
    let mut scene = editor.scene().clone();
    scene
        .objects
        .iter_mut()
        .find(|o| o.id == "instance")
        .unwrap()
        .material
        .as_mut()
        .unwrap()
        .set_color([0.1, 0.7, 1.]);
    editor.apply("Local tint override", scene)?;
    let frame = editor.render(Layer::ThreeD, 1.6)?;
    ensure!(
        frame.items.len() == 4,
        "fixture must render four material instances"
    );
    ensure!(
        std::sync::Arc::ptr_eq(
            frame.items[0].material.shader.as_ref().unwrap(),
            frame.items[1].material.shader.as_ref().unwrap()
        ),
        "base shaders must share code"
    );
    editor.save(&path)?;
    fs::write(
        root.join("game.bozzard.json"),
        r#"{"version":1,"name":"Shared Material Workshop","start_scene":"scene.json","view":"3d","cook":"universal"}"#,
    )?;
    println!("{}", path.display());
    Ok(())
}
