//! The glTF/GLB importer: preflight checks, the node walk and per-primitive mesh building.
use super::*;

pub(crate) fn gltf_preflight(bytes: &[u8]) -> Result<serde_json::Value> {
    let json: serde_json::Value = if bytes.starts_with(b"glTF") {
        let glb = gltf::binary::Glb::from_slice(bytes).context("parsing GLB")?;
        serde_json::from_slice(&glb.json).context("parsing GLB JSON")?
    } else {
        serde_json::from_slice(bytes).context("parsing glTF JSON")?
    };
    ensure!(
        json.get("extensionsRequired")
            .and_then(serde_json::Value::as_array)
            .is_none_or(|extensions| extensions
                .iter()
                .all(|e| e.as_str() == Some("KHR_materials_emissive_strength"))),
        "required glTF extensions are not supported by this importer"
    );
    if let Some(meshes) = json.get("meshes").and_then(serde_json::Value::as_array) {
        ensure!(
            meshes.iter().all(|mesh| mesh.get("weights").is_none()
                && mesh
                    .get("primitives")
                    .and_then(serde_json::Value::as_array)
                    .is_none_or(|p| p.iter().all(|x| x.get("targets").is_none()))),
            "static glTF import does not support morph targets"
        );
    }
    let extensions = json
        .get("extensionsUsed")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(serde_json::Value::as_str);
    for extension in extensions {
        ensure!(
            !matches!(
                extension,
                "KHR_draco_mesh_compression"
                    | "EXT_meshopt_compression"
                    | "KHR_texture_basisu"
                    | "EXT_texture_webp"
                    | "MSFT_texture_dds"
            ),
            "static glTF import does not support compressed geometry/textures ({extension})"
        );
    }
    Ok(json)
}

pub(crate) fn import_gltf(
    path: &Path,
    bytes: &[u8],
    snapshot: &SourceSnapshot,
) -> Result<MeshData> {
    gltf_preflight(bytes)?;
    let gltf = gltf::Gltf::from_slice(bytes).context("parsing validated glTF")?;
    let mut buffers = Vec::new();
    let mut buffer_bytes = 0usize;
    for buffer in gltf.buffers() {
        let data = match buffer.source() {
            gltf::buffer::Source::Bin => gltf
                .blob
                .as_deref()
                .context("GLB buffer is missing BIN chunk")?
                .to_vec(),
            gltf::buffer::Source::Uri(uri) if uri.starts_with("data:") => data_uri(uri)?,
            gltf::buffer::Source::Uri(uri) => snapshot_resource(path, uri, snapshot)?.to_vec(),
        };
        ensure!(
            data.len() >= buffer.length(),
            "glTF buffer {} is shorter than declared",
            buffer.index()
        );
        buffer_bytes += data.len();
        ensure!(
            buffer_bytes <= 128 * 1024 * 1024,
            "decoded model buffers exceed 128 MiB"
        );
        buffers.push(data);
    }
    let mut warnings = Vec::new();
    let scene = gltf
        .default_scene()
        .or_else(|| gltf.scenes().next())
        .context("glTF has no scene")?;
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    let mut parts = Vec::new();
    let mut images = ModelImages::default();
    let mut visited = BTreeSet::new();
    let mut animation = animation::Import::new(&gltf, &buffers)?;
    for node in scene.nodes() {
        append_node(
            node,
            Mat4::IDENTITY,
            &buffers,
            path,
            snapshot,
            &mut vertices,
            &mut indices,
            &mut parts,
            &mut warnings,
            &mut images,
            &mut visited,
            &mut animation,
            0,
        )?;
    }
    ensure!(
        !vertices.is_empty() && !indices.is_empty() && indices.len() <= 3_000_000,
        "empty or oversized triangle mesh"
    );
    Ok(MeshData {
        skin: animation.map(|a| a.finish(vertices.len())).transpose()?,
        vertices,
        indices,
        parts,
        warnings,
    }
    .with_surface_keys())
}

// Shared accumulators keep recursive traversal allocation bounded.
#[allow(clippy::too_many_arguments)]
pub(crate) fn append_node(
    node: gltf::Node<'_>,
    parent: Mat4,
    buffers: &[Vec<u8>],
    path: &Path,
    snapshot: &SourceSnapshot,
    vertices: &mut Vec<[f32; 8]>,
    indices: &mut Vec<u32>,
    parts: &mut Vec<MeshPart>,
    warnings: &mut Vec<String>,
    images: &mut ModelImages,
    visited: &mut BTreeSet<usize>,
    animation: &mut Option<animation::Import>,
    depth: usize,
) -> Result<()> {
    ensure!(visited.len() < 65_536, "glTF scene exceeds 65536 nodes");
    ensure!(
        visited.insert(node.index()),
        "glTF scene contains a cycle or shared child node"
    );
    ensure!(
        depth <= MAX_NODE_DEPTH,
        "glTF node hierarchy exceeds 256 levels"
    );
    ensure!(
        node.weights().is_none(),
        "static glTF import does not support morph-weighted nodes"
    );
    let transform = parent * Mat4::from_cols_array_2d(&node.transform().matrix());
    if let Some(mesh) = node.mesh() {
        ensure!(
            mesh.weights().is_none(),
            "static glTF import does not support mesh morph weights"
        );
        for primitive in mesh.primitives() {
            let name = format!(
                "{} / {} / Surface {}",
                node.name()
                    .map(inspection_name)
                    .unwrap_or_else(|| format!("Node {}", node.index())),
                mesh.name()
                    .map(inspection_name)
                    .unwrap_or_else(|| format!("Mesh {}", mesh.index())),
                primitive.index() + 1,
            );
            let source_identity = format!(
                "gltf-node:{}-mesh:{}-primitive:{}-material:{:?}",
                node.index(),
                mesh.index(),
                primitive.index(),
                primitive.material().index()
            );
            let first_vertex = vertices.len();
            append_primitive(
                primitive.clone(),
                transform,
                buffers,
                path,
                snapshot,
                vertices,
                indices,
                parts,
                warnings,
                images,
            )?;
            if let Some(animation) = animation {
                animation.primitive(
                    &node,
                    &primitive,
                    transform,
                    buffers,
                    vertices.len() - first_vertex,
                )?;
            }
            let part = parts.last_mut().expect("appended primitive");
            part.name = name;
            part.source_key = source_identity;
        }
    }
    for child in node.children() {
        append_node(
            child,
            transform,
            buffers,
            path,
            snapshot,
            vertices,
            indices,
            parts,
            warnings,
            images,
            visited,
            animation,
            depth + 1,
        )?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn append_primitive(
    primitive: gltf::Primitive<'_>,
    transform: Mat4,
    buffers: &[Vec<u8>],
    path: &Path,
    snapshot: &SourceSnapshot,
    vertices: &mut Vec<[f32; 8]>,
    indices: &mut Vec<u32>,
    parts: &mut Vec<MeshPart>,
    warnings: &mut Vec<String>,
    images: &mut ModelImages,
) -> Result<()> {
    ensure!(
        primitive.mode() == gltf::mesh::Mode::Triangles,
        "glTF primitive mode must be TRIANGLES"
    );
    ensure!(
        primitive.morph_targets().next().is_none(),
        "static glTF import does not support morph targets"
    );
    if primitive.get(&gltf::Semantic::Colors(0)).is_some() {
        warnings.push("Vertex colors are not rendered; base-color materials are used".into());
    }
    let reader = primitive.reader(|buffer| buffers.get(buffer.index()).map(Vec::as_slice));
    let material = primitive.material();
    let pbr = material.pbr_metallic_roughness();
    let texcoord_set = pbr
        .base_color_texture()
        .map_or(0, |texture| texture.tex_coord());
    let positions: Vec<Vec3> = reader
        .read_positions()
        .context("glTF primitive lacks POSITION")?
        .map(Vec3::from_array)
        .collect();
    ensure!(!positions.is_empty(), "glTF primitive has no vertices");
    ensure!(
        vertices.len() + positions.len() <= MAX_VERTICES,
        "mesh exceeds one million vertices"
    );
    let mut primitive_indices: Vec<u32> = reader
        .read_indices()
        .map(|v| v.into_u32().collect())
        .unwrap_or_else(|| (0..positions.len() as u32).collect());
    ensure!(
        primitive_indices.len().is_multiple_of(3)
            && primitive_indices
                .iter()
                .all(|&i| (i as usize) < positions.len()),
        "invalid glTF triangle indices"
    );
    let determinant = transform.determinant();
    ensure!(
        determinant.is_finite() && determinant != 0.0,
        "glTF node has a singular transform"
    );
    let normal_matrix = Mat3::from_mat4(transform).inverse().transpose();
    let missing_normals = reader.read_normals().is_none();
    let mut normals: Vec<Vec3> = reader
        .read_normals()
        .map(|v| v.map(Vec3::from_array).collect())
        .unwrap_or_else(|| vec![Vec3::ZERO; positions.len()]);
    ensure!(
        normals.len() == positions.len(),
        "glTF normal count does not match POSITION"
    );
    if missing_normals {
        for triangle in primitive_indices.chunks_exact(3) {
            let normal = (positions[triangle[1] as usize] - positions[triangle[0] as usize])
                .cross(positions[triangle[2] as usize] - positions[triangle[0] as usize]);
            for &index in triangle {
                normals[index as usize] += normal;
            }
        }
    }
    if determinant < 0.0 {
        for triangle in primitive_indices.chunks_exact_mut(3) {
            triangle.swap(1, 2);
        }
    }
    let texcoords: Vec<[f32; 2]> = match reader.read_tex_coords(texcoord_set) {
        Some(values) => values.into_f32().collect(),
        None if pbr.base_color_texture().is_none() => vec![[0.0, 0.0]; positions.len()],
        None => bail!("glTF base color texture requires TEXCOORD_{texcoord_set}"),
    };
    ensure!(
        texcoords.len() == positions.len(),
        "glTF texture coordinate count does not match POSITION"
    );
    let base = u32::try_from(vertices.len()).context("mesh vertex count overflow")?;
    let shading = pbr::import_surface(
        &primitive,
        transform,
        buffers,
        path,
        snapshot,
        images,
        &positions,
        &normals,
        &primitive_indices,
        base,
        warnings,
    )?;
    for ((position, normal), uv) in positions.into_iter().zip(normals).zip(texcoords) {
        ensure!(
            position.is_finite() && normal.is_finite() && uv.into_iter().all(f32::is_finite),
            "glTF contains non-finite geometry"
        );
        let normal = (normal_matrix * normal)
            .try_normalize()
            .context("glTF contains degenerate geometry or normals")?;
        let position = transform.transform_point3(position);
        ensure!(
            position.is_finite(),
            "glTF transformed position is non-finite"
        );
        vertices.push([
            position.x, position.y, position.z, normal.x, normal.y, normal.z, uv[0], uv[1],
        ]);
    }
    let start = u32::try_from(indices.len()).context("mesh index count overflow")?;
    indices.extend(primitive_indices.into_iter().map(|index| base + index));
    let mut color = pbr.base_color_factor();
    let alpha_cutoff = match material.alpha_mode() {
        gltf::material::AlphaMode::Mask => Some(material.alpha_cutoff().unwrap_or(0.5)),
        gltf::material::AlphaMode::Opaque => {
            color[3] = 1.0;
            None
        }
        gltf::material::AlphaMode::Blend => None,
    };
    let image = pbr
        .base_color_texture()
        .map(|texture| {
            images.load(
                texture.texture().source(),
                matches!(material.alpha_mode(), gltf::material::AlphaMode::Opaque),
                buffers,
                path,
                snapshot,
            )
        })
        .transpose()?;
    ensure!(parts.len() < MAX_PARTS, "glTF has too many primitive parts");
    parts.push(MeshPart {
        source_key: String::new(),
        name: String::new(),
        material_name: material.name().map(inspection_name),
        start,
        count: u32::try_from(indices.len()).context("mesh index count overflow")? - start,
        color,
        image,
        alpha_cutoff,
        shading: Some(shading),
    });
    Ok(())
}

#[derive(Default)]
pub(crate) struct ModelImages {
    images: BTreeMap<(usize, bool), Arc<ImageData>>,
    bytes: usize,
}
impl ModelImages {
    pub(crate) fn load(
        &mut self,
        image: gltf::Image<'_>,
        opaque: bool,
        buffers: &[Vec<u8>],
        path: &Path,
        snapshot: &SourceSnapshot,
    ) -> Result<Arc<ImageData>> {
        let key = (image.index(), opaque);
        if let Some(image) = self.images.get(&key) {
            return Ok(image.clone());
        }
        let mut decoded = image_for(image, buffers, path, snapshot)?;
        if opaque {
            for pixel in decoded.rgba.chunks_exact_mut(4) {
                pixel[3] = 255;
            }
        }
        self.bytes = self
            .bytes
            .checked_add(decoded.rgba.len())
            .context("decoded image byte count overflow")?;
        ensure!(
            self.bytes <= MAX_GLTF_IMAGE_BYTES,
            "unique decoded glTF images exceed 512 MiB"
        );
        let decoded = Arc::new(decoded);
        self.images.insert(key, decoded.clone());
        Ok(decoded)
    }
}

pub(crate) fn image_for(
    image: gltf::Image<'_>,
    buffers: &[Vec<u8>],
    path: &Path,
    snapshot: &SourceSnapshot,
) -> Result<ImageData> {
    let bytes = match image.source() {
        gltf::image::Source::View { view, .. } => {
            let data = buffers
                .get(view.buffer().index())
                .context("image buffer is missing")?;
            let start = view.offset();
            let end = start
                .checked_add(view.length())
                .context("image buffer view overflow")?;
            data.get(start..end)
                .context("image buffer view exceeds buffer")?
                .to_vec()
        }
        gltf::image::Source::Uri { uri, .. } if uri.starts_with("data:") => data_uri(uri)?,
        gltf::image::Source::Uri { uri, .. } => snapshot_resource(path, uri, snapshot)?.to_vec(),
    };
    decoded_image(&bytes, "glTF base color texture")
}
