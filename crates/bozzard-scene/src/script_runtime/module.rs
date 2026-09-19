//! Stateless calls into the same compiled Rhai assets used by Script Manager.
//! Prediction/replay uses isolated scopes: it cannot run scene lifecycle hooks or
//! mutate the presentation world. State is explicitly passed in and returned.
use super::*;

/// Read-only, locally bound presentation data. Never accepts remote object paths.
#[derive(Clone, Default)]
pub struct NetworkFrame {
    pub active: bool,
    pub objects: BTreeMap<String, serde_json::Value>,
    pub state: serde_json::Value,
}

#[derive(Clone)]
pub struct ScriptModule {
    asset: String,
    compiled: Arc<CompiledScript>,
    engine: Arc<Mutex<ScriptEngine>>,
}

impl ScriptModule {
    pub fn fingerprint(&self) -> u64 {
        self.compiled.fingerprint
    }

    pub fn require_function(&self, name: &str, args: usize) -> Result<()> {
        ensure!(
            self.compiled
                .ast
                .iter_functions()
                .any(|f| f.name == name && f.params.len() == args),
            "script '{}': missing {name} with {args} argument(s)",
            self.asset
        );
        Ok(())
    }

    /// Bounded Rhai execution, using the standard script function registry and
    /// f32 arithmetic. Each call starts with fresh state for deterministic replay.
    pub fn call<T: serde::de::DeserializeOwned>(
        &self,
        name: &str,
        args: Vec<serde_json::Value>,
    ) -> Result<T> {
        self.require_function(name, args.len())?;
        let engine = self.engine.lock().unwrap_or_else(|e| e.into_inner());
        *engine.lock() = Host {
            budget: 1_000_000,
            ..Default::default()
        };
        let args: Vec<Dynamic> = args
            .iter()
            .map(rhai::serde::to_dynamic)
            .collect::<std::result::Result<_, _>>()
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        let result = engine
            .engine
            .call_fn::<Dynamic>(&mut Scope::new(), &self.compiled.ast, name, args)
            .map_err(|e| anyhow::anyhow!("script '{}' function {name}: {e}", self.asset))?;
        ensure!(
            engine.lock().commands.is_empty(),
            "script '{}' function {name}: replay functions must return state, not issue scene commands",
            self.asset
        );
        rhai::serde::from_dynamic(&result)
            .map_err(|e| anyhow::anyhow!("script '{}' function {name} result: {e}", self.asset))
    }
}

impl SceneInstance {
    /// Reuses a loaded asset's AST, without file access or an embedded fallback.
    pub fn script_module(&self, asset: &str) -> Result<ScriptModule> {
        let mut engine = ScriptEngine::new();
        // Replay can depend only on the loaded, fingerprinted source. Never
        // resolve imports from disk on a worker or while replaying input.
        engine
            .engine
            .set_module_resolver(rhai::module_resolvers::DummyModuleResolver::new());
        Ok(ScriptModule {
            asset: asset.into(),
            compiled: self
                .scripts
                .get(asset)
                .with_context(|| format!("script '{asset}' was not loaded"))?
                .clone(),
            engine: Arc::new(Mutex::new(engine)),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn module(source: &str) -> ScriptModule {
        let scene = Scene::from_json(
            r#"{"version":1,"name":"module","views":{},
            "assets":{"rules":{"kind":"script","path":"rules.rs"}},"objects":[]}"#,
        )
        .unwrap();
        let mut instance = scene.spawn(&mut World::new()).unwrap();
        instance
            .register_script("rules".into(), source.into())
            .unwrap();
        instance.script_module("rules").unwrap()
    }

    #[test]
    fn calls_use_loaded_ast_and_fresh_scope_without_running_lifecycle_hooks() {
        let module = module(
            r#"
            fn on_start(me) { throw "must not run"; }
            fn next(state) { state.value += 1; state }
        "#,
        );
        for _ in 0..3 {
            let state: serde_json::Value = module
                .call("next", vec![serde_json::json!({"value": 4})])
                .unwrap();
            assert_eq!(state["value"], 5);
        }
        assert!(module.call::<bool>("missing", vec![]).is_err());
    }

    #[test]
    fn replay_rejects_side_effects_imports_and_runaway_functions() {
        let commands = module("fn change() { set_exposure(1.0); true }");
        assert!(
            commands
                .call::<bool>("change", vec![])
                .unwrap_err()
                .to_string()
                .contains("scene commands")
        );
        let imports = module("fn change() { import \"missing\" as other; true }");
        assert!(imports.call::<bool>("change", vec![]).is_err());
        let runaway = module("fn change() { loop {} }");
        assert!(runaway.call::<bool>("change", vec![]).is_err());
    }
}
