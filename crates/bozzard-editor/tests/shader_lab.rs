use bozzard_editor::Editor;
use bozzard_render_assets::shader_source;
use bozzard_scene::{Layer, Scene};
use std::time::Duration;

#[test]
fn shader_lab_loads_and_codegens_all_graphs() -> anyhow::Result<()> {
    let scene = Scene::from_json(include_str!(
        "../../../examples/demo/scenes/shader-node-lab.json"
    ))
    .unwrap();
    let graphs: Vec<_> = scene
        .objects
        .iter()
        .filter_map(|o| o.shader_graph.as_ref())
        .collect();
    let names: Vec<_> = graphs.iter().map(|g| g.name.as_str()).collect();
    assert!(
        names.contains(&"Pulse Emissive") && names.contains(&"Texture Fade"),
        "example scene should embed the two example graphs, found {names:?}"
    );
    for graph in &graphs {
        graph.validate().unwrap();
        let code = shader_source(graph).unwrap();
        assert!(code.surface.contains("graph_material_surface"));
        assert!(code.surface.contains("params."));
    }
    let water =
        Scene::from_json(include_str!("../../../examples/demo/scenes/water-lab.json")).unwrap();
    let water_graphs: Vec<_> = water
        .objects
        .iter()
        .filter_map(|o| o.shader_graph.clone())
        .collect();
    assert_eq!(water_graphs.len(), 1, "water lab embeds the Water graph");
    let wsource = shader_source(&water_graphs[0]).unwrap();
    // Water drives the surface normal and emissive sparkle; base stays opaque.
    assert!(wsource.surface.contains("params.normal"));
    assert!(wsource.surface.contains("params.emissive"));
    assert!(!wsource.surface.contains("params.alpha ="));

    // Buoy blueprints: in Play the rocks bob on the same wave field the shader
    // glitters with; base offsets are preserved so they never sink or drift.
    let rocks: Vec<_> = water
        .objects
        .iter()
        .filter(|o| o.id.starts_with("rock-"))
        .cloned()
        .collect();
    assert_eq!(rocks.len(), 4);
    let base: Vec<_> = rocks.iter().map(|o| o.transform.translation).collect();
    let mut editor = Editor::new(
        water,
        &std::env::temp_dir().join("bozzard-shader-time-test/water.json"),
    )?;
    editor.start_play()?;
    editor.advance(Duration::from_secs_f32(1.));
    let play = editor.play.as_ref().unwrap();
    for (rock, base) in rocks.iter().zip(&base) {
        let entity = play.instance().entity(&rock.id).unwrap();
        let t = play
            .app
            .world
            .get::<bozzard_scene::Transform>(entity)
            .unwrap();
        assert!(
            (t.translation[1] - base[1]).abs() < 0.3,
            "bob amplitude sane"
        );
    }
    editor.advance(Duration::from_secs_f32(0.4));
    let play = editor.play.as_ref().unwrap();
    let entity = play.instance().entity(&rocks[0].id).unwrap();
    let t = play
        .app
        .world
        .get::<bozzard_scene::Transform>(entity)
        .unwrap();
    assert!(
        (t.translation[1] - base[0][1]).abs() > f32::EPSILON,
        "rock bobbed: {} vs base {}",
        t.translation[1],
        base[0][1]
    );
    // A wave constant accidentally left on a wired input codegens to `* 0.0`,
    // flattening every sine to a constant (ship-stopping: renders uniform).
    for graph in graphs.iter().copied().chain(water_graphs.iter()) {
        let code = shader_source(graph).unwrap();
        assert!(
            !code.surface.contains("* 0.0)"),
            "{} has a zeroed multiplier",
            graph.name
        );
    }
    // Pulse Emissive drives only Emissive; Texture Fade drives Base Color and Alpha.
    let pulse = shader_source(graphs.iter().find(|g| g.name == "Pulse Emissive").unwrap())
        .unwrap()
        .surface
        .clone();
    assert!(pulse.contains("params.emissive"));
    assert!(!pulse.contains("params.base ="));
    let fade = shader_source(graphs.iter().find(|g| g.name == "Texture Fade").unwrap())
        .unwrap()
        .surface
        .clone();
    assert!(fade.contains("params.base ="));
    assert!(fade.contains("params.alpha = clamp"));
    Ok(())
}

#[test]
fn shader_time_follows_simulation_not_edit_preview() -> anyhow::Result<()> {
    use bozzard_scene::shader_graph::{Node, NodeKind, ShaderGraph};

    let mut graph = ShaderGraph::default();
    graph.nodes.push(Node::new(2, NodeKind::Time, [40., 40.]));
    let mut scene = bozzard_runtime::scene_document()?;
    scene.objects.retain(|o| o.drawable.is_none());
    scene.objects[0].shader_graph = Some(graph);
    let dir = std::env::temp_dir().join("bozzard-shader-time-test");
    std::fs::create_dir_all(&dir)?;
    let mut editor = Editor::new(scene, &dir.join("scene.json"))?;
    // Editing never animates materials.
    assert_eq!(editor.render(Layer::ThreeD, 1.).unwrap().shader_time, 0.);

    // The isolated effects preview animates its own clock for particles and
    // atmosphere, but shader Time stays at zero outside Play.
    let mut preview = bozzard_editor::EffectsPreview::new(&editor)?;
    preview.advance(&editor, Duration::from_secs_f32(1.), true)?;
    assert!(
        preview
            .render(&editor, Layer::ThreeD, 1.)?
            .display
            .time_seconds
            > 0.
    );
    assert_eq!(preview.render(&editor, Layer::ThreeD, 1.)?.shader_time, 0.);

    // Play runs the simulation clock, which is what Time nodes read.
    editor.start_play()?;
    editor.advance(Duration::from_secs_f32(2.));
    assert!(editor.render(Layer::ThreeD, 1.).unwrap().shader_time > 0.);
    Ok(())
}
