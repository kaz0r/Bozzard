//! Immutable terrain revisions; mesh preparation is off-thread and publication is one Undo step.
use super::*;
use bozzard_assets::{
    job::{Job, Progress},
    terrain::Terrain,
};
use std::io::{Read, Write};

#[derive(Clone)]
pub struct TerrainSource {
    pub terrain: Terrain,
    object: String,
    asset: String,
    path: PathBuf,
    bytes: Vec<u8>,
}

pub enum TerrainRequest {
    Create {
        terrain: Terrain,
        position: [f32; 3],
    },
    Sculpt {
        source: TerrainSource,
        terrain: Terrain,
    },
}

pub struct PreparedGeometry {
    path: PathBuf,
    revision: u64,
    asset_revision: u64,
    progress: Progress,
    expected: Option<(PathBuf, Vec<u8>)>,
    scene: Scene,
    assets: Option<AssetStore>,
    created: Option<PathBuf>,
    object: String,
    label: &'static str,
}
impl Drop for PreparedGeometry {
    fn drop(&mut self) {
        if let Some(directory) = &self.created {
            let _ = std::fs::remove_dir_all(directory);
        }
    }
}

impl TerrainSource {
    pub fn object(&self) -> &str {
        &self.object
    }
    pub fn is_current(&self, editor: &Editor) -> bool {
        editor.scene.assets.get(&self.asset).is_some_and(|entry| {
            let source = Path::new(&entry.path);
            if source.is_absolute() {
                source == self.path
            } else {
                self.path
                    .strip_prefix(root(&editor.path))
                    .is_ok_and(|relative| relative == source)
            }
        })
    }
}

impl PreparedGeometry {
    fn write_revision(
        &mut self,
        asset: &str,
        filename: &str,
        bytes: &[u8],
        assets: AssetStore,
    ) -> Result<()> {
        self.progress.stage("Writing geometry revision")?;
        let asset_root = root(&self.path).join("assets");
        std::fs::create_dir_all(&asset_root)?;
        // Do not rescan every prior stroke's directory on each new stroke.
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let directory = loop {
            self.progress.check()?;
            let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            ensure!(n != 0, "Geometry revision counter exhausted");
            let directory = asset_root.join(format!("level-revision-{n}"));
            match std::fs::create_dir(&directory) {
                Ok(()) => break directory,
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e.into()),
            }
        };
        self.created = Some(directory.clone());
        let destination = directory.join(filename);
        let mut file = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&destination)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        let relative = destination
            .strip_prefix(root(&self.path))?
            .to_str()
            .context("Geometry path must be UTF-8")?
            .replace('\\', "/");
        self.scene.assets.insert(
            asset.into(),
            AssetSource {
                kind: AssetKind::Mesh,
                path: relative,
            },
        );
        let mut assets = assets.for_catalog(root(&self.path), &self.scene.assets)?;
        assets.load_pending_with(&self.progress)?;
        assets.require_ready()?;
        self.assets = Some(assets);
        Ok(())
    }

    fn rebuild_colliders(
        &mut self,
        asset: &str,
        create_for: &BTreeSet<String>,
        update_existing: bool,
    ) -> Result<()> {
        self.progress.stage("Building geometry collision")?;
        let assets = self.assets.as_ref().unwrap();
        let entry = assets
            .get(assets.handle(asset).context("Missing prepared geometry")?)
            .unwrap();
        let Some(AssetData::Mesh(mesh)) = entry.data() else {
            anyhow::bail!("Prepared geometry is not a mesh");
        };
        let triangles = mesh
            .indices
            .chunks_exact(3)
            .map(|t| {
                std::array::from_fn(|i| {
                    let v = mesh.vertices[t[i] as usize];
                    [v[0], v[1], v[2]]
                })
            })
            .collect();
        let collision = bozzard_scene::TriangleMesh::new(triangles)?;
        for object in &mut self.scene.objects {
            if !update_existing && !create_for.contains(&object.id) {
                continue;
            }
            if object
                .drawable
                .as_ref()
                .is_some_and(|d| matches!(&d.mesh, Mesh::Asset(id) if id == asset))
            {
                if let Some(collider) = &mut object.mesh_collider {
                    collider.mesh = collision.clone();
                } else if create_for.contains(&object.id) {
                    object.mesh_collider = Some(bozzard_scene::MeshCollider {
                        enabled: true,
                        layers: 1,
                        mask: u32::MAX,
                        mesh: collision.clone(),
                    });
                }
            }
        }
        Ok(())
    }

    fn validate(&self) -> Result<()> {
        self.scene.validate()?;
        self.assets
            .as_ref()
            .unwrap()
            .validate_scene_resources(&self.scene)?;
        self.progress.check()
    }
}

fn read_source(path: &Path) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(4 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() <= 4 * 1024 * 1024,
        "terrain source exceeds 4 MiB"
    );
    Ok(bytes)
}

impl Editor {
    pub fn accept_terrain(&mut self, prepared: PreparedGeometry) -> Result<String> {
        self.accept_geometry(prepared)
    }

    /// Plant up to 256 brush instances as one scene transaction. Equal brush recipes
    /// reuse a catalog entry and decoded geometry, while transforms remain independent.
    pub fn blockout_job(
        &self,
        brush: bozzard_assets::blockout::Blockout,
        transforms: Vec<Transform>,
    ) -> Result<Job<PreparedGeometry>> {
        ensure!(
            self.play.is_none(),
            "Stop Play before placing blockout brushes"
        );
        brush.validate()?;
        ensure!(
            (1..=256).contains(&transforms.len()),
            "Place 1..256 brushes per stroke"
        );
        let mut prepared = PreparedGeometry {
            path: self.path.clone(),
            revision: self.revision,
            asset_revision: self.asset_revision,
            progress: Default::default(),
            expected: None,
            scene: self.scene.clone(),
            assets: Some(self.assets.clone()),
            created: None,
            object: String::new(),
            label: "Place blockout",
        };
        Job::start("Preparing blockout", move |progress| {
            prepared.progress = progress.clone();
            let bytes = brush.to_json()?;
            let base = format!("blockout-{}", brush.name());
            let (asset, reused) = (0_u64..)
                .find_map(|index| {
                    let id = if index == 0 {
                        base.clone()
                    } else {
                        format!("{base}-{index}")
                    };
                    match prepared.scene.assets.get(&id) {
                        None => Some(Ok((id, false))),
                        Some(source)
                            if source.kind == AssetKind::Mesh
                                && source.path.ends_with(".brush.json") =>
                        {
                            let path = root(&prepared.path).join(&source.path);
                            match read_source(&path) {
                                Ok(existing)
                                    if existing == bytes
                                        && prepared
                                            .assets
                                            .as_ref()
                                            .and_then(|a| a.handle(&id).and_then(|h| a.get(h)))
                                            .is_some_and(|entry| {
                                                entry.matches_standalone_source(&bytes)
                                            }) =>
                                {
                                    prepared.expected = Some((path, existing));
                                    Some(Ok((id, true)))
                                }
                                Ok(_) => None,
                                Err(error) => Some(Err(error)),
                            }
                        }
                        _ => None,
                    }
                })
                .unwrap()?;
            if !reused {
                let assets = prepared.assets.take().unwrap();
                prepared.write_revision(&asset, "primitive.brush.json", &bytes, assets)?;
            }
            let group =
                (transforms.len() > 1).then(|| unique_id(&prepared.scene, "blockout-stroke"));
            if let Some(group) = &group {
                prepared.scene.objects.push(Object {
                    id: group.clone(),
                    name: "Blockout stroke".into(),
                    ..Default::default()
                });
            }
            let mut occupied: BTreeSet<_> = prepared
                .scene
                .objects
                .iter()
                .map(|o| o.id.clone())
                .collect();
            let mut next_id = 1_u64;
            let mut created = BTreeSet::new();
            for transform in transforms {
                progress.check()?;
                let id = loop {
                    let id = format!("blockout-{next_id}");
                    next_id += 1;
                    if occupied.insert(id.clone()) {
                        break id;
                    }
                };
                if prepared.object.is_empty() {
                    prepared.object = group.clone().unwrap_or_else(|| id.clone());
                }
                created.insert(id.clone());
                prepared.scene.objects.push(Object {
                    id,
                    name: format!("Blockout {}", brush.name()),
                    parent: group.clone(),
                    transform,
                    drawable: Some(Drawable {
                        layer: Layer::ThreeD,
                        mesh: Mesh::Asset(asset.clone()),
                        texture: Texture::ProceduralChecker,
                        color: [0.65, 0.72, 0.8],
                        uv_scale: [4.; 2],
                        metallic: None,
                        roughness: Some(0.8),
                        gi_static: true,
                        material_overrides: Vec::new(),
                    }),
                    ..Default::default()
                });
            }
            prepared.rebuild_colliders(&asset, &created, false)?;
            prepared.validate()?;
            Ok(prepared)
        })
    }

    /// World matrix for a tool target, including authored hierarchy transforms.
    pub fn object_matrix(&self, object: &str) -> Result<Mat4> {
        let demo = self.edit_demo()?;
        demo.instance()
            .global_transforms(&demo.app.world)?
            .get(object)
            .copied()
            .context("Tool target no longer exists")
    }
    /// Read only when opening/reloading the terrain authoring draft, never per frame.
    pub fn terrain_source(&self, object: &str) -> Result<TerrainSource> {
        let target = self
            .scene
            .objects
            .iter()
            .find(|o| o.id == object)
            .context("Select a terrain object")?;
        let Mesh::Asset(asset) = &target
            .drawable
            .as_ref()
            .context("Terrain needs a mesh")?
            .mesh
        else {
            anyhow::bail!("Select a terrain mesh asset");
        };
        let entry = self
            .scene
            .assets
            .get(asset)
            .context("Terrain asset is missing")?;
        ensure!(
            entry.path.ends_with(".terrain.json"),
            "Selected mesh is not an editable terrain source"
        );
        let path = root(&self.path).join(&entry.path);
        let bytes = read_source(&path)?;
        Ok(TerrainSource {
            terrain: Terrain::from_json(&bytes)?,
            object: object.into(),
            asset: asset.clone(),
            path,
            bytes,
        })
    }

    pub fn terrain_job(&self, request: TerrainRequest) -> Result<Job<PreparedGeometry>> {
        ensure!(self.play.is_none(), "Stop Play before editing terrain");
        let path = self.path.clone();
        let scene = self.scene.clone();
        let assets = self.assets.clone();
        let revision = self.revision;
        let asset_revision = self.asset_revision;
        Job::start("Preparing terrain", move |progress| {
            let mut scene = scene;
            let (terrain, object, asset, expected, creating) = match request {
                TerrainRequest::Create { terrain, position } => {
                    ensure!(
                        position.iter().all(|v| v.is_finite()),
                        "Invalid terrain position"
                    );
                    let object = unique_id(&scene, "terrain");
                    let asset = (1_u64..)
                        .map(|n| format!("terrain-{n}"))
                        .find(|id| !scene.assets.contains_key(id))
                        .unwrap();
                    scene.objects.push(Object {
                        id: object.clone(),
                        name: "Terrain".into(),
                        transform: Transform {
                            translation: position,
                            ..Default::default()
                        },
                        drawable: Some(Drawable {
                            layer: Layer::ThreeD,
                            mesh: Mesh::Asset(asset.clone()),
                            texture: Texture::ProceduralChecker,
                            color: [0.35, 0.5, 0.25],
                            uv_scale: [16.; 2],
                            metallic: None,
                            roughness: Some(0.9),
                            gi_static: true,
                            material_overrides: Vec::new(),
                        }),
                        ..Default::default()
                    });
                    (terrain, object, asset, None, true)
                }
                TerrainRequest::Sculpt { source, terrain } => {
                    ensure!(
                        scene.objects.iter().any(|o| o.id == source.object
                            && o.drawable.as_ref().is_some_and(
                                |d| matches!(&d.mesh, Mesh::Asset(id) if id == &source.asset)
                            )),
                        "Terrain selection changed; reopen its draft"
                    );
                    let current = scene
                        .assets
                        .get(&source.asset)
                        .context("Terrain asset was removed")?;
                    ensure!(
                        root(&path).join(&current.path) == source.path,
                        "Terrain revision changed; reopen its draft"
                    );
                    ensure!(
                        read_source(&source.path)? == source.bytes,
                        "Terrain source changed externally; reload before sculpting"
                    );
                    (
                        terrain,
                        source.object,
                        source.asset,
                        Some((source.path, source.bytes)),
                        false,
                    )
                }
            };
            terrain.validate()?;
            let mut prepared = PreparedGeometry {
                path,
                revision,
                asset_revision,
                progress: progress.clone(),
                expected,
                scene,
                assets: None,
                created: None,
                object,
                label: "Edit terrain",
            };
            prepared.write_revision(
                &asset,
                "heightfield.terrain.json",
                &terrain.to_json()?,
                assets,
            )?;
            let create_for = if creating {
                [prepared.object.clone()].into()
            } else {
                BTreeSet::new()
            };
            prepared.rebuild_colliders(&asset, &create_for, true)?;
            prepared.validate()?;
            progress.check()?;
            Ok(prepared)
        })
    }

    pub fn accept_geometry(&mut self, mut prepared: PreparedGeometry) -> Result<String> {
        prepared.progress.check()?;
        ensure!(
            self.play.is_none()
                && self.path == prepared.path
                && self.revision == prepared.revision
                && self.asset_revision == prepared.asset_revision,
            "Scene or assets changed while preparing geometry; retry"
        );
        if let Some((path, bytes)) = &prepared.expected {
            ensure!(
                read_source(path)? == *bytes,
                "Terrain source changed while preparing; reload before retrying"
            );
        }
        self.validate_document(&prepared.scene)?;
        self.finish_gesture();
        let assets_changed = self.scene.assets != prepared.scene.assets;
        self.record(Change {
            label: prepared.label.into(),
            scene: self.scene.clone(),
            assets: assets_changed.then(|| self.assets.clone()),
            restore_file: None,
        });
        std::mem::swap(&mut self.scene, &mut prepared.scene);
        self.assets = prepared.assets.take().unwrap();
        self.revision += 1;
        if assets_changed {
            self.asset_revision += 1;
        }
        prepared.created = None;
        self.select_object(Some(prepared.object.clone()));
        Ok(prepared.object.clone())
    }
}
