//! Reading a scene's script assets from disk.
use super::*;

/// Reads every `script` asset of a scene catalog next to the scene file.
///
/// Mirrors the prefab loader: fixed ticks never touch the filesystem, so sources are read once when
/// the scene is opened and handed together to [`SceneInstance::register_scripts`].
pub fn load_sources(
    document: &Scene,
    path: Option<&std::path::Path>,
) -> Result<BTreeMap<String, String>> {
    load_sources_with_progress(document, path, &bozzard_app::job::Progress::default())
}
pub fn load_sources_with_progress(
    document: &Scene,
    path: Option<&std::path::Path>,
    progress: &bozzard_app::job::Progress,
) -> Result<BTreeMap<String, String>> {
    use std::io::Read;
    // Every `script` catalog entry is read, not only the ones an object names: a prefab member may
    // carry a script, and the loader merges that prefab's catalog into the scene before calling
    // this. Reading the whole catalog is cheap and leaves no source unbound.
    let root = path
        .and_then(std::path::Path::parent)
        .unwrap_or(std::path::Path::new("."));
    let mut sources = BTreeMap::new();
    let mut bytes = 0;
    for (id, source) in document
        .assets
        .iter()
        .filter(|(_, source)| source.kind == AssetKind::Script)
    {
        progress.stage(format!("Reading script {id}"))?;
        ensure!(
            sources.len() < MAX_SCRIPT_ASSETS,
            "scene catalog holds at most {MAX_SCRIPT_ASSETS} scripts"
        );
        let mut text = String::new();
        std::fs::File::open(root.join(&source.path))
            .with_context(|| format!("loading script '{id}'"))?
            .take(MAX_SCRIPT_BYTES as u64 + 1)
            .read_to_string(&mut text)
            .with_context(|| format!("reading script '{id}'"))?;
        ensure!(
            text.len() <= MAX_SCRIPT_BYTES,
            "script '{id}' exceeds 1 MiB"
        );
        progress.check()?;
        bytes += text.len();
        ensure!(bytes <= 32 * 1024 * 1024, "scripts exceed 32 MiB");
        sources.insert(id.clone(), text);
    }
    Ok(sources)
}
