//! Shared asset-to-renderer adapter used by the editor and standalone player.
//! Neither the CPU importer nor the renderer depends on this bridge.
mod residency;
use bozzard_assets::{
    AssetData, Filter, ImageData, MeshData, Sampler, SurfaceShading, TextureMap, Wrap,
};
use bozzard_render::{Gpu, MaterialMap, ModelImage, ModelPart, ModelShading, SceneRenderer, wgpu};
pub use residency::{Residency, ResidencyReport};
use std::sync::Arc;

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

struct SharedSource(Arc<AssetData>);
impl bozzard_render::UploadSource for SharedSource {
    fn data(&self) -> bozzard_render::UploadData<'_> {
        match self.0.as_ref() {
            AssetData::Prefab(_) => unreachable!("prefabs are excluded by upload_source"),
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
    anyhow::ensure!(
        !matches!(data.as_ref(), AssetData::Prefab(_)),
        "prefabs have no GPU resources"
    );
    Ok(Arc::new(SharedSource(data)))
}
pub fn upload(
    gpu: &Gpu,
    renderer: &mut SceneRenderer,
    id: &str,
    data: &AssetData,
) -> anyhow::Result<()> {
    match data {
        AssetData::Prefab(_) => Ok(()),
        AssetData::Image(image) => {
            renderer.upload_image(gpu, id, image.width, image.height, &image.rgba)
        }
        AssetData::Mesh(mesh) => {
            renderer.upload_model(gpu, id, &mesh.vertices, &mesh.indices, &model_parts(mesh))
        }
    }
}

mod display;
pub use display::display_settings;
