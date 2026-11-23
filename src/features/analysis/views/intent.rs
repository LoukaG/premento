//! Intent & Decisions tab: stated vs. inferred intent and the decision log.

use eframe::egui::{self, RichText, Sense, vec2};

use crate::features::analysis::Analysis;
use crate::ui::blocks::{labeled, list_card};
use crate::ui::theme::pal;
use crate::ui::widgets::{body, card, muted, pill, section_title};

pub fn intent(ui: &mut egui::Ui, a: &Analysis) {
    let p = pal(ui);
    let i = &a.intent;
    ui.columns(2, |cols| {
        card(&mut cols[0], |ui| {
            section_title(ui, "📝  What the PR says");
            body(ui, &i.stated_intent);
        });
        card(&mut cols[1], |ui| {
            section_title(ui, "🔬  What the code does");
            body(ui, &i.inferred_intent);
        });
    });
    card(ui, |ui| {
        section_title(ui, "⚖  Alignment");
        body(ui, &i.alignment);
    });

    ui.add_space(4.0);
    section_title(ui, &format!("Decision log · {}", i.decisions.len()));
    if i.decisions.is_empty() {
        ui.label(muted(ui, "No decisions recorded."));
    }
    for (n, d) in i.decisions.iter().enumerate() {
        ui.horizontal_top(|ui| {
            // Timeline marker
            let (rect, _) = ui.allocate_exact_size(vec2(26.0, 26.0), Sense::hover());
            ui.painter().circle_filled(rect.center(), 13.0, p.accent.gamma_multiply(0.2));
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                (n + 1).to_string(),
                egui::FontId::proportional(12.5),
                p.accent_text,
            );
            ui.vertical(|ui| {
                card(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::Label::new(RichText::new(&d.decision).size(15.0).strong().color(p.text))
                                .wrap(),
                        );
                    });
                    if !d.source.is_empty() {
                        pill(ui, &d.source, p.accent_text);
                    }
                    if !d.rationale.is_empty() {
                        labeled(ui, "Why", &d.rationale);
                    }
                    if !d.alternatives.is_empty() {
                        labeled(ui, "Alternatives", &d.alternatives);
                    }
                });
            });
        });
    }

    ui.add_space(4.0);
    list_card(ui, "❓", "Open questions", &i.open_questions, p.accent_text);
}
