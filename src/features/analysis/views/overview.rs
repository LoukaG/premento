//! Overview tab: headline numbers, why/what/impact and the riskiest areas.

use eframe::egui::{self, Align, Layout, RichText, vec2};

use crate::features::analysis::Analysis;
use crate::features::service_map;
use crate::ui::blocks::{big_number, list_card, stat_tile};
use crate::ui::theme::{pal, severity_color, severity_rank};
use crate::ui::widgets::{body, card, meter, muted, section_title, solid_pill};

pub fn overview(ui: &mut egui::Ui, a: &Analysis, repo: &str) {
    let p = pal(ui);
    let services = service_map::applies(repo).then(|| service_map::resolve_impact(repo, &a.service_impact).0);
    // High and critical risk factors, most severe first.
    let mut serious: Vec<_> = a.risk.factors.iter().filter(|f| severity_rank(&f.severity) >= 3).collect();
    serious.sort_by_key(|f| std::cmp::Reverse(severity_rank(&f.severity)));

    ui.columns(4, |cols| {
        stat_tile(&mut cols[0], "Risk", |ui| {
            let c = severity_color(&a.risk.level);
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("{:.1}", a.risk.score)).size(30.0).strong().color(c));
                ui.label(RichText::new("/10").size(15.0).color(p.muted));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    solid_pill(ui, a.risk.level.to_uppercase(), c);
                });
            });
            meter(ui, a.risk.score / 10.0, c, ui.available_width());
        });
        stat_tile(&mut cols[1], "Serious risks", |ui| {
            big_number(ui, serious.len(), a.risk.factors.len(), "factors");
        });
        if let Some(services) = &services {
            let direct = services.iter().filter(|i| i.level.eq_ignore_ascii_case("direct")).count();
            stat_tile(&mut cols[2], "Services impacted", |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(services.len().to_string()).size(30.0).strong().color(p.text));
                    ui.label(RichText::new(format!("{direct} direct")).size(14.0).color(p.muted));
                });
            });
        } else {
            let breaking = a.impact.breaking_changes.len();
            stat_tile(&mut cols[2], "Breaking changes", |ui| {
                let c = if breaking > 0 { severity_color("high") } else { p.text };
                ui.label(RichText::new(breaking.to_string()).size(30.0).strong().color(c));
            });
        }
        stat_tile(&mut cols[3], "Open questions", |ui| {
            ui.label(
                RichText::new(a.intent.open_questions.len().to_string())
                    .size(30.0)
                    .strong()
                    .color(p.text),
            );
        });
    });

    card(ui, |ui| {
        section_title(ui, "Summary");
        if a.brief.is_empty() {
            body(ui, &a.summary);
        } else {
            for (label, text) in [("Why", &a.brief.why), ("What", &a.brief.what), ("Impact", &a.brief.impact)] {
                if !text.is_empty() {
                    ui.horizontal_top(|ui| {
                        ui.allocate_ui_with_layout(vec2(56.0, 20.0), Layout::left_to_right(Align::Center), |ui| {
                            ui.set_min_width(56.0);
                            ui.label(RichText::new(label).size(12.0).strong().color(p.muted));
                        });
                        ui.add(egui::Label::new(RichText::new(text).color(p.text).line_height(Some(20.0))).wrap());
                    });
                }
            }
        }
    });

    card(ui, |ui| {
        section_title(ui, "Look here first");
        if serious.is_empty() {
            ui.label(muted(ui, "No high or critical risks identified."));
        }
        for f in serious {
            ui.horizontal_top(|ui| {
                ui.allocate_ui_with_layout(
                    vec2(76.0, 20.0),
                    Layout::left_to_right(Align::Center),
                    |ui| {
                        ui.set_min_width(76.0);
                        solid_pill(ui, f.severity.to_uppercase(), severity_color(&f.severity))
                    },
                );
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    ui.label(RichText::new(&f.area).strong().color(p.text));
                    if !f.files.is_empty() {
                        ui.label(RichText::new(f.files.join(", ")).monospace().size(12.0).color(p.muted));
                    }
                });
            });
            ui.add_space(4.0);
        }
    });

    ui.columns(2, |cols| {
        match &services {
            Some(services) => {
                let names: Vec<String> = services
                    .iter()
                    .filter_map(|i| {
                        service_map::node(&i.service).map(|n| format!("{} ({}): {}", n.name, i.level, i.reason))
                    })
                    .collect();
                list_card(&mut cols[0], "🗺", "Impacted services", &names, p.accent_text);
            }
            None => list_card(&mut cols[0], "⚠", "Breaking changes", &a.impact.breaking_changes, severity_color("high")),
        }
        list_card(&mut cols[1], "❓", "Questions for the author", &a.intent.open_questions, p.accent_text);
    });
}
