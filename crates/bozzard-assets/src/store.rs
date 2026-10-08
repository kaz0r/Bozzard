//! The asset store: loading a scene catalog, refreshing changed sources and publishing entries.
use super::*;

impl Entry {
    /// Ready data decoded from these exact bytes without external dependencies.
    pub fn matches_standalone_source(&self, bytes: &[u8]) -> bool {
        matches!(self.state, LoadState::Ready)
            && self.observed.as_ref().is_some_and(|source| {
                source.dependencies.is_empty()
                    && source
                        .primary
                        .as_deref()
                        .is_ok_and(|original| original == bytes)
            })
    }
    /// Paths observed by the latest source read, for safe project-file deletion.
    pub fn source_dependencies(&self) -> impl Iterator<Item = &Path> {
        self.observed
            .iter()
            .flat_map(|s| &s.dependencies)
            .map(|(path, _)| path.as_path())
    }
    /// Digest of the last successfully decoded source bytes and dependencies.
    pub fn content_fingerprint(&self) -> Option<u64> {
        self.content_fingerprint
    }
    pub fn state(&self) -> &LoadState {
        &self.state
    }
    pub fn data(&self) -> Option<&AssetData> {
        self.data.as_deref()
    }
    /// Immutable data identity survives catalog snapshots and Undo/Redo.
    pub fn shared_data(&self) -> Option<Arc<AssetData>> {
        self.data.clone()
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    /// Nearest triangle in immutable imported geometry; no texture alpha testing.
    pub fn raycast(&self, origin: Vec3, direction: Vec3) -> Option<MeshHit> {
        let AssetData::Mesh(mesh) = self.data()? else {
            return None;
        };
        self.mesh_index.as_ref()?.cast(mesh, origin, direction)
    }
    /// Diagnostic full-scan oracle for checking accelerated picking and measuring it.
    pub fn raycast_reference(&self, origin: Vec3, direction: Vec3) -> Option<MeshHit> {
        let AssetData::Mesh(mesh) = self.data()? else {
            return None;
        };
        picking::cast_linear(mesh, origin, direction)
    }
    /// Reuse the source BVH for transformed surface rays, filtering original triangle IDs.
    pub fn raycast_filtered(
        &self,
        origin: Vec3,
        direction: Vec3,
        accept: impl Fn(u32) -> bool,
        reference: bool,
    ) -> Option<MeshHit> {
        let AssetData::Mesh(mesh) = self.data()? else {
            return None;
        };
        if reference {
            picking::cast_linear_filtered(mesh, origin, direction, &accept)
        } else {
            self.mesh_index
                .as_ref()?
                .cast_filtered(mesh, origin, direction, &accept)
        }
    }
    /// Cached bounds of indexed triangles, available without scanning source vertices.
    pub fn mesh_bounds(&self) -> Option<[Vec3; 2]> {
        self.mesh_index.as_ref()?.bounds()
    }
    /// Immutable source-surface bounds published with the matching picking index.
    pub fn mesh_part_bounds(&self, index: usize) -> Option<[Vec3; 2]> {
        self.mesh_index.as_ref()?.part_bounds(index)
    }
    pub fn mesh_pick_stats(&self) -> Option<MeshPickStats> {
        self.mesh_index.as_ref().map(|index| index.stats())
    }
}

impl AssetStore {
    /// Resolve a stable surface binding. Changed source geometry must be rebound explicitly.
    pub fn mesh_surface(&self, mesh: &bozzard_scene::Mesh) -> Option<(&MeshPart, [Vec3; 2])> {
        let part = self.mesh_surface_binding(mesh)?;
        let bozzard_scene::Mesh::Surface { asset, index, .. } = mesh else {
            return None;
        };
        let bounds = self
            .get(self.handle(asset)?)?
            .mesh_part_bounds(*index as usize)?;
        Some((part, bounds))
    }
    /// Validate a source binding without scanning geometry or calculating bounds.
    pub fn mesh_surface_binding(&self, mesh: &bozzard_scene::Mesh) -> Option<&MeshPart> {
        let bozzard_scene::Mesh::Surface {
            asset,
            index,
            source,
        } = mesh
        else {
            return None;
        };
        let AssetData::Mesh(mesh) = self.get(self.handle(asset)?)?.data()? else {
            return None;
        };
        let part = mesh.parts.get(*index as usize)?;
        (part.source_key == *source && part.count != 0).then_some(part)
    }
    pub fn new(root: &Path, sources: &BTreeMap<String, AssetSource>) -> Result<Self> {
        let id = NEXT_STORE
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
            .map_err(|_| anyhow::anyhow!("asset store ID space exhausted"))?;
        let mut handles = BTreeMap::new();
        let entries = sources
            .iter()
            .enumerate()
            .map(|(index, (name, source))| {
                handles.insert(name.clone(), Handle { store: id, index });
                Entry {
                    audio_stamp: None,
                    id: name.clone(),
                    source: source.clone(),
                    state: LoadState::Pending,
                    data: None,
                    mesh_index: None,
                    revision: 0,
                    content_fingerprint: None,
                    observed: None,
                }
            })
            .collect();
        Ok(Self {
            id,
            root: root.to_path_buf(),
            entries,
            handles,
            publication: Arc::new(()),
            canonical_ids: Default::default(),
        })
    }

    /// Stage validated prefab bytes before the editor atomically publishes the file.
    pub fn stage_prefab(&mut self, id: &str, json: &str) -> Result<()> {
        let data = bozzard_scene::Prefab::from_json(json)?;
        let handle = self
            .handle(id)
            .context("prefab asset is not in the catalog")?;
        let entry = &mut self.entries[handle.index];
        ensure!(
            entry.source.kind == AssetKind::Prefab,
            "asset is not a prefab"
        );
        let snapshot = SourceSnapshot {
            primary: Ok(json.as_bytes().to_vec()),
            dependencies: Vec::new(),
        };
        entry.content_fingerprint = Some(fingerprint_snapshot(&snapshot));
        entry.observed = Some(Arc::new(snapshot));
        entry.data = Some(Arc::new(AssetData::Prefab(data)));
        entry.state = LoadState::Ready;
        entry.revision += 1;
        self.publication = Arc::new(());
        self.canonical_ids = Default::default();
        Ok(())
    }

    pub fn handle(&self, id: &str) -> Option<Handle> {
        self.handles.get(id).copied()
    }
    pub fn get(&self, handle: Handle) -> Option<&Entry> {
        if handle.store != self.id {
            return None;
        }
        self.entries.get(handle.index)
    }
    pub fn entries(&self) -> impl Iterator<Item = &Entry> {
        self.entries.iter()
    }
    /// Successful data publications and catalog replacement receive a fresh token.
    /// Clones share a token until they diverge; equal numeric revisions are insufficient.
    pub fn publication_identity(&self) -> &Arc<()> {
        &self.publication
    }

    /// Stable catalog ID of this exact immutable resource. Canonical file aliases
    /// share decoded storage; independently edited aliases retain distinct keys.
    /// Source mesh surface indices and authored source signatures are untouched.
    pub fn canonical_asset_id<'a>(&'a self, id: &'a str) -> &'a str {
        let Some(data) = self
            .handle(id)
            .and_then(|h| self.get(h))
            .and_then(|entry| entry.data())
        else {
            return id;
        };
        let identities = self.canonical_ids.get_or_init(|| {
            let mut identities = std::collections::HashMap::with_capacity(self.entries.len());
            for entry in &self.entries {
                if let Some(data) = entry.data() {
                    identities
                        .entry(data as *const AssetData as usize)
                        .or_insert_with(|| entry.id.clone());
                }
            }
            identities
        });
        identities
            .get(&(data as *const AssetData as usize))
            .map_or(id, String::as_str)
    }

    /// Compose an editor-only catalog from decoded entries. No files are read and
    /// mesh data, texture pixels and picking indexes retain their shared storage.
    /// Names are supplied by the document owner to isolate scene-local asset IDs.
    pub fn shared_catalog<'a>(
        entries: impl IntoIterator<Item = (String, &'a Entry)>,
    ) -> Result<Self> {
        let mut store = Self::new(Path::new("."), &BTreeMap::new())?;
        for (id, entry) in entries {
            ensure!(
                !store.handles.contains_key(&id),
                "duplicate shared asset ID '{id}'"
            );
            let handle = Handle {
                store: store.id,
                index: store.entries.len(),
            };
            let mut shared = entry.clone();
            shared.id = id.clone();
            store.entries.push(shared);
            store.handles.insert(id, handle);
        }
        Ok(store)
    }

    /// Reuse decoded data when catalog paths still resolve to the same file.
    pub fn for_catalog(
        &self,
        root: &Path,
        sources: &BTreeMap<String, AssetSource>,
    ) -> Result<Self> {
        let mut next = Self::new(root, sources)?;
        let mut paths: Option<BTreeMap<PathBuf, Vec<&Entry>>> = None;
        for entry in &mut next.entries {
            let new_path = root.join(&entry.source.path);
            let mut reused = false;
            if let Some(old) = self.handle(&entry.id).and_then(|h| self.get(h)) {
                let old_path = self.root.join(&old.source.path);
                let same_path = old_path == new_path
                    // Relative resources follow the authored location. Canonical
                    // primary aliases can bypass decoding only when there are no
                    // external dependencies and the format selector is unchanged.
                    || old.observed.as_ref().is_some_and(|snapshot| snapshot.dependencies.is_empty())
                        && old_path.extension().and_then(|x| x.to_str()).map(str::to_ascii_lowercase)
                            == new_path.extension().and_then(|x| x.to_str()).map(str::to_ascii_lowercase)
                        && std::fs::canonicalize(&old_path)
                        .ok()
                        .zip(std::fs::canonicalize(&new_path).ok())
                        .is_some_and(|(a, b)| a == b);
                if old.source.kind == entry.source.kind && same_path {
                    let source = entry.source.clone();
                    *entry = old.clone();
                    entry.source = source;
                    reused = true;
                }
            }
            if !reused {
                // Different scenes can name the same packed asset differently. Build
                // this index only when ID-based reuse misses, then share its decoded
                // data and picking index instead of reading/decoding it again.
                let paths = paths.get_or_insert_with(|| {
                    let mut paths: BTreeMap<PathBuf, Vec<&Entry>> = BTreeMap::new();
                    for old in &self.entries {
                        paths
                            .entry(self.root.join(&old.source.path))
                            .or_default()
                            .push(old);
                    }
                    paths
                });
                if let Some(old) = paths.get(&new_path).and_then(|entries| {
                    entries
                        .iter()
                        .find(|old| old.source.kind == entry.source.kind)
                }) {
                    let (id, source) = (entry.id.clone(), entry.source.clone());
                    *entry = (*old).clone();
                    entry.id = id;
                    entry.source = source;
                }
            }
        }
        Ok(next)
    }

    pub fn load_pending(&mut self) -> Result<()> {
        self.load_pending_with(&job::Progress::default())
    }
    pub fn load_pending_with(&mut self, progress: &job::Progress) -> Result<()> {
        let mut pending = Self::new(
            &self.root,
            &self
                .entries
                .iter()
                .filter(|entry| entry.observed.is_none())
                .map(|entry| (entry.id.clone(), entry.source.clone()))
                .collect(),
        )?;
        pending.refresh_with(progress)?;
        pending.require_ready()?;
        for entry in &mut self.entries {
            if let Some(loaded) = pending.handle(&entry.id).and_then(|h| pending.get(h)) {
                *entry = loaded.clone();
                self.publication = Arc::new(());
                self.canonical_ids = Default::default();
            }
        }
        Ok(())
    }

    /// Returns every changed state, including failures. A failed reload keeps data/revision intact.
    /// Call at a bounded interval, not every frame. Imports run on the calling thread for now.
    pub fn refresh(&mut self) -> Vec<Handle> {
        self.refresh_with(&job::Progress::default())
            .expect("uncancelled refresh")
    }

    pub fn refresh_with(&mut self, progress: &job::Progress) -> Result<Vec<Handle>> {
        let mut changed = Vec::new();
        let total = self.entries.len();
        // Canonical local file identities retain their complete path, including
        // literal query/fragment characters. Decode aliases only once, and share
        // only an exact observed snapshot of the same kind and resolved file.
        let mut decoded = BTreeMap::<PathBuf, Vec<Entry>>::new();
        for entry in &self.entries {
            if entry.data.is_some()
                && entry.observed.is_some()
                && matches!(entry.state, LoadState::Ready)
            {
                let path = self.root.join(&entry.source.path);
                let path = std::fs::canonicalize(&path).unwrap_or(path);
                decoded.entry(path).or_default().push(entry.clone());
            }
        }
        for (index, entry) in self.entries.iter_mut().enumerate() {
            progress.stage(format!("Checking {} ({}/{total})", entry.id, index + 1))?;
            let path = self.root.join(&entry.source.path);
            // Relative buffers/images are resolved against the authored document
            // location. A symlink in another directory can have different URI
            // dependencies, even though its primary file canonicalizes alike.
            let canonical_path = std::fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
            let audio_stamp = (entry.source.kind == AssetKind::Audio)
                .then(|| audio::stamp(&path).ok())
                .flatten();
            if audio_stamp.is_some() && audio_stamp == entry.audio_stamp && entry.observed.is_some()
            {
                continue;
            }
            let snapshot = if entry.source.kind == AssetKind::Audio {
                audio::probe(&path, progress).and_then(|metadata| {
                    Ok(SourceSnapshot {
                        primary: Ok(serde_json::to_vec(&metadata)?),
                        dependencies: Vec::new(),
                    })
                })
            } else if entry.source.kind == AssetKind::Material {
                materials::snapshot(&path, progress)
            } else {
                source_snapshot(&path)
            }
            .map_err(|error| format!("{error:#}"));
            entry.audio_stamp = audio_stamp;
            let snapshot = match snapshot {
                Ok(snapshot) => snapshot,
                Err(error) => SourceSnapshot {
                    primary: Err(error),
                    dependencies: Vec::new(),
                },
            };
            if entry.observed.as_deref() == Some(&snapshot) {
                continue;
            }
            progress.stage(format!("Decoding {} ({}/{total})", entry.id, index + 1))?;
            let alias = decoded.get(&canonical_path).and_then(|entries| {
                entries.iter().find(|candidate| {
                    candidate.source.kind == entry.source.kind
                        && candidate.observed.as_deref() == Some(&snapshot)
                })
            });
            let loaded = match &snapshot.primary {
                Ok(_) if alias.is_some() => Ok(None),
                Ok(bytes) => import(entry.source.kind, &path, bytes, &snapshot).map(Some),
                Err(error) => Err(anyhow::anyhow!(error.clone())),
            };
            let loaded = loaded.and_then(|data| {
                let Some(data) = data else {
                    let alias = alias.expect("exact decoded alias");
                    return Ok((
                        alias.data.clone().expect("decoded data"),
                        alias.mesh_index.clone(),
                    ));
                };
                let mesh_index = if let AssetData::Mesh(mesh) = &data {
                    progress.stage(format!("Indexing {} ({}/{total})", entry.id, index + 1))?;
                    Some(Arc::new(picking::MeshIndex::build(mesh, progress)?))
                } else {
                    None
                };
                Ok((Arc::new(data), mesh_index))
            });
            progress.check()?;
            let fingerprint = fingerprint_snapshot(&snapshot);
            entry.observed = Some(Arc::new(snapshot));
            match loaded {
                Ok((data, mesh_index)) => {
                    entry.content_fingerprint = Some(fingerprint);
                    entry.data = Some(data);
                    entry.mesh_index = mesh_index;
                    entry.revision += 1;
                    entry.state = LoadState::Ready;
                    self.publication = Arc::new(());
                    self.canonical_ids = Default::default();
                    decoded
                        .entry(canonical_path)
                        .or_default()
                        .push(entry.clone());
                }
                Err(error) => {
                    entry.state = LoadState::Failed(format!("asset '{}': {error:#}", entry.id))
                }
            }
            changed.push(Handle {
                store: self.id,
                index,
            });
        }
        Ok(changed)
    }

    /// The worker owns a cheap snapshot; callers publish only when their catalog still matches.
    pub fn refresh_job(&self) -> Result<job::Job<(Self, Vec<Handle>)>> {
        self.refresh_job_forced(false)
    }
    /// Explicit reload also rechecks audio files whose size/timestamp was preserved externally.
    pub fn refresh_job_forced(&self, force: bool) -> Result<job::Job<(Self, Vec<Handle>)>> {
        let mut store = self.clone();
        if force {
            for entry in &mut store.entries {
                entry.audio_stamp = None;
            }
        }
        job::Job::start("Checking assets", move |progress| {
            let changed = store.refresh_with(&progress)?;
            Ok((store, changed))
        })
    }

    pub fn require_ready(&self) -> Result<()> {
        for entry in &self.entries {
            match &entry.state {
                LoadState::Ready => {}
                LoadState::Pending => bail!("asset '{}' has not loaded", entry.id),
                LoadState::Failed(message) => bail!("{message}"),
            }
        }
        Ok(())
    }
}
