//! Resolve literal imports against the scene's script asset catalog, before any tick runs.
use super::*;
use rhai::{
    ASTFlags, ASTNode, Expr, Module, ModuleResolver, Stmt, module_resolvers::StaticModuleResolver,
};

// Supplying ASTs as well as modules lets Rhai embed function-local imports from
// transitive dependencies. No path in this resolver refers to the filesystem.
struct CatalogResolver {
    modules: StaticModuleResolver,
    programs: BTreeMap<String, Arc<CompiledScript>>,
}

impl ModuleResolver for CatalogResolver {
    fn resolve(
        &self,
        engine: &Engine,
        source: Option<&str>,
        path: &str,
        position: Position,
    ) -> std::result::Result<rhai::Shared<Module>, Box<EvalAltResult>> {
        self.modules.resolve(engine, source, path, position)
    }

    fn resolve_ast(
        &self,
        _: &Engine,
        _: Option<&str>,
        path: &str,
        _: Position,
    ) -> Option<std::result::Result<AST, Box<EvalAltResult>>> {
        self.programs.get(path).map(|script| Ok(script.ast.clone()))
    }
}

pub(super) fn compile_catalog(
    sources: BTreeMap<String, String>,
    progress: &bozzard_app::job::Progress,
) -> Result<BTreeMap<String, Arc<CompiledScript>>> {
    ensure!(
        sources.len() <= MAX_SCRIPT_ASSETS,
        "scene script catalog exceeds its limit"
    );
    ensure!(
        sources.values().map(String::len).sum::<usize>() <= 32 * 1024 * 1024,
        "scripts exceed 32 MiB"
    );
    let mut engine = ScriptEngine::new();
    let mut pending = BTreeMap::new();
    let mut imported = BTreeSet::new();
    for (id, source) in &sources {
        progress.stage(format!("Compiling script {id}"))?;
        let mut script = compile_source(&engine, id, source)?;
        let script_mut = Arc::make_mut(&mut script);
        let mut invalid = None;
        script_mut.ast.walk(&mut |path| {
            if let Some(ASTNode::Stmt(Stmt::Import(import, position))) = path.last() {
                if let Expr::StringConstant(name, _) = &import.0 {
                    script_mut.dependencies.insert(name.to_string());
                } else {
                    invalid = Some(*position);
                }
            }
            true
        });
        ensure!(
            invalid.is_none(),
            "script '{id}': imports must use literal script asset IDs ({})",
            invalid.unwrap_or_default()
        );
        for dependency in &script.dependencies {
            ensure!(
                sources.contains_key(dependency),
                "script '{id}': imported script asset '{dependency}' was not loaded"
            );
            imported.insert(dependency.clone());
        }
        pending.insert(id.clone(), script);
    }
    // Library initialization runs on the loader worker, without an object/world context.
    // State belongs in blackboards or hook scopes; module globals are immutable literals.
    for id in &imported {
        for statement in pending[id].ast.statements() {
            let allowed = match statement {
                Stmt::Noop(_) | Stmt::Import(..) | Stmt::Export(..) => true,
                Stmt::Var(value, flags, _) => {
                    flags.contains(ASTFlags::CONSTANT) && value.1.get_literal_value(None).is_some()
                }
                _ => false,
            };
            ensure!(
                allowed,
                "module '{id}': top-level code may contain only imports and literal constants; put gameplay work in functions ({})",
                statement.position()
            );
        }
    }
    let mut ready: BTreeMap<String, Arc<CompiledScript>> = BTreeMap::new();
    let mut resolver = StaticModuleResolver::new();
    while !pending.is_empty() {
        progress.check()?;
        let next = pending
            .iter()
            .find(|(_, script)| script.dependencies.iter().all(|id| ready.contains_key(id)))
            .map(|(id, _)| id.clone());
        let Some(id) = next else {
            anyhow::bail!(
                "cyclic script imports among: {}",
                pending.keys().cloned().collect::<Vec<_>>().join(", ")
            );
        };
        let mut script = pending.remove(&id).unwrap();
        let value = Arc::make_mut(&mut script);
        if !value.dependencies.is_empty() {
            engine.engine.set_module_resolver(CatalogResolver {
                modules: resolver.clone(),
                programs: ready.clone(),
            });
            value.ast = engine
                .engine
                .compile_into_self_contained(&Scope::new(), &*value.source)
                .map_err(|error| anyhow::anyhow!("script '{id}': {error}"))?;
            value.ast.set_source(id.as_str());
            let direct = value.dependencies.clone();
            for dependency in direct {
                value
                    .dependencies
                    .extend(ready[&dependency].dependencies.iter().cloned());
            }
            // Replay compatibility includes every transitive dependency, in stable asset order.
            for dependency in &value.dependencies {
                for byte in dependency
                    .bytes()
                    .chain([0])
                    .chain(sources[dependency].bytes())
                    .chain([0])
                {
                    value.fingerprint =
                        (value.fingerprint ^ u64::from(byte)).wrapping_mul(1099511628211);
                }
            }
        }
        // Re-establish import aliases for each hook without rerunning global initializers.
        // Rhai imports live in the call context, not the attachment's persistent Scope.
        value.hook_ast = value.ast.clone_functions_only();
        value.hook_ast += AST::new(
            value
                .ast
                .statements()
                .iter()
                .filter(|statement| matches!(statement, Stmt::Import(..)))
                .cloned(),
            Module::new(),
        );
        if imported.contains(&id) {
            let module = Module::eval_ast_as_new(Scope::new(), &value.ast, &engine.engine)
                .map_err(|error| anyhow::anyhow!("module '{id}': {error}"))?;
            resolver.insert(id.clone(), module);
        }
        ready.insert(id, script);
    }
    Ok(ready)
}
