//! Background LOD generation and one atomic, undoable document transaction.
use super::*;
use bozzard_assets::{SimplifySettings, job::Job};
use bozzard_scene::{Lod, LodLevel};
use std::io::Write;

#[derive(Clone, Copy, Debug)]
pub struct LodRequest {
    pub switch: f32,
    pub settings: SimplifySettings,
}
#[derive(Clone, Debug)]
pub struct GeneratedLod {
    pub asset: String,
    pub source_triangles: usize,
    pub triangles: usize,
    pub error: f32,
}
pub struct PreparedLods {
    path: PathBuf,
    revision: u64,
    asset_revision: u64,
    scene: Scene,
    assets: Option<AssetStore>,
    created: Option<PathBuf>,
    pub levels: Vec<GeneratedLod>,
}
impl Drop for PreparedLods {
    fn drop(&mut self) {
        if let Some(directory) = &self.created {
            let _ = std::fs::remove_dir_all(directory);
        }
    }
}
impl Editor {
    /// Generate an independent mesh at each requested quality from the immutable base
    /// geometry. Source files are untouched; cancelling/stale publication removes only
    /// our reserved output directory. Thresholds and materials keep normal LOD semantics.
    pub fn generate_lods_job(
        &self,
        object_id: &str,
        requests: Vec<LodRequest>,
    ) -> Result<Job<PreparedLods>> {
        ensure!(self.play.is_none(), "Stop Play before generating LODs");
        ensure!(
            !requests.is_empty() && requests.len() <= 8,
            "Generate between one and eight LOD levels"
        );
        for request in &requests {
            request.settings.validate()?;
        }
        Lod {
            levels: requests
                .iter()
                .map(|r| LodLevel {
                    switch: r.switch,
                    mesh: None,
                })
                .collect(),
            hysteresis: 0.1,
        }
        .validate()?;
        let object = self
            .scene
            .objects
            .iter()
            .find(|o| o.id == object_id)
            .context("Select a mesh object")?;
        let Some(Drawable {
            mesh: Mesh::Asset(asset),
            ..
        }) = &object.drawable
        else {
            anyhow::bail!("Automatic LOD needs a whole imported mesh");
        };
        let data = self
            .assets
            .handle(asset)
            .and_then(|h| self.assets.get(h))
            .and_then(|e| e.shared_data())
            .context("Source mesh is not loaded")?;
        let AssetData::Mesh(mesh) = data.as_ref() else {
            anyhow::bail!("Source is not a mesh");
        };
        ensure!(mesh.skin.is_none(), "Automatic LOD needs a static mesh");
        let mut prepared = PreparedLods {
            path: self.path.clone(),
            revision: self.revision,
            asset_revision: self.asset_revision,
            scene: self.scene.clone(),
            assets: Some(self.assets.clone()),
            created: None,
            levels: Vec::new(),
        };
        let object_id = object_id.to_owned();
        Job::start("Generating mesh LODs", move |progress| {
            let AssetData::Mesh(mesh) = data.as_ref() else {
                unreachable!()
            };
            let assets_root = root(&prepared.path).join("assets");
            std::fs::create_dir_all(&assets_root)?;
            // Reserve one directory atomically; never reuse files after an Undo or another worker.
            let prefix = (1_u64..)
                .find_map(|n| {
                    let prefix = format!("generated-lod-{n}");
                    if requests.iter().enumerate().any(|(i, _)| {
                        prepared
                            .scene
                            .assets
                            .contains_key(&format!("{prefix}-{}", i + 1))
                    }) {
                        return None;
                    }
                    match std::fs::create_dir(assets_root.join(&prefix)) {
                        Ok(()) => Some(Ok(prefix)),
                        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => None,
                        Err(e) => Some(Err(e)),
                    }
                })
                .expect("directory sequence exhausted")?;
            prepared.created = Some(assets_root.join(&prefix));
            let mut levels = Vec::new();
            for (index, request) in requests.iter().enumerate() {
                let result = bozzard_assets::simplify_mesh(mesh, request.settings, &progress)?;
                let (extension, bytes) = if result.mesh.parts.is_empty() {
                    // A plain OBJ has no PBR material. Preserve its Lambert rendering path.
                    ("obj", plain_obj(&result.mesh).into_bytes())
                } else {
                    ("gltf", bozzard_assets::mesh_gltf(&result.mesh, &progress)?)
                };
                progress.check()?;
                let relative = format!("assets/{prefix}/level-{}.{extension}", index + 1);
                let mut file = std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(root(&prepared.path).join(&relative))?;
                file.write_all(&bytes)?;
                file.sync_all()?;
                let id = format!("{prefix}-{}", index + 1);
                prepared.scene.assets.insert(
                    id.clone(),
                    AssetSource {
                        kind: AssetKind::Mesh,
                        path: relative,
                    },
                );
                levels.push(LodLevel {
                    switch: request.switch,
                    mesh: Some(Mesh::Asset(id.clone())),
                });
                prepared.levels.push(GeneratedLod {
                    asset: id,
                    source_triangles: result.source_triangles,
                    triangles: result.triangles,
                    error: result.error,
                });
            }
            prepared
                .scene
                .objects
                .iter_mut()
                .find(|o| o.id == object_id)
                .unwrap()
                .lod = Some(Lod {
                levels,
                hysteresis: 0.1,
            });
            prepared.scene.validate()?;
            progress.stage("Validating generated LOD assets")?;
            let mut assets = prepared
                .assets
                .take()
                .unwrap()
                .for_catalog(root(&prepared.path), &prepared.scene.assets)?;
            assets.load_pending_with(&progress)?;
            assets.require_ready()?;
            prepared.assets = Some(assets);
            Ok(prepared)
        })
    }
    pub fn accept_lods(&mut self, mut prepared: PreparedLods) -> Result<Vec<GeneratedLod>> {
        ensure!(self.play.is_none(), "Stop Play before accepting LODs");
        ensure!(
            self.path == prepared.path
                && self.revision == prepared.revision
                && self.asset_revision == prepared.asset_revision,
            "Scene or assets changed during LOD generation; generate again"
        );
        prepared.scene.validate()?;
        self.finish_gesture();
        self.record(Change {
            label: "Generate mesh LODs".into(),
            scene: self.scene.clone(),
            assets: Some(self.assets.clone()),
            restore_file: None,
        });
        std::mem::swap(&mut self.scene, &mut prepared.scene);
        self.assets = prepared.assets.take().expect("prepared LOD assets");
        self.revision += 1;
        self.asset_revision += 1;
        prepared.created = None;
        Ok(std::mem::take(&mut prepared.levels))
    }
}

fn plain_obj(mesh: &bozzard_assets::MeshData) -> String {
    use std::fmt::Write;
    let mut output = String::from("# Generated by Bozzard; source geometry is unchanged\n");
    for v in &mesh.vertices {
        writeln!(
            output,
            "v {} {} {}\nvn {} {} {}\nvt {} {}",
            v[0],
            v[1],
            v[2],
            v[3],
            v[4],
            v[5],
            v[6],
            1. - v[7]
        )
        .unwrap();
    }
    for tri in mesh.indices.chunks_exact(3) {
        let [a, b, c] = [tri[0] + 1, tri[1] + 1, tri[2] + 1];
        writeln!(output, "f {a}/{a}/{a} {b}/{b}/{b} {c}/{c}/{c}").unwrap();
    }
    output
}
