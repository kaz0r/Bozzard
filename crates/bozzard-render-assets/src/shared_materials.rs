use anyhow::Result;
use bozzard_assets::AssetStore;
use bozzard_scene::{Drawable, material_asset::MaterialInstance, shader_graph::ShaderGraph};
use std::sync::Arc;

/// Asset data and bindings stay shared. Property and keyword overrides require
/// no combined-map allocation, and uniform edits preserve pipeline identities.
pub fn material_binding(
    drawable: &mut Drawable,
    binding: Option<&MaterialInstance>,
    local_shader: Option<&ShaderGraph>,
    assets: &AssetStore,
) -> Result<Option<Arc<bozzard_render::ShaderSource>>> {
    let Some(binding) = binding else {
        return local_shader.map(crate::shader_source).transpose();
    };
    let material = assets.material(&binding.asset)?;
    material.apply(binding, drawable);
    material
        .shader_variant(binding, local_shader)?
        .map(|(graph, mask)| crate::shaders::mask(graph, mask))
        .transpose()
}
