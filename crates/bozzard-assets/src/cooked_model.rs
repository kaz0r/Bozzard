//! Versioned native model payloads: geometry, PBR maps, rigs and animation, without source files.
use crate::{
    ImageData, MeshData, MeshPart, PbrMaterial, Sampler, SurfaceShading, TextureMap,
    animation::Skin,
    job::Progress,
    texture::{self, Compression},
};
use anyhow::{Context, Result, ensure};
use bozzard_scene::middleware::animation::data::Rig;
use image::ImageEncoder;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

const MAGIC: &[u8; 8] = b"BOZZMESH";
const VERSION: u32 = 1;
pub const MAX_FILE_BYTES: usize = 512 * 1024 * 1024;
const MAX_METADATA: usize = 64 * 1024 * 1024;
const MAX_IMAGES: usize = crate::MAX_PARTS * 5;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Metadata {
    vertices: usize,
    indices: usize,
    images: usize,
    parts: Vec<Part>,
    rig: Option<Arc<Rig>>,
    warnings: Vec<String>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Part {
    source_key: String,
    name: String,
    material_name: Option<String>,
    start: u32,
    count: u32,
    color: [f32; 4],
    image: Option<usize>,
    alpha_cutoff: Option<f32>,
    shading: Option<Shading>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Shading {
    vertex_start: u32,
    vertices: usize,
    metallic: f32,
    roughness: f32,
    normal_scale: f32,
    occlusion_strength: f32,
    emissive_factor: [f32; 3],
    double_sided: bool,
    base_color_sampler: Sampler,
    // Same order as the renderer: normal, metallic/roughness, occlusion, emissive.
    maps: [Option<Map>; 4],
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Map {
    image: usize,
    sampler: Sampler,
}

#[derive(Default)]
struct Images<'a> {
    indices: BTreeMap<usize, usize>,
    data: Vec<(&'a ImageData, u8)>,
}
impl<'a> Images<'a> {
    fn insert(&mut self, image: &'a ImageData, srgb: bool) -> usize {
        let key = image as *const ImageData as usize;
        let index = *self.indices.entry(key).or_insert_with(|| {
            let index = self.data.len();
            self.data.push((image, 0));
            index
        });
        self.data[index].1 |= if srgb { 1 } else { 2 };
        index
    }
}

fn validate(mesh: &MeshData) -> Result<()> {
    crate::simplify::validate_geometry(mesh)?;
    ensure!(
        mesh.warnings.len() <= 4096 && mesh.warnings.iter().all(|w| w.len() <= 4096),
        "invalid model warnings"
    );
    let mut shading_count = 0;
    for part in &mesh.parts {
        ensure!(
            part.source_key.len() <= 256
                && part.name.len() <= 512
                && part.material_name.as_ref().is_none_or(|n| n.len() <= 512),
            "oversized surface metadata"
        );
        ensure!(
            part.color
                .iter()
                .all(|v| v.is_finite() && (0. ..=1.).contains(v))
                && part
                    .alpha_cutoff
                    .is_none_or(|v| v.is_finite() && (0. ..=1.).contains(&v)),
            "invalid material color or alpha cutoff"
        );
        if let Some(s) = &part.shading {
            shading_count += s.vertices.len();
            let m = &s.material;
            ensure!(
                !s.vertices.is_empty()
                    && s.vertices
                        .iter()
                        .all(|v| v[..3].iter().map(|x| x * x).sum::<f32>() > 1e-12
                            && (v[3].abs() - 1.).abs() < 0.0001),
                "invalid tangent frame"
            );
            ensure!(
                [m.metallic, m.roughness, m.occlusion_strength]
                    .into_iter()
                    .all(|v| v.is_finite() && (0. ..=1.).contains(&v))
                    && m.emissive_factor.iter().all(|v| v.is_finite() && *v >= 0.)
                    && m.normal_scale.is_finite(),
                "invalid PBR factors"
            );
        }
    }
    ensure!(
        shading_count <= crate::MAX_VERTICES,
        "cooked shading streams exceed one million vertices"
    );
    if let Some(skin) = &mesh.skin {
        skin.rig.validate()?;
        ensure!(
            !mesh.parts.is_empty()
                && !skin.rig.bindings.is_empty()
                && skin.vertices.len() == mesh.vertices.len(),
            "invalid skin stream size"
        );
        for v in &skin.vertices {
            let mut sum = 0.;
            for i in 0..4 {
                let weight = f32::from_bits(v[i + 4]);
                ensure!(
                    (v[i] as usize) < skin.rig.bindings.len() && weight.is_finite() && weight >= 0.,
                    "invalid skin influence"
                );
                sum += weight;
            }
            ensure!((sum - 1.).abs() < 0.001, "skin weights must sum to one");
        }
    }
    Ok(())
}

/// Empty formats retain lossless PNG maps. Other targets cook every unique map
/// in the color spaces that actually reference it; shared images stay shared.
pub fn encode(mesh: &MeshData, formats: &[Compression], progress: &Progress) -> Result<Vec<u8>> {
    progress.stage("Validating model for cooking")?;
    validate(mesh)?;
    let mut images = Images::default();
    let parts = mesh
        .parts
        .iter()
        .map(|p| Part {
            source_key: p.source_key.clone(),
            name: p.name.clone(),
            material_name: p.material_name.clone(),
            start: p.start,
            count: p.count,
            color: p.color,
            alpha_cutoff: p.alpha_cutoff,
            image: p.image.as_deref().map(|i| images.insert(i, true)),
            shading: p.shading.as_ref().map(|s| {
                let m = &s.material;
                Shading {
                    vertex_start: s.vertex_start,
                    vertices: s.vertices.len(),
                    metallic: m.metallic,
                    roughness: m.roughness,
                    normal_scale: m.normal_scale,
                    occlusion_strength: m.occlusion_strength,
                    emissive_factor: m.emissive_factor,
                    double_sided: m.double_sided,
                    base_color_sampler: m.base_color_sampler,
                    maps: std::array::from_fn(|index| {
                        [&m.normal, &m.metallic_roughness, &m.occlusion, &m.emissive][index]
                            .as_ref()
                            .map(|map| Map {
                                image: images.insert(&map.image, index == 3),
                                sampler: map.sampler,
                            })
                    }),
                }
            }),
        })
        .collect::<Vec<_>>();
    ensure!(images.data.len() <= MAX_IMAGES, "too many cooked images");
    let decoded_bytes: usize = images.data.iter().map(|(i, _)| i.rgba.len()).sum();
    ensure!(
        decoded_bytes <= crate::MAX_GLTF_IMAGE_BYTES,
        "decoded model images exceed 512 MiB"
    );
    let metadata = Metadata {
        vertices: mesh.vertices.len(),
        indices: mesh.indices.len(),
        images: images.data.len(),
        parts,
        rig: mesh.skin.as_ref().map(|s| s.rig.clone()),
        warnings: mesh.warnings.clone(),
    };
    let metadata = serde_json::to_vec(&metadata)?;
    ensure!(
        metadata.len() <= MAX_METADATA,
        "cooked model metadata exceeds 64 MiB"
    );
    let geometry_bytes = mesh.vertices.len() * 32
        + mesh.indices.len() * 4
        + mesh
            .parts
            .iter()
            .filter_map(|p| p.shading.as_ref())
            .map(|s| s.vertices.len() * 48)
            .sum::<usize>()
        + mesh.skin.as_ref().map_or(0, |s| s.vertices.len() * 32);
    let mut bytes = Vec::with_capacity(16 + metadata.len() + geometry_bytes + 32);
    bytes.extend(MAGIC);
    bytes.extend(VERSION.to_le_bytes());
    bytes.extend((metadata.len() as u32).to_le_bytes());
    bytes.extend(metadata);
    for v in mesh.vertices.iter().flatten() {
        bytes.extend(v.to_le_bytes());
    }
    for i in &mesh.indices {
        bytes.extend(i.to_le_bytes());
    }
    for s in mesh.parts.iter().filter_map(|p| p.shading.as_ref()) {
        for v in s.vertices.iter().flatten() {
            bytes.extend(v.to_le_bytes());
        }
    }
    if let Some(skin) = &mesh.skin {
        for v in skin.vertices.iter().flatten() {
            bytes.extend(v.to_le_bytes());
        }
    }
    for (index, (image, spaces)) in images.data.iter().enumerate() {
        progress.stage(format!(
            "Cooking model image {}/{}",
            index + 1,
            images.data.len()
        ))?;
        let (kind, data) = if formats.is_empty() {
            ensure!(
                (1..=4096).contains(&image.width)
                    && (1..=4096).contains(&image.height)
                    && image.rgba.len() == image.width as usize * image.height as usize * 4,
                "invalid model image"
            );
            let mut png = Vec::new();
            image::codecs::png::PngEncoder::new(&mut png).write_image(
                &image.rgba,
                image.width,
                image.height,
                image::ExtendedColorType::Rgba8,
            )?;
            (0_u32, png)
        } else {
            let spaces: Vec<_> = [false, true]
                .into_iter()
                .filter(|srgb| spaces & if *srgb { 1 } else { 2 } != 0)
                .collect();
            let cooked = texture::cook(image, formats, &spaces, progress)?;
            (1, texture::encode(image, &cooked)?)
        };
        ensure!(
            data.len() <= 32 * 1024 * 1024 && bytes.len() + 8 + data.len() + 32 <= MAX_FILE_BYTES,
            "cooked model exceeds file/image limits"
        );
        bytes.extend(kind.to_le_bytes());
        bytes.extend((data.len() as u32).to_le_bytes());
        bytes.extend(data);
    }
    progress.check()?;
    ensure!(
        bytes.len() + 32 <= MAX_FILE_BYTES,
        "cooked model exceeds 512 MiB"
    );
    let digest = Sha256::digest(&bytes);
    bytes.extend(digest);
    Ok(bytes)
}

struct Reader<'a>(&'a [u8]);
impl<'a> Reader<'a> {
    fn bytes(&mut self, n: usize) -> Result<&'a [u8]> {
        let (head, tail) = self
            .0
            .split_at_checked(n)
            .context("truncated cooked model")?;
        self.0 = tail;
        Ok(head)
    }
    fn word(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.bytes(4)?.try_into()?))
    }
    fn words<const N: usize>(&mut self, count: usize) -> Result<Vec<[u32; N]>> {
        let bytes = self.bytes(count.checked_mul(N * 4).context("cooked stream overflow")?)?;
        Ok(bytes
            .chunks_exact(N * 4)
            .map(|v| {
                std::array::from_fn(|i| u32::from_le_bytes(v[i * 4..i * 4 + 4].try_into().unwrap()))
            })
            .collect())
    }
    fn floats<const N: usize>(&mut self, count: usize) -> Result<Vec<[f32; N]>> {
        let bytes = self.bytes(count.checked_mul(N * 4).context("cooked stream overflow")?)?;
        Ok(bytes
            .chunks_exact(N * 4)
            .map(|v| {
                std::array::from_fn(|i| f32::from_le_bytes(v[i * 4..i * 4 + 4].try_into().unwrap()))
            })
            .collect())
    }
}

pub fn decode(bytes: &[u8]) -> Result<MeshData> {
    ensure!(
        (48..=MAX_FILE_BYTES).contains(&bytes.len()),
        "invalid cooked model size"
    );
    let (payload, digest) = bytes.split_at(bytes.len() - 32);
    ensure!(
        Sha256::digest(payload).as_slice() == digest,
        "cooked model checksum mismatch"
    );
    let mut r = Reader(payload);
    ensure!(
        r.bytes(8)? == MAGIC && r.word()? == VERSION,
        "unsupported cooked model version"
    );
    let size = r.word()? as usize;
    ensure!(size <= MAX_METADATA, "cooked metadata exceeds 64 MiB");
    let m: Metadata =
        serde_json::from_slice(r.bytes(size)?).context("decoding cooked model metadata")?;
    ensure!(
        (1..=crate::MAX_VERTICES).contains(&m.vertices)
            && (1..=3_000_000).contains(&m.indices)
            && m.indices.is_multiple_of(3)
            && m.parts.len() <= crate::MAX_PARTS
            && m.images <= MAX_IMAGES,
        "invalid cooked model counts"
    );
    let mut shade_count = 0_usize;
    let mut used = BTreeSet::new();
    for p in &m.parts {
        if let Some(i) = p.image {
            used.insert(i);
        }
        if let Some(s) = &p.shading {
            shade_count = shade_count
                .checked_add(s.vertices)
                .context("shading count overflow")?;
            ensure!(
                shade_count <= crate::MAX_VERTICES,
                "cooked shading streams exceed one million vertices"
            );
            for map in s.maps.iter().flatten() {
                used.insert(map.image);
            }
        }
    }
    ensure!(
        used.len() == m.images && used.iter().copied().eq(0..m.images),
        "cooked image references are incomplete or out of bounds"
    );
    let vertices = r.floats(m.vertices)?;
    let indices = r.words::<1>(m.indices)?.into_iter().map(|i| i[0]).collect();
    let mut shading = Vec::with_capacity(m.parts.len());
    for p in &m.parts {
        shading.push(
            p.shading
                .as_ref()
                .map(|s| r.floats(s.vertices))
                .transpose()?,
        );
    }
    let skin = m
        .rig
        .map(|rig| {
            Ok::<_, anyhow::Error>(Skin {
                rig,
                vertices: r.words(m.vertices)?,
            })
        })
        .transpose()?;
    let mut images = Vec::with_capacity(m.images);
    let mut decoded = 0_usize;
    for _ in 0..m.images {
        let (kind, size) = (r.word()?, r.word()? as usize);
        ensure!(size <= 32 * 1024 * 1024, "cooked image exceeds 32 MiB");
        let payload = r.bytes(size)?;
        let image = match kind {
            0 => crate::decoded_image(payload, "cooked model map")?,
            1 => texture::decode(payload)?,
            _ => anyhow::bail!("unsupported cooked image encoding"),
        };
        decoded += image.rgba.len();
        ensure!(
            decoded <= crate::MAX_GLTF_IMAGE_BYTES,
            "cooked images decode beyond 512 MiB"
        );
        images.push(Arc::new(image));
    }
    ensure!(r.0.is_empty(), "trailing cooked model data");
    let parts = m
        .parts
        .into_iter()
        .zip(shading)
        .map(|(p, vertices)| {
            let image = |i: usize| images[i].clone(); // Reference set validated before allocation.
            MeshPart {
                source_key: p.source_key,
                name: p.name,
                material_name: p.material_name,
                start: p.start,
                count: p.count,
                color: p.color,
                alpha_cutoff: p.alpha_cutoff,
                image: p.image.map(image),
                shading: p.shading.map(|s| {
                    let [normal, metallic_roughness, occlusion, emissive] = s.maps.map(|m| {
                        m.map(|m| TextureMap {
                            image: image(m.image),
                            sampler: m.sampler,
                        })
                    });
                    SurfaceShading {
                        vertex_start: s.vertex_start,
                        vertices: vertices.unwrap(),
                        material: PbrMaterial {
                            metallic: s.metallic,
                            roughness: s.roughness,
                            normal_scale: s.normal_scale,
                            occlusion_strength: s.occlusion_strength,
                            emissive_factor: s.emissive_factor,
                            double_sided: s.double_sided,
                            base_color_sampler: s.base_color_sampler,
                            normal,
                            metallic_roughness,
                            occlusion,
                            emissive,
                        },
                    }
                }),
            }
        })
        .collect();
    let mesh = MeshData {
        vertices,
        indices,
        parts,
        skin,
        warnings: m.warnings,
    };
    validate(&mesh)?;
    Ok(mesh)
}

#[cfg(test)]
mod tests;
