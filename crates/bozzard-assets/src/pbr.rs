use crate::{ImageData, ModelImages, SourceSnapshot};
use anyhow::{Context, Result, ensure};
use glam::{Mat3, Mat4, Vec2, Vec3};
use std::{path::Path, sync::Arc};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wrap {
    Repeat,
    Clamp,
    Mirror,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Filter {
    Nearest,
    Linear,
}

/// Texture sampling is independent of the shared decoded image.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sampler {
    pub wrap_u: Wrap,
    pub wrap_v: Wrap,
    pub mag: Filter,
    pub min: Filter,
    /// None means no mipmap sampling, matching glTF NEAREST/LINEAR min filters.
    pub mip: Option<Filter>,
}
impl Default for Sampler {
    fn default() -> Self {
        Self {
            wrap_u: Wrap::Repeat,
            wrap_v: Wrap::Repeat,
            mag: Filter::Linear,
            min: Filter::Linear,
            mip: Some(Filter::Linear),
        }
    }
}
impl From<gltf::texture::Sampler<'_>> for Sampler {
    fn from(value: gltf::texture::Sampler<'_>) -> Self {
        use gltf::texture::{MagFilter, MinFilter, WrappingMode};
        let wrap = |v| match v {
            WrappingMode::ClampToEdge => Wrap::Clamp,
            WrappingMode::MirroredRepeat => Wrap::Mirror,
            WrappingMode::Repeat => Wrap::Repeat,
        };
        let (min, mip) = match value.min_filter() {
            Some(MinFilter::Nearest) => (Filter::Nearest, None),
            Some(MinFilter::Linear) => (Filter::Linear, None),
            Some(MinFilter::NearestMipmapNearest) => (Filter::Nearest, Some(Filter::Nearest)),
            Some(MinFilter::LinearMipmapNearest) => (Filter::Linear, Some(Filter::Nearest)),
            Some(MinFilter::NearestMipmapLinear) => (Filter::Nearest, Some(Filter::Linear)),
            Some(MinFilter::LinearMipmapLinear) | None => (Filter::Linear, Some(Filter::Linear)),
        };
        Self {
            wrap_u: wrap(value.wrap_s()),
            wrap_v: wrap(value.wrap_t()),
            mag: if value.mag_filter() == Some(MagFilter::Nearest) {
                Filter::Nearest
            } else {
                Filter::Linear
            },
            min,
            mip,
        }
    }
}

#[derive(Clone, Debug)]
pub struct TextureMap {
    pub image: Arc<ImageData>,
    pub sampler: Sampler,
}

#[derive(Clone, Debug)]
pub struct PbrMaterial {
    pub metallic: f32,
    pub roughness: f32,
    pub normal_scale: f32,
    pub occlusion_strength: f32,
    pub emissive_factor: [f32; 3],
    pub double_sided: bool,
    pub base_color_sampler: Sampler,
    /// Linear data: glTF metallic in B, roughness in G.
    pub metallic_roughness: Option<TextureMap>,
    /// Linear tangent-space normal data.
    pub normal: Option<TextureMap>,
    /// Linear occlusion in R; only affects indirect lighting.
    pub occlusion: Option<TextureMap>,
    /// sRGB color, multiplied by the linear emissive factor.
    pub emissive: Option<TextureMap>,
}

/// Extra attributes correspond to a contiguous range of the model's vertices.
/// Tangent XYZW followed by UV pairs for normal, metallic/roughness, occlusion,
/// and emissive. Base-color UVs remain in the main vertex stream.
#[derive(Clone, Debug)]
pub struct SurfaceShading {
    pub vertex_start: u32,
    pub vertices: Vec<[f32; 12]>,
    pub material: PbrMaterial,
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn import_surface(
    primitive: &gltf::Primitive<'_>,
    transform: Mat4,
    buffers: &[Vec<u8>],
    path: &Path,
    snapshot: &SourceSnapshot,
    images: &mut ModelImages,
    positions: &[Vec3],
    normals: &[Vec3],
    indices: &[u32],
    vertex_start: u32,
    warnings: &mut Vec<String>,
) -> Result<SurfaceShading> {
    let material = primitive.material();
    let pbr = material.pbr_metallic_roughness();
    let normal = material.normal_texture();
    let mr = pbr.metallic_roughness_texture();
    let occlusion = material.occlusion_texture();
    let emissive = material.emissive_texture();
    let slots = [
        normal.as_ref().map(|t| (t.texture(), t.tex_coord())),
        mr.as_ref().map(|t| (t.texture(), t.tex_coord())),
        occlusion.as_ref().map(|t| (t.texture(), t.tex_coord())),
        emissive.as_ref().map(|t| (t.texture(), t.tex_coord())),
    ];
    let reader = primitive.reader(|buffer| buffers.get(buffer.index()).map(Vec::as_slice));
    let mut vertices = vec![[0.; 12]; positions.len()];
    let mut maps = Vec::new();
    for (slot, source) in slots.into_iter().enumerate() {
        let map = if let Some((texture, set)) = source {
            let uv: Vec<_> = reader
                .read_tex_coords(set)
                .with_context(|| format!("glTF material texture requires TEXCOORD_{set}"))?
                .into_f32()
                .collect();
            ensure!(
                uv.len() == positions.len() && uv.iter().flatten().all(|v| v.is_finite()),
                "invalid glTF material texture coordinates"
            );
            for (vertex, uv) in vertices.iter_mut().zip(uv) {
                vertex[4 + slot * 2..6 + slot * 2].copy_from_slice(&uv);
            }
            Some(TextureMap {
                image: images.load(texture.source(), false, buffers, path, snapshot)?,
                sampler: texture.sampler().into(),
            })
        } else {
            None
        };
        maps.push(map);
    }
    let mut tangents: Vec<[f32; 4]> = if let Some(values) = reader.read_tangents() {
        values.collect()
    } else {
        generate_tangents(positions, normals, indices, &vertices)
    };
    ensure!(
        tangents.len() == positions.len(),
        "glTF tangent count does not match POSITION"
    );
    let mut repairs = Vec::new();
    for (i, (tangent, normal)) in tangents.iter().zip(normals).enumerate() {
        ensure!(
            tangent.iter().all(|v| v.is_finite()) && (tangent[3].abs() - 1.).abs() < 0.0001,
            "invalid glTF tangent"
        );
        let n = normal.normalize_or_zero();
        let t = Vec3::from_slice(&tangent[..3]);
        if (t - n * n.dot(t)).length_squared() < 1e-12 {
            repairs.push(i);
        }
    }
    if !repairs.is_empty() {
        warnings.push(format!(
            "Repaired {} degenerate source tangents from normal-map UVs",
            repairs.len()
        ));
        let generated = generate_tangents(positions, normals, indices, &vertices);
        for i in repairs {
            tangents[i] = generated[i];
        }
    }
    let linear = Mat3::from_mat4(transform);
    let normal_matrix = linear.inverse().transpose();
    for ((vertex, tangent), normal) in vertices.iter_mut().zip(tangents).zip(normals) {
        ensure!(
            tangent.iter().all(|v| v.is_finite()) && (tangent[3].abs() - 1.).abs() < 0.0001,
            "invalid glTF tangent"
        );
        let n = (normal_matrix * *normal)
            .try_normalize()
            .context("invalid tangent normal")?;
        let t = linear * Vec3::from_slice(&tangent[..3]);
        let t = (t - n * n.dot(t))
            .try_normalize()
            .context("degenerate glTF tangent")?;
        vertex[..4].copy_from_slice(&[t.x, t.y, t.z, tangent[3] * linear.determinant().signum()]);
    }
    let mut maps = maps.into_iter();
    let material = PbrMaterial {
        metallic: pbr.metallic_factor(),
        roughness: pbr.roughness_factor(),
        normal_scale: normal.as_ref().map_or(1., |t| t.scale()),
        occlusion_strength: occlusion.as_ref().map_or(1., |t| t.strength()),
        emissive_factor: material.emissive_factor(),
        double_sided: material.double_sided(),
        base_color_sampler: pbr
            .base_color_texture()
            .map_or_else(Sampler::default, |t| t.texture().sampler().into()),
        normal: maps.next().flatten(),
        metallic_roughness: maps.next().flatten(),
        occlusion: maps.next().flatten(),
        emissive: maps.next().flatten(),
    };
    ensure!(
        [
            material.metallic,
            material.roughness,
            material.occlusion_strength
        ]
        .into_iter()
        .chain(material.emissive_factor)
        .all(|v| v.is_finite() && (0.0..=1.0).contains(&v))
            && material.normal_scale.is_finite(),
        "invalid glTF PBR factors"
    );
    Ok(SurfaceShading {
        vertex_start,
        vertices,
        material,
    })
}

fn generate_tangents(
    positions: &[Vec3],
    normals: &[Vec3],
    indices: &[u32],
    vertices: &[[f32; 12]],
) -> Vec<[f32; 4]> {
    let mut tangents = vec![Vec3::ZERO; positions.len()];
    let mut bitangents = tangents.clone();
    for triangle in indices.chunks_exact(3) {
        let [a, b, c] = [
            triangle[0] as usize,
            triangle[1] as usize,
            triangle[2] as usize,
        ];
        let uv = |i: usize| Vec2::from_slice(&vertices[i][4..6]);
        let e1 = positions[b] - positions[a];
        let e2 = positions[c] - positions[a];
        let d1 = uv(b) - uv(a);
        let d2 = uv(c) - uv(a);
        let determinant = d1.x * d2.y - d1.y * d2.x;
        if determinant.abs() < 1e-12 {
            continue;
        }
        let t = (e1 * d2.y - e2 * d1.y) / determinant;
        let b = (e2 * d1.x - e1 * d2.x) / determinant;
        for &i in triangle {
            tangents[i as usize] += t;
            bitangents[i as usize] += b;
        }
    }
    normals
        .iter()
        .zip(tangents)
        .zip(bitangents)
        .map(|((n, t), b)| {
            let n = n.normalize_or_zero();
            let t = (t - n * n.dot(t)).try_normalize().unwrap_or_else(|| {
                let axis = if n.x.abs() < 0.9 { Vec3::X } else { Vec3::Y };
                n.cross(axis).normalize_or_zero()
            });
            [t.x, t.y, t.z, if n.cross(t).dot(b) < 0. { -1. } else { 1. }]
        })
        .collect()
}
