//! Material inheritance resolves from one immutable dependency snapshot. Parent
//! and image edits participate in ordinary asset reload and last-good recovery.
use super::*;
use bozzard_scene::material_asset::{
    MaterialAsset, MaterialShader, MaterialTexture, MaterialValues,
};

// Weak entries do not retain unused maps. Shared inheritance and property-only
// reloads reuse immutable pixels across asset IDs and worker threads.
fn shared_image(bytes: &[u8]) -> Result<Arc<ImageData>> {
    use sha2::{Digest, Sha256};
    use std::sync::{Mutex, OnceLock, Weak};
    type Cache = BTreeMap<[u8; 32], Weak<ImageData>>;
    static IMAGES: OnceLock<Mutex<Cache>> = OnceLock::new();
    let key: [u8; 32] = Sha256::digest(bytes).into();
    let cache = IMAGES.get_or_init(Default::default);
    if let Some(image) = cache.lock().unwrap().get(&key).and_then(Weak::upgrade) {
        return Ok(image);
    }
    let image = Arc::new(if bytes.starts_with(b"BOZZTEX") {
        texture::decode(bytes)?
    } else {
        decoded_image(bytes, "material texture")?
    });
    let mut cache = cache.lock().unwrap();
    // A concurrent decode may already have published the canonical allocation.
    if let Some(existing) = cache.get(&key).and_then(Weak::upgrade) {
        return Ok(existing);
    }
    cache.retain(|_, image| image.strong_count() > 0);
    if cache.len() == 1024 {
        cache.pop_first();
    }
    cache.insert(key, Arc::downgrade(&image));
    Ok(image)
}

#[derive(Clone, Debug)]
pub struct MaterialData {
    pub definition: MaterialAsset,
    pub values: MaterialValues,
    pub texture: MaterialTexture,
    pub image: Option<Arc<ImageData>>,
    pub shader: Option<Arc<bozzard_scene::shader_graph::ShaderGraph>>,
    pub keywords: BTreeMap<String, bool>,
}

impl MaterialData {
    /// CPU and GPU consumers use the same resolved material properties.
    pub fn apply(
        &self,
        binding: &bozzard_scene::material_asset::MaterialInstance,
        drawable: &mut bozzard_scene::Drawable,
    ) {
        use bozzard_scene::Texture;
        let mut values = self.values.clone();
        binding.properties.apply(&mut values);
        let inherited = match &self.texture {
            MaterialTexture::Source => None,
            MaterialTexture::White => Some(Texture::White),
            MaterialTexture::Checker => Some(Texture::Checker),
            MaterialTexture::Normals => Some(Texture::Normals),
            MaterialTexture::ProceduralChecker => Some(Texture::ProceduralChecker),
            MaterialTexture::Toon => Some(Texture::Toon),
            MaterialTexture::Image(_) => Some(Texture::Asset(binding.asset.clone())),
        };
        bozzard_scene::Material {
            shared: None,
            color: values.color,
            uv_scale: values.uv_scale,
            metallic: values.metallic,
            roughness: values.roughness,
            texture: binding.texture.clone().or(inherited),
        }
        .apply(drawable);
    }
    pub fn shader_variant<'a>(
        &'a self,
        binding: &bozzard_scene::material_asset::MaterialInstance,
        local: Option<&'a bozzard_scene::shader_graph::ShaderGraph>,
    ) -> Result<Option<(&'a bozzard_scene::shader_graph::ShaderGraph, u8)>> {
        let inherited = local.is_none().then_some(&self.keywords);
        if let Some(graph) = local.or(self.shader.as_deref()) {
            let mask =
                graph.layered_keyword_mask(inherited.into_iter().chain([&binding.keywords]))?;
            Ok(Some((graph, mask)))
        } else {
            ensure!(
                binding.keywords.is_empty() && inherited.is_none_or(BTreeMap::is_empty),
                "stock material cannot set shader keywords"
            );
            Ok(None)
        }
    }
}

impl AssetStore {
    pub fn material(&self, id: &str) -> Result<&MaterialData> {
        match self
            .handle(id)
            .and_then(|h| self.get(h))
            .and_then(|e| e.data())
        {
            Some(AssetData::Material(material)) => Ok(material),
            _ => anyhow::bail!("material '{id}' is not loaded"),
        }
    }
    /// Validate data-dependent resources before playback, export and publication.
    pub fn validate_scene_resources(&self, scene: &bozzard_scene::Scene) -> Result<()> {
        self.validate_text_fonts(scene)?;
        for object in &scene.objects {
            if let Some(binding) = object.material.as_ref().and_then(|m| m.shared.as_deref()) {
                self.material(&binding.asset)?
                    .shader_variant(binding, object.shader_graph.as_ref())
                    .with_context(|| format!("material on '{}'", object.id))?;
            }
        }
        Ok(())
    }
}

fn normalized(path: &Path) -> Result<PathBuf> {
    let mut result = PathBuf::new();
    for component in std::path::absolute(path)?.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                result.pop();
            }
            part => result.push(part.as_os_str()),
        }
    }
    Ok(result)
}
fn dependency(source: &Path, relative: &str) -> Result<PathBuf> {
    bozzard_scene::material_asset::validate_relative_path(relative)?;
    normalized(
        &source
            .parent()
            .context("material source directory missing")?
            .join(relative),
    )
}
fn definition(bytes: &[u8]) -> Result<MaterialAsset> {
    MaterialAsset::from_json(std::str::from_utf8(bytes).context("material requires UTF-8 JSON")?)
}

pub(super) fn snapshot(path: &Path, progress: &job::Progress) -> Result<SourceSnapshot> {
    snapshot_replacing(path, progress, None)
}
fn snapshot_replacing(
    path: &Path,
    progress: &job::Progress,
    replacement: Option<(&Path, &[u8])>,
) -> Result<SourceSnapshot> {
    let root = normalized(path)?;
    let mut current = root.clone();
    let mut files = BTreeMap::<PathBuf, Vec<u8>>::new();
    let mut canonical = BTreeSet::new();
    let mut images = BTreeSet::new();
    for depth in 0..=32 {
        progress.check()?;
        ensure!(depth < 32, "material inheritance exceeds 32 sources");
        ensure!(
            canonical.insert(current.canonicalize()?),
            "material inheritance cycle"
        );
        let bytes = if let Some((path, bytes)) = replacement
            && current.canonicalize()? == path
        {
            bytes.to_vec()
        } else {
            read_source(&current)?
        };
        let material = definition(&bytes)?;
        if let Some(MaterialTexture::Image(path)) = &material.texture {
            images.insert(dependency(&current, path)?);
        }
        let parent = material
            .parent
            .as_ref()
            .map(|path| dependency(&current, path))
            .transpose()?;
        files.insert(current, bytes);
        let Some(parent) = parent else {
            break;
        };
        current = parent;
    }
    let mut total: usize = files.values().map(Vec::len).sum();
    for path in images {
        progress.check()?;
        // An image is not a material definition, even if paths alias or repeat.
        ensure!(
            !files.contains_key(&path),
            "material dependency has conflicting file types"
        );
        let bytes = read_source(&path)?;
        total = total
            .checked_add(bytes.len())
            .context("material dependency size overflow")?;
        ensure!(
            total <= 128 * 1024 * 1024,
            "material dependencies exceed 128 MiB"
        );
        files.insert(path, bytes);
    }
    Ok(SourceSnapshot {
        primary: Ok(files.remove(&root).context("material root missing")?),
        dependencies: files
            .into_iter()
            .map(|(path, bytes)| (path, Ok(bytes)))
            .collect(),
    })
}

/// A candidate catalog prepared without changing any source file. Publication
/// checks every input again so external edits cannot be silently overwritten.
pub struct MaterialEdit {
    pub store: AssetStore,
    path: PathBuf,
    previous: Vec<u8>,
    json: String,
    reads: BTreeMap<PathBuf, Vec<u8>>,
}
impl AssetStore {
    pub fn prepare_material_edit(
        &self,
        id: &str,
        expected: &str,
        json: &str,
        progress: &job::Progress,
    ) -> Result<MaterialEdit> {
        MaterialAsset::from_json(json)?;
        let entry = self
            .handle(id)
            .and_then(|h| self.get(h))
            .context("material no longer exists")?;
        ensure!(
            entry.source.kind == AssetKind::Material,
            "asset is not a material"
        );
        let path = self.root.join(&entry.source.path).canonicalize()?;
        let previous = read_source(&path)?;
        ensure!(
            previous == expected.as_bytes(),
            "Material changed on disk; reload the draft before saving"
        );
        let mut reads = BTreeMap::new();
        let mut read_bytes = 0usize;
        let mut store = self.clone();
        for entry in &mut store.entries {
            if entry.source.kind != AssetKind::Material {
                continue;
            }
            let source = self.root.join(&entry.source.path);
            // Only this source and already-observed descendants depend on this
            // edit. Other decoded materials retain their image/graph allocations.
            if matches!(entry.state, LoadState::Ready)
                && source.canonicalize()? != path
                && !entry
                    .source_dependencies()
                    .any(|p| p.canonicalize().ok().as_ref() == Some(&path))
            {
                continue;
            }
            let snapshot = snapshot_replacing(&source, progress, Some((&path, json.as_bytes())))?;
            let bytes = snapshot
                .primary
                .as_ref()
                .map_err(|e| anyhow::anyhow!(e.clone()))?;
            for (name, bytes) in std::iter::once((normalized(&source)?, bytes.as_slice())).chain(
                snapshot.dependencies.iter().filter_map(|(name, bytes)| {
                    bytes.as_ref().ok().map(|b| (name.clone(), b.as_slice()))
                }),
            ) {
                if name.canonicalize()? != path {
                    if let Some(prior) = reads.get(&name) {
                        ensure!(
                            prior == bytes,
                            "Material dependency changed during preparation; retry"
                        );
                    } else {
                        read_bytes += bytes.len();
                        ensure!(
                            read_bytes <= 128 * 1024 * 1024,
                            "material edit dependencies exceed 128 MiB"
                        );
                        reads.insert(name, bytes.to_vec());
                    }
                }
            }
            if entry.observed.as_deref() != Some(&snapshot) {
                let data = decode(&source, bytes, &snapshot)?;
                entry.content_fingerprint = Some(fingerprint_snapshot(&snapshot));
                entry.observed = Some(Arc::new(snapshot));
                entry.data = Some(Arc::new(AssetData::Material(Box::new(data))));
                entry.state = LoadState::Ready;
                entry.revision += 1;
            }
        }
        Ok(MaterialEdit {
            store,
            path,
            previous,
            json: json.to_owned(),
            reads,
        })
    }
}
impl MaterialEdit {
    pub fn publish(self) -> Result<AssetStore> {
        for (path, bytes) in &self.reads {
            ensure!(
                read_source(path)? == *bytes,
                "Material dependency changed; retry: {}",
                path.display()
            );
        }
        ensure!(
            read_source(&self.path)? == self.previous,
            "Material source changed; reload before saving"
        );
        use std::io::Write;
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let temporary = self.path.with_extension(format!(
            "{}-{}.tmp",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        let result = (|| -> Result<()> {
            file.write_all(self.json.as_bytes())?;
            file.sync_all()?;
            drop(file);
            ensure!(
                read_source(&self.path)? == self.previous,
                "Material source changed; reload before saving"
            );
            std::fs::rename(&temporary, &self.path)?;
            Ok(())
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(temporary);
        }
        result?;
        Ok(self.store)
    }
}

pub(super) fn decode(path: &Path, bytes: &[u8], snapshot: &SourceSnapshot) -> Result<MaterialData> {
    let root = normalized(path)?;
    let lookup = |path: &Path| -> Result<&[u8]> {
        if path == root {
            return Ok(bytes);
        }
        snapshot
            .dependencies
            .iter()
            .find(|(key, _)| key == path)
            .context("material dependency missing from snapshot")?
            .1
            .as_deref()
            .map_err(|error| anyhow::anyhow!(error.clone()))
    };
    let mut chain = Vec::new();
    let mut seen = BTreeSet::new();
    let mut current = root.clone();
    for depth in 0..=32 {
        ensure!(
            depth < 32 && seen.insert(current.clone()),
            "invalid material inheritance chain"
        );
        let material = definition(lookup(&current)?)?;
        let parent = material
            .parent
            .as_ref()
            .map(|path| dependency(&current, path))
            .transpose()?;
        chain.push((current, material));
        let Some(parent) = parent else {
            break;
        };
        current = parent;
    }
    let mut result = MaterialData {
        definition: chain[0].1.clone(),
        values: Default::default(),
        texture: MaterialTexture::Source,
        image: None,
        shader: None,
        keywords: BTreeMap::new(),
    };
    let mut image_path = None;
    for (path, material) in chain.into_iter().rev() {
        material.properties.apply(&mut result.values);
        if let Some(texture) = material.texture {
            image_path = match &texture {
                MaterialTexture::Image(relative) => Some(dependency(&path, relative)?),
                _ => None,
            };
            result.texture = texture;
        }
        match material.shader {
            MaterialShader::Inherit => {}
            MaterialShader::Stock => {
                result.shader = None;
                result.keywords.clear();
            }
            MaterialShader::Graph(graph) => {
                result.shader = Some(Arc::new(graph));
                result.keywords.clear();
            }
        }
        result.keywords.extend(material.keywords);
    }
    if let Some(graph) = &result.shader {
        graph.keyword_mask(&result.keywords)?;
    } else {
        ensure!(
            result.keywords.is_empty(),
            "stock material cannot set shader keywords"
        );
    }
    if let Some(path) = image_path {
        let bytes = lookup(&path)?;
        result.image = Some(shared_image(bytes)?);
    }
    Ok(result)
}

pub fn load_material(path: &Path, progress: &job::Progress) -> Result<MaterialData> {
    let snapshot = snapshot(path, progress)?;
    progress.check()?;
    decode(
        path,
        snapshot
            .primary
            .as_ref()
            .map_err(|e| anyhow::anyhow!(e.clone()))?,
        &snapshot,
    )
}

pub struct MaterialPackage {
    pub source: SourcePackage,
    pub content_fingerprint: u64,
}
pub fn package_material(path: &Path, progress: &job::Progress) -> Result<MaterialPackage> {
    package_with(path, progress, |name, bytes| Ok((name, bytes)))
}
/// Rebase the entire parent chain into one portable folder. Images can be cooked
/// from these exact snapshot bytes; dependencies are never reread during export.
pub fn package_with(
    path: &Path,
    progress: &job::Progress,
    mut image: impl FnMut(String, Vec<u8>) -> Result<(String, Vec<u8>)>,
) -> Result<MaterialPackage> {
    let snapshot = snapshot(path, progress)?;
    let fingerprint = fingerprint_snapshot(&snapshot);
    let root = normalized(path)?;
    let bytes = snapshot
        .primary
        .as_ref()
        .map_err(|e| anyhow::anyhow!(e.clone()))?;
    decode(&root, bytes, &snapshot)?;
    let mut inputs = BTreeMap::from([(root.clone(), bytes.clone())]);
    for (path, bytes) in snapshot.dependencies {
        inputs.insert(path, bytes.map_err(anyhow::Error::msg)?);
    }
    let mut current = root;
    let mut definitions = Vec::new();
    let mut names = BTreeMap::new();
    loop {
        progress.check()?;
        let material = definition(&inputs[&current])?;
        let name = if definitions.is_empty() {
            "material.material.json".into()
        } else {
            format!("parent-{}.material.json", definitions.len())
        };
        names.insert(current.clone(), name);
        let parent = material
            .parent
            .as_ref()
            .map(|p| dependency(&current, p))
            .transpose()?;
        definitions.push((current, material));
        let Some(parent) = parent else {
            break;
        };
        current = parent;
    }
    let mut files = BTreeMap::new();
    let mut images = BTreeMap::<PathBuf, String>::new();
    for (path, mut material) in definitions {
        if let Some(parent) = &mut material.parent {
            *parent = names[&dependency(&path, parent)?].clone();
        }
        if let Some(MaterialTexture::Image(relative)) = &mut material.texture {
            let source = dependency(&path, relative)?;
            if !images.contains_key(&source) {
                let extension = source.extension().and_then(|s| s.to_str()).unwrap_or("png");
                let name = format!("image-{}.{}", images.len(), extension);
                let (name, bytes) = image(
                    name,
                    inputs
                        .remove(&source)
                        .context("material image snapshot missing")?,
                )?;
                ensure!(
                    !name.is_empty()
                        && Path::new(&name).components().count() == 1
                        && !name.contains(['/', '\\', ':'])
                        && !name.ends_with(".material.json"),
                    "material image package requires a flat, unique filename"
                );
                ensure!(
                    !files.contains_key(&name),
                    "duplicate packaged material image"
                );
                files.insert(name.clone(), bytes);
                images.insert(source.clone(), name);
            }
            *relative = images[&source].clone();
        }
        files.insert(names[&path].clone(), material.to_json()?.into_bytes());
    }
    Ok(MaterialPackage {
        source: SourcePackage {
            primary: "material.material.json".into(),
            files,
        },
        content_fingerprint: fingerprint,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use bozzard_scene::material_asset::PropertyOverrides;
    #[test]
    fn inherited_maps_and_keywords_resolve_from_immutable_sources() -> Result<()> {
        let root = std::env::temp_dir().join(format!("bozzard-material-{}", std::process::id()));
        std::fs::create_dir(&root)?;
        std::fs::create_dir(root.join("variants"))?;
        std::fs::write(
            root.join("map.png"),
            include_bytes!("../../../examples/demo/scenes/assets/palette.png"),
        )?;
        let mut graph = bozzard_scene::shader_graph::ShaderGraph::default();
        graph.keywords.insert("DETAIL".into(), false);
        let base = MaterialAsset {
            properties: PropertyOverrides {
                color: Some([0.2, 0.4, 0.6]),
                metallic: Some(0.7),
                ..Default::default()
            },
            texture: Some(MaterialTexture::Image("map.png".into())),
            shader: MaterialShader::Graph(graph),
            ..Default::default()
        };
        std::fs::write(root.join("base.material.json"), base.to_json()?)?;
        let mut variant = MaterialAsset {
            parent: Some("../base.material.json".into()),
            properties: PropertyOverrides {
                roughness: Some(0.3),
                ..Default::default()
            },
            keywords: BTreeMap::from([("DETAIL".into(), true)]),
            ..Default::default()
        };
        let path = root.join("variants/blue.material.json");
        std::fs::write(&path, variant.to_json()?)?;
        let snapshot = snapshot(&path, &Default::default())?;
        let loaded = decode(&path, snapshot.primary.as_ref().unwrap(), &snapshot)?;
        assert_eq!(loaded.values.color, [0.2, 0.4, 0.6]);
        assert_eq!(
            (loaded.values.metallic, loaded.values.roughness),
            (Some(0.7), Some(0.3))
        );
        assert!(loaded.image.is_some() && loaded.keywords["DETAIL"]);
        variant.parent = Some("blue.material.json".into());
        std::fs::write(&path, variant.to_json()?)?;
        assert!(load_material(&path, &Default::default()).is_err());
        std::fs::remove_dir_all(&root)?;
        let retained = decode(&path, snapshot.primary.as_ref().unwrap(), &snapshot)?;
        assert_eq!(retained.image.unwrap().rgba, loaded.image.unwrap().rgba);
        Ok(())
    }
}
