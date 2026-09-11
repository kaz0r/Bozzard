//! CPU-only inspection of the same surface selection/framing paths used by the UI.
use anyhow::{Context, Result, ensure};
use bozzard_editor::Editor;
use bozzard_scene::{Layer, Mesh};
use std::path::PathBuf;

fn main() -> Result<()> {
    let path = PathBuf::from(
        std::env::args()
            .nth(1)
            .context("usage: inspect_surfaces SCENE.json")?,
    );
    let mut editor = Editor::open(&path)?;
    let original = editor.scene().clone();
    let objects: Vec<_> = original
        .objects
        .iter()
        .filter_map(|object| {
            let drawable = object.drawable.as_ref()?;
            matches!(drawable.mesh, Mesh::Asset(_)).then(|| (object.id.clone(), drawable.layer))
        })
        .collect();
    let mut total = 0;
    for (id, layer) in objects {
        editor.select_object(Some(id.clone()));
        let count = editor.selected_mesh().map_or(0, |mesh| mesh.parts.len());
        for index in 0..count {
            editor.select_surface(index)?;
            let bounds = editor
                .frame_selection_bounds(layer)?
                .context("surface has no bounds")?;
            ensure!(
                bounds.iter().all(|p| p.is_finite()) && bounds[0].cmple(bounds[1]).all(),
                "invalid bounds"
            );
            ensure!(
                editor.selected_surface().is_some(),
                "surface selection lost"
            );
        }
        println!("object={id:?} surfaces={count} selection_and_bounds_ok");
        total += count;
    }
    ensure!(total > 0, "scene has no imported material surfaces");
    if original.views.contains_key(&Layer::ThreeD) {
        let projection = editor.render(Layer::ThreeD, 1.6)?.view_projection;
        for ndc in [[0.0, 0.0], [-0.4, 0.0], [0.4, 0.0]] {
            let hit = editor.pick_surface_with_projection(Layer::ThreeD, projection, ndc)?;
            editor.select_pick(hit.clone())?;
            println!("ray={ndc:?} hit={hit:?}");
        }
    }
    ensure!(
        editor.scene() == &original && !editor.dirty() && editor.undo_label().is_none(),
        "inspection modified document/history"
    );
    println!("surface_inspection_ok surfaces={total} unchanged_document_and_history");
    Ok(())
}
