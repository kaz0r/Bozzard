//! Kennel registries, package checks and project installs. Upstream archives and remote
//! registries are served by the loopback HTTP fixture; nothing reaches the network.
use bozzard_assets::job::Progress;
use bozzard_project::kennel::{
    self, Archive, Bin, Category, Engine, InstallOptions, Lockfile, Manifest, PackageFile,
    Registry, Script, Source,
};
use bozzard_scene::{AssetKind, AssetSource, Scene};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

#[allow(dead_code, reason = "shared fixture; Kennel tests need no redirects")]
#[path = "support/content_http.rs"]
mod http;

const MATH: &str = "fn twice(x) { x * 2.0 }\n";
const PLAYER: &str =
    "import \"kit/math\" as math;\nfn network_input(key) { math::twice(1.0) > 1.0 }\n";

struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "bozzard-kennel-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        Self(root.canonicalize().unwrap())
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn sha(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn message(error: anyhow::Error) -> String {
    format!("{error:#}")
}

/// A package under construction; `write` lays it out in a registry checkout.
struct Package {
    manifest: Manifest,
    files: Vec<(String, Vec<u8>)>,
}
impl Package {
    fn new(name: &str) -> Self {
        Self {
            manifest: Manifest {
                schema: Some("../../schema/pkg.schema.json".into()),
                kennel: kennel::KENNEL,
                name: name.into(),
                version: "1.0.0".into(),
                title: format!("Test {name}"),
                summary: "A package for Kennel tests.".into(),
                category: Category::Scripts,
                tags: vec!["test".into()],
                authors: vec!["Bozzard tests".into()],
                license: "MIT OR Apache-2.0".into(),
                repository: None,
                engine: Engine {
                    bozzard: ">=0.1.0".into(),
                    script_api: 1,
                    features: Vec::new(),
                },
                dependencies: Default::default(),
                build_env: Default::default(),
                bins: Vec::new(),
                scripts: Vec::new(),
                assets: Vec::new(),
                files: Vec::new(),
            },
            files: Vec::new(),
        }
    }
    fn with(mut self, edit: impl FnOnce(&mut Manifest)) -> Self {
        edit(&mut self.manifest);
        self
    }
    fn script(mut self, id: &str, path: &str, text: &str) -> Self {
        self.manifest.scripts.push(Script {
            id: id.into(),
            path: path.into(),
            bytes: text.len() as u64,
            sha256: sha(text.as_bytes()),
        });
        self.files.push((path.into(), text.into()));
        self
    }
    fn file(mut self, path: &str, bytes: &[u8]) -> Self {
        self.manifest.files.push(PackageFile {
            path: path.into(),
            bytes: bytes.len() as u64,
            sha256: sha(bytes),
        });
        self.files.push((path.into(), bytes.to_vec()));
        self
    }
    fn bin(mut self, targets: &[&str], path: &str, bytes: &[u8], source: Option<Source>) -> Self {
        if source.is_none() {
            self.files.push((path.into(), bytes.to_vec()));
        }
        self.manifest.bins.push(Bin {
            targets: targets.iter().map(|t| (*t).to_owned()).collect(),
            path: path.into(),
            bytes: bytes.len() as u64,
            sha256: sha(bytes),
            license: None,
            source,
        });
        self
    }
    fn depends(self, name: &str, requirement: &str) -> Self {
        self.with(|m| {
            m.dependencies.insert(name.into(), requirement.into());
        })
    }
    fn write(&self, registry: &Path) {
        let folder = registry.join("packages").join(&self.manifest.name);
        fs::create_dir_all(&folder).unwrap();
        for (path, bytes) in &self.files {
            let path = folder.join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, bytes).unwrap();
        }
        let json = serde_json::to_string_pretty(&self.manifest).unwrap() + "\n";
        fs::write(folder.join(Manifest::file_name(&self.manifest.name)), json).unwrap();
    }
}

fn publish(registry: &Path, packages: &[Package]) {
    for package in packages {
        package.write(registry);
    }
    kennel::build_index(registry).unwrap();
}

fn kit() -> Package {
    Package::new("kit")
        .script("kit/math", "scripts/math.rhai", MATH)
        .script("kit/player", "scripts/player.rhai", PLAYER)
        .file("README.md", b"# Kit\n")
}

fn project(root: &Path) -> PathBuf {
    let folder = root.join("game");
    bozzard_project::create_project(
        &folder,
        "Kennel Game",
        bozzard_project::ProjectTemplate::Collect2d,
    )
    .unwrap();
    folder
}

/// A gzip-compressed ustar archive of regular files.
fn tar_gz(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut tar = Vec::new();
    for (name, data) in entries {
        let mut header = [0u8; 512];
        header[..name.len()].copy_from_slice(name.as_bytes());
        header[100..108].copy_from_slice(b"0000644\0");
        header[124..136].copy_from_slice(format!("{:011o}\0", data.len()).as_bytes());
        header[156] = b'0';
        header[257..263].copy_from_slice(b"ustar\0");
        header[148..156].copy_from_slice(b"        ");
        let sum: u32 = header.iter().map(|&b| u32::from(b)).sum();
        header[148..156].copy_from_slice(format!("{sum:06o}\0 ").as_bytes());
        tar.extend_from_slice(&header);
        tar.extend_from_slice(data);
        tar.resize(tar.len().div_ceil(512) * 512, 0);
    }
    tar.extend_from_slice(&[0; 1024]);
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    encoder.write_all(&tar).unwrap();
    encoder.finish().unwrap()
}

/// Serves `/registry/...` from a registry checkout and `/upstream.tar.gz` from memory.
fn serve(registry: &Path, upstream: Arc<Mutex<Vec<u8>>>) -> http::Server {
    let registry = registry.to_owned();
    http::Server::start(move |path| {
        if path == "/upstream.tar.gz" {
            return http::Response::ok(upstream.lock().unwrap().clone());
        }
        match path
            .strip_prefix("/registry/")
            .map(|relative| fs::read(registry.join(relative)))
        {
            Some(Ok(bytes)) => http::Response::ok(bytes),
            _ => http::Response {
                status: "404 Not Found",
                ..http::Response::ok(Vec::new())
            },
        }
    })
    .unwrap()
}

fn options(cache: &Path, all_targets: bool, force: bool) -> InstallOptions {
    InstallOptions {
        all_targets,
        force,
        cache: Some(cache.to_owned()),
    }
}

#[test]
fn check_accepts_published_packages_and_rejects_drift() {
    let temp = Temp::new();
    let registry = temp.0.join("registry");
    let app = Package::new("app").depends("kit", "^1").script(
        "app/main",
        "main.rhai",
        "import \"kit/math\" as math;\nfn on_update(me, dt) { math::twice(dt); }\n",
    );
    publish(&registry, &[kit(), app]);
    let progress = Progress::default();
    let report = kennel::check(&registry, None, &progress).unwrap();
    assert_eq!(
        (
            report.packages,
            report.files,
            report.scripts,
            report.sources
        ),
        (2, 4, 3, 0)
    );
    let index = Registry::open(registry.to_str().unwrap(), &progress).unwrap();
    assert_eq!(index.index.packages["kit"].version, "1.0.0");

    let fails = |expected: &str| {
        let error = message(kennel::check(&registry, None, &progress).unwrap_err());
        assert!(error.contains(expected), "{error}");
    };
    let readme = registry.join("packages/kit/README.md");
    fs::write(registry.join("packages/kit/notes.txt"), b"unlisted").unwrap();
    fails("kit: notes.txt is not listed in kit.pkg.json");
    fs::remove_file(registry.join("packages/kit/notes.txt")).unwrap();
    fs::write(&readme, b"# Changed\n").unwrap();
    fails("kit: README.md does not match its manifest entry");
    fs::write(&readme, b"# Kit\n").unwrap();

    kit()
        .with(|m| m.summary = "Edited after indexing.".into())
        .write(&registry);
    fails("index.json is out of date");
    kennel::build_index(&registry).unwrap();
    fs::write(registry.join("schema/pkg.schema.json"), "{}").unwrap();
    fails("schema/pkg.schema.json is out of date");
    kennel::build_index(&registry).unwrap();
    kennel::check(&registry, None, &progress).unwrap();

    publish(
        &registry,
        &[Package::new("broken").script("broken/x", "x.rhai", "fn broken( {")],
    );
    fails("broken: scripts do not compile");
    fs::remove_dir_all(registry.join("packages/broken")).unwrap();
    publish(
        &registry,
        &[Package::new("lonely")
            .depends("absent", "^1")
            .file("a.txt", b"a")],
    );
    fails("lonely depends on absent");
    fs::remove_dir_all(registry.join("packages/lonely")).unwrap();

    // Folder names must match manifests.
    kennel::build_index(&registry).unwrap();
    fs::rename(
        registry.join("packages/app"),
        registry.join("packages/other"),
    )
    .unwrap();
    fs::rename(
        registry.join("packages/other/app.pkg.json"),
        registry.join("packages/other/other.pkg.json"),
    )
    .unwrap();
    fails("names package 'app', not its folder 'other'");
}

#[test]
fn manifests_reject_unsafe_paths_names_ids_and_sources() {
    let rejects = |edit: &dyn Fn(&mut Manifest), expected: &str| {
        let mut manifest = kit().manifest;
        edit(&mut manifest);
        let error = message(manifest.validate().unwrap_err());
        assert!(error.contains(expected), "{expected}: {error}");
    };
    let file = |path: &str| PackageFile {
        path: path.into(),
        bytes: 1,
        sha256: "0".repeat(64),
    };
    kit().manifest.validate().unwrap();
    rejects(&|m| m.files.push(file("../escape.txt")), "portable");
    rejects(&|m| m.files.push(file("folder\\file.txt")), "portable");
    rejects(&|m| m.files.push(file("Readme.md")), "unique portable");
    rejects(&|m| m.files.push(file("kit.pkg.json")), "unique portable");
    rejects(
        &|m| m.files.push(file("scripts/math.rhai/inner")),
        "parent directory",
    );
    rejects(
        &|m| m.scripts[0].id = "other/math".into(),
        "start with 'kit/'",
    );
    rejects(&|m| m.scripts[1].id = "kit/math".into(), "unique");
    rejects(
        &|m| m.scripts[0].path = "scripts/math.txt".into(),
        ".rhai or .rs",
    );
    rejects(&|m| m.name = "Kit".into(), "package names");
    rejects(&|m| m.version = "1.0".into(), "semver");
    rejects(&|m| m.engine.bozzard = "newest".into(), "engine.bozzard");
    rejects(
        &|m| {
            m.build_env.insert("KIT_SDK".into(), "sdk".into());
        },
        "build_env KIT_SDK",
    );
    let bin = |target: &str, url: &str| Bin {
        targets: vec![target.into()],
        path: "sdk/lib.so".into(),
        bytes: 1,
        sha256: "1".repeat(64),
        license: None,
        source: Some(Source {
            url: url.into(),
            sha256: "1".repeat(64),
            archive: None,
            member: None,
        }),
    };
    rejects(
        &|m| m.bins.push(bin("linux", "https://example.com/lib.so")),
        "invalid or repeated target",
    );
    rejects(
        &|m| {
            m.bins
                .push(bin("linux-x86_64", "http://example.com/lib.so"))
        },
        "https URL",
    );
    rejects(
        &|m| {
            let mut bin = bin("linux-x86_64", "https://example.com/sdk.tar.gz");
            bin.source.as_mut().unwrap().archive = Some(Archive::TarGz);
            m.bins.push(bin);
        },
        "both archive and member",
    );
    let mut value = serde_json::to_value(kit().manifest).unwrap();
    value["unexpected"] = 1.into();
    let error = message(Manifest::parse(&serde_json::to_vec(&value).unwrap()).unwrap_err());
    assert!(error.contains("unknown field"), "{error}");
}

#[test]
fn install_fetches_upstream_bins_verifies_upgrades_and_removes() {
    let temp = Temp::new();
    let registry = temp.0.join("registry");
    let host = kennel::host_target();
    let (host_lib, other_lib) = (b"host library".to_vec(), b"other library".to_vec());
    let archive = tar_gz(&[
        ("sdk-1.0/bin/host/lib.so", &host_lib),
        ("sdk-1.0/bin/other/lib.so", &other_lib),
    ]);
    let server = serve(&registry, Arc::new(Mutex::new(archive.clone())));
    let source = |member: &str| Source {
        url: format!("{}/upstream.tar.gz", server.base),
        sha256: sha(&archive),
        archive: Some(Archive::TarGz),
        member: Some(member.into()),
    };
    let kit = kit()
        .bin(
            &[host.as_str()],
            "sdk/bin/host/lib.so",
            &host_lib,
            Some(source("sdk-1.0/bin/host/lib.so")),
        )
        .bin(
            &["plan9-mips"],
            "sdk/bin/other/lib.so",
            &other_lib,
            Some(source("sdk-1.0/bin/other/lib.so")),
        )
        .with(|m| {
            m.category = Category::Integration;
            m.engine.features = vec!["steam".into()];
            m.build_env.insert("KIT_SDK".into(), "sdk".into());
        });
    let app = Package::new("app")
        .depends("kit", "^1")
        .file("README.md", b"app\n");
    publish(&registry, &[kit, app]);
    let progress = Progress::default();
    let cache = temp.0.join("cache");
    let report = kennel::check(&registry, Some(&cache), &progress).unwrap();
    assert_eq!(report.sources, 2);
    // Both bins come from one archive, downloaded once into the content-addressed cache.
    let downloads = || {
        let requests = server.requests.lock().unwrap();
        requests
            .iter()
            .filter(|path| *path == "/upstream.tar.gz")
            .count()
    };
    assert_eq!(downloads(), 1);

    let game = project(&temp.0);
    let location = format!("{}/registry", server.base);
    let open = || Registry::open(&location, &progress).unwrap();
    let installed = kennel::install(
        &game,
        "app",
        &open(),
        options(&cache, false, false),
        &progress,
    )
    .unwrap();
    let names: Vec<_> = installed.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["kit", "app"]);
    let kit_dir = game.join("kennel/kit");
    assert_eq!(installed[0].directory, kit_dir);
    assert_eq!(
        fs::read(kit_dir.join("sdk/bin/host/lib.so")).unwrap(),
        host_lib
    );
    assert!(!kit_dir.join("sdk/bin/other").exists());
    assert_eq!(
        fs::read_to_string(kit_dir.join("scripts/math.rhai")).unwrap(),
        MATH
    );
    assert!(kit_dir.join("kit.pkg.json").is_file());
    assert_eq!(installed[0].targets, [host.as_str()]);
    assert_eq!(installed[0].features, ["steam"]);
    assert_eq!(
        installed[0].build_env,
        [("KIT_SDK".to_owned(), kit_dir.join("sdk"))]
    );
    let lockfile = Lockfile::load(&game).unwrap();
    assert_eq!(lockfile.packages["kit"].targets, [host.as_str()]);
    assert_eq!(lockfile.packages["kit"].registry, location);
    assert!(lockfile.packages["app"].targets.is_empty());
    assert_eq!(kennel::verify(&game, &progress).unwrap().len(), 2);
    assert_eq!(downloads(), 1);

    // Same content again is a no-op.
    let again = kennel::install(
        &game,
        "app",
        &open(),
        options(&cache, false, false),
        &progress,
    )
    .unwrap();
    assert!(again.iter().all(|package| package.unchanged));

    // Local edits are detected and protected until forced.
    fs::write(kit_dir.join("scripts/math.rhai"), "fn twice(x) { x }\n").unwrap();
    let error = message(kennel::verify(&game, &progress).unwrap_err());
    assert!(
        error.contains("kennel/kit/scripts/math.rhai was modified"),
        "{error}"
    );
    let error = message(
        kennel::install(
            &game,
            "kit",
            &open(),
            options(&cache, false, false),
            &progress,
        )
        .unwrap_err(),
    );
    assert!(error.contains("--force"), "{error}");
    let forced = kennel::install(
        &game,
        "kit",
        &open(),
        options(&cache, true, true),
        &progress,
    )
    .unwrap();
    let mut every = vec![host.clone(), "plan9-mips".to_owned()];
    every.sort();
    assert_eq!(forced[0].targets, every);
    assert_eq!(
        fs::read(kit_dir.join("sdk/bin/other/lib.so")).unwrap(),
        other_lib
    );
    assert_eq!(kennel::verify(&game, &progress).unwrap().len(), 2);
    fs::write(kit_dir.join("stray.txt"), b"?").unwrap();
    let error = message(kennel::verify(&game, &progress).unwrap_err());
    assert!(
        error.contains("kennel/kit/stray.txt is not part of the package"),
        "{error}"
    );
    fs::remove_file(kit_dir.join("stray.txt")).unwrap();

    // Dependents keep their dependencies installed.
    let error = message(kennel::remove(&game, "kit", false, &progress).unwrap_err());
    assert!(error.contains("app depends on kit"), "{error}");
    kennel::remove(&game, "app", false, &progress).unwrap();
    kennel::remove(&game, "kit", false, &progress).unwrap();
    assert!(!kit_dir.exists());
    assert!(Lockfile::load(&game).unwrap().packages.is_empty());
    assert!(kennel::verify(&game, &progress).unwrap().is_empty());
}

#[test]
fn install_rejects_cycles_incompatible_engines_and_corrupt_downloads() {
    let temp = Temp::new();
    let registry = temp.0.join("registry");
    let library = b"pinned library".to_vec();
    let archive = tar_gz(&[("sdk/lib.so", &library)]);
    let upstream = Arc::new(Mutex::new(archive.clone()));
    let server = serve(&registry, upstream.clone());
    let host = kennel::host_target();
    let corrupt = Package::new("corrupt").bin(
        &[host.as_str()],
        "lib.so",
        &library,
        Some(Source {
            url: format!("{}/upstream.tar.gz", server.base),
            sha256: sha(&archive),
            archive: Some(Archive::TarGz),
            member: Some("sdk/lib.so".into()),
        }),
    );
    publish(
        &registry,
        &[
            Package::new("a").depends("b", "^1"),
            Package::new("b").depends("a", "^1"),
            Package::new("future").with(|m| m.engine.bozzard = ">=99.0.0".into()),
            Package::new("newer-api").with(|m| m.engine.script_api = 99),
            Package::new("picky").depends("kit", "^2"),
            Package::new("elsewhere").bin(&["plan9-mips"], "lib.so", b"lib", None),
            kit(),
            corrupt,
        ],
    );
    let game = project(&temp.0);
    let cache = temp.0.join("cache");
    let progress = Progress::default();
    let registry = Registry::open(registry.to_str().unwrap(), &progress).unwrap();
    let fails = |name: &str, expected: &str| {
        let result = kennel::install(
            &game,
            name,
            &registry,
            options(&cache, false, false),
            &progress,
        );
        let error = message(result.unwrap_err());
        assert!(error.contains(expected), "{name}: {error}");
    };
    fails("a", "dependency cycle: a -> b -> a");
    fails("future", "needs Bozzard >=99.0.0");
    fails("newer-api", "needs script API 99");
    fails("picky", "picky needs kit ^2, but the registry has 1.0.0");
    fails("elsewhere", &format!("has no binaries for {host}"));
    fails("absent", "Kennel has no package 'absent'");

    *upstream.lock().unwrap() = tar_gz(&[("sdk/lib.so", b"tampered library")]);
    fails("corrupt", "does not match its pinned sha256");
    // Nothing was published or recorded, and no staging or partial download remains.
    let leftovers: Vec<_> = fs::read_dir(game.join("kennel"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    assert_eq!(leftovers, [".lock"]);
    assert!(Lockfile::load(&game).unwrap().packages.is_empty());
    let cached: Vec<_> = fs::read_dir(cache.join("sources"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .filter(|name| !name.ends_with(".lock"))
        .collect();
    assert!(cached.is_empty(), "{cached:?}");

    // Once upstream serves the pinned bytes again, the install succeeds.
    *upstream.lock().unwrap() = archive;
    kennel::install(
        &game,
        "corrupt",
        &registry,
        options(&cache, false, false),
        &progress,
    )
    .unwrap();
    assert_eq!(
        fs::read(game.join("kennel/corrupt/lib.so")).unwrap(),
        library
    );
}

#[test]
fn installed_scripts_compile_from_a_project_scene_catalog() {
    let temp = Temp::new();
    let registry = temp.0.join("registry");
    publish(&registry, &[kit()]);
    let game = project(&temp.0);
    let progress = Progress::default();
    let opened = Registry::open(registry.to_str().unwrap(), &progress).unwrap();
    kennel::install(
        &game,
        "kit",
        &opened,
        options(&temp.0.join("cache"), false, false),
        &progress,
    )
    .unwrap();

    let path = game.join("scenes/main.json");
    let mut scene = Scene::from_json(&fs::read_to_string(&path).unwrap()).unwrap();
    for (id, file) in [("kit/math", "math"), ("kit/player", "player")] {
        let source = AssetSource {
            kind: AssetKind::Script,
            path: format!("../kennel/kit/scripts/{file}.rhai"),
        };
        scene.assets.insert(id.into(), source);
    }
    scene.validate().unwrap();
    let sources = bozzard_scene::load_sources(&scene, Some(&path)).unwrap();
    assert_eq!(sources["kit/player"], PLAYER);
    bozzard_scene::check_script_sources(sources, &progress).unwrap();
}
