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
        let args: Vec<Dynamic> = args
            .iter()
            .map(rhai::serde::to_dynamic)
            .collect::<std::result::Result<_, _>>()
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        self.call_dynamic(name, args)
    }

    /// Calls a function with a serializable tuple, avoiding a JSON value allocation for each
    /// argument. This is useful for fixed-shape state passed through high-frequency replay calls.
    pub fn call_args<A, T>(&self, name: &str, args: A) -> Result<T>
    where
        A: serde::Serialize,
        T: serde::de::DeserializeOwned,
    {
        let args = rhai::serde::to_dynamic(&args).map_err(|e| anyhow::anyhow!(e.to_string()))?;
        let args = args
            .try_cast::<rhai::Array>()
            .ok_or_else(|| anyhow::anyhow!("script arguments must serialize as a tuple"))?;
        self.call_dynamic(name, args)
    }

    fn call_dynamic<T: serde::de::DeserializeOwned>(
        &self,
        name: &str,
        args: Vec<Dynamic>,
    ) -> Result<T> {
        self.require_function(name, args.len())?;
        let engine = self.engine.lock().unwrap_or_else(|e| e.into_inner());
        *engine.lock() = Host {
            budget: 1_000_000,
            ..Default::default()
        };
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
        let state: serde_json::Value = module
            .call_args("next", (serde_json::json!({"value": 4}),))
            .unwrap();
        assert_eq!(state["value"], 5);
        assert!(module.call::<bool>("missing", vec![]).is_err());
    }

    #[test]
    fn serializable_tuple_arguments_match_legacy_json_arguments() {
        #[derive(serde::Serialize)]
        struct Player {
            slot: u8,
            x: f32,
            alive: bool,
        }

        #[derive(serde::Serialize)]
        struct Pipe {
            x: f32,
            gap: f32,
            cycle: u32,
        }

        let module = module(
            r#"
            fn predict(player, pressed, dt) {
                if pressed { player.x += dt; }
                player
            }
            fn step(pipes, dt) {
                pipes[0].x += dt;
                pipes
            }
        "#,
        );
        let player = Player {
            slot: 1,
            x: -2.0,
            alive: true,
        };
        let legacy_player: serde_json::Value = module
            .call(
                "predict",
                vec![
                    serde_json::json!({"slot": 1, "x": -2.0, "alive": true}),
                    serde_json::json!(true),
                    serde_json::json!(0.25),
                ],
            )
            .unwrap();
        let direct_player: serde_json::Value = module
            .call_args("predict", (&player, true, 0.25f32))
            .unwrap();
        assert_eq!(direct_player, legacy_player);

        let pipes = [
            Pipe {
                x: 1.0,
                gap: 0.0,
                cycle: 0,
            },
            Pipe {
                x: 2.0,
                gap: -1.0,
                cycle: 4,
            },
            Pipe {
                x: 3.0,
                gap: 1.0,
                cycle: 8,
            },
        ];
        let legacy_pipes: serde_json::Value = module
            .call(
                "step",
                vec![
                    serde_json::json!([
                        {"x": 1.0, "gap": 0.0, "cycle": 0},
                        {"x": 2.0, "gap": -1.0, "cycle": 4},
                        {"x": 3.0, "gap": 1.0, "cycle": 8}
                    ]),
                    serde_json::json!(0.25),
                ],
            )
            .unwrap();
        let direct_pipes: serde_json::Value = module.call_args("step", (&pipes, 0.25f32)).unwrap();
        assert_eq!(direct_pipes, legacy_pipes);
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

    #[test]
    #[ignore = "manual baseline for network script call cost"]
    fn benchmark_repeated_player_prediction() {
        use std::time::Instant;
        let module = module(
            r#"
            fn network_predict(player, pressed, dt) {
                if !player.alive { return player; }
                if pressed { player.velocity = 6.5; }
                player.velocity -= 22.0 * dt;
                player.y += player.velocity * dt;
                player
            }
        "#,
        );
        let state = serde_json::json!({"slot":0,"x":-5.0,"y":0.65,"velocity":0.0,"alive":true,"score":0,"input_ack":0});
        let started = Instant::now();
        for _ in 0..100_000 {
            let _: serde_json::Value = module
                .call(
                    "network_predict",
                    vec![
                        state.clone(),
                        serde_json::json!(false),
                        serde_json::json!(1.0 / 60.0),
                    ],
                )
                .unwrap();
        }
        let json_ms = started.elapsed().as_secs_f64() * 1000.0;
        let started = Instant::now();
        for _ in 0..100_000 {
            let _: serde_json::Value = module
                .call_args("network_predict", (&state, false, 1.0f32 / 60.0))
                .unwrap();
        }
        let direct_ms = started.elapsed().as_secs_f64() * 1000.0;
        eprintln!("100k_prediction_calls json_ms={json_ms:.3} direct_ms={direct_ms:.3}");
    }
}
