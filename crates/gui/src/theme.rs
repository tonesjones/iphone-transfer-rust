use eframe::egui::{self, Color32, FontFamily, FontId, Painter, Pos2, Rect, Stroke, Vec2, epaint};
use std::f32::consts::TAU;

pub const BACKGROUND: Color32 = Color32::from_rgb(11, 14, 22);
pub const SIDEBAR: Color32 = Color32::from_rgb(16, 20, 31);
pub const CARD: Color32 = Color32::from_rgb(22, 28, 42);
pub const CARD_HOVER: Color32 = Color32::from_rgb(28, 36, 54);
pub const LINE: Color32 = Color32::from_rgb(38, 47, 68);
pub const TEXT: Color32 = Color32::from_rgb(234, 238, 246);
pub const MUTED: Color32 = Color32::from_rgb(140, 151, 172);
pub const FAINT: Color32 = Color32::from_rgb(92, 103, 126);
pub const CYAN: Color32 = Color32::from_rgb(56, 214, 200);
pub const VIOLET: Color32 = Color32::from_rgb(132, 108, 246);
pub const AMBER: Color32 = Color32::from_rgb(246, 189, 82);
pub const SUCCESS: Color32 = Color32::from_rgb(52, 211, 153);
pub const ERROR: Color32 = Color32::from_rgb(248, 113, 113);

pub fn semibold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("semibold".into()))
}

pub fn bold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("bold".into()))
}

/// Uses Windows' own Segoe UI when present, falling back to egui's bundled fonts.
fn install_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    let windows = std::path::PathBuf::from(
        std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into()),
    );
    let mut load = |name: &str, file: &str| {
        let bytes = std::fs::read(windows.join("Fonts").join(file)).ok()?;
        fonts
            .font_data
            .insert(name.into(), egui::FontData::from_owned(bytes).into());
        Some(name.to_string())
    };
    let regular = load("Segoe UI", "segoeui.ttf");
    let semibold = load("Segoe UI Semibold", "seguisb.ttf");
    let bold = load("Segoe UI Bold", "segoeuib.ttf");
    let base = fonts.families.entry(FontFamily::Proportional).or_default();
    if let Some(regular) = regular {
        base.insert(0, regular);
    }
    let base = base.clone();
    for (family, font) in [("semibold", semibold), ("bold", bold)] {
        let mut list = base.clone();
        list.splice(0..0, font);
        fonts.families.insert(FontFamily::Name(family.into()), list);
    }
    ctx.set_fonts(fonts);
}

pub fn install(ctx: &egui::Context) {
    install_fonts(ctx);
    ctx.set_theme(egui::Theme::Dark);
    ctx.global_style_mut(|style| {
        let visuals = &mut style.visuals;
        visuals.panel_fill = BACKGROUND;
        visuals.window_fill = CARD;
        visuals.override_text_color = Some(TEXT);
        visuals.selection.bg_fill = VIOLET.gamma_multiply(0.45);
        visuals.selection.stroke = Stroke::new(1.0, TEXT);
        visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, LINE);
        visuals.widgets.inactive.weak_bg_fill = CARD;
        visuals.widgets.hovered.weak_bg_fill = CARD_HOVER;
        visuals.widgets.active.weak_bg_fill = CARD_HOVER;
        style.spacing.item_spacing = egui::vec2(10.0, 6.0);
        style.spacing.scroll.floating = true;
        for (text_style, size) in [
            (egui::TextStyle::Body, 15.0),
            (egui::TextStyle::Button, 15.0),
            (egui::TextStyle::Small, 12.5),
            (egui::TextStyle::Monospace, 13.0),
        ] {
            let family = if text_style == egui::TextStyle::Monospace {
                FontFamily::Monospace
            } else {
                FontFamily::Proportional
            };
            style
                .text_styles
                .insert(text_style, FontId::new(size, family));
        }
    });
}

/// Fills a convex outline with a diagonal gradient from `from` (top left) to `to` (bottom right).
fn gradient_fill(painter: &Painter, outline: &[Pos2], from: Color32, to: Color32) {
    let bounds = Rect::from_points(outline);
    let diagonal = bounds.max - bounds.min;
    let color_at = |point: Pos2| {
        let t = (point - bounds.min).dot(diagonal) / diagonal.length_sq();
        from.lerp_to_gamma(to, t.clamp(0.0, 1.0))
    };
    let mut mesh = epaint::Mesh::default();
    mesh.colored_vertex(bounds.center(), color_at(bounds.center()));
    let count = outline.len() as u32;
    for (i, &point) in outline.iter().enumerate() {
        mesh.colored_vertex(point, color_at(point));
        let i = i as u32;
        mesh.add_triangle(0, 1 + i, 1 + (i + 1) % count);
    }
    painter.add(mesh);
}

pub fn gradient_pill(painter: &Painter, rect: Rect, from: Color32, to: Color32) {
    gradient_fill(
        painter,
        &rounded_outline(rect, rect.height() / 2.0),
        from,
        to,
    );
}

fn rounded_outline(rect: Rect, radius: f32) -> Vec<Pos2> {
    let corners = [
        (rect.right_bottom() - Vec2::splat(radius), 0.0),
        (rect.left_bottom() + Vec2::new(radius, -radius), 0.25),
        (rect.left_top() + Vec2::splat(radius), 0.5),
        (rect.right_top() + Vec2::new(-radius, radius), 0.75),
    ];
    corners
        .into_iter()
        .flat_map(|(center, turn)| {
            (0..=8).map(move |i| center + Vec2::angled((turn + i as f32 / 32.0) * TAU) * radius)
        })
        .collect()
}

/// A soft halo: stacked translucent circles that fade out past `radius`.
pub fn glow(
    painter: &Painter,
    center: Pos2,
    radius: f32,
    reach: f32,
    color: Color32,
    strength: f32,
) {
    const RINGS: usize = 14;
    for i in (0..RINGS).rev() {
        let t = i as f32 / RINGS as f32;
        let alpha = strength * 0.11 * (1.0 - t).powi(2);
        painter.circle_filled(center, radius + reach * t, color.gamma_multiply(alpha));
    }
}

pub fn check_mark(painter: &Painter, center: Pos2, size: f32, stroke: Stroke) {
    let points = vec![
        center + Vec2::new(-0.45, 0.02) * size,
        center + Vec2::new(-0.12, 0.34) * size,
        center + Vec2::new(0.48, -0.32) * size,
    ];
    painter.line(points, stroke);
}

pub fn folder_icon(painter: &Painter, rect: Rect, color: Color32) {
    let body = Rect::from_min_max(
        rect.left_top() + Vec2::new(0.0, rect.height() * 0.22),
        rect.right_bottom(),
    );
    let tab = Rect::from_min_size(
        rect.left_top(),
        Vec2::new(rect.width() * 0.45, rect.height() * 0.4),
    );
    painter.rect_filled(tab, 2.0, color.gamma_multiply(0.7));
    painter.rect_filled(body, 3.0, color);
}

pub fn cloud_icon(painter: &Painter, rect: Rect, color: Color32) {
    let (w, h) = (rect.width(), rect.height());
    let at = |x: f32, y: f32| rect.left_top() + Vec2::new(x * w, y * h);
    painter.circle_filled(at(0.3, 0.6), 0.22 * w, color);
    painter.circle_filled(at(0.55, 0.42), 0.3 * w, color);
    painter.circle_filled(at(0.78, 0.66), 0.2 * w, color);
    let base = Rect::from_min_max(at(0.1, 0.6), at(0.95, 0.95));
    painter.rect_filled(base, 0.17 * w, color);
}

/// The app mark: a gradient tile with a camera-lens ring.
pub fn logo(painter: &Painter, rect: Rect) {
    let center = rect.center();
    let radius = rect.width() / 2.0;
    glow(painter, center, radius * 0.8, radius * 0.9, VIOLET, 0.6);
    gradient_fill(painter, &rounded_outline(rect, radius * 0.5), CYAN, VIOLET);
    painter.circle_stroke(center, radius * 0.48, Stroke::new(2.4, Color32::WHITE));
    painter.circle_filled(center, radius * 0.17, Color32::WHITE);
    painter.circle_filled(
        center + Vec2::new(radius * 0.52, -radius * 0.52),
        radius * 0.09,
        Color32::WHITE,
    );
}
