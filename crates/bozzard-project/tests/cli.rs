use std::{
    fs,
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};

struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        loop {
            let root = std::env::temp_dir().join(format!(
                "bozzard-project-cli-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match fs::create_dir(&root) {
                Ok(()) => return Self(root),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("{error}"),
            }
        }
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_bozzard-project"))
}

#[test]
fn bundle_commands_build_list_and_resolve_a_relocated_game() -> anyhow::Result<()> {
    let temp = Temp::new();
    let source = temp.0.join("authoring");
    bozzard_project::create_project(
        &source,
        "CLI content",
        bozzard_project::ProjectTemplate::Collect2d,
    )?;
    fs::write(
        source.join("pack.json"),
        serde_json::to_vec(&serde_json::json!({
            "version":1, "id":"cli-game", "name":"CLI game", "cook":"universal",
            "scenes":{"levels/main":"scenes/main.json"}
        }))?,
    )?;
    let run = || {
        cli()
            .arg("bundle")
            .arg(source.join("pack.json"))
            .arg(temp.0.join("release"))
            .output()
    };
    let output = run()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!run()?.status.success());
    fs::remove_dir_all(source)?;
    fs::rename(temp.0.join("release"), temp.0.join("Moved release"))?;
    let catalog = temp.0.join("Moved release/catalog.json");
    let listing = cli().arg("list-content").arg(&catalog).output()?;
    assert!(listing.status.success());
    let addresses: serde_json::Value = serde_json::from_slice(&listing.stdout)?;
    assert_eq!(addresses["levels/main"]["pack"], "cli-game");
    let resolved = cli()
        .arg("fetch-content")
        .arg(&catalog)
        .arg("levels/main")
        .arg(temp.0.join("cache"))
        .output()?;
    assert!(
        resolved.status.success(),
        "{}",
        String::from_utf8_lossy(&resolved.stderr)
    );
    let resolved: serde_json::Value = serde_json::from_slice(&resolved.stdout)?;
    let path = PathBuf::from(resolved["path"].as_str().unwrap());
    let scene = bozzard_scene::Scene::from_json(&fs::read_to_string(&path)?)?;
    let mut game = bozzard_demo::SceneDemo::new_with_prefabs(&scene, Some(&path))?;
    game.game_action(bozzard_scene::GameAction::Start)?;
    game.app.step();
    game.check_simulation()?;
    assert!(
        !cli()
            .arg("fetch-content")
            .arg(catalog)
            .arg("unknown")
            .arg(temp.0.join("cache"))
            .output()?
            .status
            .success()
    );
    Ok(())
}

#[test]
fn cooked_model_command_is_self_contained_and_refuses_overwrites() -> anyhow::Result<()> {
    use bozzard_assets::{AssetData, AssetStore, cooked_model};
    use bozzard_scene::{AssetKind, AssetSource};
    let temp = Temp::new();
    let source = temp.0.join("source.glb");
    fs::write(
        &source,
        include_bytes!("../../../examples/demo/scenes/assets/courier.glb"),
    )?;
    let output = temp.0.join("courier.bmesh");
    let run = || {
        cli()
            .args(["cook-model", "universal"])
            .arg(&source)
            .arg(&output)
            .output()
    };
    let result = run()?;
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let bytes = fs::read(&output)?;
    let mesh = cooked_model::decode(&bytes)?;
    assert!(!mesh.vertices.is_empty());
    assert!(
        mesh.parts
            .iter()
            .any(|p| p.image.as_ref().is_some_and(|i| i.compressed.is_some()))
    );
    assert!(!run()?.status.success());
    assert_eq!(fs::read(&output)?, bytes);
    fs::remove_file(source)?;
    let sources = [(
        "model".into(),
        AssetSource {
            kind: AssetKind::Mesh,
            path: "courier.bmesh".into(),
        },
    )]
    .into();
    let mut store = AssetStore::new(&temp.0, &sources)?;
    store.load_pending()?;
    store.require_ready()?;
    assert!(
        matches!(store.get(store.handle("model").unwrap()).unwrap().data(),Some(AssetData::Mesh(m)) if m.vertices==mesh.vertices)
    );
    Ok(())
}

#[test]
fn texture_cook_preserves_sources_refuses_overwrite_and_produces_importable_assets()
-> anyhow::Result<()> {
    use bozzard_assets::{AssetData, AssetStore, texture};
    use bozzard_scene::{AssetKind, AssetSource};
    let temp = Temp::new();
    let input = temp.0.join("palette.png");
    let original = include_bytes!("../../../examples/demo/scenes/assets/palette.png");
    fs::write(&input, original)?;
    let output = temp.0.join("palette.btex");
    let run = || {
        cli()
            .args(["cook-texture", "universal"])
            .arg(&input)
            .arg(&output)
            .output()
    };
    let result = run()?;
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let bytes = fs::read(&output)?;
    let cooked = texture::decode(&bytes)?;
    assert_eq!(cooked.compressed.as_ref().unwrap().variants().len(), 2);
    assert!(!run()?.status.success());
    assert_eq!(fs::read(&output)?, bytes);
    assert_eq!(fs::read(&input)?, original);
    fs::remove_file(input)?;
    let sources = [(
        "cooked".into(),
        AssetSource {
            kind: AssetKind::Image,
            path: "palette.btex".into(),
        },
    )]
    .into();
    let mut store = AssetStore::new(&temp.0, &sources)?;
    store.load_pending()?;
    store.require_ready()?;
    assert!(
        matches!(store.get(store.handle("cooked").unwrap()).unwrap().data(), Some(AssetData::Image(i)) if i.compressed.is_some())
    );
    Ok(())
}

#[test]
fn commands_publish_valid_results_and_keep_conflicts_out_of_scene_files() -> anyhow::Result<()> {
    let temp = Temp::new();
    let project = temp.0.join("New project with spaces");
    let result = cli()
        .args(["new", "collect-2d"])
        .arg(&project)
        .arg("CLI game")
        .output()?;
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let manifest = fs::read(project.join(bozzard_project::MANIFEST))?;
    assert!(
        !cli()
            .args(["new", "collect-2d"])
            .arg(&project)
            .arg("Overwrite")
            .output()?
            .status
            .success()
    );
    assert_eq!(fs::read(project.join(bozzard_project::MANIFEST))?, manifest);

    let base = serde_json::json!({"version":1,"name":"Original","views":{},"objects":[]});
    let mut ours = base.clone();
    let mut theirs = base.clone();
    ours["name"] = "Left".into();
    theirs["name"] = "Right".into();
    for (name, value) in [("base", &base), ("ours", &ours), ("theirs", &theirs)] {
        fs::write(
            temp.0.join(format!("{name}.json")),
            serde_json::to_vec(value)?,
        )?;
    }
    let run = |output: &str| {
        cli()
            .arg("merge")
            .args(["base.json", "ours.json", "theirs.json", output])
            .current_dir(&temp.0)
            .output()
    };
    assert!(!run("conflicted.json")?.status.success());
    assert!(!temp.0.join("conflicted.json").exists());
    let report: serde_json::Value =
        serde_json::from_slice(&fs::read(temp.0.join("conflicted.json.conflicts.json"))?)?;
    assert_eq!(report["conflicts"][0]["path"], "/name");
    assert_eq!(
        fs::read(temp.0.join("ours.json"))?,
        serde_json::to_vec(&ours)?
    );
    fs::write(temp.0.join("theirs.json"), serde_json::to_vec(&base)?)?;
    assert!(run("merged.json")?.status.success());
    let scene = bozzard_scene::Scene::from_json(&fs::read_to_string(temp.0.join("merged.json"))?)?;
    assert_eq!(scene.name, "Left");
    assert!(!run("merged.json")?.status.success());
    Ok(())
}
