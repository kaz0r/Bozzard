use bozzard_project::{Project, prepare_export};
use bozzard_scene::{Layer, Scene};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        loop {
            let path = std::env::temp_dir().join(format!(
                "bozzard-export-test-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match fs::create_dir(&path) {
                Ok(()) => return Self(path),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => panic!("{e}"),
            }
        }
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/demo/scenes")
}

#[test]
fn cooked_textures_export_with_platform_payloads_and_no_source_dependency() -> anyhow::Result<()> {
    use bozzard_assets::{
        AssetData, AssetStore, ImageData,
        job::Progress,
        texture::{self, Compression},
    };
    use bozzard_scene::{AssetKind, AssetSource, Drawable, Mesh, Object, Texture};
    use std::sync::Arc;
    let temp = Temp::new();
    let source = temp.0.join("source");
    fs::create_dir(&source)?;
    let scene_path = source.join("scene.json");
    let mut scene = Scene::from_json(r#"{"version":1,"name":"Cooked","views":{},"objects":[]}"#)?;
    scene.views.insert(Layer::ThreeD, "camera".into());
    scene.objects.push(Object {
        id: "camera".into(),
        name: "Camera".into(),
        camera: Some(bozzard_scene::Camera::Perspective {
            vertical_fov_degrees: 60.,
            near: 0.1,
            far: 100.,
        }),
        transform: bozzard_scene::Transform {
            translation: [0., 0., 3.],
            ..Default::default()
        },
        ..Default::default()
    });
    let mut image = ImageData {
        width: 16,
        height: 16,
        rgba: [72, 140, 210, 255].repeat(16 * 16),
        compressed: None,
    };
    let cooked = texture::cook(
        &image,
        &[Compression::Bc3, Compression::Astc4x4],
        &[true],
        &Progress::default(),
    )?;
    let bytes = texture::encode(&image, &cooked)?;
    image.compressed = Some(Arc::new(cooked));
    fs::write(source.join("paint.btex"), &bytes)?;
    scene.assets.insert(
        "paint".into(),
        AssetSource {
            kind: AssetKind::Image,
            path: "paint.btex".into(),
        },
    );
    scene.objects.push(Object {
        id: "cube".into(),
        name: "Cube".into(),
        drawable: Some(Drawable {
            mesh: Mesh::Cube,
            texture: Texture::Asset("paint".into()),
            layer: Layer::ThreeD,
            color: [1.; 3],
            uv_scale: [1.; 2],
            metallic: None,
            roughness: None,
            material_overrides: vec![],
            gi_static: true,
        }),
        ..Default::default()
    });
    fs::write(&scene_path, scene.to_json()?)?;
    export(&scene, &scene_path, &temp.0.join("game"));
    fs::remove_dir_all(source)?;
    fs::rename(temp.0.join("game"), temp.0.join("relocated game"))?;
    let root = data(&temp.0.join("relocated game"));
    let (_, scene_path) = Project::load(&root.join(bozzard_project::MANIFEST))?;
    let scene = load(&scene_path);
    let mut store = AssetStore::new(scene_path.parent().unwrap(), &scene.assets)?;
    store.load_pending()?;
    store.require_ready()?;
    let AssetData::Image(loaded) = store
        .get(store.handle("paint").unwrap())
        .unwrap()
        .data()
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(loaded.rgba, image.rgba);
    assert_eq!(
        texture::encode(loaded, loaded.compressed.as_ref().unwrap())?,
        bytes
    );
    Ok(())
}

#[test]
fn starter_projects_play_relocate_export_and_refuse_existing_destinations() -> anyhow::Result<()> {
    use bozzard_project::{ProjectTemplate, create_project};
    use bozzard_scene::{GameAction, GameplayInput, Transform};
    let temp = Temp::new();
    for template in ProjectTemplate::ALL {
        let source = temp.0.join(template.id());
        let manifest = create_project(&source, "Starter Test", template)?;
        let (project, scene_path) = Project::load(&manifest)?;
        let scene = load(&scene_path);
        assert!(create_project(&source, "Overwrite", template).is_err());
        assert_eq!(load(&scene_path), scene);
        let exported = temp.0.join(format!("export-{}", template.id()));
        prepare_export(
            &project,
            &scene,
            &scene_path,
            &std::env::current_exe()?,
            &exported,
            &Default::default(),
        )?
        .commit()?;
        fs::remove_dir_all(&source)?;
        let (_, scene_path) = Project::load(&data(&exported).join(bozzard_project::MANIFEST))?;
        let scene = load(&scene_path);
        let mut runtime = bozzard_demo::SceneDemo::new_with_prefabs(&scene, Some(&scene_path))?;
        runtime.game_action(GameAction::Start)?;
        let player = runtime.instance().entity("player").unwrap();
        let initial = runtime
            .app
            .world
            .get::<Transform>(player)
            .unwrap()
            .translation;
        for _ in 0..30 {
            runtime.set_gameplay_input(GameplayInput {
                movement: [1., 0.],
                ..Default::default()
            });
            runtime.app.step();
            runtime.check_simulation()?;
        }
        let moved = runtime
            .app
            .world
            .get::<Transform>(player)
            .unwrap()
            .translation;
        assert!(
            moved[0] > initial[0] + 1.,
            "{} must be playable",
            template.id()
        );
        if template == ProjectTemplate::Collect2d {
            let board = runtime
                .app
                .world
                .resource::<bozzard_scene::BlueprintRuntime>()
                .unwrap()
                .scene_blackboard();
            assert_eq!(
                serde_json::to_value(&board["score"])?["scalar"]["number"],
                1.
            );
        }
        runtime.game_action(GameAction::Restart)?;
        runtime.check_simulation()?;
        let player = runtime.instance().entity("player").unwrap();
        assert_eq!(
            runtime
                .app
                .world
                .get::<Transform>(player)
                .unwrap()
                .translation,
            initial
        );
    }
    Ok(())
}
fn project() -> Project {
    Project {
        version: 1,
        name: "Test & Game".into(),
        start_scene: "scene.json".into(),
        runtime_modules: Vec::new(),
        view: Layer::ThreeD,
        cook: Default::default(),
    }
}

#[test]
fn styled_text_exports_primary_and_fallback_fonts_and_loads_without_source_files()
-> anyhow::Result<()> {
    use bozzard_assets::{AssetStore, job::Progress};
    use bozzard_scene::{AssetKind, AssetSource, Object, TextFont, TextRendering};
    let temp = Temp::new();
    let source = temp.0.join("fonts");
    fs::create_dir(&source)?;
    fs::write(
        source.join("variable.ttf"),
        include_bytes!("../../bozzard-text/tests/fonts/Roboto.ttf"),
    )?;
    fs::write(
        source.join("fallback.ttf"),
        include_bytes!("../../bozzard-assets/tests/fonts/test.ttf"),
    )?;
    let mut scene = bozzard_demo::scene_document()?;
    for id in ["variable", "fallback"] {
        scene.assets.insert(
            id.into(),
            AssetSource {
                kind: AssetKind::Font,
                path: format!("{id}.ttf"),
            },
        );
    }
    let style = TextRendering {
        text: "Variable text 😀".into(),
        font: TextFont::Custom("variable".into()),
        font_axes: [("wdth".into(), 75.), ("wght".into(), 900.)].into(),
        font_fallbacks: vec!["fallback".into()],
        builtin_font_fallback: true,
        ..Default::default()
    };
    scene.objects.push(Object {
        id: "font-label".into(),
        name: "Font label".into(),
        text_rendering: Some(style.clone()),
        ..Default::default()
    });
    let target = temp.0.join("font-game");
    prepare_export(
        &project(),
        &scene,
        &source.join("scene.json"),
        &std::env::current_exe()?,
        &target,
        &Progress::default(),
    )?
    .commit()?;
    fs::remove_dir_all(source)?;
    let path = data(&target).join("scene.json");
    let loaded = load(&path);
    let text = loaded
        .objects
        .iter()
        .find(|o| o.id == "font-label")
        .unwrap()
        .text_rendering
        .as_ref()
        .unwrap();
    assert_eq!(text, &style);
    let mut assets = AssetStore::new(path.parent().unwrap(), &loaded.assets)?;
    assets.load_pending()?;
    assets.require_ready()?;
    assets.validate_scene_resources(&loaded)?;
    assert!(assets.text_font(text)?.is_some());
    Ok(())
}

#[test]
fn automatically_simplified_pbr_lod_exports_without_its_source_project() -> anyhow::Result<()> {
    use bozzard_assets::{AssetData, AssetStore, SimplifySettings, job::Progress};
    use bozzard_scene::{AssetKind, AssetSource, Drawable, Lod, LodLevel, Mesh, Object, Texture};
    let temp = Temp::new();
    let source = temp.0.join("authoring");
    fs::create_dir(&source)?;
    let fixture = fixtures().join("assets/material-gallery/gold-polished.gltf");
    fs::write(
        source.join("base.gltf"),
        bozzard_assets::portable_gltf(&fixture)?,
    )?;
    let mut scene = bozzard_demo::scene_document()?;
    scene.assets.insert(
        "base".into(),
        AssetSource {
            kind: AssetKind::Mesh,
            path: "base.gltf".into(),
        },
    );
    let mut store = AssetStore::new(&source, &scene.assets)?;
    store.load_pending()?;
    let AssetData::Mesh(mesh) = store
        .get(store.handle("base").unwrap())
        .unwrap()
        .data()
        .unwrap()
    else {
        panic!()
    };
    let simplified = bozzard_assets::simplify_mesh(
        mesh,
        SimplifySettings {
            ratio: 0.25,
            max_error: 0.05,
            lock_borders: true,
        },
        &Progress::default(),
    )?;
    assert!(simplified.triangles < simplified.source_triangles);
    fs::write(
        source.join("low.gltf"),
        bozzard_assets::mesh_gltf(&simplified.mesh, &Progress::default())?,
    )?;
    scene.assets.insert(
        "low".into(),
        AssetSource {
            kind: AssetKind::Mesh,
            path: "low.gltf".into(),
        },
    );
    scene.objects.push(Object {
        id: "generated".into(),
        name: "Generated LOD".into(),
        drawable: Some(Drawable {
            metallic: None,
            roughness: None,
            gi_static: true,
            material_overrides: Vec::new(),
            layer: Layer::ThreeD,
            mesh: Mesh::Asset("base".into()),
            texture: Texture::White,
            color: [1.; 3],
            uv_scale: [1.; 2],
        }),
        lod: Some(Lod {
            levels: vec![LodLevel {
                switch: 0.1,
                mesh: Some(Mesh::Asset("low".into())),
            }],
            hysteresis: 0.1,
        }),
        ..Default::default()
    });
    let target = temp.0.join("game");
    prepare_export(
        &project(),
        &scene,
        &source.join("scene.json"),
        &std::env::current_exe()?,
        &target,
        &Progress::default(),
    )?
    .commit()?;
    fs::remove_dir_all(source)?;
    let path = data(&target).join("scene.json");
    let exported = load(&path);
    let runtime = bozzard_demo::SceneDemo::new_with_prefabs(&exported, Some(&path))?;
    let frame = runtime
        .instance()
        .view(&runtime.app.world, Layer::ThreeD, 1.)?;
    assert!(
        frame
            .objects
            .iter()
            .any(|(_, drawable)| drawable.mesh == Mesh::Asset("low".into()))
    );
    let mut store = AssetStore::new(path.parent().unwrap(), &exported.assets)?;
    store.load_pending()?;
    store.require_ready()?;
    let AssetData::Mesh(mesh) = store
        .get(store.handle("low").unwrap())
        .unwrap()
        .data()
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(mesh.indices.len() / 3, simplified.triangles);
    assert_eq!(
        mesh.parts[0].shading.as_ref().unwrap().material.metallic,
        simplified.mesh.parts[0]
            .shading
            .as_ref()
            .unwrap()
            .material
            .metallic
    );
    Ok(())
}
fn data(folder: &Path) -> PathBuf {
    if cfg!(target_os = "macos") {
        folder.join("Game.app/Contents/Resources/game")
    } else {
        folder.into()
    }
}
fn copy_tree(source: &Path, target: &Path) {
    fs::create_dir_all(target).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let path = entry.unwrap().path();
        let destination = target.join(path.file_name().unwrap());
        if path.is_dir() {
            copy_tree(&path, &destination);
        } else {
            fs::copy(path, destination).unwrap();
        }
    }
}
fn load(path: &Path) -> Scene {
    Scene::from_json(&fs::read_to_string(path).unwrap()).unwrap()
}
fn export(scene: &Scene, source: &Path, destination: &Path) {
    prepare_export(
        &project(),
        scene,
        source,
        &std::env::current_exe().unwrap(),
        destination,
        &Default::default(),
    )
    .unwrap()
    .commit()
    .unwrap();
}

#[test]
fn declared_compute_assets_survive_relocation_and_source_removal() {
    let temp = Temp::new();
    let source = temp.0.join("source");
    fs::create_dir_all(source.join("assets")).unwrap();
    copy_tree(
        &fixtures().join("assets/compute"),
        &source.join("assets/compute"),
    );
    for name in ["compute-waves", "compute-numbers"] {
        fs::copy(
            fixtures().join(format!("{name}.json")),
            source.join(format!("{name}.json")),
        )
        .unwrap();
        let path = source.join(format!("{name}.json"));
        export(&load(&path), &path, &temp.0.join(name));
        fs::rename(temp.0.join(name), temp.0.join(format!("relocated {name}"))).unwrap();
    }
    fs::remove_dir_all(source).unwrap();
    for name in ["compute-waves", "compute-numbers"] {
        let root = data(&temp.0.join(format!("relocated {name}")));
        let (_, path) = Project::load(&root.join(bozzard_project::MANIFEST)).unwrap();
        let cooked = load(&path);
        let mut runtime = bozzard_demo::SceneDemo::new_with_prefabs(&cooked, Some(&path)).unwrap();
        assert_eq!(runtime.instance().compute_kernels().len(), 1);
        let mut assets = bozzard_assets::AssetStore::new(&root, &cooked.assets).unwrap();
        assets.load_pending().unwrap();
        assets.require_ready().unwrap();
        assert!(
            cooked
                .assets
                .keys()
                .filter_map(|id| assets.handle(id))
                .filter_map(|h| assets.get(h))
                .any(|entry| matches!(
                    entry.data(),
                    Some(bozzard_assets::AssetData::ComputeShader(_))
                ))
        );
        runtime.app.step();
        runtime.check_simulation().unwrap();
        assert!(!runtime.instance().compute_capabilities().available());
    }
}

#[test]
fn export_survives_source_removal_and_runs_the_whole_trail() {
    let temp = Temp::new();
    let source = temp.0.join("source");
    fs::create_dir_all(source.join("assets")).unwrap();
    fs::copy(
        fixtures().join("first-trail.json"),
        source.join("scene.json"),
    )
    .unwrap();
    fs::copy(
        fixtures().join("assets/octahedron.obj"),
        source.join("assets/octahedron.obj"),
    )
    .unwrap();
    let scene = load(&source.join("scene.json"));
    let folder = temp.0.join("game");
    export(&scene, &source.join("scene.json"), &folder);
    fs::remove_dir_all(&source).unwrap();
    let relocated = temp.0.join("renamed game with spaces");
    fs::rename(folder, &relocated).unwrap();
    let (manifest, path) =
        Project::load(&data(&relocated).join(bozzard_project::MANIFEST)).unwrap();
    assert_eq!(manifest.name, "Test & Game");
    let cooked = load(&path);
    assert_eq!(scene.objects, cooked.objects);
    let mut runtime = bozzard_demo::SceneDemo::new_with_prefabs(&cooked, Some(&path)).unwrap();
    let mut assets = bozzard_assets::AssetStore::new(
        path.parent().unwrap(),
        &runtime.instance().document().assets,
    )
    .unwrap();
    assets.load_pending().unwrap();
    assets.require_ready().unwrap();
    for tick in 0..340 {
        runtime.set_gameplay_input(bozzard_scene::GameplayInput {
            movement: [0., 1.],
            jump: tick == 80,
            ..Default::default()
        });
        runtime.app.step();
        runtime.check_simulation().unwrap();
    }
    let state = runtime.gameplay().unwrap();
    assert!(
        state.won
            && state.collected.len() == 3
            && state.checkpoint.as_deref() == Some("checkpoint")
    );
}

#[test]
fn model_images_buffers_and_spawn_prefabs_are_relocatable_and_deterministic() {
    let temp = Temp::new();
    let source = temp.0.join("source");
    copy_tree(&fixtures(), &source);
    for name in ["model-lab", "bonfire-lab", "gold-yard", "prefab-lab"] {
        let scene_path = source.join(format!("{name}.json"));
        let scene = load(&scene_path);
        export(&scene, &scene_path, &temp.0.join(name));
        let exported = load(&data(&temp.0.join(name)).join("scene.json"));
        let mut before = bozzard_assets::AssetStore::new(&source, &scene.assets).unwrap();
        before.load_pending().unwrap();
        let mut after =
            bozzard_assets::AssetStore::new(&data(&temp.0.join(name)), &exported.assets).unwrap();
        after.load_pending().unwrap();
        for (id, asset) in &scene.assets {
            if asset.kind != bozzard_scene::AssetKind::Prefab {
                let a = before.get(before.handle(id).unwrap()).unwrap();
                let b = after.get(after.handle(id).unwrap()).unwrap();
                assert_eq!(
                    a.content_fingerprint(),
                    b.content_fingerprint(),
                    "{name}: {id} changed source identity"
                );
            }
        }

        export(&scene, &scene_path, &temp.0.join(format!("{name}-again")));
        assert_eq!(
            fs::read(temp.0.join(name).join("package.json")).unwrap(),
            fs::read(temp.0.join(format!("{name}-again")).join("package.json")).unwrap()
        );
    }
    fs::remove_dir_all(source).unwrap();
    for name in ["model-lab", "bonfire-lab", "gold-yard", "prefab-lab"] {
        let path = data(&temp.0.join(name)).join("scene.json");
        let scene = load(&path);
        let mut runtime = bozzard_demo::SceneDemo::new_with_prefabs(&scene, Some(&path)).unwrap();
        let mut assets = bozzard_assets::AssetStore::new(
            path.parent().unwrap(),
            &runtime.instance().document().assets,
        )
        .unwrap();
        assets.load_pending().unwrap();
        assets.require_ready().unwrap();
        for _ in 0..120 {
            runtime.app.step();
            runtime.check_simulation().unwrap();
        }
        assert!(!fs::read_to_string(path).unwrap().contains("source/"));
    }
}

#[test]
fn errors_cancellation_and_existing_destinations_never_publish_partial_games() {
    let temp = Temp::new();
    let path = fixtures().join("first-trail.json");
    let mut scene = load(&path);
    let runtime = std::env::current_exe().unwrap();
    let destination = temp.0.join("game");
    let prepared = prepare_export(
        &project(),
        &scene,
        &path,
        &runtime,
        &destination,
        &Default::default(),
    )
    .unwrap();
    assert!(!destination.exists());
    drop(prepared); // Job cancellation drops an unpublished export.
    assert_eq!(fs::read_dir(&temp.0).unwrap().count(), 0);
    scene.assets.values_mut().next().unwrap().path = "missing.obj".into();
    assert!(
        prepare_export(
            &project(),
            &scene,
            &path,
            &runtime,
            &destination,
            &Default::default()
        )
        .is_err()
    );
    assert_eq!(fs::read_dir(&temp.0).unwrap().count(), 0);
    fs::create_dir(&destination).unwrap();
    fs::write(destination.join("keep.txt"), "keep me").unwrap();
    assert!(
        prepare_export(
            &project(),
            &load(&path),
            &path,
            &runtime,
            &destination,
            &Default::default()
        )
        .is_err()
    );
    assert_eq!(
        fs::read_to_string(destination.join("keep.txt")).unwrap(),
        "keep me"
    );
}

#[test]
fn project_rejects_unknown_versions_escaping_paths_and_missing_views() {
    let mut manifest = project();
    manifest.version = 2;
    assert!(manifest.validate().is_err());
    manifest.version = 1;
    for path in [
        "../scene.json",
        "/scene.json",
        "C:/scene.json",
        "scene\\file.json",
        "",
    ] {
        manifest.start_scene = path.into();
        assert!(manifest.validate().is_err(), "{path}");
    }
    manifest.start_scene = "scene.json".into();
    manifest.view = Layer::TwoD;
    assert!(
        manifest
            .validate_scene(&load(&fixtures().join("first-trail.json")))
            .is_err()
    );
    let temp = Temp::new();
    fs::write(
        temp.0.join("bad.json"),
        r#"{"version":1,"name":"Test","start_scene":"scene.json","view":"3d","typo":true}"#,
    )
    .unwrap();
    assert!(Project::load(&temp.0.join("bad.json")).is_err());
}

#[test]
fn runtime_scene_library_assets_are_relocated_and_load_without_sources() {
    let temp = Temp::new();
    let source = temp.0.join("source");
    fs::create_dir_all(source.join("assets")).unwrap();
    fs::copy(
        fixtures().join("assets/octahedron.obj"),
        source.join("assets/octahedron.obj"),
    )
    .unwrap();
    let mut scene = load(&fixtures().join("first-trail.json"));
    let mut level = scene.clone();
    level.name = "Second trail".into();
    scene.runtime_scenes.insert("second".into(), level.into());
    let folder = temp.0.join("export");
    export(&scene, &source.join("scene.json"), &folder);
    fs::remove_dir_all(&source).unwrap();
    let path = data(&folder).join("scene.json");
    let cooked = load(&path);
    let mut runtime = bozzard_demo::SceneDemo::new_with_prefabs(&cooked, Some(&path)).unwrap();
    runtime
        .with_instance(|i, w| i.load_runtime_scene(w, "second", false))
        .unwrap();
    assert_eq!(runtime.instance().document().name, "Second trail");
    let mut assets = bozzard_assets::AssetStore::new(
        path.parent().unwrap(),
        &runtime.instance().document().assets,
    )
    .unwrap();
    assets.load_pending().unwrap();
    assets.require_ready().unwrap();
}

#[test]
fn middleware_exports_keep_skin_audio_ui_nav_and_atlas_content_after_relocation() {
    use bozzard_scene::middleware::{
        animation::Animator, audio::AudioSource, navigation::NavSurface, registry, ui::Input,
    };
    for (name, view) in [
        ("middleware-lab", Layer::ThreeD),
        ("ui-2d-lab", Layer::TwoD),
    ] {
        let temp = Temp::new();
        let source = temp.0.join("source");
        fs::create_dir_all(source.join("assets")).unwrap();
        let scene = load(&fixtures().join(format!("{name}.json")));
        fs::write(source.join("scene.json"), scene.to_json().unwrap()).unwrap();
        for asset in scene.assets.values() {
            fs::copy(fixtures().join(&asset.path), source.join(&asset.path)).unwrap();
        }
        let mut project = project();
        project.view = view;
        prepare_export(
            &project,
            &scene,
            &source.join("scene.json"),
            &std::env::current_exe().unwrap(),
            &temp.0.join("game"),
            &Default::default(),
        )
        .unwrap()
        .commit()
        .unwrap();
        fs::remove_dir_all(&source).unwrap();
        fs::rename(temp.0.join("game"), temp.0.join("relocated game")).unwrap();
        let root = data(&temp.0.join("relocated game"));
        let cooked = load(&root.join("scene.json"));
        let mut assets = bozzard_assets::AssetStore::new(&root, &cooked.assets).unwrap();
        assets.load_pending().unwrap();
        assets.require_ready().unwrap();
        let mut demo =
            bozzard_demo::SceneDemo::new_with_prefabs(&cooked, Some(&root.join("scene.json")))
                .unwrap();
        if view == Layer::TwoD {
            demo.ui_input(view, [1280., 720.], Input::Key("Enter".into()))
                .unwrap();
        }
        for _ in 0..120 {
            demo.app.step();
        }
        demo.check_simulation().unwrap();
        assert!(
            !demo
                .instance()
                .ui_frame(&demo.app.world, view, [1280., 720.])
                .unwrap()
                .elements
                .is_empty()
        );
        if view == Layer::ThreeD {
            let object = cooked
                .objects
                .iter()
                .find(|o| o.id == "animated-banner")
                .unwrap();
            let animator = registry::get::<Animator>(object).unwrap().unwrap();
            assert_eq!(animator.rig.clips.len(), 2);
            let source = registry::get::<AudioSource>(object).unwrap().unwrap();
            let bozzard_assets::AssetData::Audio(audio) = assets
                .get(assets.handle(&source.asset).unwrap())
                .unwrap()
                .data()
                .unwrap()
            else {
                panic!()
            };
            assert_eq!(audio.duration, source.duration);
            let nav = registry::get::<NavSurface>(
                cooked
                    .objects
                    .iter()
                    .find(|o| o.id == "baked-navigation")
                    .unwrap(),
            )
            .unwrap()
            .unwrap();
            assert!(nav.baked.unwrap().triangles().count() > 100);
        }
    }
}

#[test]
fn nested_variant_prefabs_resolve_and_spawn_after_export_and_source_removal() -> anyhow::Result<()>
{
    use bozzard_assets::{AssetStore, job::Progress};
    use bozzard_scene::{
        AssetKind, AssetSource, Object, Prefab, PrefabBase, PrefabInstance, Texture,
    };
    let temp = Temp::new();
    let source = temp.0.join("prefab-sources");
    fs::create_dir(&source)?;
    fs::copy(
        fixtures().join("assets/middleware-panel.png"),
        source.join("image.png"),
    )?;
    let demo = bozzard_demo::scene_document()?;
    let mut object = demo
        .objects
        .iter()
        .find(|o| {
            o.drawable
                .as_ref()
                .is_some_and(|d| d.layer == Layer::ThreeD)
        })
        .unwrap()
        .clone();
    object.id = "part".into();
    object.name = "Original part".into();
    object.parent = None;
    object.drawable.as_mut().unwrap().texture = Texture::Asset("image".into());
    let mut base = Prefab {
        version: 1,
        name: "Base".into(),
        root: "part".into(),
        objects: vec![object],
        assets: [(
            "image".into(),
            AssetSource {
                kind: AssetKind::Image,
                path: "image.png".into(),
            },
        )]
        .into(),
        nested: Default::default(),
        base: None,
    };
    let mut variant = base.clone();
    variant.name = "Variant".into();
    variant.assets.insert(
        "base".into(),
        AssetSource {
            kind: AssetKind::Prefab,
            path: "base.prefab.json".into(),
        },
    );
    variant.base = Some(PrefabBase {
        asset: "base".into(),
        baseline: base.objects.clone(),
        nested: Default::default(),
    });
    variant.objects[0].drawable.as_mut().unwrap().color = [1., 0., 0.];
    fs::write(source.join("variant.prefab.json"), variant.to_json()?)?;
    let mut nested = variant.objects[0].clone();
    nested.remap_ids(&[("part".into(), "nested-part".into())].into());
    let baseline = vec![nested.clone()];
    nested.parent = Some("assembly".into());
    let mut outer = Prefab {
        version: 1,
        name: "Assembly".into(),
        root: "assembly".into(),
        objects: vec![
            Object {
                id: "assembly".into(),
                name: "Assembly".into(),
                ..Default::default()
            },
            nested,
        ],
        assets: variant.assets.clone(),
        base: None,
        nested: [(
            "nested-part".into(),
            PrefabInstance {
                asset: "variant".into(),
                members: [("part".into(), "nested-part".into())].into(),
                baseline,
            },
        )]
        .into(),
    };
    outer.assets.insert(
        "variant".into(),
        AssetSource {
            kind: AssetKind::Prefab,
            path: "variant.prefab.json".into(),
        },
    );
    fs::write(source.join("outer.prefab.json"), outer.to_json()?)?;
    // The exporter must follow current dependencies, not only cached expanded members.
    base.objects[0].name = "Updated base part".into();
    fs::write(source.join("base.prefab.json"), base.to_json()?)?;
    let mut scene = Scene::from_json(
        r#"{"version":1,"name":"Nested variants","views":{},
      "assets":{"assembly":{"kind":"prefab","path":"outer.prefab.json"}},
      "objects":[{"id":"spawner","name":"Spawner","transform":{"translation":[0,0,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]},"blueprints":[{"enabled":true,"graph":{"version":1,"name":"spawn","nodes":[
        {"id":1,"position":[0,0],"kind":"start","inputs":[]},
        {"id":2,"position":[0,0],"kind":"spawn_prefab","prefab":"assembly","inputs":["exec",{"vector":[0,0,0]}]}],
        "wires":[{"from":{"node":1,"port":0},"to":{"node":2,"port":0}}]}}]}]}"#,
    )?;
    let camera = demo
        .objects
        .iter()
        .find(|o| o.id == demo.views[&Layer::ThreeD])
        .unwrap()
        .clone();
    scene.views.insert(Layer::ThreeD, camera.id.clone());
    scene.objects.push(camera);
    let output = temp.0.join("game");
    prepare_export(
        &project(),
        &scene,
        &source.join("scene.json"),
        &std::env::current_exe()?,
        &output,
        &Progress::default(),
    )?
    .commit()?;
    fs::remove_dir_all(source)?;
    let path = data(&output).join("scene.json");
    let exported = load(&path);
    let mut runtime = bozzard_demo::SceneDemo::new_with_prefabs(&exported, Some(&path))?;
    runtime.game_action(bozzard_scene::GameAction::Start)?;
    runtime.app.step();
    runtime.check_simulation()?;
    let spawned = runtime.instance().document();
    assert_eq!(spawned.prefabs.len(), 1);
    let inherited = spawned
        .objects
        .iter()
        .find(|o| o.name == "Updated base part")
        .unwrap();
    assert_eq!(inherited.drawable.as_ref().unwrap().color, [1., 0., 0.]);
    assert!(inherited.parent.as_ref().unwrap().starts_with("spawn-"));
    let mut assets = AssetStore::new(path.parent().unwrap(), &spawned.assets)?;
    assets.load_pending()?;
    assets.require_ready()?;
    Ok(())
}
