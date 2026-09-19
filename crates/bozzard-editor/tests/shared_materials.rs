use anyhow::Result;
use bozzard_editor::Editor;
use bozzard_scene::{
    Scene,
    material_asset::{MaterialAsset, MaterialShader, MaterialTexture},
};
use std::{
    fs,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "bozzard-shared-material-tests-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        Self(root)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn editor(root: &Temp) -> Result<Editor> {
    let scene = Scene::from_json(
        r#"{"version":1,"name":"Shared materials","views":{"3d":"camera"},"objects":[
        {"id":"first","name":"First","transform":{"translation":[0,0,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]},
         "drawable":{"layer":"3d","mesh":"cube","texture":"white","color":[1,1,1],"uv_scale":[1,1]}},
        {"id":"second","name":"Second","transform":{"translation":[2,0,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]},
         "drawable":{"layer":"3d","mesh":"cube","texture":"white","color":[1,1,1],"uv_scale":[1,1]}},
        {"id":"camera","name":"Camera","transform":{"translation":[0,0,6],"rotation_degrees":[0,0,0],"scale":[1,1,1]},
         "camera":{"projection":"perspective","vertical_fov_degrees":60,"near":0.1,"far":100}}
    ]}"#,
    )?;
    Editor::new(scene, &root.0.join("scene.json"))
}
#[test]
fn shared_sources_variants_independent_overrides_history_and_guarded_save() -> Result<()> {
    let temp = Temp::new();
    let mut editor = editor(&temp)?;
    let base = editor.create_material(None)?;
    let saved = editor.material_source(&base)?;
    let mut definition = MaterialAsset::from_json(&saved)?;
    definition.properties.color = Some([0.2, 0.4, 0.6]);
    definition.properties.roughness = Some(0.3);
    let mut graph = bozzard_scene::shader_graph::ShaderGraph::default();
    graph.keywords.insert("DETAIL".into(), false);
    definition.shader = MaterialShader::Graph(graph);
    editor.save_material(&base, &saved, &definition)?;
    let variant = editor.create_material(Some(&base))?;
    for (object, id) in [("first", &base), ("second", &variant)] {
        editor.select_object(Some(object.into()));
        editor.assign_asset_to_selected(id)?;
    }
    let bound = editor.scene().clone();
    let mut scene = bound.clone();
    let material = scene.objects[1].material.as_mut().unwrap();
    material.set_color([0.8, 0.1, 0.2]);
    material.set_roughness(Some(0.9));
    Arc::make_mut(material.shared.as_mut().unwrap())
        .keywords
        .insert("DETAIL".into(), true);
    editor.apply("Independent material instance", scene)?;
    let edited = editor.scene().clone();
    editor.undo()?;
    assert_eq!(editor.scene(), &bound);
    editor.redo()?;
    assert_eq!(editor.scene(), &edited);
    let first = editor.render(bozzard_scene::Layer::ThreeD, 1.0)?;
    assert_eq!(first.items[0].material.tint, [0.2, 0.4, 0.6]);
    assert_eq!(first.items[1].material.tint, [0.8, 0.1, 0.2]);
    assert_eq!(first.items[1].material.roughness, Some(0.9));
    assert!(Arc::ptr_eq(
        first.items[0].material.shader.as_ref().unwrap(),
        first.items[1].material.shader.as_ref().unwrap()
    ));
    let current = editor.material_source(&base)?;
    definition.properties.color = Some([0.3, 0.6, 0.9]);
    editor.save_material(&base, &current, &definition)?;
    assert_eq!(
        editor.assets.material(&variant)?.values.color,
        [0.3, 0.6, 0.9]
    );
    assert_eq!(
        editor.render(bozzard_scene::Layer::ThreeD, 1.0)?.items[1]
            .material
            .tint,
        [0.8, 0.1, 0.2]
    );
    // A source change that invalidates a live instance is rejected before disk publication.
    let current = editor.material_source(&base)?;
    let MaterialShader::Graph(graph) = &mut definition.shader else {
        panic!()
    };
    graph.keywords.clear();
    assert!(editor.save_material(&base, &current, &definition).is_err());
    assert_eq!(editor.material_source(&base)?, current);
    definition = MaterialAsset::from_json(&current)?;
    let path = temp.0.join(&editor.scene().assets[&base].path);
    fs::write(&path, format!("{current}\n"))?;
    assert!(editor.save_material(&base, &current, &definition).is_err());
    fs::write(&path, &current)?;
    let scene_path = editor.path.clone();
    editor.save(&scene_path)?;
    let mut opened = Editor::open(&scene_path)?;
    let render = opened.render(bozzard_scene::Layer::ThreeD, 1.0)?;
    assert_eq!(render.items[1].material.tint, [0.8, 0.1, 0.2]);
    let handle = opened.assets.handle(&variant).unwrap();
    let previous = opened.assets.get(handle).unwrap().shared_data().unwrap();
    fs::write(&path, b"invalid source")?;
    opened.assets.refresh();
    assert!(opened.assets.require_ready().is_err());
    assert!(Arc::ptr_eq(
        &previous,
        &opened.assets.get(handle).unwrap().shared_data().unwrap()
    ));
    assert_eq!(
        opened.render(bozzard_scene::Layer::ThreeD, 1.0)?.items[1]
            .material
            .tint,
        [0.8, 0.1, 0.2]
    );
    Ok(())
}
#[test]
fn material_import_is_portable_and_resolved_properties_invalidate_baked_gi() -> Result<()> {
    let source = Temp::new();
    let target = Temp::new();
    let mut editor = editor(&target)?;
    let image = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/demo/scenes/assets/palette.png");
    fs::copy(&image, source.0.join("map.png"))?;
    let base = MaterialAsset {
        texture: Some(MaterialTexture::Image("map.png".into())),
        ..Default::default()
    };
    fs::write(source.0.join("base.material.json"), base.to_json()?)?;
    fs::create_dir(source.0.join("nested"))?;
    let variant = MaterialAsset {
        parent: Some("../base.material.json".into()),
        ..Default::default()
    };
    let path = source.0.join("nested/variant.material.json");
    fs::write(&path, variant.to_json()?)?;
    let id = editor.import(&path)?;
    drop(source);
    editor.select_object(Some("first".into()));
    editor.assign_asset_to_selected(&id)?;
    let mut scene = editor.scene().clone();
    scene.objects[0].drawable.as_mut().unwrap().gi_static = true;
    editor.apply("Static geometry", scene)?;
    let fingerprint =
        bozzard_assets::gi::source(editor.scene(), &editor.assets, editor.scene().gi.volume)?;
    let mut scene = editor.scene().clone();
    scene.objects[0]
        .material
        .as_mut()
        .unwrap()
        .set_color([0.1, 0.2, 0.3]);
    editor.apply("Instance color", scene)?;
    assert_ne!(
        fingerprint,
        bozzard_assets::gi::source(editor.scene(), &editor.assets, editor.scene().gi.volume)?
    );
    editor.save(&target.0.join("scene.json"))?;
    let opened = Editor::open(&target.0.join("scene.json"))?;
    assert!(opened.assets.material(&id)?.image.is_some());
    assert!(opened.assets.material(&id)?.definition.parent.is_some());
    Ok(())
}

#[test]
fn background_source_save_requires_fresh_state_and_explicit_publication() -> Result<()> {
    use bozzard_assets::job::Job;
    fn wait<T: Send + 'static>(job: &Job<T>) -> Result<T> {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        loop {
            if let Some(result) = job.poll() {
                return result;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
    }
    let temp = Temp::new();
    let mut editor = editor(&temp)?;
    let id = editor.create_material(None)?;
    let saved = editor.material_source(&id)?;
    let mut definition = MaterialAsset::from_json(&saved)?;
    definition.properties.color = Some([0.2, 0.3, 0.4]);
    let job = editor.material_job(&id, &saved, &definition)?;
    let prepared = wait(&job)?;
    assert_eq!(editor.material_source(&id)?, saved);
    let mut scene = editor.scene().clone();
    scene.name.push_str(" edited");
    editor.apply("Concurrent edit", scene)?;
    assert!(editor.accept_material(prepared).is_err());
    assert_eq!(editor.material_source(&id)?, saved);
    let cancelled = editor.material_job(&id, &saved, &definition)?;
    cancelled.cancel();
    assert!(wait(&cancelled).is_err());
    assert_eq!(editor.material_source(&id)?, saved);
    let job = editor.material_job(&id, &saved, &definition)?;
    editor.accept_material(wait(&job)?)?;
    assert_eq!(editor.assets.material(&id)?.values.color, [0.2, 0.3, 0.4]);
    Ok(())
}

#[test]
fn prefabs_and_additive_loading_remap_shared_material_bindings() -> Result<()> {
    use bozzard_editor::PrefabCommand;
    use bozzard_scene::scene_loading::{LoadPhase, SceneSource};
    use std::time::{Duration, Instant};
    let temp = Temp::new();
    let mut main = editor(&temp)?;
    let base = main.create_material(None)?;
    let saved = main.material_source(&base)?;
    let mut source = MaterialAsset::from_json(&saved)?;
    source.properties.color = Some([0.8, 0.1, 0.2]);
    main.save_material(&base, &saved, &source)?;
    main.select_object(Some("first".into()));
    main.assign_asset_to_selected(&base)?;
    let job = main.prefab_job(PrefabCommand::Create)?;
    let deadline = Instant::now() + Duration::from_secs(20);
    let prepared = loop {
        if let Some(result) = job.poll() {
            break result?;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(2));
    };
    let prefab = main.accept_prefab(prepared)?;
    let job = main.prefab_job(PrefabCommand::Instantiate {
        asset: prefab,
        position: Some([4., 0., 0.]),
    })?;
    let prepared = loop {
        if let Some(result) = job.poll() {
            break result?;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(2));
    };
    main.accept_prefab(prepared)?;
    let instance = main
        .selected_object()
        .unwrap()
        .material
        .as_ref()
        .unwrap()
        .shared
        .as_ref()
        .unwrap();
    assert_eq!(
        main.assets.material(&instance.asset)?.values.color,
        [0.8, 0.1, 0.2]
    );
    let chunk_root = Temp::new();
    let mut chunk = editor(&chunk_root)?;
    let chunk_material = chunk.create_material(None)?;
    assert_eq!(base, chunk_material); // Exercise catalog-ID collisions during additive publication.
    let saved = chunk.material_source(&chunk_material)?;
    let mut source = MaterialAsset::from_json(&saved)?;
    source.properties.color = Some([0.1, 0.2, 0.8]);
    chunk.save_material(&chunk_material, &saved, &source)?;
    chunk.select_object(Some("first".into()));
    chunk.assign_asset_to_selected(&chunk_material)?;
    chunk.save(&chunk_root.0.join("scene.json"))?;
    main.set_runtime_scene_source(
        "chunk",
        Some(SceneSource::File {
            path: bozzard_demo::relative_reference(&chunk.path, &temp.0)?,
        }),
    )?;
    main.start_play()?;
    main.play
        .as_mut()
        .unwrap()
        .with_instance(|instance, world| instance.begin_scene_load(world, "chunk", true))?;
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        main.advance(Duration::from_secs_f64(1. / 60.));
        let play = main.play.as_ref().unwrap();
        play.check_simulation()?;
        let status = play.instance().scene_load_status(&play.app.world);
        assert_ne!(status.phase, LoadPhase::Failed, "{}", status.error);
        if status.phase == LoadPhase::Loaded {
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(2));
    }
    let frame = main.render(bozzard_scene::Layer::ThreeD, 1.)?;
    assert!(
        frame
            .items
            .iter()
            .any(|i| i.material.tint == [0.8, 0.1, 0.2])
    );
    assert!(
        frame
            .items
            .iter()
            .any(|i| i.material.tint == [0.1, 0.2, 0.8])
    );
    main.stop_play();
    assert!(
        !main
            .render(bozzard_scene::Layer::ThreeD, 1.)?
            .items
            .iter()
            .any(|i| i.material.tint == [0.1, 0.2, 0.8])
    );
    Ok(())
}
