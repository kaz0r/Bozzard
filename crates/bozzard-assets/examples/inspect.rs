//! Reproducible CPU import measurements without a GPU or editor.
use anyhow::{Context, Result};
use bozzard_assets::{AssetData, AssetStore};
use bozzard_scene::{AssetKind, AssetSource};
use std::{collections::BTreeMap, path::PathBuf, time::Instant};

fn main() -> Result<()> {
    let path = std::path::absolute(PathBuf::from(
        std::env::args_os().nth(1).context("usage: inspect MODEL")?,
    ))?;
    let sources = BTreeMap::from([(
        "model".into(),
        AssetSource {
            kind: AssetKind::Mesh,
            path: path
                .file_name()
                .context("model filename")?
                .to_string_lossy()
                .into_owned(),
        },
    )]);
    let mut store = AssetStore::new(path.parent().context("model directory")?, &sources)?;
    let start = Instant::now();
    store.refresh();
    println!("import_ms={:.2}", start.elapsed().as_secs_f64() * 1000.0);
    store.require_ready()?;
    let AssetData::Mesh(mesh) = store
        .entries()
        .next()
        .and_then(|e| e.data())
        .context("mesh missing")?
    else {
        anyhow::bail!("expected mesh");
    };
    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];
    for vertex in &mesh.vertices {
        for axis in 0..3 {
            min[axis] = min[axis].min(vertex[axis]);
            max[axis] = max[axis].max(vertex[axis]);
        }
    }
    println!(
        "vertices={} triangles={} surfaces={} bounds_min={min:?} bounds_max={max:?}",
        mesh.vertices.len(),
        mesh.indices.len() / 3,
        mesh.parts.len()
    );
    for warning in &mesh.warnings {
        println!("warning={warning}");
    }
    let mut images = BTreeMap::new();
    for part in &mesh.parts {
        if let Some(image) = &part.image {
            images.insert(std::sync::Arc::as_ptr(image) as usize, image.rgba.len());
        }
    }
    println!(
        "unique_images={} decoded_image_mib={:.2}",
        images.len(),
        images.values().sum::<usize>() as f64 / 1048576.0
    );
    let start = Instant::now();
    let changed = store.refresh();
    println!(
        "unchanged_check_ms={:.2} changed={}",
        start.elapsed().as_secs_f64() * 1000.0,
        changed.len()
    );
    Ok(())
}
