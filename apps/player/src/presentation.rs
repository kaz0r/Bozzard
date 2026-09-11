use anyhow::Result;
use bozzard_demo::SceneDemo;
use bozzard_render::{DrawItem, Material, MeshKind, RenderScene, TextureKind};
use bozzard_scene::{Layer, Mesh, Texture};

#[cfg(test)]
#[path = "fog_tests.rs"]
mod fog_tests;

pub fn extract(
    demo: &SceneDemo,
    assets: &bozzard_assets::AssetStore,
    layer: Layer,
    aspect: f32,
) -> Result<RenderScene> {
    demo.check_simulation()?;
    let view = demo.instance.view(&demo.app.world, layer, aspect)?;
    let mut gi = None;
    if layer == Layer::ThreeD
        && demo.instance.document().gi.enabled
        && demo.instance.document().gi.baked.is_some()
    {
        let scene = demo.instance.capture(&demo.app.world)?;
        if bozzard_assets::gi::is_current(&scene, assets).unwrap_or(false) {
            let baked = scene.gi.baked.as_ref().unwrap();
            gi = Some(bozzard_render::IrradianceVolume {
                min: baked.volume.min,
                max: baked.volume.max,
                resolution: baked.volume.resolution,
                intensity: scene.gi.intensity,
                normal_bias: scene.gi.normal_bias,
                probes: baked.probes.clone(),
            });
        }
    }

    Ok(RenderScene {
        fog: bozzard_render::FogSettings {
            enabled: layer == Layer::ThreeD && view.fog.enabled,
            color: view.fog.color,
            distance_density: view.fog.distance_density,
            start_distance: view.fog.start_distance,
            height_density: view.fog.height_density,
            base_height: view.fog.base_height,
            height_falloff: view.fog.height_falloff,
        },
        gi,
        lights: view
            .lights
            .iter()
            .map(|world| bozzard_render::LocalLight {
                directional: world.light.kind == bozzard_scene::LightKind::Directional,
                shadows: world.light.requests_shadow_map().then_some(
                    bozzard_render::SpotShadowSettings {
                        bias: world.light.shadow_bias,
                        normal_bias: world.light.shadow_normal_bias,
                    },
                ),
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
            bloom: bozzard_render::BloomSettings {
                enabled: layer == Layer::ThreeD && view.display.bloom.enabled,
                intensity: view.display.bloom.intensity,
                threshold: view.display.bloom.threshold,
                scatter: view.display.bloom.scatter,
            },
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
                        Texture::Normals => TextureKind::Normals,
                        Texture::ProceduralChecker => TextureKind::ProceduralChecker,
                        Texture::Toon => TextureKind::Toon,
                        Texture::Asset(id) => TextureKind::Imported(id),
                    },
                    lit: layer == Layer::ThreeD,
                },
            })
            .collect(),
    })
}
