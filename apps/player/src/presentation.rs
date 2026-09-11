use anyhow::Result;
use bozzard_demo::SceneDemo;
use bozzard_render::{DrawItem, Material, MeshKind, RenderScene, TextureKind};
use bozzard_scene::{Layer, Mesh, Texture};

pub fn extract(demo: &SceneDemo, layer: Layer, aspect: f32) -> Result<RenderScene> {
    demo.check_simulation()?;
    let view = demo.instance.view(&demo.app.world, layer, aspect)?;
    Ok(RenderScene {
        lights: view
            .lights
            .iter()
            .map(|world| bozzard_render::LocalLight {
                position: world.position,
                direction: world.direction,
                color: world.light.color,
                intensity: world.light.intensity,
                range: world.light.range,
                spot_angles: (world.light.kind == bozzard_scene::LightKind::Spot).then_some([
                    world.light.inner_angle_degrees,
                    world.light.outer_angle_degrees,
                ]),
            })
            .collect(),
        environment: bozzard_render::EnvironmentSettings {
            zenith: view.environment.zenith,
            horizon: view.environment.horizon,
            ground: view.environment.ground,
            intensity: if layer == Layer::ThreeD {
                view.environment.intensity
            } else {
                0.
            },
            background: layer == Layer::ThreeD && view.environment.background,
        },
        display: bozzard_render::DisplaySettings {
            exposure_ev: if layer == Layer::ThreeD {
                view.display.exposure_ev
            } else {
                0.
            },
            tone_mapping: layer == Layer::ThreeD && view.display.tone_mapping,
        },
        lighting: bozzard_render::Lighting {
            shadows: view.lighting.shadows,
            shadow_resolution: view.lighting.shadow_resolution,
            shadow_bias: view.lighting.shadow_bias,
            shadow_normal_bias: view.lighting.shadow_normal_bias,
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
                    surface_overrides: drawable
                        .material_overrides
                        .into_iter()
                        .map(|value| bozzard_render::SurfaceMaterialOverride {
                            surface: value.surface,
                            source: value.source,
                            tint: value.tint,
                            metallic: value.metallic,
                            roughness: value.roughness,
                        })
                        .collect(),
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
