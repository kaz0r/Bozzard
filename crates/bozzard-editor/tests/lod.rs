use bozzard_editor::Editor;
use bozzard_render::{Gpu, SceneRenderer, capture_offscreen, wgpu};
use bozzard_scene::{Layer, Scene};

#[test]
fn inspection_and_effects_preview_choose_lod_from_their_actual_camera() -> anyhow::Result<()> {
    let scene = Scene::from_json(
        r#"{"version":1,"name":"inspection LOD","views":{"3d":"camera"},"objects":[
      {"id":"camera","name":"Camera","transform":{"translation":[0,0,100],"rotation_degrees":[0,0,0],"scale":[1,1,1]},"camera":{"projection":"perspective","vertical_fov_degrees":60,"near":0.1,"far":1000}},
      {"id":"mesh","name":"Mesh","transform":{"translation":[0,0,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]},"drawable":{"layer":"3d","mesh":"cube","texture":"white","color":[1,1,1],"uv_scale":[1,1]},"lod":{"levels":[{"switch":10,"mesh":"quad"},{"switch":50}]}}
    ]}"#,
    )?;
    let editor = Editor::new(scene.clone(), std::path::Path::new("inspection-lod.json"))?;
    let mut preview = bozzard_editor::EffectsPreview::new(&editor)?;
    assert!(editor.render(Layer::ThreeD, 1.)?.items.is_empty());
    for (distance, expected) in [
        (3., bozzard_render::MeshKind::Cube),
        (20., bozzard_render::MeshKind::Quad),
    ] {
        let pose = Some(glam::Mat4::from_translation(glam::Vec3::new(
            0., 0., distance,
        )));
        let frame = editor.render_from_camera(Layer::ThreeD, 1., pose)?;
        assert_eq!(frame.items[0].mesh, expected);
        let effects = preview.render_from_camera(&editor, Layer::ThreeD, 1., pose)?;
        assert_eq!(effects.items[0].mesh, expected);
        assert_eq!(effects.view_projection, frame.view_projection);
    }
    assert_eq!(editor.scene(), &scene);
    assert!(editor.render(Layer::ThreeD, 1.)?.items.is_empty());
    Ok(())
}

#[test]
fn imported_lod_renders_matches_reference_and_supports_undo() -> anyhow::Result<()> {
    let directory = std::env::temp_dir().join(format!("bozzard-lod-{}", std::process::id()));
    std::fs::create_dir_all(&directory)?;
    std::fs::write(
        directory.join("low.obj"),
        "v -1 -1 0\nv 1 -1 0\nv 0 1 0\nf 1 2 3\n",
    )?;
    let mut scene = Scene::from_json(
        r#"{"version":1,"name":"LOD","views":{"3d":"camera"},
      "assets":{"low":{"kind":"mesh","path":"low.obj"}},"objects":[
      {"id":"camera","name":"Camera","transform":{"translation":[0,0,3],"rotation_degrees":[0,0,0],"scale":[1,1,1]},"camera":{"projection":"perspective","vertical_fov_degrees":60,"near":0.1,"far":100}},
      {"id":"mesh","name":"Mesh","transform":{"translation":[0,0,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]},
       "drawable":{"layer":"3d","mesh":"cube","texture":"white","color":[1,1,1],"uv_scale":[1,1]},
       "lod":{"levels":[{"switch":4,"mesh":{"asset":"low"}},{"switch":8,"mesh":null}]}}
      ]}"#,
    )?;
    scene.lighting.shadows = false;
    scene.environment.intensity = 0.;
    scene.environment.background = false;
    let mut editor = Editor::new(scene.clone(), &directory.join("scene.json"))?;
    let instance = wgpu::Instance::default();
    let gpu = pollster::block_on(Gpu::request_prefer_software(&instance))?;
    let mut renderer = SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm);
    bozzard_render_assets::Residency::default().sync(&gpu, &mut renderer, &editor.assets)?;
    let mut counts = Vec::new();
    for (distance, expected) in [(3., 12), (5., 1), (9., 0)] {
        scene.objects[0].transform.translation[2] = distance;
        editor.apply("Move camera", scene.clone())?;
        let rendered = editor.render(Layer::ThreeD, 1.)?;
        let frame = capture_offscreen(&gpu, 64, 64, |target| {
            renderer.draw(&gpu, target, [64; 2], &rendered)
        })?;
        let triangles = renderer.frame_stats().color_triangles;
        assert_eq!(triangles, expected);
        counts.push(triangles);
        // Compare to explicitly authored geometry at the same camera pose.
        let mut reference = scene.clone();
        reference.objects[1].lod = None;
        if distance == 5. {
            reference.objects[1].drawable.as_mut().unwrap().mesh =
                bozzard_scene::Mesh::Asset("low".into());
        } else if distance == 9. {
            reference.objects[1].drawable = None;
        }
        let reference =
            Editor::new(reference, &directory.join("scene.json"))?.render(Layer::ThreeD, 1.)?;
        let expected_frame = capture_offscreen(&gpu, 64, 64, |target| {
            renderer.draw(&gpu, target, [64; 2], &reference)
        })?;
        assert_eq!(frame.rgba, expected_frame.rgba);
    }
    eprintln!("LOD 64x64 triangle counts: {counts:?} (cube -> imported OBJ -> culled)");
    editor.undo()?;
    assert_eq!(editor.scene().objects[0].transform.translation[2], 5.);
    assert!(matches!(
        editor.render(Layer::ThreeD, 1.)?.items[0].mesh,
        bozzard_render::MeshKind::Imported(_)
    ));
    let mut missing = scene;
    missing.assets.get_mut("low").unwrap().path = "missing.obj".into();
    assert!(Editor::new(missing, &directory.join("scene.json")).is_err());
    std::fs::remove_dir_all(directory)?;
    Ok(())
}
