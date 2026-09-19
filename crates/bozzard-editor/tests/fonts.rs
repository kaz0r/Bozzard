use bozzard_editor::Editor;
use bozzard_scene::{AssetKind, AssetSource, Camera, Layer, Scene, TextFont, TextRendering};
use std::{collections::BTreeMap, path::PathBuf};

/// Reuses the TTF vendored for the importer test in bozzard-assets.
fn vendored_font() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../bozzard-assets/tests/fonts/test.ttf")
}

fn font_scene() -> Scene {
    Scene {
        version: bozzard_scene::SCENE_VERSION,
        name: "font-lab".into(),
        views: BTreeMap::from([(Layer::ThreeD, "camera".into())]),
        objects: vec![
            bozzard_scene::Object {
                id: "camera".into(),
                name: "Camera".into(),
                transform: bozzard_scene::Transform {
                    translation: [0., 0., 3.],
                    ..Default::default()
                },
                camera: Some(Camera::Perspective {
                    vertical_fov_degrees: 60.,
                    near: 0.1,
                    far: 100.,
                }),
                ..Default::default()
            },
            bozzard_scene::Object {
                id: "label".into(),
                name: "Label".into(),
                text_rendering: Some(TextRendering {
                    text: "hello".into(),
                    font: TextFont::Custom("brand".into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
        ],
        assets: BTreeMap::from([(
            "brand".into(),
            AssetSource {
                kind: AssetKind::Font,
                path: "test.ttf".into(),
            },
        )]),
        fog: Default::default(),
        gi: Default::default(),
        environment: Default::default(),
        display: Default::default(),
        lighting: Default::default(),
        blackboard: Default::default(),
        runtime_scenes: Default::default(),
        runtime_scene_sources: Default::default(),
        game_flow: None,
        prefabs: Default::default(),
        post_process_volumes: Vec::new(),
    }
}

#[test]
fn custom_font_renders_bounds_and_fails_closed_on_missing_or_invalid_assets() -> anyhow::Result<()>
{
    let project = std::env::temp_dir().join(format!("bozzard-editor-fonts-{}", std::process::id()));
    std::fs::create_dir_all(&project)?;
    std::fs::copy(vendored_font(), project.join("test.ttf"))?;
    let scene_path = project.join("scene.json");
    let mut editor = Editor::new(font_scene(), &scene_path)?;
    editor.assets.load_pending()?;
    assert_eq!(
        Scene::from_json(&editor.scene().to_json()?)?,
        editor.scene().clone()
    );
    let render = editor.render(Layer::ThreeD, 1.)?;
    assert_eq!(
        render
            .items
            .iter()
            .filter(|item| matches!(item.mesh, bozzard_render::MeshKind::Text(_)))
            .count(),
        1
    );
    let bounds = bozzard_render::text_bounds(&bozzard_render_assets::text_mesh(
        editor.scene().objects[1].text_rendering.as_ref().unwrap(),
        &editor.assets,
    )?)?
    .unwrap();
    assert!(bounds[1][0] > bounds[0][0]);
    assert_ne!(
        bounds,
        bozzard_render::text_bounds(&bozzard_render_assets::text_mesh(
            &bozzard_scene::TextRendering {
                font: Default::default(),
                ..editor.scene().objects[1].text_rendering.clone().unwrap()
            },
            &editor.assets,
        )?)
        .unwrap()
        .unwrap()
    );
    // A font asset that fails to load must fail the editor open, not fall back silently.
    std::fs::remove_file(project.join("test.ttf"))?;
    assert!(Editor::new(font_scene(), &scene_path).is_err());
    // An empty custom font id is rejected by scene validation.
    let mut scene = font_scene();
    if let Some(text) = &mut scene.objects[1].text_rendering {
        text.font = TextFont::Custom(String::new());
    }
    assert!(scene.validate().is_err());
    // The browser's assign action targets the Text Rendering font field.
    std::fs::copy(vendored_font(), project.join("test.ttf"))?;
    let mut editor = Editor::new(
        {
            let mut scene = font_scene();
            if let Some(text) = &mut scene.objects[1].text_rendering {
                text.font = TextFont::Sans;
            }
            scene
        },
        &scene_path,
    )?;
    editor.assets.load_pending()?;
    editor.select_object(Some("label".into()));
    editor.assign_asset_to_selected("brand")?;
    assert_eq!(
        editor.scene().objects[1]
            .text_rendering
            .as_ref()
            .unwrap()
            .font,
        TextFont::Custom("brand".into())
    );
    editor.select_object(Some("camera".into()));
    assert!(editor.assign_asset_to_selected("brand").is_err());
    let _ = std::fs::remove_dir_all(&project);
    Ok(())
}

#[test]
fn variable_fonts_and_fallbacks_round_trip_history_prefabs_and_native_pixels() -> anyhow::Result<()>
{
    use bozzard_assets::job::Job;
    use bozzard_editor::PrefabCommand;
    use bozzard_render::*;
    fn wait<T: Send + 'static>(job: &Job<T>) -> anyhow::Result<T> {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        loop {
            if let Some(result) = job.poll() {
                return result;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }
    let path = std::env::temp_dir().join(format!("bozzard-font-styles-{}", std::process::id()));
    std::fs::create_dir_all(&path)?;
    std::fs::copy(vendored_font(), path.join("test.ttf"))?;
    std::fs::write(
        path.join("variable.ttf"),
        include_bytes!("../../bozzard-text/tests/fonts/Roboto.ttf"),
    )?;
    let mut scene = font_scene();
    scene.assets.insert(
        "variable".into(),
        AssetSource {
            kind: AssetKind::Font,
            path: "variable.ttf".into(),
        },
    );
    let text = scene.objects[1].text_rendering.as_mut().unwrap();
    text.font = TextFont::Custom("variable".into());
    text.text = "Wide 😀".into();
    text.font_size = 0.4;
    let mut editor = Editor::new(scene, &path.join("scene.json"))?;
    let gpu = pollster::block_on(Gpu::request(&instance(Backend::native()), None, false))?;
    let mut renderer = SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm);
    let capture = |editor: &mut Editor, renderer: &mut SceneRenderer| -> anyhow::Result<Frame> {
        let scene = editor.render(Layer::ThreeD, 2.)?;
        capture_offscreen(&gpu, 256, 128, |target| {
            renderer.draw(&gpu, target, [256, 128], &scene)
        })
    };
    let original = capture(&mut editor, &mut renderer)?;
    let mut changed = editor.scene().clone();
    let style = changed.objects[1].text_rendering.as_mut().unwrap();
    style.font_axes.insert("wdth".into(), 75.);
    style.font_axes.insert("wght".into(), 900.);
    style.font_fallbacks.push("brand".into());
    style.builtin_font_fallback = true;
    let expected = style.clone();
    editor.apply("Font style", changed)?;
    let styled = capture(&mut editor, &mut renderer)?;
    assert_ne!(original.rgba, styled.rgba);
    for _ in 0..3 {
        assert_eq!(capture(&mut editor, &mut renderer)?.rgba, styled.rgba);
    }
    editor.undo()?;
    assert_eq!(capture(&mut editor, &mut renderer)?.rgba, original.rgba);
    editor.redo()?;
    assert_eq!(capture(&mut editor, &mut renderer)?.rgba, styled.rgba);
    let mut invalid = editor.scene().clone();
    invalid.objects[1]
        .text_rendering
        .as_mut()
        .unwrap()
        .font_axes
        .insert("wght".into(), 10000.);
    assert!(editor.apply("Invalid style", invalid).is_err());
    editor.select_object(Some("label".into()));
    let job = editor.prefab_job(PrefabCommand::Create)?;
    let prepared = wait(&job)?;
    let id = editor.accept_prefab(prepared)?;
    let job = editor.prefab_job(PrefabCommand::Instantiate {
        asset: id,
        position: Some([0., -1., 0.]),
    })?;
    let prepared = wait(&job)?;
    editor.accept_prefab(prepared)?;
    assert_eq!(
        editor.selected_object().unwrap().text_rendering.as_ref(),
        Some(&expected)
    );
    let job = editor.save_job(path.join("scene.json"))?;
    let saved = wait(&job)?;
    editor.accept_save(saved)?;
    let loaded = Editor::open(&path.join("scene.json"))?;
    assert_eq!(loaded.scene(), editor.scene());
    let mut remapped = loaded.scene().objects[1].clone();
    remapped.remap_assets(&BTreeMap::from([
        ("brand".into(), "moved-brand".into()),
        ("variable".into(), "moved-variable".into()),
    ]));
    let deps = remapped.asset_dependencies();
    assert!(
        deps.contains(&("moved-brand", AssetKind::Font))
            && deps.contains(&("moved-variable", AssetKind::Font))
    );
    std::fs::remove_dir_all(path)?;
    Ok(())
}
