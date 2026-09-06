use anyhow::Result;
use bozzard_demo::SceneDemo;
use bozzard_render::{DrawItem, Material, MeshKind, RenderScene};
use bozzard_scene::{Layer, Mesh, Texture};

pub fn extract(demo: &SceneDemo, layer: Layer, aspect: f32) -> Result<RenderScene> {
    let view = demo.instance.view(&demo.app.world, layer, aspect)?;
    Ok(RenderScene {
        view_projection: view.view_projection,
        items: view
            .objects
            .into_iter()
            .map(|(model, drawable)| DrawItem {
                model,
                mesh: match drawable.mesh {
                    Mesh::Quad => MeshKind::Quad,
                    Mesh::Cube => MeshKind::Cube,
                },
                material: Material {
                    tint: drawable.color,
                    uv_scale: drawable.uv_scale,
                    checker: drawable.texture == Texture::Checker,
                    lit: layer == Layer::ThreeD,
                },
            })
            .collect(),
    })
}
