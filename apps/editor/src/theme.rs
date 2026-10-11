//! Editor look, "Graphite": flat charcoal panels split by hairlines, one warm accent for
//! selection and focus, Inter type with Material Symbols icons, and a shared painter kit
//! for the shader and blueprint node graphs.
use crate::icons;
use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Frame, Margin,
    Pos2, Rect, Response, RichText, Shadow, Stroke, Vec2,
};
use std::sync::Arc;

// Surfaces, darkest first. Panels sit on the backdrop with a one-pixel gutter.
pub const BACKDROP: Color32 = Color32::from_rgb(12, 13, 15);
pub const SUNKEN: Color32 = Color32::from_rgb(17, 18, 21);
pub const PANEL: Color32 = Color32::from_rgb(24, 25, 29);
pub const RAISED: Color32 = Color32::from_rgb(33, 35, 40);
pub const HOVER: Color32 = Color32::from_rgb(44, 46, 53);
pub const BORDER: Color32 = Color32::from_rgb(38, 40, 46);
pub const BORDER_STRONG: Color32 = Color32::from_rgb(58, 61, 70);
pub const TEXT: Color32 = Color32::from_rgb(222, 224, 230);
pub const TEXT_DIM: Color32 = Color32::from_rgb(158, 162, 172);
pub const TEXT_MUTED: Color32 = Color32::from_rgb(106, 111, 122);

pub const ACCENT: Color32 = Color32::from_rgb(255, 140, 66);
pub const GREEN: Color32 = Color32::from_rgb(82, 214, 138);
pub const CORAL: Color32 = Color32::from_rgb(244, 112, 112);
pub const AMBER: Color32 = Color32::from_rgb(246, 196, 72);
pub const SKY: Color32 = Color32::from_rgb(98, 166, 250);
pub const RADIUS: u8 = 6;
pub const AXES: [Color32; 3] = [
    Color32::from_rgb(226, 78, 92),
    Color32::from_rgb(98, 190, 92),
    Color32::from_rgb(70, 132, 236),
];

/// The accent at `alpha` (0-255), for selection fills and drop targets.
pub fn accent(alpha: u8) -> Color32 {
    let [r, g, b, _] = ACCENT.to_array();
    Color32::from_rgba_unmultiplied(r, g, b, alpha)
}

/// White at `alpha` (0-255): a neutral highlight that works on any surface.
pub fn glass(alpha: u8) -> Color32 {
    Color32::from_white_alpha(alpha)
}

/// Semibold Inter when the theme is installed, the plain proportional font otherwise.
pub fn bold(ctx: &egui::Context, size: f32) -> FontId {
    let family = FontFamily::Name("bold".into());
    if ctx.fonts(|f| f.families().contains(&family)) {
        FontId::new(size, family)
    } else {
        FontId::proportional(size)
    }
}

fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert(
        "Inter".into(),
        Arc::new(FontData::from_static(include_bytes!(
            "../assets/fonts/Inter-Regular.ttf"
        ))),
    );
    fonts.font_data.insert(
        "Inter-SemiBold".into(),
        Arc::new(FontData::from_static(include_bytes!(
            "../assets/fonts/Inter-SemiBold.ttf"
        ))),
    );
    // Icons share the text line, slightly larger than the letters and centred on them.
    fonts.font_data.insert(
        "Icons".into(),
        Arc::new(
            FontData::from_static(include_bytes!("../assets/fonts/BozzardIcons.ttf")).tweak(
                egui::FontTweak {
                    scale: 1.2,
                    y_offset_factor: 0.0,
                    ..Default::default()
                },
            ),
        ),
    );
    // Inter first, then icons; egui's bundled fonts stay behind them for symbols and emoji.
    for family in [FontFamily::Proportional, FontFamily::Monospace] {
        let list = fonts.families.get_mut(&family).unwrap();
        if family == FontFamily::Proportional {
            list.insert(0, "Inter".into());
            list.insert(1, "Icons".into());
        } else {
            list.insert(1, "Icons".into());
        }
    }
    fonts.families.insert(
        FontFamily::Name("bold".into()),
        vec!["Inter-SemiBold".into(), "Icons".into(), "Inter".into()],
    );
    ctx.set_fonts(fonts);
}

pub fn install(ctx: &egui::Context) {
    install_fonts(ctx);
    let mut style = (*ctx.style_of(egui::Theme::Dark)).clone();
    for (text, size) in [
        (egui::TextStyle::Body, 12.5),
        (egui::TextStyle::Button, 12.5),
        (egui::TextStyle::Small, 11.0),
        (egui::TextStyle::Heading, 15.0),
    ] {
        style.text_styles.insert(text, FontId::proportional(size));
    }
    style
        .text_styles
        .insert(egui::TextStyle::Monospace, FontId::monospace(12.0));
    let spacing = &mut style.spacing;
    spacing.item_spacing = Vec2::new(6.0, 5.0);
    spacing.button_padding = Vec2::new(7.0, 3.0);
    spacing.interact_size = Vec2::new(24.0, 22.0);
    spacing.indent = 16.0;
    spacing.slider_width = 120.0;
    spacing.combo_width = 120.0;
    spacing.icon_width = 13.0;
    spacing.icon_width_inner = 7.0;
    spacing.icon_spacing = 6.0;
    spacing.menu_margin = Margin::same(6);
    spacing.window_margin = Margin::same(12);
    spacing.scroll = egui::style::ScrollStyle::thin();
    style.visuals = egui::Visuals::dark();
    let v = &mut style.visuals;
    v.panel_fill = PANEL;
    v.window_fill = PANEL;
    v.window_stroke = Stroke::new(1.0, BORDER_STRONG);
    v.window_shadow = Shadow {
        offset: [0, 10],
        blur: 32,
        spread: 0,
        color: Color32::from_black_alpha(150),
    };
    v.popup_shadow = Shadow {
        offset: [0, 6],
        blur: 18,
        spread: 0,
        color: Color32::from_black_alpha(140),
    };
    v.extreme_bg_color = SUNKEN;
    v.text_edit_bg_color = Some(SUNKEN);
    v.faint_bg_color = glass(5);
    v.code_bg_color = SUNKEN;
    v.weak_text_color = Some(TEXT_MUTED);
    v.hyperlink_color = SKY;
    v.warn_fg_color = AMBER;
    v.error_fg_color = CORAL;
    v.selection.bg_fill = accent(64);
    v.selection.stroke = Stroke::new(1.0, Color32::from_rgb(255, 226, 206));
    v.text_cursor.stroke = Stroke::new(2.0, ACCENT);
    v.slider_trailing_fill = true;
    v.indent_has_left_vline = true;
    v.collapsing_header_frame = true;
    v.striped = false;
    let w = &mut v.widgets;
    w.noninteractive.bg_fill = PANEL;
    w.noninteractive.weak_bg_fill = PANEL;
    w.noninteractive.bg_stroke = Stroke::new(1.0, BORDER);
    w.noninteractive.fg_stroke = Stroke::new(1.0, TEXT);
    w.inactive.bg_fill = Color32::from_rgb(40, 42, 48);
    w.inactive.weak_bg_fill = RAISED;
    w.inactive.bg_stroke = Stroke::new(1.0, Color32::from_rgb(46, 48, 55));
    w.inactive.fg_stroke = Stroke::new(1.0, TEXT);
    w.hovered.bg_fill = Color32::from_rgb(52, 55, 63);
    w.hovered.weak_bg_fill = HOVER;
    w.hovered.bg_stroke = Stroke::new(1.0, BORDER_STRONG);
    w.hovered.fg_stroke = Stroke::new(1.5, Color32::WHITE);
    w.active.bg_fill = accent(110);
    w.active.weak_bg_fill = accent(80);
    w.active.bg_stroke = Stroke::new(1.0, ACCENT);
    w.active.fg_stroke = Stroke::new(1.5, Color32::WHITE);
    w.open.bg_fill = HOVER;
    w.open.weak_bg_fill = HOVER;
    w.open.bg_stroke = Stroke::new(1.0, BORDER_STRONG);
    w.open.fg_stroke = Stroke::new(1.0, Color32::WHITE);
    for widget in [
        &mut w.noninteractive,
        &mut w.inactive,
        &mut w.hovered,
        &mut w.active,
        &mut w.open,
    ] {
        widget.corner_radius = 4.into();
        widget.expansion = 0.0;
    }
    v.window_corner_radius = (RADIUS + 2).into();
    v.menu_corner_radius = RADIUS.into();
    ctx.set_style_of(egui::Theme::Dark, style);
    ctx.set_theme(egui::ThemePreference::Dark);
}

/// A docked panel: flat, edge to edge. Hairline separators come from the panel edges.
pub fn dock() -> Frame {
    Frame::new().fill(PANEL)
}

/// The application bars at the top and bottom of the window.
pub fn bar() -> Frame {
    Frame::new()
        .fill(BACKDROP)
        .inner_margin(Margin::symmetric(8, 4))
}

/// Plain backdrop under every panel; it shows through as the gutters between them.
pub fn backdrop(ctx: &egui::Context) {
    let rect = ctx.content_rect();
    ctx.layer_painter(egui::LayerId::background())
        .rect_filled(rect, 0.0, BACKDROP);
}

/// Modal window chrome.
pub fn dialog_frame() -> Frame {
    Frame::new()
        .fill(PANEL)
        .stroke(Stroke::new(1.0, BORDER_STRONG))
        .corner_radius(RADIUS + 2)
        .shadow(Shadow {
            offset: [0, 12],
            blur: 36,
            spread: 0,
            color: Color32::from_black_alpha(170),
        })
}

/// Small-caps heading over an inset group, like a Unity inspector section.
pub fn section(ui: &mut egui::Ui, title: &str, add: impl FnOnce(&mut egui::Ui)) {
    ui.label(
        RichText::new(title.to_uppercase())
            .font(bold(ui.ctx(), 10.5))
            .color(TEXT_MUTED),
    );
    ui.add_space(2.0);
    Frame::new()
        .fill(SUNKEN)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(RADIUS)
        .inner_margin(Margin::same(10))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            add(ui);
        });
    ui.add_space(10.0);
}

/// Rounded tag, e.g. the target platform.
pub fn chip(ui: &mut egui::Ui, text: &str, tint: Color32) {
    Frame::new()
        .fill(tint.gamma_multiply(0.14))
        .stroke(Stroke::new(1.0, tint.gamma_multiply(0.45)))
        .corner_radius(20)
        .inner_margin(Margin::symmetric(8, 2))
        .show(ui, |ui| {
            ui.label(RichText::new(text).font(bold(ui.ctx(), 11.0)).color(tint))
        });
}

/// Square, frameless icon button for toolbars. `selected` lights it with the accent.
pub fn icon_button(ui: &mut egui::Ui, icon: &str, selected: bool, tooltip: &str) -> Response {
    let color = if selected { ACCENT } else { TEXT_DIM };
    let button = egui::Button::new(RichText::new(icon).size(15.0).color(color))
        .min_size(Vec2::splat(26.0))
        .frame_when_inactive(selected)
        .fill(if selected { accent(40) } else { RAISED })
        .stroke(if selected {
            Stroke::new(1.0, accent(110))
        } else {
            Stroke::NONE
        });
    ui.add(button).on_hover_text(tooltip)
}

/// The Bozzard mark: an accent tile with a bold B.
pub fn logo(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(22.0), egui::Sense::hover());
    ui.painter().rect_filled(rect, 5, ACCENT);
    ui.painter().text(
        rect.center() + Vec2::new(0.5, 0.5),
        egui::Align2::CENTER_CENTER,
        "B",
        bold(ui.ctx(), 14.0),
        BACKDROP,
    );
    ui.add_space(4.0);
}

/// A row of mutually exclusive options in one rounded trough. Returns whether it changed.
pub fn segmented(ui: &mut egui::Ui, value: &mut usize, options: &[(&str, &str)]) -> bool {
    let before = *value;
    Frame::new()
        .fill(SUNKEN)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(RADIUS)
        .inner_margin(Margin::same(2))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                for (index, (icon, label)) in options.iter().enumerate() {
                    let selected = *value == index;
                    let text = RichText::new(labeled(icon, label)).color(if selected {
                        TEXT
                    } else {
                        TEXT_DIM
                    });
                    let button = egui::Button::new(text)
                        .frame_when_inactive(selected)
                        .fill(if selected { HOVER } else { RAISED })
                        .stroke(Stroke::NONE)
                        .corner_radius(4)
                        .min_size(Vec2::new(0.0, 22.0));
                    if ui.add(button).clicked() {
                        *value = index;
                    }
                }
            });
        });
    *value != before
}

/// Text with a leading icon, for buttons, menus and tabs.
pub fn labeled(icon: &str, text: &str) -> String {
    format!("{icon}  {text}")
}

/// A thin vertical divider between toolbar groups.
pub fn toolbar_divider(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(9.0, 18.0), egui::Sense::hover());
    ui.painter().vline(
        rect.center().x,
        rect.y_range(),
        Stroke::new(1.0, BORDER_STRONG),
    );
}

/// Muted placeholder for an empty panel: an icon over one or two lines of guidance.
pub fn empty_state(ui: &mut egui::Ui, icon: &str, title: &str, hint: &str) {
    ui.add_space(18.0);
    ui.vertical_centered(|ui| {
        ui.label(RichText::new(icon).size(26.0).color(TEXT_MUTED));
        ui.add_space(4.0);
        ui.label(RichText::new(title).color(TEXT_DIM));
        ui.label(RichText::new(hint).small().color(TEXT_MUTED));
    });
}

pub const GRAPH_HELP: &str = "Drag headers to move · Output → input to connect · Right-click input to disconnect · Middle-drag / scroll to pan · Ctrl+scroll to zoom";
const NODE_RADIUS: u8 = 7;
const GRID: f32 = 24.0;
const INK: Color32 = Color32::from_rgb(15, 16, 19);

/// Node-editor background: deep ink with a dot grid.
pub fn graph_background(painter: &egui::Painter, clip: Rect) {
    painter.rect_filled(clip, 0.0, INK);
    // ponytail: step doubles to cap dot count instead of reading the real zoom.
    let mut step = GRID;
    while (clip.width() / step) * (clip.height() / step) > 5000.0 {
        step *= 2.0;
    }
    let (x0, x1) = (
        (clip.min.x / step).floor() as i32,
        (clip.max.x / step).ceil() as i32,
    );
    let (y0, y1) = (
        (clip.min.y / step).floor() as i32,
        (clip.max.y / step).ceil() as i32,
    );
    for ix in x0..=x1 {
        for iy in y0..=y1 {
            let major = ix % 4 == 0 && iy % 4 == 0;
            painter.circle_filled(
                Pos2::new(ix as f32 * step, iy as f32 * step),
                if major { 1.4 } else { 0.9 },
                glass(if major { 34 } else { 16 }),
            );
        }
    }
}

/// Node body: soft shadow, flat card, tinted header. Returns the header rect.
/// `outline` overrides the hairline (selection, execution, recent) and adds a glow.
pub fn node_card(
    painter: &egui::Painter,
    rect: Rect,
    header_height: f32,
    tint: Color32,
    outline: Option<(f32, Color32)>,
) -> Rect {
    let r = NODE_RADIUS;
    painter.add(
        Shadow {
            offset: [0, 6],
            blur: 18,
            spread: 0,
            color: Color32::from_black_alpha(150),
        }
        .as_shape(rect, r),
    );
    if let Some((_, color)) = outline {
        painter.add(
            Shadow {
                offset: [0, 0],
                blur: 18,
                spread: 2,
                color: color.gamma_multiply(0.45),
            }
            .as_shape(rect, r),
        );
    }
    painter.rect_filled(rect, r, Color32::from_rgb(30, 32, 37));
    let header = Rect::from_min_size(rect.min, Vec2::new(rect.width(), header_height));
    let top = CornerRadius {
        nw: r,
        ne: r,
        sw: 0,
        se: 0,
    };
    let [red, green, blue, _] = tint.to_array();
    painter.rect_filled(
        header,
        top,
        Color32::from_rgba_unmultiplied(red, green, blue, 46),
    );
    painter.line_segment(
        [header.left_bottom(), header.right_bottom()],
        Stroke::new(1.0, Color32::from_rgba_unmultiplied(red, green, blue, 150)),
    );
    let dot = header.left_center() + Vec2::new(14.0, 0.0);
    painter.circle_filled(dot, 3.5, tint);
    let (width, color) = outline.unwrap_or((1.0, Color32::from_rgb(52, 55, 63)));
    painter.rect_stroke(rect, r, Stroke::new(width, color), egui::StrokeKind::Inside);
    header
}

pub fn node_title(painter: &egui::Painter, header: Rect, title: &str) {
    painter.text(
        header.left_center() + Vec2::new(26.0, 0.0),
        egui::Align2::LEFT_CENTER,
        title,
        bold(painter.ctx(), 12.5),
        TEXT,
    );
}

/// A pin: hollow ring when unlinked, solid dot when linked (outputs are always filled).
pub fn pin_dot(painter: &egui::Painter, at: Pos2, tint: Color32, filled: bool) {
    if filled {
        painter.circle_filled(at, 8.0, tint.gamma_multiply(0.14));
    }
    painter.circle(
        at,
        4.5,
        if filled { tint } else { INK },
        Stroke::new(1.5, tint),
    );
}

pub fn pin_label(painter: &egui::Painter, at: Pos2, label: &str, output: bool) {
    let (dx, align) = if output {
        (-12.0, egui::Align2::RIGHT_CENTER)
    } else {
        (12.0, egui::Align2::LEFT_CENTER)
    };
    painter.text(
        at + Vec2::new(dx, 0.0),
        align,
        label,
        FontId::proportional(11.5),
        TEXT_DIM,
    );
}

/// A wire: soft halo under a colored core.
pub fn wire(painter: &egui::Painter, from: Pos2, to: Pos2, tint: Color32) {
    let offset = ((to.x - from.x).abs() * 0.5).max(45.0);
    let points = [
        from,
        from + Vec2::new(offset, 0.0),
        to - Vec2::new(offset, 0.0),
        to,
    ];
    for (width, color) in [(6.0, tint.gamma_multiply(0.12)), (2.2, tint)] {
        painter.add(egui::epaint::CubicBezierShape::from_points_stroke(
            points,
            false,
            Color32::TRANSPARENT,
            Stroke::new(width, color),
        ));
    }
}

/// The icon for a pane tab or menu entry.
pub fn pane_icon(title: &str) -> &'static str {
    match title {
        "Scene" | "Editor" => icons::VIEW_IN_AR,
        "Blueprint" => icons::SCHEMA,
        "Shader" => icons::HUB,
        "Hierarchy" => icons::ACCOUNT_TREE,
        "Inspector" => icons::TUNE,
        "Content" => icons::INVENTORY_2,
        "Settings" => icons::SETTINGS,
        "Debug" => icons::BUG_REPORT,
        "Script source" => icons::CODE,
        "Timeline" => icons::MOVIE,
        "Kennel" => icons::PETS,
        _ => icons::WIDGETS,
    }
}

/// The icon and tint that say what an object is, for hierarchy rows and the inspector.
pub fn object_icon(object: &bozzard_scene::Object, has_children: bool) -> (&'static str, Color32) {
    let extra = |key: &str| object.extras.contains_key(key);
    if object.camera.is_some() {
        (icons::VIDEOCAM, SKY)
    } else if object.light.is_some() {
        (icons::LIGHTBULB, AMBER)
    } else if object.player_controller.is_some() {
        (icons::PERSON, GREEN)
    } else if object.particle_emitter.is_some() {
        (icons::GRAIN, CORAL)
    } else if object.text_rendering.is_some() {
        (icons::TEXT_FIELDS, TEXT_DIM)
    } else if extra("ui_canvas") || extra("ui_widget") {
        (icons::DASHBOARD, SKY)
    } else if extra("audio_source") {
        (icons::VOLUME_UP, SKY)
    } else if extra("nav_agent") {
        (icons::SMART_TOY, GREEN)
    } else if extra("sprite") || extra("tilemap") {
        (icons::IMAGE, TEXT_DIM)
    } else if object.drawable.is_some() {
        (icons::DEPLOYED_CODE, TEXT_DIM)
    } else if object.script_manager.is_some() || !object.blueprints.is_empty() {
        (icons::CODE, ACCENT)
    } else if has_children {
        (icons::FOLDER, TEXT_MUTED)
    } else {
        (icons::CROP_FREE, TEXT_MUTED)
    }
}

/// The icon for a registered component, by its registry name.
pub fn component_icon(name: &str) -> &'static str {
    match name {
        "transform" => icons::OPEN_WITH,
        "drawable" => icons::DEPLOYED_CODE,
        "material" => icons::PALETTE,
        "shader_graph" => icons::HUB,
        "collider" => icons::SELECT_ALL,
        "mesh_collider" => icons::SHAPES,
        "gravity" => icons::WEIGHT,
        "player_controller" => icons::PERSON,
        "trigger" => icons::SENSORS,
        "joint" => icons::LINK,
        "light" => icons::LIGHTBULB,
        "camera" => icons::VIDEOCAM,
        "text_rendering" => icons::TEXT_FIELDS,
        "particle_emitter" | "particle_modules" => icons::GRAIN,
        "spin" => icons::AUTORENEW,
        "lod" => icons::LAYERS,
        "script_manager" => icons::CODE,
        "blueprints" => icons::SCHEMA,
        "blackboard" => icons::DATA_OBJECT,
        "tween" => icons::ANIMATION,
        "timeline" => icons::MOVIE,
        "animator" => icons::DIRECTIONS_RUN,
        "audio_source" => icons::VOLUME_UP,
        "audio_mixer" => icons::TUNE,
        "audio_listener" => icons::HEARING,
        "nav_surface" => icons::MAP,
        "nav_agent" => icons::SMART_TOY,
        "sprite" => icons::IMAGE,
        "tilemap" => icons::GRID_VIEW,
        "ui_canvas" => icons::DASHBOARD,
        "ui_widget" => icons::WIDGETS,
        "localization" => icons::PUBLIC,
        "steam_multiplayer" | "network_obstacle" => icons::LAN,
        "network_player" => icons::GROUP,
        _ => icons::EXTENSION,
    }
}

/// A collapsible inspector section: a raised bar with chevron, icon and title, optional
/// actions at the right, and the body below. Mirrors `egui::CollapsingHeader`'s builder.
pub struct Foldout<'a> {
    icon: &'a str,
    title: &'a str,
    id_salt: Option<egui::Id>,
    default_open: bool,
    open: Option<bool>,
}

impl<'a> Foldout<'a> {
    pub fn new(icon: &'a str, title: &'a str) -> Self {
        Self {
            icon,
            title,
            id_salt: None,
            default_open: false,
            open: None,
        }
    }

    /// Force the section open or closed this frame, as `CollapsingHeader::open` does.
    pub fn open(mut self, open: Option<bool>) -> Self {
        self.open = open;
        self
    }

    pub fn id_salt(mut self, salt: impl std::hash::Hash + std::fmt::Debug) -> Self {
        self.id_salt = Some(egui::Id::new(salt));
        self
    }

    pub fn default_open(mut self, open: bool) -> Self {
        self.default_open = open;
        self
    }

    pub fn show(self, ui: &mut egui::Ui, body: impl FnOnce(&mut egui::Ui)) {
        self.show_with_actions(ui, |_| {}, body);
    }

    /// The bar's click is registered before `actions`, so buttons there keep their clicks.
    pub fn show_with_actions(
        self,
        ui: &mut egui::Ui,
        actions: impl FnOnce(&mut egui::Ui),
        body: impl FnOnce(&mut egui::Ui),
    ) {
        let id = ui.make_persistent_id(self.id_salt.unwrap_or_else(|| egui::Id::new(self.title)));
        let mut state = egui::collapsing_header::CollapsingState::load_with_default_open(
            ui.ctx(),
            id,
            self.default_open,
        );
        if let Some(open) = self.open {
            state.set_open(open);
        }
        let (rect, header) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), 26.0), egui::Sense::click());
        if header.clicked() {
            state.toggle(ui);
        }
        let fill = if header.hovered() { HOVER } else { RAISED };
        ui.painter().rect_filled(rect, 4, fill);
        // Actions take the right end first; the title truncates into what is left.
        let mut bar = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(rect.shrink2(Vec2::new(6.0, 0.0)))
                .layout(egui::Layout::right_to_left(egui::Align::Center)),
        );
        bar.spacing_mut().item_spacing.x = 6.0;
        actions(&mut bar);
        let open = state.is_open();
        bar.with_layout(egui::Layout::left_to_right(egui::Align::Center), |bar| {
            let chevron = if open {
                icons::EXPAND_MORE
            } else {
                icons::CHEVRON_RIGHT
            };
            bar.add(egui::Label::new(RichText::new(chevron).color(TEXT_MUTED)).selectable(false));
            bar.add(egui::Label::new(RichText::new(self.icon).color(ACCENT)).selectable(false));
            bar.add(
                egui::Label::new(RichText::new(self.title).font(bold(bar.ctx(), 12.5)))
                    .selectable(false)
                    .truncate(),
            );
        });
        state.show_body_unindented(ui, |ui| {
            Frame::new()
                .inner_margin(Margin {
                    left: 6,
                    right: 2,
                    top: 4,
                    bottom: 8,
                })
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    body(ui)
                });
        });
        ui.add_space(2.0);
    }
}
