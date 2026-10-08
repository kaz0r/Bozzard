//! Headless project creation and structural scene merge tools.
use anyhow::{Context, Result, bail, ensure};
use bozzard_project::{create_project, kennel, merge_scenes};
use bozzard_scene::Scene;
use std::{
    fs,
    io::{Read, Write},
    path::Path,
};

fn load(path: &Path) -> Result<Scene> {
    let mut source = String::new();
    fs::File::open(path)
        .with_context(|| format!("open {}", path.display()))?
        .take(64 * 1024 * 1024 + 1)
        .read_to_string(&mut source)?;
    ensure!(source.len() <= 64 * 1024 * 1024, "scene exceeds 64 MiB");
    Scene::from_json(&source).with_context(|| format!("load {}", path.display()))
}
fn create_file(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .with_context(|| format!("create {} (must not already exist)", path.display()))?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}
/// `kennel <subcommand>`: flags may appear anywhere; positionals select the action.
fn kennel(args: &[std::ffi::OsString]) -> Result<()> {
    let mut registry = None;
    let (mut all_targets, mut force, mut fetch) = (false, false, false);
    let mut positional = Vec::new();
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        let arg = arg.to_str().context("kennel arguments must be UTF-8")?;
        match arg {
            "--registry" => {
                let value = args.next().context("--registry needs a folder or URL")?;
                registry = Some(value.to_str().context("registry must be UTF-8")?.to_owned());
            }
            "--all-targets" => all_targets = true,
            "--force" => force = true,
            "--fetch" => fetch = true,
            flag if flag.starts_with("--") => bail!("unknown kennel option {flag}"),
            _ => positional.push(arg),
        }
    }
    let progress = bozzard_assets::job::Progress::default();
    let open = || {
        let location = registry.clone().unwrap_or_else(kennel::default_registry);
        kennel::Registry::open(&location, &progress)
    };
    match positional.as_slice() {
        ["list", query @ ..] if query.len() <= 1 => {
            let registry = open()?;
            let query = query.first().copied().unwrap_or_default();
            for (name, entry) in &registry.index.packages {
                if entry.matches(name, query) {
                    println!(
                        "{name} {} [{}] {}",
                        entry.version,
                        entry.category.as_str(),
                        entry.summary
                    );
                }
            }
        }
        ["info", name] => {
            let (manifest, _) = open()?.manifest(name, &progress)?;
            println!("{}", serde_json::to_string_pretty(&manifest)?);
        }
        ["install", name, project] => {
            let options = kennel::InstallOptions {
                all_targets,
                force,
                cache: None,
            };
            let installed =
                kennel::install(Path::new(project), name, &open()?, options, &progress)?;
            for package in installed {
                println!(
                    "kennel_install_ok name={} version={} dir={} files={} targets={} requires_features={}{}",
                    package.name,
                    package.version,
                    package.directory.display(),
                    package.files,
                    package.targets.join(","),
                    package.features.join(","),
                    if package.unchanged { " unchanged" } else { "" }
                );
                for (variable, folder) in package.build_env {
                    println!("{variable}={}", folder.display());
                }
            }
        }
        ["verify", project] => {
            let verified = kennel::verify(Path::new(project), &progress)?;
            for (name, version, files) in &verified {
                println!("kennel_verify_ok name={name} version={version} files={files}");
            }
            if verified.is_empty() {
                println!("kennel_verify_ok packages=0");
            }
        }
        ["remove", name, project] => {
            kennel::remove(Path::new(project), name, force, &progress)?;
            println!("kennel_remove_ok name={name}");
        }
        ["index", folder] => {
            let index = kennel::build_index(Path::new(folder))?;
            println!(
                "kennel_index_ok packages={} index={}",
                index.packages.len(),
                Path::new(folder).join(kennel::INDEX_FILE).display()
            );
        }
        ["check", folder] => {
            let cache = if fetch {
                Some(kennel::cache_directory()?)
            } else {
                None
            };
            let report = kennel::check(Path::new(folder), cache.as_deref(), &progress)?;
            println!(
                "kennel_check_ok packages={} files={} scripts={} assets={} sources={}",
                report.packages, report.files, report.scripts, report.assets, report.sources
            );
        }
        _ => bail!("invalid kennel arguments; run bozzard-project --help"),
    }
    Ok(())
}

fn main() -> Result<()> {
    if std::env::args().nth(1).as_deref() == Some("--runtime-info") {
        println!("{}", bozzard_project::runtime::description());
        return Ok(());
    }
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let command = args.first().and_then(|arg| arg.to_str()).unwrap_or("help");
    match command {
        "bundle" if args.len() == 3 => {
            let prepared = bozzard_project::content::prepare_pack(
                Path::new(&args[1]),
                Path::new(&args[2]),
                &Default::default(),
            )?;
            let report = prepared.report();
            let catalog = prepared.commit()?;
            println!(
                "bundle_ok catalog={} cooked={} reused={} copied={}",
                catalog.display(),
                report.built,
                report.reused,
                report.copied
            );
        }
        "fetch-content" if args.len() == 4 => {
            use bozzard_project::content::{ContentStore, load_catalog};
            let progress = bozzard_assets::job::Progress::default();
            let catalog = load_catalog(
                args[1].to_str().context("catalog location must be UTF-8")?,
                &progress,
            )?;
            let address = args[2].to_str().context("content address must be UTF-8")?;
            let mut store = ContentStore::new(Path::new(&args[3]));
            let content = store.resolve(&catalog, address, &progress)?;
            println!(
                "{}",
                serde_json::to_string_pretty(
                    &serde_json::json!({"address":address,"pack":content.pack().id(),"entry":content.entry(),"path":content.path()})
                )?
            );
        }
        "list-content" if args.len() == 2 => {
            let catalog = bozzard_project::content::load_catalog(
                args[1].to_str().context("catalog location must be UTF-8")?,
                &Default::default(),
            )?;
            println!(
                "{}",
                serde_json::to_string_pretty(&catalog.catalog.addresses)?
            );
        }
        "cook-model" if args.len() == 4 => {
            use bozzard_assets::{
                AssetData, AssetStore, cooked_model, job::Progress, texture::Compression,
            };
            use bozzard_scene::{AssetKind, AssetSource};
            let formats: &[_] = match args[1].to_str() {
                Some("rgba") => &[],
                Some("bc") => &[Compression::Bc3],
                Some("astc") => &[Compression::Astc4x4],
                Some("universal") => &[Compression::Bc3, Compression::Astc4x4],
                _ => bail!("model target must be rgba, bc, astc, or universal"),
            };
            let source = Path::new(&args[2]).canonicalize()?;
            let output = Path::new(&args[3]);
            ensure!(
                output.extension().is_some_and(|e| e == "bmesh"),
                "cooked output must end in .bmesh"
            );
            ensure!(!output.exists(), "cooked output already exists");
            let sources = [(
                "model".into(),
                AssetSource {
                    kind: AssetKind::Mesh,
                    path: source
                        .file_name()
                        .context("model filename")?
                        .to_str()
                        .context("model filename must be UTF-8")?
                        .into(),
                },
            )]
            .into();
            let mut store = AssetStore::new(source.parent().context("model parent")?, &sources)?;
            store.load_pending()?;
            store.require_ready()?;
            let AssetData::Mesh(mesh) = store
                .get(store.handle("model").unwrap())
                .unwrap()
                .data()
                .unwrap()
            else {
                unreachable!()
            };
            let bytes = cooked_model::encode(mesh, formats, &Progress::default())?;
            create_file(output, &bytes)?;
            println!(
                "Cooked {}: {} vertices, {} triangles, {} surfaces, {} bytes",
                output.display(),
                mesh.vertices.len(),
                mesh.indices.len() / 3,
                mesh.parts.len(),
                bytes.len()
            );
        }
        "cook-texture" if args.len() == 4 || args.len() == 5 => {
            use bozzard_assets::{
                AssetData, AssetStore,
                job::Progress,
                texture::{self, Compression},
            };
            use bozzard_scene::{AssetKind, AssetSource};
            let formats: &[_] = match args[1].to_str() {
                Some("bc") => &[Compression::Bc3],
                Some("astc") => &[Compression::Astc4x4],
                Some("universal") => &[Compression::Bc3, Compression::Astc4x4],
                _ => bail!("texture target must be bc, astc, or universal"),
            };
            let srgb = match args.get(4).and_then(|v| v.to_str()).unwrap_or("srgb") {
                "srgb" => true,
                "linear" => false,
                _ => bail!("color space must be srgb or linear"),
            };
            let source = Path::new(&args[2]).canonicalize()?;
            let output = Path::new(&args[3]);
            ensure!(
                output.extension().is_some_and(|e| e == "btex"),
                "cooked output must end in .btex"
            );
            ensure!(!output.exists(), "cooked output already exists");
            let sources = [(
                "texture".into(),
                AssetSource {
                    kind: AssetKind::Image,
                    path: source
                        .file_name()
                        .context("texture filename")?
                        .to_str()
                        .context("texture filename must be UTF-8")?
                        .into(),
                },
            )]
            .into();
            let mut store = AssetStore::new(source.parent().context("texture parent")?, &sources)?;
            store.load_pending()?;
            store.require_ready()?;
            let AssetData::Image(image) = store
                .get(store.handle("texture").unwrap())
                .unwrap()
                .data()
                .unwrap()
            else {
                unreachable!()
            };
            let cooked = texture::cook(image, formats, &[srgb], &Progress::default())?;
            let bytes = texture::encode(image, &cooked)?;
            create_file(output, &bytes)?;
            println!(
                "Cooked {}: {}x{}, {} bytes on disk; RGBA fallback included",
                output.display(),
                image.width,
                image.height,
                bytes.len()
            );
            for variant in cooked.variants() {
                println!(
                    "  {:?}: {} mip levels, {} GPU bytes",
                    variant.format(),
                    variant.levels().len(),
                    variant.bytes()
                );
            }
            if !image.width.is_multiple_of(4) || !image.height.is_multiple_of(4) {
                println!("  Base dimensions are not multiples of four; GPU upload will use RGBA.");
            }
        }
        "new" if args.len() == 4 => {
            let template = args[1]
                .to_str()
                .context("template must be UTF-8")?
                .parse()?;
            let manifest = create_project(
                Path::new(&args[2]),
                args[3].to_str().context("name must be UTF-8")?,
                template,
            )?;
            println!("Created {}", manifest.display());
        }
        "merge" if args.len() == 5 => {
            let [base, ours, theirs] = [
                load(Path::new(&args[1]))?,
                load(Path::new(&args[2]))?,
                load(Path::new(&args[3]))?,
            ];
            let result = merge_scenes(&base, &ours, &theirs)?;
            match result.resolved_scene() {
                Ok(scene) => {
                    create_file(Path::new(&args[4]), scene.to_json()?.as_bytes())?;
                    println!("Merged {}", Path::new(&args[4]).display());
                }
                Err(error) => {
                    let mut report = args[4].clone();
                    report.push(".conflicts.json");
                    create_file(Path::new(&report), &serde_json::to_vec_pretty(&result)?)?;
                    bail!(
                        "{error:#}; review {}. Output scene was not written.",
                        Path::new(&report).display()
                    );
                }
            }
        }
        "kennel" if args.len() >= 2 => kennel(&args[1..])?,
        "help" | "--help" | "-h" => println!(
            "Bozzard project tools\n  bundle <SPEC.json> <NEW_RELEASE_FOLDER>\n  list-content <CATALOG_FILE_OR_URL>\n  fetch-content <CATALOG_FILE_OR_URL> <ADDRESS> <CACHE_DIR>\n  new <third-person|collect-2d> <NEW_FOLDER> <NAME>\n  merge <BASE.json> <OURS.json> <THEIRS.json> <NEW_OUTPUT.json>\n  cook-texture <bc|astc|universal> <INPUT.png> <NEW_OUTPUT.btex> [srgb|linear]\n  cook-model <rgba|bc|astc|universal> <INPUT.obj|gltf|glb> <NEW_OUTPUT.bmesh>\n\nKennel packages (registry: --registry <FOLDER|URL>, $BOZZARD_KENNEL_REGISTRY, or {})\n  kennel list [QUERY]\n  kennel info <NAME>\n  kennel install <NAME> <PROJECT> [--all-targets] [--force]\n  kennel verify <PROJECT>\n  kennel remove <NAME> <PROJECT> [--force]\n  kennel index <REGISTRY_FOLDER>\n  kennel check <REGISTRY_FOLDER> [--fetch]\n\nMerge conflicts produce NEW_OUTPUT.json.conflicts.json and a nonzero exit code.\nAll inputs and existing destinations are preserved.",
            kennel::DEFAULT_REGISTRY
        ),
        _ => bail!("invalid arguments; run bozzard-project --help"),
    }
    Ok(())
}
