//! Impact tab: the generic impact analysis, for PRs outside the service map.

use eframe::egui::{self, Align, Color32, Layout, RichText, vec2};

use crate::features::analysis::Analysis;
use crate::ui::blocks::list_card;
use crate::ui::theme::{pal, severity_color};
use crate::ui::widgets::{body, card, muted, section_title};

pub fn impact(ui: &mut egui::Ui, a: &Analysis) {
    let p = pal(ui);
    let i = &a.impact;

    card(ui, |ui| {
        section_title(ui, &format!("🗺  Affected components · {}", i.affected_components.len()));
        if i.affected_components.is_empty() {
            ui.label(muted(ui, "None identified."));
        }
        for c in &i.affected_components {
            ui.horizontal_top(|ui| {
                ui.allocate_ui_with_layout(
                    vec2(180.0, 20.0),
                    Layout::left_to_right(Align::Min),
                    |ui| {
                        ui.set_min_width(180.0);
                        ui.add(egui::Label::new(RichText::new(&c.name).strong().color(p.text)).truncate())
                    },
                );
                ui.add(egui::Label::new(RichText::new(&c.description).color(p.muted)).wrap());
            });
        }
    });

    let groups: [(&str, &str, &Vec<String>, Color32); 5] = [
        ("⚠", "Breaking changes", &i.breaking_changes, severity_color("critical")),
        ("🔗", "Downstream effects", &i.downstream_effects, Color32::from_rgb(56, 189, 248)),
        ("💾", "Data & migrations", &i.data_and_migrations, Color32::from_rgb(168, 85, 247)),
        ("🔒", "Security", &i.security, severity_color("high")),
        ("⚡", "Performance", &i.performance, severity_color("medium")),
    ];
    ui.columns(2, |cols| {
        for (n, (icon, title, items, color)) in groups.iter().enumerate() {
            list_card(&mut cols[n % 2], icon, title, items, *color);
            cols[n % 2].add_space(2.0);
        }
        card(&mut cols[1], |ui| {
            section_title(ui, "✅  Test coverage");
            body(ui, &i.test_coverage);
        });
    });
    list_card(ui, "🔬", "Suggested tests", &i.suggested_tests, severity_color("low"));
}
