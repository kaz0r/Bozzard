//! Preserve glTF metadata while extracting resources into a relocatable project folder.
use super::*;

pub struct ModelPackage {
    /// Generated flat filenames only. The document is always `model.gltf`.
    pub files: BTreeMap<String, Vec<u8>>,
}

pub fn package_gltf(path: &Path, progress: &job::Progress) -> Result<ModelPackage> {
    progress.stage("Reading model resources")?;
    let snapshot = source_snapshot(path)?;
    let bytes = snapshot
        .primary
        .as_ref()
        .map_err(|e| anyhow::anyhow!(e.clone()))?;
    let gltf = gltf::Gltf::from_slice(bytes)?;
    let mut json = gltf_preflight(bytes)?;
    let mut files = BTreeMap::new();
    let mut uris = BTreeMap::<String, String>::new();
    for buffer in gltf.buffers() {
        progress.check()?;
        let key = match buffer.source() {
            gltf::buffer::Source::Bin => "glb:bin",
            gltf::buffer::Source::Uri(uri) => uri,
        };
        let name = if let Some(name) = uris.get(key) {
            name.clone()
        } else {
            let data = match buffer.source() {
                gltf::buffer::Source::Bin => gltf.blob.clone().context("missing GLB buffer")?,
                gltf::buffer::Source::Uri(uri) if uri.starts_with("data:") => data_uri(uri)?,
                gltf::buffer::Source::Uri(uri) => snapshot_resource(path, uri, &snapshot)?.to_vec(),
            };
            let name = format!("buffer-{}.bin", buffer.index());
            files.insert(name.clone(), data);
            uris.insert(key.to_owned(), name.clone());
            name
        };
        json["buffers"][buffer.index()]["uri"] = name.into();
    }
    for image in gltf.images() {
        progress.check()?;
        if let gltf::image::Source::Uri { uri, mime_type } = image.source() {
            let name = if let Some(name) = uris.get(uri) {
                name.clone()
            } else {
                let data = if uri.starts_with("data:") {
                    data_uri(uri)?
                } else {
                    snapshot_resource(path, uri, &snapshot)?.to_vec()
                };
                let mime = mime_type.unwrap_or_else(|| mime_for_uri(uri));
                let extension = if mime == "image/jpeg" || uri.starts_with("data:image/jpeg;") {
                    "jpg"
                } else {
                    "png"
                };
                let name = format!("image-{}.{}", image.index(), extension);
                files.insert(name.clone(), data);
                uris.insert(uri.to_owned(), name.clone());
                name
            };
            json["images"][image.index()]["uri"] = name.into();
        }
    }
    files.insert("model.gltf".into(), serde_json::to_vec(&json)?);
    Ok(ModelPackage { files })
}
