use anyhow::Result;
use bozzard_demo::SceneDemo;
use bozzard_render::{DrawItem, Material, MeshKind, RenderScene, TextureKind};
use bozzard_scene::{Layer, Mesh, Texture};

pub fn extract(demo: &SceneDemo, layer: Layer, aspect: f32) -> Result<RenderScene> {
    demo.check_simulation()?;
    let view = demo.instance.view(&demo.app.world, layer, aspect)?;
    Ok(RenderScene {
        lighting: bozzard_render::Lighting {
            sun_direction: view.lighting.sun_direction,
            sun_color: view.lighting.sun_color,
            sun_intensity: view.lighting.sun_intensity,
            ambient_color: view.lighting.ambient_color,
            ambient_intensity: view.lighting.ambient_intensity,
        },
        view_projection: view.view_projection,
        items: view
            .objects
            .into_iter()
            .map(|(model, drawable)| DrawItem {
                model,
                mesh: match drawable.mesh {
                    Mesh::Quad => MeshKind::Quad,
                    Mesh::Cube => MeshKind::Cube,
                    Mesh::Asset(id) => MeshKind::Imported(id),
                },
                material: Material {
                    tint: drawable.color,
                    uv_scale: drawable.uv_scale,
                    texture: match drawable.texture {
                        Texture::White => TextureKind::White,
                        Texture::Checker => TextureKind::Checker,
                        Texture::Asset(id) => TextureKind::Imported(id),
                    },
                    lit: layer == Layer::ThreeD,
                },
            })
            .collect(),
    })
}
