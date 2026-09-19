//! An editor build with a game-owned inspector; scene/runtime crates stay headless.
use anyhow::Result;
use bozzard_editor_app::{
    custom_inspectors::{Context, Registry},
    egui,
};
use bozzard_scene::Spin;
use serde_json::Value;

fn rpm(ui: &mut egui::Ui, value: &mut Value, context: Context<'_>) -> Result<()> {
    let spin: Spin = serde_json::from_value(value.clone())?;
    let mut rates = spin.0.map(|n| n / 6.);
    ui.label(format!("{} · revolutions per minute", context.object.name));
    let mut changed = false;
    for (axis, rate) in ["X", "Y", "Z"].into_iter().zip(&mut rates) {
        changed |= ui
            .add(egui::Slider::new(rate, -120.0..=120.0).text(axis))
            .changed();
    }
    if changed {
        *value = serde_json::to_value(Spin(rates.map(|n| n * 6.)))?;
    }
    Ok(())
}
fn main() -> Result<()> {
    // Register game-owned ComponentType rows here first, if used by this build.
    let mut inspectors = Registry::new();
    inspectors.register("spin", rpm)?;
    bozzard_editor_app::run_with_inspectors(inspectors)
}
