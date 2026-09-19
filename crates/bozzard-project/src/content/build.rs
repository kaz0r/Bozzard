use super::*;
use crate::export::Cooker;
use std::fs;

/// Complete release folder containing content.bpack and its address catalog.
pub struct PreparedPack {
    stage: archive::Staging,
    destination: PathBuf,
    report: CookReport,
    catalog: Catalog,
}
impl PreparedPack {
    pub fn report(&self) -> CookReport {
        self.report
    }
    pub fn catalog(&self) -> &Catalog {
        &self.catalog
    }
    pub fn commit(self) -> Result<PathBuf> {
        ensure!(
            !self.destination.exists(),
            "content release destination already exists"
        );
        fs::rename(self.stage.path.join("release"), &self.destination)
            .context("publishing content release")?;
        Ok(self.destination.join("catalog.json"))
    }
}

pub fn prepare_pack(
    spec_path: &Path,
    destination: &Path,
    progress: &Progress,
) -> Result<PreparedPack> {
    progress.stage("Reading content pack specification")?;
    let spec: PackSpec = serde_json::from_slice(&archive::read_bounded(spec_path, 1024 * 1024)?)?;
    spec.validate()?;
    let source = spec_path
        .parent()
        .unwrap_or(Path::new("."))
        .canonicalize()?;
    let destination = std::path::absolute(destination)?;
    ensure!(
        !destination.exists(),
        "content release destination already exists"
    );
    let stage = archive::Staging::new(destination.parent().context("content destination parent")?)?;
    let data = stage.path.join("data");
    let release = stage.path.join("release");
    fs::create_dir_all(data.join("assets"))?;
    fs::create_dir(&release)?;
    // Fingerprints let any scene bake validate resources shared with an earlier scene.
    let mut cooker = Cooker::new(
        &data,
        source.join(".bozzard-cache/cook-v1"),
        spec.cook,
        true,
        progress,
    );
    let mut entries = BTreeMap::new();
    for (i, (address, path)) in spec.scenes.iter().enumerate() {
        progress.stage(format!("Cooking scene {address}"))?;
        let path = source.join(path);
        let scene = bozzard_scene::Scene::from_json(std::str::from_utf8(&archive::read_bounded(
            &path,
            64 * 1024 * 1024,
        )?)?)?;
        let view = [Layer::ThreeD, Layer::TwoD]
            .into_iter()
            .find(|layer| scene.views.contains_key(layer));
        let filename = format!("scene-{i:04}.json");
        cooker.scene(&scene, &path, &filename)?;
        entries.insert(
            address.clone(),
            Entry::Scene {
                path: filename,
                view,
            },
        );
    }
    for (address, asset) in &spec.assets {
        let path = cooker.asset(asset.kind, &source.join(&asset.path))?;
        entries.insert(
            address.clone(),
            Entry::Asset {
                path,
                kind: asset.kind,
            },
        );
    }
    let report = cooker.report();
    let index = Index {
        version: VERSION,
        id: spec.id.clone(),
        name: spec.name,
        cook: spec.cook,
        entries,
        files: archive::inventory(&data, progress)?,
    };
    index.validate()?;
    archive::validate_content(&data, &index, progress)?;
    let reference = archive::write_pack(&data, &release.join("content.bpack"), &index, progress)?;
    let catalog = Catalog {
        version: VERSION,
        packs: BTreeMap::from([(spec.id.clone(), reference)]),
        addresses: index
            .entries
            .keys()
            .map(|entry| {
                (
                    entry.clone(),
                    Address {
                        pack: spec.id.clone(),
                        entry: entry.clone(),
                    },
                )
            })
            .collect(),
    };
    catalog.validate()?;
    fs::write(
        release.join("catalog.json"),
        serde_json::to_vec_pretty(&catalog)?,
    )?;
    progress.check()?;
    Ok(PreparedPack {
        stage,
        destination,
        report,
        catalog,
    })
}
