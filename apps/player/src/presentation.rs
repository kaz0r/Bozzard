use anyhow::Result;
use bozzard_render::RenderScene;
use bozzard_render_assets::{RenderFrame, RenderSceneCache};
use bozzard_runtime::SceneRuntime;
use bozzard_scene::Layer;

#[cfg(test)]
#[path = "fog_tests.rs"]
mod fog_tests;
#[cfg(test)]
#[path = "interpolation_tests.rs"]
mod interpolation_tests;

pub fn extract(
    demo: &SceneRuntime,
    assets: &bozzard_assets::AssetStore,
    layer: Layer,
    aspect: f32,
) -> Result<RenderScene> {
    demo.check_simulation()?;
    let view = demo.render_view(layer, aspect, None)?;
    let gi = bozzard_render_assets::irradiance_volume(
        demo.instance(),
        &demo.app.world,
        assets,
        layer,
        None,
    )?;
    bozzard_render_assets::render_scene(view, assets, layer, gi)
}

pub fn extract_frame(
    demo: &SceneRuntime,
    assets: &bozzard_assets::AssetStore,
    cache: &RenderSceneCache,
    layer: Layer,
    aspect: f32,
) -> Result<RenderFrame> {
    extract_frame_from(demo, assets, cache, layer, aspect, None)
}

/// As `extract_frame`, seen from `pose` (a camera world matrix) instead of the view camera.
pub fn extract_frame_from(
    demo: &SceneRuntime,
    assets: &bozzard_assets::AssetStore,
    cache: &RenderSceneCache,
    layer: Layer,
    aspect: f32,
    pose: Option<glam::Mat4>,
) -> Result<RenderFrame> {
    demo.check_simulation()?;
    if cache.enabled() {
        let view = demo.render_view_shared(layer, aspect, pose)?;
        let gi = bozzard_render_assets::irradiance_volume(
            demo.instance(),
            &demo.app.world,
            assets,
            layer,
            None,
        )?;
        cache.extract(view, assets, layer, gi)
    } else {
        let view = demo.render_view(layer, aspect, pose)?;
        let gi = bozzard_render_assets::irradiance_volume(
            demo.instance(),
            &demo.app.world,
            assets,
            layer,
            None,
        )?;
        cache.reference(view, assets, layer, gi)
    }
}
