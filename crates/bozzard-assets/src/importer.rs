//! Import dispatch by asset kind, the OBJ importer, and resource and image helpers shared with glTF.
use super::*;

pub(crate) fn import(
    kind: AssetKind,
    path: &Path,
    bytes: &[u8],
    snapshot: &SourceSnapshot,
) -> Result<AssetData> {
    let extension = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match kind {
        AssetKind::Material => {
            materials::decode(path, bytes, snapshot).map(|data| AssetData::Material(Box::new(data)))
        }
        AssetKind::Font => {
            ensure!(
                matches!(extension.as_str(), "ttf" | "otf"),
                "font import supports TTF/OTF"
            );
            Ok(AssetData::Font(bozzard_text::Font::parse(bytes.to_vec())?))
        }
        AssetKind::ComputeShader => {
            ensure!(
                extension == "wgsl",
                "compute shader import supports .compute.wgsl and .wgsl"
            );
            let source = std::str::from_utf8(bytes).context("compute shader is not UTF-8 text")?;
            Ok(AssetData::ComputeShader(Arc::new(
                bozzard_compute::Kernel::parse(source)?,
            )))
        }
        AssetKind::Audio => Ok(AssetData::Audio(serde_json::from_slice(bytes)?)),
        AssetKind::Prefab => Ok(AssetData::Prefab(bozzard_scene::Prefab::from_json(
            std::str::from_utf8(bytes)?,
        )?)),
        AssetKind::Script => {
            ensure!(
                matches!(extension.as_str(), "rs" | "rhai"),
                "script import supports .rs and .rhai"
            );
            ensure!(
                bytes.len() <= 1024 * 1024,
                "script exceeds the 1 MiB source limit"
            );
            let source = std::str::from_utf8(bytes).context("script is not UTF-8 text")?;
            Ok(AssetData::Script(source.to_owned()))
        }
        AssetKind::Image => {
            ensure!(
                matches!(extension.as_str(), "png" | "jpg" | "jpeg" | "btex"),
                "image import supports PNG/JPEG/BTEX"
            );
            Ok(AssetData::Image(if extension == "btex" {
                texture::decode(bytes)?
            } else {
                decoded_image(bytes, "image asset")?
            }))
        }
        AssetKind::Mesh => {
            if path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with(".brush.json"))
            {
                return blockout::Blockout::from_json(bytes)?
                    .mesh(&job::Progress::default())
                    .map(AssetData::Mesh);
            }
            if path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with(".terrain.json"))
            {
                return terrain::Terrain::from_json(bytes)?
                    .mesh(&job::Progress::default())
                    .map(AssetData::Mesh);
            }
            if extension == "bmesh" {
                return cooked_model::decode(bytes).map(AssetData::Mesh);
            }
            if matches!(extension.as_str(), "gltf" | "glb") {
                return Ok(AssetData::Mesh(import_gltf(path, bytes, snapshot)?));
            }
            ensure!(extension == "obj", "mesh import supports OBJ/glTF/GLB");
            let (models, materials) = tobj::load_obj_buf(
                &mut Cursor::new(bytes),
                &tobj::LoadOptions {
                    single_index: true,
                    triangulate: true,
                    ignore_points: true,
                    ignore_lines: true,
                },
                |mtl| {
                    let Ok(resource) = resource_path(path, &mtl.to_string_lossy()) else {
                        return Err(tobj::LoadError::OpenFileFailed);
                    };
                    let Some((_, Ok(contents))) = snapshot
                        .dependencies
                        .iter()
                        .find(|(candidate, _)| *candidate == resource)
                    else {
                        return Err(tobj::LoadError::OpenFileFailed);
                    };
                    tobj::load_mtl_buf(&mut Cursor::new(contents))
                },
            )
            .context("parsing OBJ")?;
            let materials = materials.context("parsing OBJ material library")?;
            let mut vertices = Vec::new();
            let mut indices = Vec::new();
            let mut parts = Vec::new();
            let mut warnings = Vec::new();
            let mut image_bytes = 0usize;
            for model in models {
                let mesh = model.mesh;
                let count = mesh.positions.len() / 3;
                ensure!(
                    vertices.len() + count <= MAX_VERTICES,
                    "mesh exceeds one million vertices"
                );
                ensure!(
                    mesh.indices.len().is_multiple_of(3),
                    "OBJ did not produce triangles"
                );
                ensure!(
                    mesh.indices.iter().all(|i| (*i as usize) < count),
                    "invalid mesh index"
                );
                ensure!(
                    mesh.positions
                        .iter()
                        .chain(&mesh.normals)
                        .chain(&mesh.texcoords)
                        .all(|v| v.is_finite()),
                    "non-finite mesh data"
                );
                ensure!(
                    mesh.normals.is_empty() || mesh.normals.len() == count * 3,
                    "incomplete normals"
                );
                ensure!(
                    mesh.texcoords.is_empty() || mesh.texcoords.len() == count * 2,
                    "incomplete UVs"
                );
                let mut normals = vec![Vec3::ZERO; count];
                if mesh.normals.is_empty() {
                    for triangle in mesh.indices.chunks_exact(3) {
                        let position = |i: u32| {
                            Vec3::from_slice(&mesh.positions[i as usize * 3..i as usize * 3 + 3])
                        };
                        let n = (position(triangle[1]) - position(triangle[0]))
                            .cross(position(triangle[2]) - position(triangle[0]));
                        for &i in triangle {
                            normals[i as usize] += n;
                        }
                    }
                } else {
                    for (normal, values) in normals.iter_mut().zip(mesh.normals.chunks_exact(3)) {
                        *normal = Vec3::from_slice(values);
                    }
                }
                let base = vertices.len() as u32;
                for (i, normal) in normals.into_iter().enumerate() {
                    let normal = normal
                        .try_normalize()
                        .context("mesh contains a zero/invalid normal or degenerate geometry")?;
                    let p = &mesh.positions[i * 3..i * 3 + 3];
                    let uv = if mesh.texcoords.is_empty() {
                        [0.0, 0.0]
                    } else {
                        [mesh.texcoords[i * 2], 1.0 - mesh.texcoords[i * 2 + 1]]
                    };
                    vertices.push([p[0], p[1], p[2], normal.x, normal.y, normal.z, uv[0], uv[1]]);
                }
                let start = u32::try_from(indices.len()).context("mesh index count overflow")?;
                indices.extend(mesh.indices.into_iter().map(|i| base + i));
                if !materials.is_empty() {
                    let material = mesh.material_id.and_then(|index| materials.get(index));
                    let mut color = [1.0, 1.0, 1.0, 1.0];
                    let mut image = None;
                    if let Some(material) = material {
                        if let Some(diffuse) = material.diffuse {
                            color[..3].copy_from_slice(&diffuse);
                        }
                        color[3] = material.dissolve.unwrap_or(1.0);
                        if let Some(texture) = &material.diffuse_texture {
                            let mtl_path = obj_mtl_path(path, bytes, snapshot, &material.name)?;
                            let texture_path = resource_path(&mtl_path, texture)?;
                            let contents = snapshot
                                .dependencies
                                .iter()
                                .find(|(candidate, _)| *candidate == texture_path)
                                .context("OBJ diffuse texture was not observed")?
                                .1
                                .as_ref()
                                .map_err(|error| anyhow::anyhow!(error.clone()))?;
                            image = Some(Arc::new(decoded_image(contents, "OBJ diffuse texture")?));
                        }
                        if material.normal_texture.is_some()
                            || material.specular_texture.is_some()
                            || material.emissive.is_some()
                        {
                            warnings.push("OBJ normal, specular, and emissive material properties are not rendered".into());
                        }
                    }
                    ensure!(
                        color
                            .iter()
                            .all(|c| c.is_finite() && (0.0..=1.0).contains(c)),
                        "invalid OBJ diffuse color or opacity"
                    );
                    ensure!(parts.len() < MAX_PARTS, "OBJ has too many material parts");
                    image_bytes += image.as_ref().map_or(0, |image| image.rgba.len());
                    ensure!(
                        image_bytes <= MAX_DECODED_IMAGE_BYTES,
                        "decoded OBJ images exceed 128 MiB"
                    );
                    parts.push(MeshPart {
                        source_key: format!("obj-material:{:?}", mesh.material_id),
                        name: inspection_name(&model.name),
                        material_name: material.map(|m| inspection_name(&m.name)),
                        start,
                        count: u32::try_from(indices.len()).context("mesh index count overflow")?
                            - start,
                        color,
                        image,
                        alpha_cutoff: None,
                        shading: None,
                    });
                }
            }
            ensure!(
                !vertices.is_empty() && !indices.is_empty() && indices.len() <= 3_000_000,
                "empty or oversized triangle mesh"
            );
            Ok(AssetData::Mesh(
                MeshData {
                    skin: None,
                    vertices,
                    indices,
                    parts,
                    warnings,
                }
                .with_surface_keys(),
            ))
        }
    }
}

pub(crate) fn obj_mtl_path(
    path: &Path,
    obj: &[u8],
    snapshot: &SourceSnapshot,
    material_name: &str,
) -> Result<PathBuf> {
    for uri in obj_mtllibs(obj) {
        let mtl_path = resource_path(path, uri)?;
        let Some((_, Ok(contents))) = snapshot
            .dependencies
            .iter()
            .find(|(candidate, _)| *candidate == mtl_path)
        else {
            continue;
        };
        let (materials, _) = tobj::load_mtl_buf(&mut Cursor::new(contents))
            .context("parsing OBJ material library")?;
        if materials
            .iter()
            .any(|material| material.name == material_name)
        {
            return Ok(mtl_path);
        }
    }
    bail!("could not locate OBJ material library for '{material_name}'")
}

pub(crate) fn resource_path(source: &Path, uri: &str) -> Result<PathBuf> {
    ensure!(!uri.contains('\0'), "resource URI contains NUL");
    ensure!(
        !uri.contains(':') && !uri.starts_with('/') && !uri.starts_with('\\'),
        "only relative local resource URIs are supported: {uri}"
    );
    let decoded = percent_decode(uri)?;
    let relative = Path::new(&decoded);
    ensure!(
        relative.components().all(|part| matches!(
            part,
            std::path::Component::Normal(_) | std::path::Component::CurDir
        )),
        "resource URI escapes its glTF directory: {uri}"
    );
    let parent = source.parent().unwrap_or_else(|| Path::new("."));
    Ok(parent.join(relative))
}

pub(crate) fn percent_decode(uri: &str) -> Result<String> {
    let mut result = Vec::with_capacity(uri.len());
    let bytes = uri.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            ensure!(
                i + 2 < bytes.len(),
                "invalid percent escape in resource URI: {uri}"
            );
            let value = |byte: u8| -> Result<u8> {
                match byte {
                    b'0'..=b'9' => Ok(byte - b'0'),
                    b'a'..=b'f' => Ok(byte - b'a' + 10),
                    b'A'..=b'F' => Ok(byte - b'A' + 10),
                    _ => bail!("invalid percent escape in resource URI: {uri}"),
                }
            };
            result.push(value(bytes[i + 1])? * 16 + value(bytes[i + 2])?);
            i += 3;
        } else {
            result.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(result).context("resource URI is not UTF-8")
}

pub(crate) fn data_uri(uri: &str) -> Result<Vec<u8>> {
    let (header, encoded) = uri.split_once(',').context("malformed data URI")?;
    ensure!(
        header.starts_with("data:") && header.ends_with(";base64"),
        "only base64 data URIs are supported"
    );
    let bytes = STANDARD
        .decode(encoded)
        .context("decoding base64 data URI")?;
    ensure!(
        bytes.len() as u64 <= MAX_SOURCE_BYTES,
        "data URI exceeds 32 MiB"
    );
    Ok(bytes)
}

pub(crate) fn snapshot_resource<'a>(
    path: &Path,
    uri: &str,
    snapshot: &'a SourceSnapshot,
) -> Result<&'a [u8]> {
    if uri.starts_with("data:") {
        bail!("data URI cannot borrow source bytes")
    }
    let resource = resource_path(path, uri)?;
    snapshot
        .dependencies
        .iter()
        .find(|(candidate, _)| *candidate == resource)
        .context("resource was not observed")?
        .1
        .as_ref()
        .map(Vec::as_slice)
        .map_err(|error| anyhow::anyhow!(error.clone()))
}

pub(crate) fn decoded_image(bytes: &[u8], label: &str) -> Result<ImageData> {
    let mut reader = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .with_context(|| format!("recognizing image {label}"))?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(4096);
    limits.max_image_height = Some(4096);
    limits.max_alloc = Some(128 * 1024 * 1024);
    reader.limits(limits);
    let image = reader
        .decode()
        .with_context(|| format!("decoding image {label}"))?
        .into_rgba8();
    Ok(ImageData {
        width: image.width(),
        height: image.height(),
        rgba: image.into_raw(),
        compressed: None,
    })
}
