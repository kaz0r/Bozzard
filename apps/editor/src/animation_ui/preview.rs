use bozzard_scene::middleware::animation::Animator;
use eframe::egui;

pub enum PreviewRequest {
    Sample { state: String, phase: f32 },
    Clear,
}
#[derive(Clone)]
struct Selection {
    state: String,
    phase: f32,
}
pub(super) fn editor(
    ui: &mut egui::Ui,
    animator: &Animator,
    changed: bool,
) -> Option<PreviewRequest> {
    if animator.states.is_empty() {
        ui.weak("Add a state to preview this skeleton's clips.");
        return None;
    }
    let mut request = None;
    egui::CollapsingHeader::new("Preview animation").default_open(true).show(ui, |ui| {
        ui.small("Scrub a state in the viewport without starting gameplay. Preview poses are never saved.");
        let id = ui.make_persistent_id("animation-preview");
        let mut selection = ui.data_mut(|data| data.get_temp::<Selection>(id))
            .unwrap_or_else(|| Selection { state: animator.initial.clone(), phase: 0. });
        if !animator.states.iter().any(|state| state.name == selection.state) { selection.state = animator.initial.clone(); }
        let before = selection.state.clone();
        egui::ComboBox::from_id_salt("preview-state").selected_text(&selection.state).show_ui(ui, |ui| {
            for state in animator.states.iter() { ui.selectable_value(&mut selection.state, state.name.clone(), &state.name); }
        });
        if ui.add(egui::Slider::new(&mut selection.phase, 0.0..=1.).text("Cycle progress")).changed()
            || before != selection.state || changed {
            request = Some(PreviewRequest::Sample { state: selection.state.clone(), phase: selection.phase });
        }
        if ui.button("Show rest pose").clicked() { request = Some(PreviewRequest::Clear); }
        ui.data_mut(|data| data.insert_temp(id, selection));
    });
    request
}
