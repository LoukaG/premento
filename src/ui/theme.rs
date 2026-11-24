//! Visual design system: palettes, typography, spacing and semantic colors.

use eframe::egui::{
    self, Color32, CornerRadius, FontFamily, FontId, Margin, Shadow, Stroke, TextStyle, Theme, Ui, vec2,
};

#[derive(Clone, Copy)]
pub struct Palette {
    pub bg: Color32,
    pub sidebar: Color32,
    pub surface: Color32,
    pub surface_hover: Color32,
    pub surface_sunken: Color32,
    pub border: Color32,
    pub text: Color32,
    pub muted: Color32,
    pub faint: Color32,
    pub accent: Color32,
    pub accent_text: Color32,
    pub danger: Color32,
}

pub const DARK: Palette = Palette {
    bg: Color32::from_rgb(13, 15, 19),
    sidebar: Color32::from_rgb(17, 19, 24),
    surface: Color32::from_rgb(24, 27, 34),
    surface_hover: Color32::from_rgb(31, 35, 44),
    surface_sunken: Color32::from_rgb(19, 22, 28),
    border: Color32::from_rgb(38, 43, 54),
    text: Color32::from_rgb(230, 232, 237),
    muted: Color32::from_rgb(142, 150, 165),
    faint: Color32::from_rgb(92, 100, 115),
    accent: Color32::from_rgb(124, 108, 242),
    accent_text: Color32::from_rgb(166, 155, 255),
    danger: Color32::from_rgb(239, 90, 90),
};

pub const LIGHT: Palette = Palette {
    bg: Color32::from_rgb(246, 247, 250),
    sidebar: Color32::from_rgb(255, 255, 255),
    surface: Color32::from_rgb(255, 255, 255),
    surface_hover: Color32::from_rgb(243, 244, 248),
    surface_sunken: Color32::from_rgb(240, 241, 245),
    border: Color32::from_rgb(226, 229, 236),
    text: Color32::from_rgb(17, 19, 26),
    muted: Color32::from_rgb(98, 106, 122),
    faint: Color32::from_rgb(150, 156, 170),
    accent: Color32::from_rgb(98, 78, 230),
    accent_text: Color32::from_rgb(88, 66, 220),
    danger: Color32::from_rgb(210, 50, 50),
};

pub fn pal(ui: &Ui) -> Palette {
    if ui.visuals().dark_mode { DARK } else { LIGHT }
}

pub fn install(ctx: &egui::Context) {
    ctx.set_style_of(Theme::Dark, style(Theme::Dark));
    ctx.set_style_of(Theme::Light, style(Theme::Light));
}

fn style(theme: Theme) -> egui::Style {
    let (p, mut visuals) = match theme {
        Theme::Dark => (DARK, egui::Visuals::dark()),
        Theme::Light => (LIGHT, egui::Visuals::light()),
    };

    let r8 = CornerRadius::same(8);
    visuals.panel_fill = p.bg;
    visuals.window_fill = p.surface;
    visuals.extreme_bg_color = p.surface_sunken;
    visuals.faint_bg_color = p.surface_hover;
    visuals.code_bg_color = p.surface_sunken;
    visuals.hyperlink_color = p.accent_text;
    visuals.window_corner_radius = CornerRadius::same(12);
    visuals.menu_corner_radius = CornerRadius::same(10);
    visuals.window_stroke = Stroke::new(1.0, p.border);
    let shadow = Shadow {
        offset: [0, 8],
        blur: 24,
        spread: 0,
        color: Color32::from_black_alpha(if theme == Theme::Dark { 120 } else { 40 }),
    };
    visuals.window_shadow = shadow;
    visuals.popup_shadow = shadow;
    visuals.indent_has_left_vline = false;
    visuals.selection.bg_fill = p.accent.gamma_multiply(0.35);
    visuals.selection.stroke = Stroke::new(1.0, p.accent_text);

    let w = &mut visuals.widgets;
    w.noninteractive.bg_fill = p.surface;
    w.noninteractive.weak_bg_fill = p.surface;
    w.noninteractive.bg_stroke = Stroke::new(1.0, p.border);
    w.noninteractive.fg_stroke = Stroke::new(1.0, p.text);
    w.noninteractive.corner_radius = r8;

    w.inactive.bg_fill = p.surface_hover;
    w.inactive.weak_bg_fill = p.surface_hover;
    w.inactive.bg_stroke = Stroke::new(1.0, p.border);
    w.inactive.fg_stroke = Stroke::new(1.0, p.text);
    w.inactive.corner_radius = r8;

    w.hovered.bg_fill = p.surface_hover;
    w.hovered.weak_bg_fill = p.surface_hover;
    w.hovered.bg_stroke = Stroke::new(1.0, p.accent.gamma_multiply(0.6));
    w.hovered.fg_stroke = Stroke::new(1.0, p.text);
    w.hovered.corner_radius = r8;
    w.hovered.expansion = 0.0;

    w.active.bg_fill = p.accent.gamma_multiply(0.25);
    w.active.weak_bg_fill = p.accent.gamma_multiply(0.25);
    w.active.bg_stroke = Stroke::new(1.0, p.accent);
    w.active.fg_stroke = Stroke::new(1.0, p.text);
    w.active.corner_radius = r8;
    w.active.expansion = 0.0;

    w.open = w.active;

    let mut style = egui::Style {
        visuals,
        ..Default::default()
    };
    style.spacing.item_spacing = vec2(8.0, 8.0);
    style.spacing.button_padding = vec2(12.0, 6.0);
    style.spacing.interact_size.y = 30.0;
    style.spacing.menu_margin = Margin::same(8);
    style.spacing.window_margin = Margin::same(12);
    style.spacing.scroll.bar_width = 6.0;
    style.spacing.scroll.floating = true;

    use FontFamily::{Monospace, Proportional};
    style.text_styles = [
        (TextStyle::Small, FontId::new(11.5, Proportional)),
        (TextStyle::Body, FontId::new(14.0, Proportional)),
        (TextStyle::Button, FontId::new(14.0, Proportional)),
        (TextStyle::Heading, FontId::new(22.0, Proportional)),
        (TextStyle::Monospace, FontId::new(12.5, Monospace)),
    ]
    .into();
    style
}

// ---------------------------------------------------------------- semantic colors

/// An analysis made at an older commit than the PR's head.
pub const OUTDATED: Color32 = Color32::from_rgb(245, 158, 11);

pub fn severity_rank(s: &str) -> u8 {
    match s.trim().to_lowercase().as_str() {
        "critical" => 4,
        "high" => 3,
        "medium" => 2,
        "low" => 1,
        _ => 0,
    }
}

pub fn severity_color(s: &str) -> Color32 {
    match severity_rank(s) {
        4 => Color32::from_rgb(239, 68, 68),
        3 => Color32::from_rgb(249, 115, 22),
        2 => Color32::from_rgb(234, 179, 8),
        1 => Color32::from_rgb(34, 197, 94),
        _ => Color32::from_rgb(130, 138, 152),
    }
}
