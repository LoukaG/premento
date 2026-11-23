//! Shown in place of the tabs before a PR has been analyzed.

use eframe::egui::{self, RichText};

use crate::ui::theme::pal;
use crate::ui::widgets::card;

pub fn intro(ui: &mut egui::Ui) {
    let p = pal(ui);
    ui.vertical_centered(|ui| {
        ui.add_space(20.0);
        ui.label(RichText::new("✨").size(32.0).color(p.accent_text));
        ui.label(RichText::new("Get an AI review of this pull request").size(18.0).strong().color(p.text));
        ui.label(RichText::new("Choose a model and click Analyze PR. Your local CLI reads the diff, commits and discussion.").color(p.muted));
    });
    ui.add_space(16.0);
    let items = [
        ("🛡", "Risk Review", "How risky is it, where, and how deep to review"),
        ("📝", "Intent & Decisions", "What it tries to do and the choices it makes"),
        ("🗺", "Impact", "What else in the system is affected"),
    ];
    ui.columns(3, |cols| {
        for (col, (icon, title, text)) in cols.iter_mut().zip(items) {
            card(col, |ui| {
                ui.label(RichText::new(icon).size(20.0).color(p.accent_text));
                ui.label(RichText::new(title).strong().color(p.text));
                ui.add(egui::Label::new(RichText::new(text).size(12.5).color(p.muted)).wrap());
            });
        }
    });
}
