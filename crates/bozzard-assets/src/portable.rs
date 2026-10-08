//! Self-contained exports of source models, with buffers and images as data URIs.
use super::*;

/// Read a glTF/GLB and return an ordinary JSON `.gltf` whose buffers and images are data URIs.
/// Resource URIs are restricted to files below the source directory, just like normal imports.
pub fn portable_gltf(path: &Path) -> Result<Vec<u8>> {
    let snapshot = source_snapshot(path)?;
    let bytes = snapshot
        .primary
        .as_ref()
        .map_err(|error| anyhow::anyhow!(error.clone()))?;
    let gltf = gltf::Gltf::from_slice(bytes).context("parsing validated glTF")?;
    let mut json = gltf_preflight(bytes)?;
    let buffer_values = json
        .get_mut("buffers")
        .and_then(serde_json::Value::as_array_mut)
        .context("glTF has invalid buffers")?;
    for buffer in gltf.buffers() {
        let data = match buffer.source() {
            gltf::buffer::Source::Bin => gltf
                .blob
                .as_deref()
                .context("GLB buffer is missing BIN chunk")?
                .to_vec(),
            gltf::buffer::Source::Uri(uri) if uri.starts_with("data:") => continue,
            gltf::buffer::Source::Uri(uri) => snapshot_resource(path, uri, &snapshot)?.to_vec(),
        };
        buffer_values[buffer.index()]["uri"] = serde_json::Value::String(format!(
            "data:application/octet-stream;base64,{}",
            STANDARD.encode(data)
        ));
    }
    if let Some(image_values) = json
        .get_mut("images")
        .and_then(serde_json::Value::as_array_mut)
    {
        for image in gltf.images() {
            let (uri, mime_type) = match image.source() {
                gltf::image::Source::Uri { uri, .. } if uri.starts_with("data:") => continue,
                gltf::image::Source::Uri { uri, mime_type } => {
                    (uri, mime_type.unwrap_or_else(|| mime_for_uri(uri)))
                }
                gltf::image::Source::View { .. } => continue,
            };
            let data = snapshot_resource(path, uri, &snapshot)?;
            image_values[image.index()]["uri"] = serde_json::Value::String(format!(
                "data:{mime_type};base64,{}",
                STANDARD.encode(data)
            ));
        }
    }
    serde_json::to_vec(&json).context("serializing portable glTF")
}

/// Convert a source model into a self-contained JSON glTF when it has material data.
/// Plain OBJ files return `Ok(None)` because their original bytes contain all supported data.
pub fn portable_model(path: &Path) -> Result<Option<Vec<u8>>> {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if matches!(extension.as_str(), "gltf" | "glb") {
        return portable_gltf(path).map(Some);
    }
    if extension == "bmesh" {
        return Ok(None);
    }
    ensure!(
        extension == "obj",
        "portable model supports OBJ/glTF/GLB/BMESH"
    );
    let snapshot = source_snapshot(path)?;
    let bytes = snapshot
        .primary
        .as_ref()
        .map_err(|error| anyhow::anyhow!(error.clone()))?;
    let AssetData::Mesh(mesh) = import(AssetKind::Mesh, path, bytes, &snapshot)? else {
        unreachable!()
    };
    if mesh.parts.is_empty() {
        Ok(None)
    } else {
        mesh_gltf(&mesh, &job::Progress::default()).map(Some)
    }
}

pub(crate) fn mime_for_uri(uri: &str) -> &'static str {
    match uri
        .rsplit('.')
        .next()
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        _ => "application/octet-stream",
    }
}
