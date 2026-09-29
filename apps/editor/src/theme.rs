//! Editor look: dark glass cards floating over a soft aurora backdrop, Inter type,
//! and a shared painter kit for the shader and blueprint node graphs.
use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Frame, Margin,
    Pos2, Rect, RichText, Shadow, Stroke, Vec2,
};
use std::sync::Arc;

pub const ACCENT: Color32 = Color32::from_rgb(132, 158, 255);
pub const GREEN: Color32 = Color32::from_rgb(110, 231, 183);
pub const CORAL: Color32 = Color32::from_rgb(255, 122, 144);
pub const AMBER: Color32 = Color32::from_rgb(255, 200, 110);
pub const SKY: Color32 = Color32::from_rgb(112, 178, 255);
pub const PANEL: Color32 = Color32::from_rgb(22, 24, 34);
pub const BACKDROP: Color32 = Color32::from_rgb(9, 10, 16);
pub const RADIUS: u8 = 12;
pub const AXES: [Color32; 3] = [
    Color32::from_rgb(232, 88, 104),
    Color32::from_rgb(96, 200, 128),
    Color32::from_rgb(92, 140, 240),
];

/// White at `alpha` (0-255): the glass highlight used for strokes and hover fills.
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
    // Inter first; egui's bundled fonts stay behind it for symbols and emoji.
    fonts
        .families
        .get_mut(&FontFamily::Proportional)
        .unwrap()
        .insert(0, "Inter".into());
    fonts.families.insert(
        FontFamily::Name("bold".into()),
        vec!["Inter-SemiBold".into(), "Inter".into()],
    );
    ctx.set_fonts(fonts);
}

pub fn install(ctx: &egui::Context) {
    install_fonts(ctx);
    let mut style = (*ctx.style_of(egui::Theme::Dark)).clone();
    for (text, size) in [
        (egui::TextStyle::Body, 12.0),
        (egui::TextStyle::Button, 12.0),
        (egui::TextStyle::Small, 10.5),
        (egui::TextStyle::Heading, 14.0),
    ] {
        style.text_styles.insert(text, FontId::proportional(size));
    }
    style.spacing.item_spacing = Vec2::new(5.0, 5.0);
    style.spacing.button_padding = Vec2::new(5.0, 4.0);
    style.spacing.interact_size = Vec2::new(26.0, 24.0);
    style.spacing.indent = 14.0;
    style.spacing.slider_width = 110.0;
    style.spacing.combo_width = 120.0;
    style.spacing.menu_margin = Margin::same(8);
    style.spacing.window_margin = Margin::same(12);
    style.visuals = egui::Visuals::dark();
    let v = &mut style.visuals;
    v.panel_fill = PANEL;
    v.window_fill = Color32::from_rgba_unmultiplied(24, 26, 38, 244);
    v.window_stroke = Stroke::new(1.0, glass(30));
    v.window_shadow = Shadow {
        offset: [0, 14],
        blur: 40,
        spread: 0,
        color: Color32::from_black_alpha(140),
    };
    v.popup_shadow = Shadow {
        offset: [0, 8],
        blur: 24,
        spread: 0,
        color: Color32::from_black_alpha(130),
    };
    v.extreme_bg_color = Color32::from_rgb(14, 15, 22);
    v.faint_bg_color = glass(6);
    v.code_bg_color = Color32::from_rgb(14, 15, 22);
    v.hyperlink_color = ACCENT;
    v.selection.bg_fill = Color32::from_rgba_unmultiplied(132, 158, 255, 92);
    v.selection.stroke = Stroke::new(1.0, Color32::from_rgb(226, 232, 255));
    v.text_cursor.stroke = Stroke::new(2.0, ACCENT);
    v.slider_trailing_fill = true;
    let w = &mut v.widgets;
    w.noninteractive.bg_stroke = Stroke::new(1.0, glass(16));
    w.noninteractive.fg_stroke = Stroke::new(1.0, Color32::from_gray(212));
    w.inactive.bg_fill = glass(14);
    w.inactive.weak_bg_fill = glass(10);
    w.inactive.bg_stroke = Stroke::new(1.0, glass(10));
    w.inactive.fg_stroke = Stroke::new(1.0, Color32::from_gray(200));
    w.hovered.bg_fill = glass(28);
    w.hovered.weak_bg_fill = glass(22);
    w.hovered.bg_stroke = Stroke::new(1.0, glass(60));
    w.hovered.fg_stroke = Stroke::new(1.5, Color32::WHITE);
    w.active.bg_fill = Color32::from_rgba_unmultiplied(132, 158, 255, 120);
    w.active.weak_bg_fill = Color32::from_rgba_unmultiplied(132, 158, 255, 90);
    w.active.bg_stroke = Stroke::new(1.0, ACCENT);
    w.active.fg_stroke = Stroke::new(1.5, Color32::WHITE);
    w.open.bg_fill = glass(24);
    w.open.weak_bg_fill = glass(18);
    w.open.bg_stroke = Stroke::new(1.0, glass(40));
    for widget in [
        &mut w.noninteractive,
        &mut w.inactive,
        &mut w.hovered,
        &mut w.active,
        &mut w.open,
    ] {
        widget.corner_radius = 8.into();
        widget.expansion = 0.0;
    }
    v.window_corner_radius = (RADIUS + 2).into();
    v.menu_corner_radius = 10.into();
    ctx.set_style_of(egui::Theme::Dark, style);
    ctx.set_theme(egui::ThemePreference::Dark);
}

/// A translucent rounded card: the shell for every docked panel and bar.
pub fn card() -> Frame {
    Frame::new()
        .fill(Color32::from_rgba_unmultiplied(22, 24, 38, 208))
        .stroke(Stroke::new(1.0, glass(22)))
        .corner_radius(RADIUS)
        .shadow(Shadow {
            offset: [0, 8],
            blur: 28,
            spread: 0,
            color: Color32::from_black_alpha(110),
        })
        .inner_margin(Margin::same(8))
        .outer_margin(Margin::same(5))
}

/// A slimmer card for the menu, scene and status bars.
pub fn bar() -> Frame {
    card()
        .inner_margin(Margin::symmetric(10, 5))
        .outer_margin(Margin::symmetric(8, 3))
}

/// Soft colored glows behind the cards, painted once per frame under everything.
pub fn backdrop(ctx: &egui::Context) {
    let rect = ctx.content_rect();
    let painter = ctx
        .layer_painter(egui::LayerId::background())
        .with_clip_rect(rect);
    painter.rect_filled(rect, 0.0, BACKDROP);
    let glow = |center: Pos2, radius: f32, color: Color32| {
        const N: u32 = 32;
        let mut mesh = egui::epaint::Mesh::default();
        mesh.colored_vertex(center, color);
        for i in 0..N {
            let a = i as f32 / N as f32 * std::f32::consts::TAU;
            mesh.colored_vertex(center + Vec2::angled(a) * radius, Color32::TRANSPARENT);
        }
        for i in 0..N {
            mesh.add_triangle(0, 1 + i, 1 + (i + 1) % N);
        }
        painter.add(mesh);
    };
    let (w, h) = (rect.width(), rect.height());
    let violet = Color32::from_rgba_unmultiplied(110, 80, 230, 120);
    let blue = Color32::from_rgba_unmultiplied(40, 130, 230, 100);
    glow(rect.min + Vec2::new(w * 0.15, h * 0.05), w * 0.55, violet);
    glow(rect.min + Vec2::new(w * 0.9, h * 0.95), w * 0.6, blue);
}

/// Title strip shared by each workspace panel.
pub fn panel_title(ui: &mut egui::Ui, title: &str) {
    Frame::new()
        .fill(glass(10))
        .corner_radius(8)
        .inner_margin(Margin::symmetric(8, 5))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.label(RichText::new(title).font(bold(ui.ctx(), 12.5)));
        });
}

// ---- node graph kit (shader + blueprint canvases) ----

/// Modal window chrome: a denser, brighter card than the docked panels.
pub fn dialog_frame() -> Frame {
    card()
        .fill(Color32::from_rgba_unmultiplied(20, 22, 34, 246))
        .stroke(Stroke::new(1.0, glass(40)))
        .inner_margin(Margin::same(0))
        .outer_margin(Margin::ZERO)
}

/// Small-caps heading over an inset group, like a Unity inspector section.
pub fn section(ui: &mut egui::Ui, title: &str, add: impl FnOnce(&mut egui::Ui)) {
    ui.label(
        RichText::new(title.to_uppercase())
            .font(bold(ui.ctx(), 10.5))
            .color(ACCENT),
    );
    ui.add_space(3.0);
    Frame::new()
        .fill(glass(9))
        .stroke(Stroke::new(1.0, glass(14)))
        .corner_radius(8)
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
        .fill(tint.gamma_multiply(0.16))
        .stroke(Stroke::new(1.0, tint.gamma_multiply(0.5)))
        .corner_radius(20)
        .inner_margin(Margin::symmetric(9, 3))
        .show(ui, |ui| {
            ui.label(RichText::new(text).font(bold(ui.ctx(), 11.5)).color(tint))
        });
}

pub const GRAPH_HELP: &str = "Drag headers to move · Output → input to connect · Right-click input to disconnect · Middle-drag / scroll to pan · Ctrl+scroll to zoom";
const NODE_RADIUS: u8 = 10;
const GRID: f32 = 24.0;
const INK: Color32 = Color32::from_rgb(12, 13, 20);

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
                if major { 1.6 } else { 1.0 },
                glass(if major { 46 } else { 22 }),
            );
        }
    }
}

/// Node body: shadow, tinted halo, glass card, glowing header. Returns the header rect.
/// `outline` overrides the hairline and halo (selection, execution, recent).
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
            offset: [0, 8],
            blur: 20,
            spread: 0,
            color: Color32::from_black_alpha(130),
        }
        .as_shape(rect, r),
    );
    // Colored halo: the header tint at rest, the outline color when picked.
    let (halo, strength) = outline.map_or((tint, 0.30), |(_, c)| (c, 0.65));
    painter.add(
        Shadow {
            offset: [0, 0],
            blur: 32,
            spread: 3,
            color: halo.gamma_multiply(strength),
        }
        .as_shape(rect, r),
    );
    painter.rect_filled(rect, r, Color32::from_rgba_unmultiplied(26, 28, 42, 248));
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
        Color32::from_rgba_unmultiplied(red, green, blue, 72),
    );
    // Light catching the top edge, the tinted underline, and a status dot.
    let inset = Vec2::new(r as f32, 1.0);
    painter.line_segment(
        [
            header.left_top() + inset,
            header.right_top() + inset * Vec2::new(-1.0, 1.0),
        ],
        Stroke::new(1.0, glass(60)),
    );
    painter.line_segment(
        [header.left_bottom(), header.right_bottom()],
        Stroke::new(1.5, Color32::from_rgba_unmultiplied(red, green, blue, 170)),
    );
    let dot = header.left_center() + Vec2::new(14.0, 0.0);
    painter.circle_filled(dot, 7.0, tint.gamma_multiply(0.2));
    painter.circle_filled(dot, 3.5, tint);
    let (width, color) = outline.unwrap_or((1.0, glass(26)));
    painter.rect_stroke(rect, r, Stroke::new(width, color), egui::StrokeKind::Inside);
    header
}

pub fn node_title(painter: &egui::Painter, header: Rect, title: &str) {
    painter.text(
        header.left_center() + Vec2::new(28.0, 0.0),
        egui::Align2::LEFT_CENTER,
        title,
        bold(painter.ctx(), 13.0),
        Color32::WHITE,
    );
}

/// A pin: hollow ring when unlinked, glowing dot when linked (outputs are always filled).
pub fn pin_dot(painter: &egui::Painter, at: Pos2, tint: Color32, filled: bool) {
    if filled {
        painter.circle_filled(at, 11.0, tint.gamma_multiply(0.10));
        painter.circle_filled(at, 8.0, tint.gamma_multiply(0.22));
    }
    painter.circle(
        at,
        5.0,
        if filled { tint } else { INK },
        Stroke::new(1.6, tint),
    );
}

pub fn pin_label(painter: &egui::Painter, at: Pos2, label: &str, output: bool) {
    let (dx, align) = if output {
        (-13.0, egui::Align2::RIGHT_CENTER)
    } else {
        (13.0, egui::Align2::LEFT_CENTER)
    };
    painter.text(
        at + Vec2::new(dx, 0.0),
        align,
        label,
        FontId::proportional(11.5),
        Color32::from_gray(200),
    );
}

/// Wire that reads as a lit cable: wide halo, tight halo, colored core, hot center.
pub fn wire(painter: &egui::Painter, from: Pos2, to: Pos2, tint: Color32) {
    let offset = ((to.x - from.x).abs() * 0.5).max(45.0);
    let points = [
        from,
        from + Vec2::new(offset, 0.0),
        to - Vec2::new(offset, 0.0),
        to,
    ];
    let hot = tint.lerp_to_gamma(Color32::WHITE, 0.6);
    for (width, color) in [
        (13.0, tint.gamma_multiply(0.07)),
        (6.0, tint.gamma_multiply(0.18)),
        (2.6, tint),
        (1.0, hot.gamma_multiply(0.6)),
    ] {
        painter.add(egui::epaint::CubicBezierShape::from_points_stroke(
            points,
            false,
            Color32::TRANSPARENT,
            Stroke::new(width, color),
        ));
    }
}
