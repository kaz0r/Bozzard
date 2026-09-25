use bozzard_assets::{AssetData, AssetStore, gi, job::Progress};
use bozzard_project::{CookReport, CookTarget, Project, prepare_export};
use bozzard_scene::{AssetKind, Scene};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "bozzard-cooking-{}-{}",
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
fn data(root: &Path) -> PathBuf {
    if cfg!(target_os = "macos") {
        root.join("Game.app/Contents/Resources/game")
    } else {
        root.to_owned()
    }
}
fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/demo/scenes")
}
fn copy_tree(source: &Path, target: &Path) -> anyhow::Result<()> {
    fs::create_dir_all(target)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            copy_tree(&entry.path(), &target.join(entry.file_name()))?;
        } else {
            fs::copy(entry.path(), target.join(entry.file_name()))?;
        }
    }
    Ok(())
}
fn load(path: &Path) -> anyhow::Result<(Scene, AssetStore)> {
    let scene = Scene::from_json(&fs::read_to_string(path)?)?;
    let mut assets = AssetStore::new(path.parent().unwrap(), &scene.assets)?;
    assets.load_pending()?;
    assets.require_ready()?;
    Ok((scene, assets))
}
fn export(
    scene: &Scene,
    path: &Path,
    destination: &Path,
    cook: CookTarget,
) -> anyhow::Result<CookReport> {
    let project = Project {
        version: 1,
        name: "Cook test".into(),
        start_scene: "scene.json".into(),
        runtime_modules: Vec::new(),
        view: bozzard_scene::Layer::ThreeD,
        cook,
    };
    let prepared = prepare_export(
        &project,
        scene,
        path,
        &std::env::current_exe()?,
        destination,
        &Progress::default(),
    )?;
    let report = prepared.report();
    prepared.commit()?;
    Ok(report)
}
fn entry<'a>(store: &'a AssetStore, id: &str) -> &'a AssetData {
    store
        .get(store.handle(id).unwrap())
        .unwrap()
        .data()
        .unwrap()
}

#[test]
fn automatic_cooking_reuses_content_keys_rebuilds_only_dependents_and_recovers_bad_cache()
-> anyhow::Result<()> {
    let temp = Temp::new();
    let source = temp.0.join("source");
    copy_tree(&fixtures(), &source)?;
    let path = source.join("model-lab.json");
    let (scene, original) = load(&path)?;
    let first = export(&scene, &path, &temp.0.join("first"), CookTarget::Universal)?;
    assert_eq!((first.built, first.reused), (5, 0));
    let second = export(&scene, &path, &temp.0.join("second"), CookTarget::Universal)?;
    assert_eq!((second.built, second.reused), (0, 5));
    assert_eq!(first.cooked_bytes, second.cooked_bytes);
    assert_eq!(
        fs::read(temp.0.join("first/package.json"))?,
        fs::read(temp.0.join("second/package.json"))?
    );
    let (cooked, assets) = load(&data(&temp.0.join("first")).join("scene.json"))?;
    assert_eq!(cooked.objects, scene.objects);
    for (id, asset) in &cooked.assets {
        match (entry(&assets, id), entry(&original, id)) {
            (AssetData::Mesh(after), AssetData::Mesh(before)) => {
                assert!(asset.path.ends_with(".bmesh"));
                assert_eq!(after.vertices, before.vertices);
                assert_eq!(after.indices, before.indices);
                for (a, b) in after.parts.iter().zip(&before.parts) {
                    assert_eq!(a.source_key, b.source_key);
                    assert_eq!(
                        a.image.as_ref().map(|i| &i.rgba),
                        b.image.as_ref().map(|i| &i.rgba)
                    );
                    if let Some(image) = &a.image {
                        assert_eq!(image.compressed.as_ref().unwrap().variants().len(), 2);
                    }
                }
            }
            (AssetData::Image(after), AssetData::Image(before)) => {
                assert!(asset.path.ends_with(".btex"));
                assert_eq!(after.rgba, before.rgba);
                assert_eq!(after.compressed.as_ref().unwrap().variants().len(), 2);
            }
            _ => panic!("unexpected fixture asset"),
        }
    }
    // Identical bytes after moving the authoring folder retain cache hits.
    let moved = temp.0.join("moved source");
    fs::rename(&source, &moved)?;
    let path = moved.join("model-lab.json");
    assert_eq!(
        export(&scene, &path, &temp.0.join("moved"), CookTarget::Universal)?.reused,
        5
    );
    // Editing a shared external image invalidates the glTF and standalone image,
    // while the embedded GLB and other independent assets reuse their old output.
    fs::copy(
        moved.join("assets/palette.png"),
        moved.join("assets/courier-paint.png"),
    )?;
    let changed = export(
        &scene,
        &path,
        &temp.0.join("changed"),
        CookTarget::Universal,
    )?;
    assert_eq!((changed.built, changed.reused), (2, 3));
    let (_, changed_assets) = load(&data(&temp.0.join("changed")).join("scene.json"))?;
    let AssetData::Mesh(before) = entry(&assets, "courier-gltf") else {
        panic!()
    };
    let AssetData::Mesh(after) = entry(&changed_assets, "courier-gltf") else {
        panic!()
    };
    assert_ne!(
        before.parts[0].image.as_ref().unwrap().rgba,
        after.parts[0].image.as_ref().unwrap().rgba
    );
    let cache = moved.join(".bozzard-cache/cook-v1");
    for file in fs::read_dir(cache)? {
        fs::write(file?.path(), b"interrupted/corrupt cache")?;
    }
    let repaired = export(
        &scene,
        &path,
        &temp.0.join("repaired"),
        CookTarget::Universal,
    )?;
    assert_eq!((repaired.built, repaired.reused), (5, 0));
    assert_eq!(
        fs::read(temp.0.join("changed/package.json"))?,
        fs::read(temp.0.join("repaired/package.json"))?
    );
    // Target settings are part of the key. Lossless GPU fallback remains available.
    let rgba = export(&scene, &path, &temp.0.join("rgba"), CookTarget::Rgba)?;
    assert_eq!((rgba.built, rgba.reused), (5, 0));
    let (_, raw) = load(&data(&temp.0.join("rgba")).join("scene.json"))?;
    let AssetData::Image(image) = entry(&raw, "courier-paint") else {
        panic!()
    };
    assert!(image.compressed.is_none());
    fs::remove_dir_all(moved)?;
    fs::rename(temp.0.join("first"), temp.0.join("relocated game"))?;
    let root = data(&temp.0.join("relocated game"));
    let (project, path) = Project::load(&root.join(bozzard_project::MANIFEST))?;
    assert_eq!(project.cook, CookTarget::Source);
    let (scene, _) = load(&path)?;
    let mut runtime = bozzard_demo::SceneDemo::new_with_prefabs(&scene, Some(&path))?;
    runtime.app.step();
    runtime.check_simulation()?;
    Ok(())
}

#[test]
fn cooking_preserves_current_bakes_but_never_promotes_stale_bakes() -> anyhow::Result<()> {
    let temp = Temp::new();
    let source = temp.0.join("source");
    copy_tree(&fixtures(), &source)?;
    let path = source.join("model-lab.json");
    let (mut scene, assets) = load(&path)?;
    scene.gi.volume.resolution = [2; 3];
    scene.gi.volume.samples = 64;
    scene.gi.volume.bounces = 1;
    let bake = gi::bake(&scene, &assets, scene.gi.volume, &Progress::default())?;
    assert!(
        bake.probes
            .iter()
            .any(|probe| probe[..3].iter().any(|v| *v != 0.))
    );
    scene.gi.baked = Some(bake.into());
    let probes = scene.gi.baked.as_ref().unwrap().probes.clone();
    let mut level = scene.clone();
    level.name = "Second baked level".into();
    scene.runtime_scenes.insert("second".into(), level.into());
    export(
        &scene,
        &path,
        &temp.0.join("current"),
        CookTarget::Universal,
    )?;
    let (cooked, cooked_assets) = load(&data(&temp.0.join("current")).join("scene.json"))?;
    assert!(gi::is_current(&cooked, &cooked_assets)?);
    assert!(gi::is_current(
        &cooked.runtime_scenes["second"],
        &cooked_assets
    )?);
    assert_eq!(*cooked.gi.baked.as_ref().unwrap().probes, *probes);
    assert_ne!(
        cooked.gi.baked.as_ref().unwrap().source,
        scene.gi.baked.as_ref().unwrap().source
    );
    scene.lighting.sun_intensity += 1.;
    assert!(!gi::is_current(&scene, &assets)?);
    export(&scene, &path, &temp.0.join("stale"), CookTarget::Universal)?;
    let (stale, assets) = load(&data(&temp.0.join("stale")).join("scene.json"))?;
    assert!(!gi::is_current(&stale, &assets)?);
    assert_eq!(
        stale.gi.baked.as_ref().unwrap().source,
        scene.gi.baked.as_ref().unwrap().source
    );
    Ok(())
}

#[test]
fn animated_models_and_transitive_prefab_models_are_cooked_and_relocated() -> anyhow::Result<()> {
    let temp = Temp::new();
    let source = temp.0.join("source");
    copy_tree(&fixtures(), &source)?;
    let prefab_path = source.join("assets/cargo.prefab.json");
    let mut prefab = bozzard_scene::Prefab::from_json(&fs::read_to_string(&prefab_path)?)?;
    prefab.assets.insert(
        "courier".into(),
        bozzard_scene::AssetSource {
            kind: AssetKind::Mesh,
            path: "courier.gltf".into(),
        },
    );
    prefab
        .objects
        .iter_mut()
        .find(|o| o.id == "body")
        .unwrap()
        .drawable
        .as_mut()
        .unwrap()
        .mesh = bozzard_scene::Mesh::Asset("courier".into());
    fs::write(prefab_path, prefab.to_json()?)?;
    for name in ["middleware-lab", "prefab-lab", "material-gallery"] {
        let path = source.join(format!("{name}.json"));
        let (mut scene, _) = load(&path)?;
        if name == "prefab-lab" {
            scene.objects[0].blueprints.push(serde_json::from_value(serde_json::json!({
                "enabled": true, "graph": {"version":1, "name":"Spawn cooked prefab",
                "nodes":[{"id":1,"position":[0,0],"kind":"spawn_prefab","prefab":"cargo-prefab","inputs":["exec",{"vector":[0,0,0]}]}],"wires":[]}
            }))?);
        }
        let report = export(&scene, &path, &temp.0.join(name), CookTarget::Bc)?;
        assert!(report.built > 0);
    }
    fs::remove_dir_all(source)?;
    for name in ["middleware-lab", "prefab-lab", "material-gallery"] {
        let path = data(&temp.0.join(name)).join("scene.json");
        let (scene, _) = load(&path)?;
        let mut runtime = bozzard_demo::SceneDemo::new_with_prefabs(&scene, Some(&path))?;
        if name == "prefab-lab" {
            let id = runtime.with_instance(|instance, world| {
                instance.spawn_prefab(world, "cargo-prefab", [0.; 3])
            })?;
            assert!(runtime.instance().entity(&id).is_some());
        }
        let document = runtime.instance().document();
        let mut assets = AssetStore::new(path.parent().unwrap(), &document.assets)?;
        assets.load_pending()?;
        assets.require_ready()?;
        let mut meshes = 0;
        for (id, source) in &document.assets {
            if source.kind == AssetKind::Mesh {
                meshes += 1;
                assert!(source.path.ends_with(".bmesh"));
                let AssetData::Mesh(mesh) = entry(&assets, id) else {
                    panic!()
                };
                if name == "middleware-lab" {
                    assert!(mesh.skin.is_some());
                }
            }
        }
        assert!(meshes > 0);
        for _ in 0..30 {
            runtime.app.step();
            runtime.check_simulation()?;
        }
    }
    Ok(())
}

#[test]
fn material_parents_and_maps_export_with_cooking_cache_and_without_authoring_sources()
-> anyhow::Result<()> {
    use bozzard_scene::{
        AssetSource, Material,
        material_asset::{MaterialAsset, MaterialInstance, MaterialTexture},
    };
    use std::sync::Arc;
    let temp = Temp::new();
    let source = temp.0.join("source");
    fs::create_dir(&source)?;
    let path = source.join("scene.json");
    let mut scene = Scene::from_json(&fs::read_to_string(
        fixtures().join("material-gallery.json"),
    )?)?;
    scene.assets.clear();
    for object in &mut scene.objects {
        if let Some(drawable) = &mut object.drawable {
            drawable.mesh = bozzard_scene::Mesh::Cube;
            drawable.material_overrides.clear();
        }
    }
    let base = MaterialAsset {
        texture: Some(MaterialTexture::Image("palette.png".into())),
        ..Default::default()
    };
    fs::copy(
        fixtures().join("assets/palette.png"),
        source.join("palette.png"),
    )?;
    fs::write(source.join("base.material.json"), base.to_json()?)?;
    fs::create_dir(source.join("nested"))?;
    let variant = MaterialAsset {
        parent: Some("../base.material.json".into()),
        ..Default::default()
    };
    fs::write(
        source.join("nested/variant.material.json"),
        variant.to_json()?,
    )?;
    scene.assets.insert(
        "shared".into(),
        AssetSource {
            kind: AssetKind::Material,
            path: "nested/variant.material.json".into(),
        },
    );
    let object = scene
        .objects
        .iter_mut()
        .find(|o| o.drawable.is_some())
        .unwrap();
    let mut material = Material::from_drawable(object.drawable.as_ref().unwrap());
    material.shared = Some(Arc::new(MaterialInstance::new("shared".into())));
    object.material = Some(material);
    scene.validate()?;
    fs::write(&path, scene.to_json()?)?;
    let first = export(&scene, &path, &temp.0.join("first"), CookTarget::Universal)?;
    assert_eq!(first.built, 1);
    let second = export(&scene, &path, &temp.0.join("second"), CookTarget::Universal)?;
    assert_eq!((second.built, second.reused), (0, 1));
    fs::copy(
        fixtures().join("assets/courier-paint.png"),
        source.join("palette.png"),
    )?;
    let third = export(&scene, &path, &temp.0.join("third"), CookTarget::Universal)?;
    assert_eq!((third.built, third.reused), (1, 0));
    fs::remove_dir_all(source)?;
    let (scene, assets) = load(&data(&temp.0.join("first")).join("scene.json"))?;
    assets.validate_scene_resources(&scene)?;
    let material = assets.material("shared")?;
    assert!(material.definition.parent.is_some());
    assert_eq!(
        material
            .image
            .as_ref()
            .unwrap()
            .compressed
            .as_ref()
            .unwrap()
            .variants()
            .len(),
        2
    );
    let expected = bozzard_assets::CookSource::read(
        AssetKind::Image,
        &fixtures().join("assets/palette.png"),
        &Progress::default(),
    )?
    .decode()?;
    let AssetData::Image(expected) = expected else {
        panic!()
    };
    assert_eq!(material.image.as_ref().unwrap().rgba, expected.rgba);
    Ok(())
}
