//! Binding script catalogs and attachments to a scene instance, and live reload.
use super::*;

impl SceneInstance {
    /// Compile a catalog together so imports resolve regardless of asset ordering.
    /// Validate attachments before publishing anything; a bad source or missing asset leaves
    /// the previously loaded catalog intact.
    pub fn register_scripts(&mut self, sources: BTreeMap<String, String>) -> Result<()> {
        self.register_scripts_with_progress(sources, &bozzard_app::job::Progress::default())
    }
    /// Resolve the entire import catalog together, with cancellable loading progress.
    pub fn register_scripts_with_progress(
        &mut self,
        sources: BTreeMap<String, String>,
        progress: &bozzard_app::job::Progress,
    ) -> Result<()> {
        self.bind_script_sources(sources, progress, true)
    }
    fn bind_script_sources(
        &mut self,
        sources: BTreeMap<String, String>,
        progress: &bozzard_app::job::Progress,
        require_attachments: bool,
    ) -> Result<()> {
        for asset in sources.keys() {
            ensure!(
                self.document
                    .assets
                    .get(asset)
                    .is_some_and(|entry| entry.kind == AssetKind::Script),
                "asset '{asset}' is not a script"
            );
        }
        let mut catalog = self.script_sources();
        catalog.extend(sources);
        let compiled = compile_sources(catalog, progress)?;
        if require_attachments {
            for object in &self.document.objects {
                for (index, attachment) in object
                    .script_manager
                    .iter()
                    .flat_map(|manager| &manager.scripts)
                    .enumerate()
                {
                    let attachment = &attachment.script;
                    ensure!(
                        compiled.contains_key(attachment),
                        "script '{attachment}' on '{}' (attachment {index}) was not loaded; \
                     the scene catalog does not list it as a script asset",
                        object.id
                    );
                }
            }
        }
        for (asset, script) in &compiled {
            if self
                .scripts
                .get(asset)
                .is_none_or(|old| old.source != script.source)
            {
                *self
                    .script_reload_revisions
                    .entry(asset.clone())
                    .or_default() += 1;
            }
        }
        self.scripts = compiled;
        Ok(())
    }
    /// Script asset IDs the object's attachments name, in order.
    pub(super) fn document_attachments(&self, object: &str) -> Vec<String> {
        self.document
            .objects
            .iter()
            .find(|candidate| candidate.id == object)
            .and_then(|candidate| candidate.script_manager.as_ref())
            .map(|manager| {
                manager
                    .scripts
                    .iter()
                    .map(|attachment| attachment.script.clone())
                    .collect()
            })
            .unwrap_or_default()
    }
    pub fn register_script(&mut self, asset: String, source: String) -> Result<()> {
        self.bind_script_sources(
            BTreeMap::from([(asset, source)]),
            &bozzard_app::job::Progress::default(),
            false,
        )
    }
    fn script_sources(&self) -> BTreeMap<String, String> {
        self.scripts
            .iter()
            .map(|(id, script)| (id.clone(), script.source.to_string()))
            .collect()
    }
    /// Reserve a revision before starting background compilation. A newer edit invalidates any
    /// older result, even if the older worker completes last. The caller must reject this route
    /// while multiplayer is active and coordinate a restart instead.
    pub fn request_script_reload(
        &mut self,
        asset: &str,
        source: String,
    ) -> Result<ScriptReloadRequest> {
        ensure!(
            source.len() <= MAX_SCRIPT_BYTES,
            "script '{asset}' exceeds 1 MiB"
        );
        ensure!(
            self.document
                .assets
                .get(asset)
                .is_some_and(|entry| entry.kind == AssetKind::Script),
            "asset '{asset}' is not a script"
        );
        ensure!(
            self.scripts.contains_key(asset),
            "script '{asset}' was not loaded"
        );
        let revision = self
            .script_reload_revisions
            .entry(asset.to_owned())
            .or_default();
        *revision = revision
            .checked_add(1)
            .context("script edit revision exhausted")?;
        let revision = *revision;
        let affected: BTreeSet<_> = self
            .scripts
            .iter()
            .filter(|(id, script)| id.as_str() == asset || script.dependencies.contains(asset))
            .map(|(id, _)| id.clone())
            .collect();
        let attachments = self
            .document
            .objects
            .iter()
            .flat_map(|object| {
                object
                    .script_manager
                    .iter()
                    .flat_map(|manager| manager.scripts.iter().enumerate())
                    .filter(|(_, attachment)| affected.contains(&attachment.script))
                    .map(move |(index, attachment)| {
                        (object.id.clone(), index, attachment.script.clone())
                    })
            })
            .collect();
        let mut sources = self.script_sources();
        sources.insert(asset.to_owned(), source);
        Ok(ScriptReloadRequest {
            asset: asset.to_owned(),
            sources,
            baseline: self.scripts.clone(),
            revisions: self.script_reload_revisions.clone(),
            instance: self.instance_id,
            serial: self.scene_serial,
            revision,
            attachments,
        })
    }
    /// Publish only at a completed tick boundary. This does not call lifecycle hooks, discard
    /// queued actions or reset world/blackboard state. Each matching attachment retains its
    /// started/enabled/input/contact state and gets a fresh script-local scope.
    pub fn publish_script_reload(
        &mut self,
        world: &mut World,
        candidate: ScriptReloadCandidate,
    ) -> Result<ScriptReloadStatus> {
        crate::scene_control::require_tick_boundary(world)?;
        let stale = if self.instance_id != candidate.instance
            || self.scene_serial != candidate.serial
        {
            Some("scene changed")
        } else if self.script_reload_revisions.get(&candidate.asset) != Some(&candidate.revision) {
            Some("newer script edit")
        } else if !self.scripts.contains_key(&candidate.asset)
            || !self
                .document
                .assets
                .get(&candidate.asset)
                .is_some_and(|a| a.kind == AssetKind::Script)
        {
            Some("script asset removed")
        } else if candidate
            .stamps
            .iter()
            .any(|(id, (revision, fingerprint))| {
                self.script_reload_revisions
                    .get(id)
                    .copied()
                    .unwrap_or_default()
                    != *revision
                    || self
                        .scripts
                        .get(id)
                        .is_none_or(|script| script.fingerprint != *fingerprint)
                    || !self
                        .document
                        .assets
                        .get(id)
                        .is_some_and(|entry| entry.kind == AssetKind::Script)
            })
        {
            Some("import dependency or consumer changed")
        } else if self.scripts.iter().any(|(id, script)| {
            script.dependencies.contains(&candidate.asset) && !candidate.compiled.contains_key(id)
        }) {
            Some("new import consumer loaded")
        } else if candidate.attachments.iter().any(|(owner, index, asset)| {
            self.document
                .objects
                .iter()
                .find(|o| &o.id == owner)
                .and_then(|o| o.script_manager.as_ref())
                .and_then(|m| m.scripts.get(*index))
                .is_none_or(|a| &a.script != asset)
        }) {
            Some("script attachment removed or changed")
        } else {
            None
        };
        if let Some(reason) = stale {
            return Ok(ScriptReloadStatus::Stale {
                asset: candidate.asset,
                reason: reason.into(),
            });
        }
        if let Some(runtime) = world.resource_mut::<ScriptRuntime>() {
            // Prefab and additive-scene attachments can appear while a worker compiles.
            // Every live consumer of the asset needs fresh script-local globals, including
            // attachments that were not present when this request was made.
            for object in &self.document.objects {
                if let Some(manager) = &object.script_manager {
                    for (index, attachment) in manager.scripts.iter().enumerate() {
                        if candidate.compiled.contains_key(&attachment.script)
                            && let Some(run) = runtime.runs.get_mut(&(object.id.clone(), index))
                        {
                            run.scope = Scope::new();
                            run.scope_initialized = false;
                        }
                    }
                }
            }
        }
        let asset = candidate.asset;
        let revision = candidate.revision;
        self.scripts.extend(candidate.compiled);
        Ok(ScriptReloadStatus::Applied { asset, revision })
    }
    pub(super) fn script_engine(&self) -> Arc<ScriptEngine> {
        self.script_engine
            .get_or_init(|| Arc::new(ScriptEngine::new()))
            .clone()
    }
    /// Whether the scene runs gameplay logic at all, from graphs, scripts, or both.
    pub fn has_gameplay_logic(&self) -> bool {
        self.has_blueprints() || self.has_scripts()
    }
    /// Whether any object carries a script, which makes the scene a gameplay scene.
    pub fn has_scripts(&self) -> bool {
        self.document.has_scripts()
    }
    pub fn set_script_enabled(&mut self, owner: &str, index: usize, enabled: bool) -> Result<()> {
        self.document
            .objects
            .iter_mut()
            .find(|object| object.id == owner)
            .context("unknown script owner")?
            .script_manager
            .as_mut()
            .context("owner has no Script Manager")?
            .scripts
            .get_mut(index)
            .context("script attachment index out of bounds")?
            .enabled = enabled;
        Ok(())
    }
}
