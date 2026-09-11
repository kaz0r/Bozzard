//! CPU acceptance on a real scene, producing a separate edited document for GPU review.
use anyhow::{Context, Result, ensure};
use bozzard_editor::Editor;
use bozzard_scene::Layer;
use std::path::PathBuf;

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let input = PathBuf::from(
        args.next()
            .context("usage: material_override INPUT.json OUTPUT.json")?,
    );
    let output = PathBuf::from(args.next().context("provide a separate output scene")?);
    ensure!(!output.exists(), "output already exists; choose a new path");
    let mut editor = Editor::open(&input)?;
    let original = editor.scene().clone();
    let projection = editor.render(Layer::ThreeD, 1.6)?.view_projection;
    let hit = editor
        .pick_surface_with_projection(Layer::ThreeD, projection, [0.0, 0.0])?
        .context("camera center must hit an imported surface")?;
    let index = hit.surface.context("camera center hit a built-in mesh")?;
    let object = hit.object.clone();
    editor.select_pick(Some(hit))?;
    let mut value = editor.selected_material_override()?;
    ensure!(
        editor.selected_surface().unwrap().part.shading.is_some(),
        "select a PBR scene"
    );
    value.tint = [0.2, 0.7, 1.0];
    value.metallic = Some(0.65);
    value.roughness = Some(0.2);
    let bounds = editor
        .selected_mesh()
        .unwrap()
        .part_bounds(index)
        .context("missing surface bounds")?;
    let size = (bounds[1] - bounds[0]).max(glam::Vec3::ONE);
    value.transform.translation = (size * glam::Vec3::new(0.05, 0.1, 0.)).to_array();
    value.transform.rotation_degrees = [0., 12., 5.];
    value.transform.scale = [0.9, 1.1, 1.];
    value.texture = Some(bozzard_scene::Texture::Checker);
    value.uv_scale = [3.; 2];
    let asset_revision = editor.asset_revision();
    editor.begin_gesture("Edit surface material");
    editor.set_selected_material_override(value.clone())?;
    editor.finish_gesture();
    editor.undo()?;
    ensure!(
        editor.scene() == &original && !editor.dirty(),
        "Undo failed to restore document"
    );
    editor.redo()?;
    ensure!(
        editor.selected_material_override()? == value,
        "Redo lost material override"
    );
    ensure!(
        editor.asset_revision() == asset_revision,
        "material edit reimported assets"
    );
    editor.save(&output)?;
    let saved = editor.scene().clone();
    drop(editor);
    let mut reopened = Editor::open(&output)?;
    ensure!(reopened.scene() == &saved, "saved scene mismatch");
    reopened.select_object(Some(object.clone()));
    reopened.select_surface(index)?;
    ensure!(
        reopened.selected_material_override()? == value,
        "source signature changed during Save As"
    );
    reopened.start_play()?;
    let render = reopened.render(Layer::ThreeD, 1.6)?;
    ensure!(
        render.items.iter().any(
            |item| item
                .material
                .surface_overrides
                .iter()
                .any(|v| v.surface == value.surface
                    && v.source == value.source
                    && v.tint == value.tint
                    && v.transform == value.transform.matrix()
                    && v.texture == Some(bozzard_render::TextureKind::Checker)
                    && v.uv_scale == value.uv_scale)
        ),
        "Play lost material override"
    );
    reopened.stop_play();
    ensure!(reopened.scene() == &saved, "Play changed authored state");
    println!(
        "real_scene_surface_edit_ok object={object:?} surface={index} transform texture material undo redo save_reopen source_identity play output={}",
        output.display()
    );
    Ok(())
}
