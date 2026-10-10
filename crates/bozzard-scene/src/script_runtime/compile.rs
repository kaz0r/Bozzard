//! Compiling script assets and whole script catalogs.
use super::*;

pub(super) fn compile_source(
    engine: &ScriptEngine,
    asset: &str,
    source: &str,
) -> Result<Arc<CompiledScript>> {
    ensure!(
        source.len() <= MAX_SCRIPT_BYTES,
        "script '{asset}' exceeds 1 MiB"
    );
    let mut ast = engine
        .engine
        .compile(source)
        .map_err(|error| anyhow::anyhow!("script '{asset}': {error}"))?;
    ast.set_source(asset);
    let mut hooks = BTreeMap::new();
    for function in ast.iter_functions() {
        if let Some((name, args)) = HOOKS.iter().find(|(name, _)| *name == function.name) {
            let declaration = format!("fn {name}");
            let line = source
                .lines()
                .position(|line| line.contains(&declaration))
                .map_or(1, |index| index + 1);
            ensure!(
                function.params.len() == *args,
                "script '{asset}' line {line}: {name} takes {args} argument(s), got {}",
                function.params.len()
            );
            hooks.insert(function.name.to_owned(), function.params.len());
        }
    }
    let fingerprint = source.bytes().fold(14695981039346656037u64, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(1099511628211)
    });
    Ok(Arc::new(CompiledScript {
        hook_ast: ast.clone_functions_only(),
        ast,
        hooks,
        fingerprint,
        source: Arc::from(source),
        dependencies: BTreeSet::new(),
    }))
}

pub(crate) fn compile_sources(
    sources: BTreeMap<String, String>,
    progress: &bozzard_app::job::Progress,
) -> Result<BTreeMap<String, Arc<CompiledScript>>> {
    imports::compile_catalog(sources, progress)
}

/// Compiles a script catalog keyed by asset ID without a scene, as a scene load would: imports
/// resolve within the catalog and hook arities are checked. Tools use it to vet scripts before
/// they reach a project.
pub fn check_script_sources(
    sources: BTreeMap<String, String>,
    progress: &bozzard_app::job::Progress,
) -> Result<()> {
    compile_sources(sources, progress).map(drop)
}
