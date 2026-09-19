//! Shared asset-to-renderer adapter used by the editor and standalone player.
//! Neither the CPU importer nor the renderer depends on this bridge.
mod ui;
pub use ui::{nine_slice, widget_items};
mod compute;
mod residency;
pub use compute::ComputeBridge;

use bozzard_assets::{
    AssetData, Filter, ImageData, MeshData, Sampler, SurfaceShading, TextureMap, Wrap,
};
use bozzard_render::{Gpu, MaterialMap, ModelImage, ModelPart, ModelShading, SceneRenderer, wgpu};
pub use residency::{Residency, ResidencyReport, ResidencyStats, required_assets};
use std::sync::Arc;

pub fn skin_poses(
    poses: &std::collections::BTreeMap<u64, bozzard_scene::middleware::animation::Palette>,
) -> std::collections::BTreeMap<u64, bozzard_render::SkinPose> {
    poses
        .iter()
        .map(|(&id, pose)| {
            (
                id,
                bozzard_render::SkinPose {
                    signature: pose.signature,
                    matrices: pose.matrices.clone(),
                },
            )
        })
        .collect()
}

/// The same text settings feed rendering, editor bounds, and picking.
pub fn text_mesh(
    text: &bozzard_scene::TextRendering,
    assets: &bozzard_assets::AssetStore,
) -> anyhow::Result<bozzard_render::TextMesh> {
    let custom_font = assets.text_font(text)?;
    Ok(bozzard_render::TextMesh {
        custom_font,
        clip: None,
        screen: text.screen.map(|s| bozzard_render::ScreenText {
            anchor: s.anchor,
            offset: s.offset,
        }),
        text: text.text.clone(),
        font_size: text.font_size,
        max_width: text.max_width,
        monospace: text.font == bozzard_scene::TextFont::Monospace,
        alignment: match text.alignment {
            bozzard_scene::TextAlignment::Left => bozzard_render::TextAlignment::Left,
            bozzard_scene::TextAlignment::Center => bozzard_render::TextAlignment::Center,
            bozzard_scene::TextAlignment::Right => bozzard_render::TextAlignment::Right,
        },
        opacity: text.color[3],
    })
}
pub fn text_item(
    model: glam::Mat4,
    text: &bozzard_scene::TextRendering,
    assets: &bozzard_assets::AssetStore,
) -> anyhow::Result<bozzard_render::DrawItem> {
    Ok(bozzard_render::DrawItem {
        motion_id: 0,
        model,
        mesh: bozzard_render::MeshKind::Text(text_mesh(text, assets)?),
        material: bozzard_render::Material {
            metallic: None,
            roughness: None,
            surface_overrides: Default::default(),
            tint: [text.color[0], text.color[1], text.color[2]],
            uv_scale: [1.; 2],
            texture: bozzard_render::TextureKind::Text,
            lit: false,
            shader: None,
        },
    })
}

fn image(source: &ImageData) -> ModelImage<'_> {
    ModelImage {
        width: source.width,
        height: source.height,
        rgba: &source.rgba,
    }
}
fn sampler(source: Sampler) -> wgpu::SamplerDescriptor<'static> {
    let wrap = |v| match v {
        Wrap::Repeat => wgpu::AddressMode::Repeat,
        Wrap::Clamp => wgpu::AddressMode::ClampToEdge,
        Wrap::Mirror => wgpu::AddressMode::MirrorRepeat,
    };
    let filter = |v| match v {
        Filter::Nearest => wgpu::FilterMode::Nearest,
        Filter::Linear => wgpu::FilterMode::Linear,
    };
    wgpu::SamplerDescriptor {
        label: Some("authored model sampler"),
        address_mode_u: wrap(source.wrap_u),
        address_mode_v: wrap(source.wrap_v),
        mag_filter: filter(source.mag),
        min_filter: filter(source.min),
        mipmap_filter: if source.mip == Some(Filter::Linear) {
            wgpu::MipmapFilterMode::Linear
        } else {
            wgpu::MipmapFilterMode::Nearest
        },
        lod_max_clamp: if source.mip.is_some() { 32. } else { 0. },
        ..Default::default()
    }
}
fn map(source: &TextureMap) -> MaterialMap<'_> {
    MaterialMap {
        image: image(&source.image),
        sampler: sampler(source.sampler),
    }
}
fn shading(source: &SurfaceShading) -> ModelShading<'_> {
    let m = &source.material;
    ModelShading {
        vertex_start: source.vertex_start,
        vertices: &source.vertices,
        metallic: m.metallic,
        roughness: m.roughness,
        normal_scale: m.normal_scale,
        occlusion_strength: m.occlusion_strength,
        emissive_factor: m.emissive_factor,
        double_sided: m.double_sided,
        base_color_sampler: sampler(m.base_color_sampler),
        normal: m.normal.as_ref().map(map),
        metallic_roughness: m.metallic_roughness.as_ref().map(map),
        occlusion: m.occlusion.as_ref().map(map),
        emissive: m.emissive.as_ref().map(map),
    }
}
pub fn model_parts(mesh: &MeshData) -> Vec<ModelPart<'_>> {
    mesh.parts
        .iter()
        .map(|part| ModelPart {
            source_key: &part.source_key,
            start: part.start,
            count: part.count,
            color: part.color,
            alpha_cutoff: part.alpha_cutoff,
            image: part.image.as_deref().map(image),
            shading: part.shading.as_ref().map(shading),
        })
        .collect()
}

/// Whether an imported asset has anything to put on the GPU.
///
/// Prefabs, scripts, and audio have no graphics resources. The residency pass and
/// `upload_source` share this classification.
pub fn needs_gpu(data: &AssetData) -> bool {
    matches!(data, AssetData::Image(_) | AssetData::Mesh(_))
        || matches!(data, AssetData::Material(material) if material.image.is_some())
}

struct SharedSource(Arc<AssetData>, wgpu::Features);
impl bozzard_render::UploadSource for SharedSource {
    fn validate(&self) -> anyhow::Result<()> {
        let mut seen = std::collections::BTreeSet::new();
        let mut check = |image: &ImageData| -> anyhow::Result<()> {
            if let Some(cooked) = &image.compressed
                && seen.insert(image.rgba.as_ptr() as usize)
            {
                anyhow::ensure!(
                    cooked.matches(image),
                    "stale cooked texture; recook modified pixels"
                );
            }
            Ok(())
        };
        match self.0.as_ref() {
            AssetData::Material(material) => check(
                material
                    .image
                    .as_deref()
                    .expect("material requires an image upload"),
            )?,
            AssetData::Image(image) => check(image)?,
            AssetData::Mesh(mesh) => {
                for p in &mesh.parts {
                    if let Some(image) = &p.image {
                        check(image)?;
                    }
                    if let Some(shading) = &p.shading {
                        let s = &shading.material;
                        for map in [&s.normal, &s.metallic_roughness, &s.occlusion, &s.emissive]
                            .into_iter()
                            .flatten()
                        {
                            check(&map.image)?;
                        }
                    }
                }
            }
            _ => unreachable!(),
        }
        Ok(())
    }

    fn compressed(
        &self,
        locator: Option<(usize, usize)>,
        srgb: bool,
    ) -> Option<bozzard_render::CompressedImage<'_>> {
        let image = match (self.0.as_ref(), locator) {
            (AssetData::Material(material), None) => material.image.as_deref()?,
            (AssetData::Image(image), None) => image,
            (AssetData::Mesh(mesh), Some((part, slot))) => {
                let part = mesh.parts.get(part)?;
                if slot == 0 {
                    part.image.as_deref()?
                } else {
                    let s = &part.shading.as_ref()?.material;
                    [&s.normal, &s.metallic_roughness, &s.occlusion, &s.emissive]
                        .get(slot.checked_sub(1)?)?
                        .as_ref()?
                        .image
                        .as_ref()
                }
            }
            _ => return None,
        };
        // WebGPU requires block-aligned base dimensions. Do not resize authored
        // textures or change their UV mapping just to make compression fit.
        if !image.width.is_multiple_of(4) || !image.height.is_multiple_of(4) {
            return None;
        }
        let cooked = image.compressed.as_ref()?;
        use bozzard_assets::texture::Compression;
        let selected = [
            (
                Compression::Astc4x4,
                wgpu::Features::TEXTURE_COMPRESSION_ASTC,
                bozzard_render::BlockCompression::Astc4x4,
            ),
            (
                Compression::Bc3,
                wgpu::Features::TEXTURE_COMPRESSION_BC,
                bozzard_render::BlockCompression::Bc3,
            ),
        ]
        .into_iter()
        .find_map(|(format, feature, gpu_format)| {
            self.1
                .contains(feature)
                .then(|| {
                    cooked
                        .variants()
                        .iter()
                        .find(|v| v.format() == format && v.srgb() == srgb)
                        .map(|v| (v, gpu_format))
                })
                .flatten()
        })?;
        Some(bozzard_render::CompressedImage {
            format: selected.1,
            levels: selected.0.levels(),
        })
    }
    fn skin(&self) -> Option<bozzard_render::SkinData<'_>> {
        let AssetData::Mesh(mesh) = self.0.as_ref() else {
            return None;
        };
        let skin = mesh.skin.as_ref()?;
        Some(bozzard_render::SkinData {
            signature: skin.rig.signature(),
            bindings: skin.rig.bindings.len(),
            vertices: &skin.vertices,
        })
    }
    fn data(&self) -> bozzard_render::UploadData<'_> {
        match self.0.as_ref() {
            AssetData::Material(material) => bozzard_render::UploadData::Image(image(
                material
                    .image
                    .as_deref()
                    .expect("material requires an image upload"),
            )),
            AssetData::Prefab(_)
            | AssetData::Font(_)
            | AssetData::Audio(_)
            | AssetData::Script(_)
            | AssetData::ComputeShader(_) => {
                unreachable!("non-rendered assets are excluded by upload_source")
            }
            AssetData::Image(data) => bozzard_render::UploadData::Image(image(data)),
            AssetData::Mesh(mesh) => bozzard_render::UploadData::Model {
                vertices: &mesh.vertices,
                indices: &mesh.indices,
                parts: model_parts(mesh),
            },
        }
    }
}
pub fn upload_source(
    data: Arc<AssetData>,
) -> anyhow::Result<Arc<dyn bozzard_render::UploadSource>> {
    upload_source_with_features(data, wgpu::Features::empty())
}
/// Select only enabled device formats; an unsupported or unaligned texture uses
/// original pixels. The default upload_source remains an exact RGBA reference path.
pub fn upload_source_with_features(
    data: Arc<AssetData>,
    features: wgpu::Features,
) -> anyhow::Result<Arc<dyn bozzard_render::UploadSource>> {
    anyhow::ensure!(needs_gpu(&data), "this asset has no GPU resources");
    Ok(Arc::new(SharedSource(data, features)))
}
pub fn upload(
    gpu: &Gpu,
    renderer: &mut SceneRenderer,
    id: &str,
    data: &AssetData,
) -> anyhow::Result<()> {
    match data {
        AssetData::Material(material) => {
            if let Some(image) = &material.image {
                renderer.upload_image(gpu, id, image.width, image.height, &image.rgba)
            } else {
                Ok(())
            }
        }
        AssetData::Prefab(_)
        | AssetData::Font(_)
        | AssetData::Audio(_)
        | AssetData::Script(_)
        | AssetData::ComputeShader(_) => Ok(()),
        AssetData::Image(image) => {
            renderer.upload_image(gpu, id, image.width, image.height, &image.rgba)
        }
        AssetData::Mesh(mesh) => {
            if let Some(skin) = &mesh.skin {
                renderer.upload_skinned_model(
                    gpu,
                    id,
                    &mesh.vertices,
                    &mesh.indices,
                    &model_parts(mesh),
                    bozzard_render::SkinData {
                        signature: skin.rig.signature(),
                        bindings: skin.rig.bindings.len(),
                        vertices: &skin.vertices,
                    },
                )
            } else {
                renderer.upload_model(gpu, id, &mesh.vertices, &mesh.indices, &model_parts(mesh))
            }
        }
    }
}

mod display;
pub use display::{display_settings, particle_frame};

/// Compile one scene shader graph to the renderer's surface-function source.
/// The content hash of the generated WGSL keys the renderer's pipeline cache.
pub fn shader_source(
    graph: &bozzard_scene::shader_graph::ShaderGraph,
) -> anyhow::Result<Arc<bozzard_render::ShaderSource>> {
    shaders::source(graph)
}
/// Select static branches before WGSL compilation. Values outside the graph's
/// declared keyword set are rejected instead of silently selecting a fallback.
pub fn shader_variant_source(
    graph: &bozzard_scene::shader_graph::ShaderGraph,
    keywords: &std::collections::BTreeMap<String, bool>,
) -> anyhow::Result<Arc<bozzard_render::ShaderSource>> {
    shaders::variant(graph, keywords)
}

mod shaders;
mod shared_materials;
pub use shared_materials::material_binding;

/// Shared atlas geometry uses content-cached GPU meshes, including an entire tilemap in one draw.
pub fn sprite_items(
    sprites: &[bozzard_scene::middleware::sprite::Visual],
) -> anyhow::Result<Vec<bozzard_render::DrawItem>> {
    sprites
        .iter()
        .map(|sprite| {
            Ok(bozzard_render::DrawItem {
                motion_id: sprite.motion_id,
                model: sprite.model,
                mesh: bozzard_render::MeshKind::Sprite(bozzard_render::SpriteMesh {
                    geometry: bozzard_render::SpriteGeometry::shared(sprite.quads.clone())?,
                    screen: None,
                    clip: None,
                    opacity: sprite.color[3],
                }),
                material: bozzard_render::Material {
                    metallic: None,
                    roughness: None,
                    surface_overrides: Default::default(),
                    tint: [sprite.color[0], sprite.color[1], sprite.color[2]],
                    uv_scale: [1.; 2],
                    texture: bozzard_render::TextureKind::Imported(sprite.image.clone()),
                    lit: false,
                    shader: None,
                },
            })
        })
        .collect()
}

pub mod accessibility;
