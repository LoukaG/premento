//! Larger building blocks composed from the widgets: stat tiles, list cards, banners.

use eframe::egui::{
    self, Align, Color32, CornerRadius, Layout, Margin, RichText, Sense, Stroke, pos2, vec2,
};

use super::theme::pal;
use super::widgets::{body, card, pill, section_title};

pub fn labeled(ui: &mut egui::Ui, label: &str, text: &str) {
    let p = pal(ui);
    ui.label(RichText::new(label).size(12.0).strong().color(p.muted));
    body(ui, text);
}

pub fn stat_tile(ui: &mut egui::Ui, title: &str, add: impl FnOnce(&mut egui::Ui)) {
    card(ui, |ui| {
        ui.set_min_height(72.0);
        section_title(ui, title);
        add(ui);
    });
}

pub fn big_number(ui: &mut egui::Ui, n: usize, total: usize, unit: &str) {
    let p = pal(ui);
    ui.horizontal(|ui| {
        ui.label(RichText::new(n.to_string()).size(30.0).strong().color(p.text));
        ui.label(RichText::new(format!("/ {total} {unit}")).size(14.0).color(p.muted));
    });
}

pub fn list_card(ui: &mut egui::Ui, icon: &str, title: &str, items: &[String], color: Color32) {
    let p = pal(ui);
    card(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new(icon).color(color));
            section_title(ui, title);
            if !items.is_empty() {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    pill(ui, items.len().to_string(), color);
                });
            }
        });
        if items.is_empty() {
            ui.label(RichText::new("None identified").color(p.faint));
        }
        for item in items {
            ui.horizontal_top(|ui| {
                let (rect, _) = ui.allocate_exact_size(vec2(6.0, 20.0), Sense::hover());
                ui.painter().circle_filled(pos2(rect.center().x, rect.top() + 10.0), 2.5, color);
                ui.add(egui::Label::new(RichText::new(item).color(p.text).line_height(Some(20.0))).wrap());
            });
        }
    });
}

pub fn code_chip(ui: &mut egui::Ui, text: &str) {
    let p = pal(ui);
    egui::Frame::new()
        .fill(p.surface_sunken)
        .stroke(Stroke::new(1.0, p.border))
        .corner_radius(CornerRadius::same(6))
        .inner_margin(Margin::symmetric(6, 2))
        .show(ui, |ui| {
            ui.label(RichText::new(text).monospace().size(11.5).color(p.muted));
        });
}

pub fn indeterminate_bar(ui: &mut egui::Ui) {
    let p = pal(ui);
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 3.0), Sense::hover());
    ui.painter().rect_filled(rect, CornerRadius::same(2), p.surface_sunken);
    let t = ui.input(|i| i.time) as f32;
    let w = rect.width() * 0.3;
    let x = rect.left() + ((t * 0.7).fract() * 1.3 - 0.3) * rect.width();
    let seg = egui::Rect::from_min_size(pos2(x, rect.top()), vec2(w, rect.height()));
    ui.painter()
        .with_clip_rect(rect)
        .rect_filled(seg, CornerRadius::same(2), p.accent);
    ui.ctx().request_repaint();
}

pub fn banner(ui: &mut egui::Ui, text: &str) {
    let p = pal(ui);
    egui::Frame::new()
        .fill(p.danger.gamma_multiply(0.12))
        .stroke(Stroke::new(1.0, p.danger.gamma_multiply(0.4)))
        .corner_radius(CornerRadius::same(8))
        .inner_margin(Margin::symmetric(12, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_top(|ui| {
                ui.label(RichText::new("⚠").color(p.danger));
                ui.add(egui::Label::new(RichText::new(text).color(p.danger)).wrap());
            });
        });
}
