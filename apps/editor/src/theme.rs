use eframe::egui::{self, Color32, FontId, RichText, Stroke, Vec2};

pub const ACCENT: Color32 = Color32::from_rgb(207, 174, 119);
pub const GREEN: Color32 = Color32::from_rgb(91, 183, 151);
pub const PANEL: Color32 = Color32::from_rgb(30, 30, 32);
pub const HEADER: Color32 = Color32::from_rgb(39, 39, 41);
pub const AXES: [Color32; 3] = [
    Color32::from_rgb(178, 55, 65),
    Color32::from_rgb(62, 130, 76),
    Color32::from_rgb(55, 98, 172),
];

pub fn install(ctx: &egui::Context) {
    let mut style = (*ctx.style_of(egui::Theme::Dark)).clone();
    style
        .text_styles
        .insert(egui::TextStyle::Body, FontId::proportional(12.0));
    style
        .text_styles
        .insert(egui::TextStyle::Button, FontId::proportional(12.0));
    style
        .text_styles
        .insert(egui::TextStyle::Small, FontId::proportional(10.0));
    style
        .text_styles
        .insert(egui::TextStyle::Heading, FontId::proportional(13.0));
    style.spacing.item_spacing = Vec2::new(5.0, 4.0);
    style.spacing.button_padding = Vec2::new(6.0, 3.0);
    style.spacing.interact_size = Vec2::new(24.0, 22.0);
    style.spacing.indent = 12.0;
    style.spacing.slider_width = 110.0;
    style.spacing.combo_width = 120.0;
    style.visuals = egui::Visuals::dark();
    let visuals = &mut style.visuals;
    visuals.panel_fill = PANEL;
    visuals.window_fill = PANEL;
    visuals.extreme_bg_color = Color32::from_rgb(21, 21, 23);
    visuals.faint_bg_color = Color32::from_rgb(34, 34, 36);
    visuals.selection.bg_fill = ACCENT;
    visuals.selection.stroke = Stroke::new(1.0, Color32::from_rgb(24, 22, 19));
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, Color32::from_gray(49));
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, Color32::from_gray(206));
    visuals.widgets.inactive.bg_fill = Color32::from_gray(44);
    visuals.widgets.inactive.weak_bg_fill = Color32::from_gray(37);
    visuals.widgets.inactive.bg_stroke = Stroke::NONE;
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, Color32::from_gray(193));
    visuals.widgets.hovered.bg_fill = Color32::from_gray(62);
    visuals.widgets.hovered.weak_bg_fill = Color32::from_gray(56);
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, Color32::from_gray(85));
    visuals.widgets.active.bg_fill = Color32::from_gray(73);
    visuals.widgets.active.weak_bg_fill = Color32::from_gray(65);
    visuals.widgets.active.bg_stroke = Stroke::new(1.0, ACCENT);
    for widget in [
        &mut visuals.widgets.noninteractive,
        &mut visuals.widgets.inactive,
        &mut visuals.widgets.hovered,
        &mut visuals.widgets.active,
        &mut visuals.widgets.open,
    ] {
        widget.corner_radius = 2.into();
    }
    visuals.window_corner_radius = 3.into();
    ctx.set_style_of(egui::Theme::Dark, style);
    ctx.set_theme(egui::ThemePreference::Dark);
}

/// A dock-style title strip, shared by each workspace panel.
pub fn panel_title(ui: &mut egui::Ui, title: &str) {
    egui::Frame::new()
        .fill(HEADER)
        .inner_margin(5)
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            let response = ui.label(RichText::new(title).strong());
            ui.painter().line_segment(
                [
                    response.rect.left_bottom() + Vec2::new(0.0, 4.0),
                    response.rect.right_bottom() + Vec2::new(0.0, 4.0),
                ],
                Stroke::new(2.0, ACCENT),
            );
        });
}
