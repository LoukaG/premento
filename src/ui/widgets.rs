//! Reusable widgets of the design system: pills, cards, buttons, tabs, meters, avatars, text.

use eframe::egui::{
    self, Align2, Color32, CornerRadius, FontId, Margin, Rect, Response, RichText,
    Sense, Stroke, Ui, Vec2, pos2, vec2,
};

use super::theme::pal;

fn paint_pill(ui: &mut Ui, text: String, fg: Color32, bg: Color32, size: f32) -> Response {
    let galley = ui
        .painter()
        .layout_no_wrap(text, FontId::proportional(size), fg);
    let pad = vec2(8.0, 2.5);
    let (rect, resp) = ui.allocate_exact_size(galley.size() + pad * 2.0, Sense::hover());
    ui.painter()
        .rect_filled(rect, CornerRadius::same(255), bg);
    ui.painter().galley(rect.min + pad, galley, fg);
    resp
}

/// Soft tinted pill: translucent background with colored text.
pub fn pill(ui: &mut Ui, text: impl Into<String>, color: Color32) -> Response {
    let dark = ui.visuals().dark_mode;
    let fg = if dark {
        color
    } else {
        color.lerp_to_gamma(Color32::BLACK, 0.3)
    };
    let bg = color.gamma_multiply(if dark { 0.18 } else { 0.14 });
    paint_pill(ui, text.into(), fg, bg, 11.5)
}

/// Solid pill with white text, for strong signals like risk level.
pub fn solid_pill(ui: &mut Ui, text: impl Into<String>, color: Color32) -> Response {
    paint_pill(ui, text.into(), Color32::WHITE, color, 11.0)
}

/// Plain container card.
pub fn card<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    let p = pal(ui);
    egui::Frame::new()
        .fill(p.surface)
        .stroke(Stroke::new(1.0, p.border))
        .corner_radius(CornerRadius::same(12))
        .inner_margin(Margin::same(16))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            // Never inherit a justified layout (e.g. from `ui.columns`): it stretches text.
            ui.with_layout(egui::Layout::top_down(egui::Align::Min), add)
                .inner
        })
        .inner
}

/// Card with a colored stripe on its left edge.
pub fn accent_card(ui: &mut Ui, stripe: Color32, add: impl FnOnce(&mut Ui)) {
    let p = pal(ui);
    let resp = egui::Frame::new()
        .fill(p.surface)
        .stroke(Stroke::new(1.0, p.border))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(Margin {
            left: 18,
            right: 14,
            top: 12,
            bottom: 12,
        })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.with_layout(egui::Layout::top_down(egui::Align::Min), add);
        })
        .response;
    paint_stripe(ui, resp.rect, stripe);
}

pub fn paint_stripe(ui: &Ui, rect: Rect, color: Color32) {
    let stripe = Rect::from_min_max(
        pos2(rect.left() + 6.0, rect.top() + 10.0),
        pos2(rect.left() + 9.0, rect.bottom() - 10.0),
    );
    ui.painter().rect_filled(stripe, CornerRadius::same(2), color);
}

/// A card that reacts to hover and clicks.
pub fn clickable_card(
    ui: &mut Ui,
    id: egui::Id,
    selected: bool,
    add: impl FnOnce(&mut Ui),
) -> Response {
    let p = pal(ui);
    let mut prepared = egui::Frame::new()
        .corner_radius(CornerRadius::same(10))
        .inner_margin(Margin {
            left: 18,
            right: 12,
            top: 10,
            bottom: 10,
        })
        .begin(ui);
    prepared.content_ui.set_width(prepared.content_ui.available_width());
    add(&mut prepared.content_ui);
    let rect = prepared.allocate_space(ui).rect;
    let resp = ui.interact(rect, id, Sense::click());

    prepared.frame.fill = if selected {
        p.accent.gamma_multiply(if ui.visuals().dark_mode { 0.16 } else { 0.08 })
    } else if resp.hovered() {
        p.surface_hover
    } else {
        Color32::TRANSPARENT
    };
    prepared.frame.stroke = if selected {
        Stroke::new(1.0, p.accent.gamma_multiply(0.7))
    } else {
        Stroke::NONE
    };
    prepared.paint(ui);
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

pub fn primary_button(ui: &mut Ui, text: &str, enabled: bool) -> Response {
    let p = pal(ui);
    let fill = if enabled { p.accent } else { p.accent.gamma_multiply(0.35) };
    ui.add_enabled(
        enabled,
        egui::Button::new(RichText::new(text).strong().color(Color32::WHITE))
            .fill(fill)
            .stroke(Stroke::NONE)
            .corner_radius(CornerRadius::same(8))
            .min_size(vec2(0.0, 34.0)),
    )
}

pub fn ghost_button(ui: &mut Ui, text: impl Into<RichText>) -> Response {
    let p = pal(ui);
    ui.add(
        egui::Button::new(text.into().color(p.text))
            .fill(Color32::TRANSPARENT)
            .stroke(Stroke::new(1.0, p.border))
            .corner_radius(CornerRadius::same(8))
            .min_size(vec2(0.0, 30.0)),
    )
}

/// Segmented control.
pub fn segmented<T: PartialEq + Copy>(ui: &mut Ui, value: &mut T, options: &[(T, &str)]) {
    let p = pal(ui);
    egui::Frame::new()
        .fill(p.surface_sunken)
        .stroke(Stroke::new(1.0, p.border))
        .corner_radius(CornerRadius::same(9))
        .inner_margin(Margin::same(3))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            ui.horizontal(|ui| {
                let w = (ui.available_width() - 2.0 * (options.len() as f32 - 1.0))
                    / options.len() as f32;
                for (opt, label) in options {
                    let selected = *value == *opt;
                    let text = RichText::new(*label).size(13.0).color(if selected {
                        p.text
                    } else {
                        p.muted
                    });
                    let btn = egui::Button::new(if selected { text.strong() } else { text })
                        .fill(if selected { p.surface_hover } else { Color32::TRANSPARENT })
                        .stroke(if selected {
                            Stroke::new(1.0, p.border)
                        } else {
                            Stroke::NONE
                        })
                        .corner_radius(CornerRadius::same(7))
                        .min_size(vec2(w, 26.0));
                    if ui.add(btn).clicked() {
                        *value = *opt;
                    }
                }
            });
        });
}

/// Underlined tab bar.
pub fn tabs<T: PartialEq + Copy>(ui: &mut Ui, value: &mut T, options: &[(T, &str)]) {
    let p = pal(ui);
    let bar = ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 22.0;
        for (opt, label) in options {
            let selected = *value == *opt;
            let text = RichText::new(*label).size(14.0).color(if selected {
                p.text
            } else {
                p.muted
            });
            let resp = ui.add(
                egui::Label::new(if selected { text.strong() } else { text })
                    .sense(Sense::click())
                    .selectable(false),
            );
            let y = resp.rect.bottom() + 7.0;
            if selected {
                ui.painter().line_segment(
                    [pos2(resp.rect.left(), y), pos2(resp.rect.right(), y)],
                    Stroke::new(2.0, p.accent),
                );
            } else if resp.hovered() {
                ui.painter().line_segment(
                    [pos2(resp.rect.left(), y), pos2(resp.rect.right(), y)],
                    Stroke::new(2.0, p.border),
                );
            }
            if resp.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            if resp.clicked() {
                *value = *opt;
            }
        }
    });
    let y = bar.response.rect.bottom() + 8.0;
    let full = ui.max_rect();
    ui.painter().line_segment(
        [pos2(full.left(), y), pos2(full.right(), y)],
        Stroke::new(1.0, p.border),
    );
    ui.add_space(14.0);
}

/// Horizontal meter filled to `fraction`.
pub fn meter(ui: &mut Ui, fraction: f32, color: Color32, width: f32) {
    let p = pal(ui);
    let (rect, _) = ui.allocate_exact_size(vec2(width, 6.0), Sense::hover());
    ui.painter()
        .rect_filled(rect, CornerRadius::same(3), p.surface_sunken);
    let mut fill = rect;
    fill.set_width(rect.width() * fraction.clamp(0.0, 1.0));
    if fill.width() > 0.0 {
        ui.painter().rect_filled(fill, CornerRadius::same(3), color);
    }
}

/// Small circle with a person's initial.
pub fn avatar(ui: &mut Ui, name: &str, size: f32) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    let hue = name.bytes().fold(0u32, |h, b| h.wrapping_mul(31).wrapping_add(b as u32));
    let palette = [
        Color32::from_rgb(124, 108, 242),
        Color32::from_rgb(56, 189, 248),
        Color32::from_rgb(52, 211, 153),
        Color32::from_rgb(251, 146, 60),
        Color32::from_rgb(244, 114, 182),
        Color32::from_rgb(250, 204, 21),
    ];
    let color = palette[(hue as usize) % palette.len()];
    ui.painter()
        .circle_filled(rect.center(), size / 2.0, color.gamma_multiply(0.85));
    let initial = name.chars().next().unwrap_or('?').to_ascii_uppercase();
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        initial,
        FontId::proportional(size * 0.5),
        Color32::WHITE,
    );
    // The GitHub profile picture covers the initial once it has loaded; until then, or if it
    // can't be fetched (offline, bot accounts), the initial stays visible.
    let px = (size * ui.ctx().pixels_per_point()).ceil() as u32;
    let uri = format!("https://avatars.githubusercontent.com/{name}?s={}", px.max(32) * 2);
    if let Ok(egui::load::TexturePoll::Ready { texture }) = ui.ctx().try_load_texture(
        &uri,
        egui::TextureOptions::LINEAR,
        egui::SizeHint::Size { width: px, height: px, maintain_aspect_ratio: true },
    ) {
        egui::Image::from_texture(texture)
            .corner_radius(CornerRadius::same((size / 2.0).round() as u8))
            .paint_at(ui, rect);
    }
    resp.on_hover_text(name)
}

pub fn section_title(ui: &mut Ui, title: &str) {
    let p = pal(ui);
    ui.label(
        RichText::new(title.to_uppercase())
            .size(11.5)
            .strong()
            .color(p.muted)
            .extra_letter_spacing(0.6),
    );
    ui.add_space(2.0);
}

/// Body paragraph; an em dash when empty.
pub fn body(ui: &mut Ui, text: &str) {
    let p = pal(ui);
    if text.trim().is_empty() {
        ui.label(RichText::new("—").color(p.faint));
    } else {
        ui.add(egui::Label::new(RichText::new(text).color(p.text).line_height(Some(21.0))).wrap());
    }
}

pub fn muted(ui: &Ui, text: impl Into<String>) -> RichText {
    RichText::new(text.into()).color(pal(ui).muted)
}
