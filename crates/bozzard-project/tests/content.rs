use bozzard_assets::{
    AssetData, AssetStore,
    job::{Job, Progress},
};
use bozzard_project::{
    CookTarget,
    content::{Catalog, ContentStore, PackSpec, load_catalog, prepare_pack},
};
use bozzard_scene::{AssetKind, AssetSource, Layer, Scene};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

#[path = "support/content_http.rs"]
mod http;

struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "bozzard-content-test-{}-{}",
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
fn spec(root: &Path) -> anyhow::Result<PathBuf> {
    let source = root.join("source");
    copy_tree(&fixtures(), &source)?;
    bozzard_project::create_project(
        &source.join("two-d"),
        "Packed 2D",
        bozzard_project::ProjectTemplate::Collect2d,
    )?;
    let spec = PackSpec {
        version: 1,
        id: "test-content".into(),
        name: "Content test".into(),
        cook: CookTarget::Universal,
        scenes: [
            ("levels/workshop".into(), "model-lab.json".into()),
            ("levels/middleware".into(), "middleware-lab.json".into()),
            ("levels/two-d".into(), "two-d/scenes/main.json".into()),
        ]
        .into(),
        assets: [
            (
                "props/courier".into(),
                AssetSource {
                    kind: AssetKind::Mesh,
                    path: "assets/courier.glb".into(),
                },
            ),
            (
                "sounds/chime".into(),
                AssetSource {
                    kind: AssetKind::Audio,
                    path: "assets/middleware-chime.ogg".into(),
                },
            ),
        ]
        .into(),
    };
    let path = source.join("pack.json");
    fs::write(&path, serde_json::to_vec(&spec)?)?;
    Ok(path)
}
fn build(root: &Path) -> anyhow::Result<PathBuf> {
    prepare_pack(&spec(root)?, &root.join("release"), &Progress::default())?.commit()
}
fn hex(bytes: impl AsRef<[u8]>) -> String {
    bytes.as_ref().iter().map(|b| format!("{b:02x}")).collect()
}
fn wait<T: Send + 'static>(job: &Job<T>, timeout: Duration) -> anyhow::Result<T> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(result) = job.poll() {
            return result;
        }
        assert!(Instant::now() < deadline, "content job timed out");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn bundles_are_deterministic_reusable_and_addressable_without_authoring_sources()
-> anyhow::Result<()> {
    let temp = Temp::new();
    let spec_path = spec(&temp.0)?;
    let first = prepare_pack(&spec_path, &temp.0.join("first"), &Progress::default())?;
    assert_eq!(first.report().built, 7);
    let catalog = first.commit()?;
    assert!(prepare_pack(&spec_path, &temp.0.join("first"), &Progress::default()).is_err());
    let second = prepare_pack(&spec_path, &temp.0.join("second"), &Progress::default())?;
    assert_eq!((second.report().built, second.report().reused), (0, 7));
    second.commit()?;
    for file in ["content.bpack", "catalog.json"] {
        assert_eq!(
            fs::read(temp.0.join("first").join(file))?,
            fs::read(temp.0.join("second").join(file))?
        );
    }
    fs::remove_dir_all(temp.0.join("source"))?;
    let catalog = load_catalog(catalog.to_str().unwrap(), &Progress::default())?;
    let mut store = ContentStore::new(temp.0.join("cache"));
    for (address, view) in [
        ("levels/workshop", Layer::ThreeD),
        ("levels/middleware", Layer::ThreeD),
        ("levels/two-d", Layer::TwoD),
    ] {
        let resolved = store.resolve(&catalog, address, &Progress::default())?;
        assert_eq!(resolved.scene_view()?, view);
        assert!(resolved.asset_source().is_err());
        let path = resolved.path();
        let scene = Scene::from_json(&fs::read_to_string(&path)?)?;
        let mut runtime = bozzard_demo::SceneDemo::new_with_prefabs(&scene, Some(&path))?;
        let mut assets = AssetStore::new(
            path.parent().unwrap(),
            &runtime.instance().document().assets,
        )?;
        assets.load_pending()?;
        assets.require_ready()?;
        for asset in scene.assets.values().filter(|a| a.kind == AssetKind::Mesh) {
            assert!(asset.path.ends_with(".bmesh"));
        }
        if address == "levels/two-d" {
            runtime.game_action(bozzard_scene::GameAction::Start)?;
        }
        for _ in 0..12 {
            runtime.app.step();
            runtime.check_simulation()?;
        }
    }
    let mesh = store.resolve(&catalog, "props/courier", &Progress::default())?;
    let audio = store.resolve(&catalog, "sounds/chime", &Progress::default())?;
    assert!(Arc::ptr_eq(mesh.pack(), audio.pack()));
    assert!(mesh.scene_view().is_err());
    let source = mesh.asset_source()?;
    let mut assets = AssetStore::new(
        mesh.pack().root(),
        &BTreeMap::from([("mesh".into(), source)]),
    )?;
    assets.load_pending()?;
    let AssetData::Mesh(model) = assets
        .get(assets.handle("mesh").unwrap())
        .unwrap()
        .data()
        .unwrap()
    else {
        panic!()
    };
    assert!(
        model
            .parts
            .iter()
            .any(|p| p.image.as_ref().is_some_and(|i| i.compressed.is_some()))
    );
    // A new store revalidates an installed generation without needing the source bundle.
    fs::remove_file(temp.0.join("first/content.bpack"))?;
    let again = ContentStore::new(temp.0.join("cache")).resolve(
        &catalog,
        "props/courier",
        &Progress::default(),
    )?;
    assert_eq!(mesh.path(), again.path());
    assert!(
        store
            .resolve(&catalog, "missing", &Progress::default())
            .is_err()
    );
    Ok(())
}

#[test]
fn damaged_cache_repairs_and_failed_updates_preserve_existing_handles() -> anyhow::Result<()> {
    let temp = Temp::new();
    let path = build(&temp.0)?;
    let catalog = load_catalog(path.to_str().unwrap(), &Progress::default())?;
    let cache = temp.0.join("cache");
    let mut store = ContentStore::new(&cache);
    let previous = store.resolve(&catalog, "props/courier", &Progress::default())?;
    let original = fs::read(previous.path())?;
    fs::write(previous.path(), b"damaged installed model")?;
    let repaired =
        ContentStore::new(&cache).resolve(&catalog, "props/courier", &Progress::default())?;
    assert_eq!(fs::read(repaired.path())?, original);
    let mut changed = catalog.clone();
    let reference = changed.catalog.packs.values_mut().next().unwrap();
    reference.sha256 = "0".repeat(64);
    assert!(
        store
            .resolve(&changed, "props/courier", &Progress::default())
            .is_err()
    );
    assert_eq!(fs::read(previous.path())?, original);
    assert!(!cache.join("0".repeat(64)).exists());
    assert!(!fs::read_dir(&cache)?.any(|e| {
        e.unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".bozzard-content-")
    }));
    Ok(())
}

#[test]
fn malformed_indices_and_payloads_never_publish_or_escape_the_cache() -> anyhow::Result<()> {
    let temp = Temp::new();
    let catalog_path = build(&temp.0)?;
    let original_catalog: Catalog = serde_json::from_slice(&fs::read(&catalog_path)?)?;
    let original = fs::read(temp.0.join("release/content.bpack"))?;
    let length = u32::from_le_bytes(original[12..16].try_into()?) as usize;
    let metadata: serde_json::Value = serde_json::from_slice(&original[16..16 + length])?;
    let payload = &original[16 + length..];
    fs::write(temp.0.join("outside"), b"keep me")?;
    for case in [
        "traversal",
        "absolute",
        "windows",
        "duplicate",
        "case",
        "parent",
        "reserved",
        "reserved-directory",
        "too-big",
        "missing-entry",
        "corrupt",
        "truncated",
        "trailing",
    ] {
        let mut metadata = metadata.clone();
        let files = metadata["files"].as_array_mut().unwrap();
        match case {
            "traversal" => files[0]["path"] = "../outside".into(),
            "absolute" => files[0]["path"] = "/outside".into(),
            "windows" => files[0]["path"] = "C:\\outside".into(),
            "reserved" => files[0]["path"] = ".BOZZARD-CONTENT-INDEX.JSON".into(),
            "reserved-directory" => files[0]["path"] = ".bozzard-content-index.json/file".into(),
            "duplicate" => files[1]["path"] = files[0]["path"].clone(),
            "case" => {
                files[0]["path"] = "assets/A".into();
                files[1]["path"] = "assets/a".into();
            }
            "parent" => {
                files[0]["path"] = "assets/a".into();
                files[1]["path"] = "assets/a/b".into();
            }
            "too-big" => files[0]["bytes"] = (u64::MAX).into(),
            "missing-entry" => {
                metadata["entries"]["props/courier"]["path"] = "missing.bmesh".into()
            }
            _ => {}
        }
        let metadata = serde_json::to_vec(&metadata)?;
        let mut bytes = b"BOZZPACK".to_vec();
        bytes.extend(1_u32.to_le_bytes());
        bytes.extend((metadata.len() as u32).to_le_bytes());
        bytes.extend(&metadata);
        bytes.extend(payload);
        match case {
            "corrupt" => *bytes.last_mut().unwrap() ^= 1,
            "truncated" => {
                bytes.truncate(bytes.len() - 100);
            }
            "trailing" => bytes.push(42),
            _ => {}
        }
        let mut catalog = original_catalog.clone();
        let reference = catalog.packs.values_mut().next().unwrap();
        reference.bytes = bytes.len() as u64;
        reference.sha256 = hex(Sha256::digest(&bytes));
        reference.index_sha256 = hex(Sha256::digest(&metadata));
        let digest = reference.sha256.clone();
        fs::write(temp.0.join("release/content.bpack"), bytes)?;
        fs::write(&catalog_path, serde_json::to_vec(&catalog)?)?;
        let resolved = load_catalog(catalog_path.to_str().unwrap(), &Progress::default())?;
        let cache = temp.0.join(case);
        assert!(
            ContentStore::new(&cache)
                .resolve(&resolved, "props/courier", &Progress::default())
                .is_err(),
            "accepted {case}"
        );
        assert!(!cache.join(digest).exists());
        assert_eq!(fs::read(temp.0.join("outside"))?, b"keep me");
    }
    Ok(())
}

#[test]
fn concurrent_installers_share_a_verified_generation_and_cancel_while_waiting() -> anyhow::Result<()>
{
    let temp = Temp::new();
    let catalog = load_catalog(build(&temp.0)?.to_str().unwrap(), &Progress::default())?;
    let cache = temp.0.join("cache");
    fs::create_dir(&cache)?;
    let digest = catalog
        .catalog
        .packs
        .values()
        .next()
        .unwrap()
        .sha256
        .clone();
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(cache.join(format!(".{digest}.lock")))?;
    lock.lock()?;
    let (job_catalog, job_cache) = (catalog.clone(), cache.clone());
    let job = Job::start("Waiting", move |progress| {
        ContentStore::new(job_cache).resolve(&job_catalog, "props/courier", &progress)
    })?;
    let deadline = Instant::now() + Duration::from_secs(5);
    while job.label() != "Waiting for another content installer" {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
    job.cancel();
    assert!(wait(&job, Duration::from_secs(2)).is_err());
    assert!(!cache.join(&digest).exists());
    lock.unlock()?;
    let mut jobs = Vec::new();
    for _ in 0..3 {
        let (catalog, cache) = (catalog.clone(), cache.clone());
        jobs.push(Job::start("Installing", move |progress| {
            ContentStore::new(cache).resolve(&catalog, "props/courier", &progress)
        })?);
    }
    let paths = jobs
        .iter()
        .map(|j| wait(j, Duration::from_secs(20)).map(|v| v.path()))
        .collect::<anyhow::Result<Vec<_>>>()?;
    assert!(paths.iter().all(|p| p == &paths[0]));
    assert!(cache.join(digest).is_dir());
    Ok(())
}

#[test]
fn redirected_catalogs_stream_relative_packs_for_longer_than_a_read_timeout() -> anyhow::Result<()>
{
    let temp = Temp::new();
    let catalog = fs::read(build(&temp.0)?)?;
    let pack = fs::read(temp.0.join("release/content.bpack"))?;
    let server = http::Server::start(move |path| match path {
        "/catalog.json" => http::Response::redirect("/release/catalog.json"),
        "/release/catalog.json" => http::Response::ok(catalog.clone()),
        "/release/content.bpack" => http::Response {
            chunk: pack.len().div_ceil(20),
            delay: Duration::from_millis(300),
            ..http::Response::ok(pack.clone())
        },
        _ => panic!("unexpected request {path}"),
    })?;
    let catalog = load_catalog(
        &format!("{}/catalog.json", server.base),
        &Progress::default(),
    )?;
    let start = Instant::now();
    let cache = temp.0.join("cache");
    let worker_catalog = catalog.clone();
    let worker_cache = cache.clone();
    let job = Job::start("Downloading", move |progress| {
        ContentStore::new(worker_cache).resolve(&worker_catalog, "props/courier", &progress)
    })?;
    loop {
        let fraction = job.fraction();
        if fraction > 0. && fraction < 0.8 {
            break;
        }
        assert!(
            job.poll().is_none(),
            "download must report intermediate byte progress"
        );
        assert!(start.elapsed() < Duration::from_secs(15));
        std::thread::sleep(Duration::from_millis(5));
    }
    let mesh = wait(&job, Duration::from_secs(20))?;
    assert!(start.elapsed() > Duration::from_secs(5));
    assert!(mesh.path().is_file());
    assert_eq!(
        *server.requests.lock().unwrap(),
        [
            "/catalog.json",
            "/release/catalog.json",
            "/release/content.bpack"
        ]
    );
    drop(server);
    // Catalog already held by the caller: cache reuse needs no network connection.
    assert_eq!(
        ContentStore::new(cache)
            .resolve(&catalog, "props/courier", &Progress::default())?
            .path(),
        mesh.path()
    );
    Ok(())
}

#[test]
fn http_failures_and_cancelled_stalls_do_not_publish_partial_content() -> anyhow::Result<()> {
    let temp = Temp::new();
    let catalog_path = build(&temp.0)?;
    let catalog = load_catalog(catalog_path.to_str().unwrap(), &Progress::default())?;
    let pack = fs::read(temp.0.join("release/content.bpack"))?;
    let server = http::Server::start(move |path| {
        let mut response = http::Response::ok(pack.clone());
        match path {
            "/oversize" => response.declared += 1,
            "/truncated" => {
                response.body.truncate(32);
            }
            "/corrupt" => *response.body.last_mut().unwrap() ^= 1,
            "/partial" => response.status = "206 Partial Content",
            "/missing" => response.status = "404 Not Found",
            "/redirect" => return http::Response::redirect("http://example.com/content.bpack"),
            "/loop" => return http::Response::redirect("/loop"),
            "/stall" => response.delay = Duration::from_secs(30),
            _ => panic!("unexpected request {path}"),
        }
        response
    })?;
    for case in [
        "oversize",
        "truncated",
        "corrupt",
        "partial",
        "missing",
        "redirect",
        "loop",
    ] {
        let mut catalog = catalog.clone();
        let reference = catalog.catalog.packs.values_mut().next().unwrap();
        reference.location = format!("{}/{case}", server.base);
        let digest = reference.sha256.clone();
        let cache = temp.0.join(case);
        assert!(
            ContentStore::new(&cache)
                .resolve(&catalog, "props/courier", &Progress::default())
                .is_err(),
            "accepted {case}"
        );
        assert!(!cache.join(digest).exists());
        assert!(!fs::read_dir(cache)?.any(|e| {
            e.unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".bozzard-content-")
        }));
    }
    let mut catalog = catalog;
    let reference = catalog.catalog.packs.values_mut().next().unwrap();
    reference.location = format!("{}/stall", server.base);
    let digest = reference.sha256.clone();
    let cache = temp.0.join("stall");
    let worker_cache = cache.clone();
    let job = Job::start("Download", move |progress| {
        ContentStore::new(worker_cache).resolve(&catalog, "props/courier", &progress)
    })?;
    server.wait_for("/stall");
    job.cancel();
    assert!(wait(&job, Duration::from_secs(7)).is_err());
    assert!(!cache.join(digest).exists());
    Ok(())
}

#[test]
fn valid_checksums_do_not_bypass_typed_validation_of_loose_prefab_dependencies()
-> anyhow::Result<()> {
    let temp = Temp::new();
    for case in ["valid", "bad-image", "escape", "unindexed", "source-model"] {
        let release = temp.0.join(case);
        fs::create_dir(&release)?;
        let (kind, path) = match case {
            "escape" => ("image", "../../outside.png"),
            "unindexed" => ("image", "unlisted.png"),
            "source-model" => ("mesh", "data.obj"),
            _ => ("image", "data.png"),
        };
        let prefab = serde_json::to_vec(&serde_json::json!({
            "version":1, "name":"Loose prefab", "root":"root", "objects":[{"id":"root", "name":"Root", "transform":{"translation":[0,0,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]}}],
            "assets":{"dependency":{"kind":kind,"path":path}}
        }))?;
        let bytes = if case == "source-model" {
            b"v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n".to_vec()
        } else if case == "bad-image" {
            b"not an image".to_vec()
        } else {
            include_bytes!("../../../examples/demo/scenes/assets/palette.png").to_vec()
        };
        let files = BTreeMap::from([
            (
                format!(
                    "assets/data.{}",
                    if case == "source-model" { "obj" } else { "png" }
                ),
                bytes,
            ),
            ("assets/prefab.json".into(), prefab),
        ]);
        let metadata = serde_json::to_vec(&serde_json::json!({
            "version":1, "id":"typed", "name":"Typed", "cook":"rgba",
            "entries":{"props/item":{"type":"asset", "kind":"prefab", "path":"assets/prefab.json"}},
            "files":files.iter().map(|(path, bytes)| serde_json::json!({"path":path,"bytes":bytes.len(),"sha256":hex(Sha256::digest(bytes))})).collect::<Vec<_>>()
        }))?;
        let mut archive = b"BOZZPACK".to_vec();
        archive.extend(1_u32.to_le_bytes());
        archive.extend((metadata.len() as u32).to_le_bytes());
        archive.extend(&metadata);
        for bytes in files.values() {
            archive.extend(bytes);
        }
        let digest = hex(Sha256::digest(&archive));
        fs::write(
            release.join("catalog.json"),
            serde_json::to_vec(&serde_json::json!({
                "version":1, "packs":{"typed":{"location":"content.bpack","bytes":archive.len(),"sha256":digest,"index_sha256":hex(Sha256::digest(&metadata))}},
                "addresses":{"props/item":{"pack":"typed","entry":"props/item"}}
            }))?,
        )?;
        fs::write(release.join("content.bpack"), archive)?;
        let catalog = load_catalog(
            release.join("catalog.json").to_str().unwrap(),
            &Progress::default(),
        )?;
        let cache = release.join("cache");
        let result =
            ContentStore::new(&cache).resolve(&catalog, "props/item", &Progress::default());
        if case == "valid" {
            result?;
        } else {
            assert!(result.is_err(), "accepted {case}");
            assert!(!cache.join(digest).exists());
        }
    }
    Ok(())
}

#[test]
fn catalog_updates_keep_previous_content_generations_usable() -> anyhow::Result<()> {
    let temp = Temp::new();
    let path = spec(&temp.0)?;
    let first = prepare_pack(&path, &temp.0.join("v1"), &Progress::default())?.commit()?;
    let first = load_catalog(first.to_str().unwrap(), &Progress::default())?;
    let mut store = ContentStore::new(temp.0.join("cache"));
    let previous = store.resolve(&first, "levels/workshop", &Progress::default())?;
    let old_json = fs::read_to_string(previous.path())?;
    let scene_path = temp.0.join("source/model-lab.json");
    let mut updated = Scene::from_json(&fs::read_to_string(&scene_path)?)?;
    updated.name = "Updated workshop".into();
    fs::write(scene_path, updated.to_json()?)?;
    let next = prepare_pack(&path, &temp.0.join("v2"), &Progress::default())?;
    assert_eq!((next.report().built, next.report().reused), (0, 7));
    let next = load_catalog(next.commit()?.to_str().unwrap(), &Progress::default())?;
    let current = store.resolve(&next, "levels/workshop", &Progress::default())?;
    assert_ne!(previous.path(), current.path());
    assert_eq!(fs::read_to_string(previous.path())?, old_json);
    assert_eq!(
        Scene::from_json(&fs::read_to_string(current.path())?)?.name,
        "Updated workshop"
    );
    assert!(Arc::ptr_eq(
        previous.pack(),
        store
            .resolve(&first, "levels/workshop", &Progress::default())?
            .pack()
    ));
    Ok(())
}

#[test]
fn cancellation_at_the_build_handoff_discards_the_unpublished_release() -> anyhow::Result<()> {
    let temp = Temp::new();
    let spec = spec(&temp.0)?;
    let destination = temp.0.join("cancelled-release");
    let worker_destination = destination.clone();
    let (built, ready) = std::sync::mpsc::channel();
    let (release, gate) = std::sync::mpsc::channel();
    let job = Job::start("Building", move |progress| {
        let prepared = prepare_pack(&spec, &worker_destination, &progress)?;
        built.send(())?;
        gate.recv_timeout(Duration::from_secs(10))?;
        Ok(prepared)
    })?;
    ready.recv_timeout(Duration::from_secs(20))?;
    assert!(!destination.exists());
    job.cancel();
    release.send(())?;
    assert!(wait(&job, Duration::from_secs(5)).is_err());
    assert!(!destination.exists());
    assert!(temp.0.join("source/model-lab.json").is_file());
    assert!(!fs::read_dir(&temp.0)?.any(|e| {
        e.unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".bozzard-content-")
    }));
    Ok(())
}
