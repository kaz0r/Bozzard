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
    demo.check_simulation()?;
    if cache.enabled() {
        let view = demo.render_view_shared(layer, aspect, None)?;
        let gi = bozzard_render_assets::irradiance_volume(
            demo.instance(),
            &demo.app.world,
            assets,
            layer,
            None,
        )?;
        cache.extract(view, assets, layer, gi)
    } else {
        let view = demo.render_view(layer, aspect, None)?;
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
