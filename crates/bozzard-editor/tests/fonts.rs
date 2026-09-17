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
