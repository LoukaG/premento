//! Risk Review tab: score, how it is computed and the risk factors.

use eframe::egui::{self, Align, Layout, RichText};

use crate::core::format::humanize;
use crate::features::analysis::{Analysis, RiskReview, scoring};
use crate::ui::blocks::{callout, code_chip};
use crate::ui::theme::{pal, severity_color, severity_rank};
use crate::ui::widgets::{accent_card, body, card, meter, muted, pill, section_title, solid_pill};

pub fn risk(ui: &mut egui::Ui, a: &Analysis) {
    let p = pal(ui);
    let c = severity_color(&a.risk.level);
    card(ui, |ui| {
        ui.horizontal(|ui| {
            solid_pill(ui, format!("{} RISK", a.risk.level.to_uppercase()), c);
            ui.label(RichText::new(format!("{:.1}/10", a.risk.score)).size(18.0).strong().color(c));
            meter(ui, a.risk.score / 10.0, c, 160.0);
        });
        ui.add_space(4.0);
        body(ui, &a.risk.rationale);
        if !a.risk.recommended_review_depth.is_empty() {
            ui.add_space(4.0);
            callout(ui, "🎯", "Recommended review", &a.risk.recommended_review_depth);
        }
    });

    if a.risk.has_breakdown() {
        risk_breakdown(ui, &a.risk);
    }

    ui.add_space(4.0);
    section_title(ui, &format!("Risk factors · {}", a.risk.factors.len()));
    if a.risk.factors.is_empty() {
        ui.label(muted(ui, "None identified."));
    }
    let mut factors = a.risk.factors.clone();
    factors.sort_by_key(|f| std::cmp::Reverse(severity_rank(&f.severity)));
    for f in &factors {
        accent_card(ui, severity_color(&f.severity), |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(&f.area).size(15.0).strong().color(p.text));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    pill(ui, f.severity.to_uppercase(), severity_color(&f.severity));
                });
            });
            body(ui, &f.description);
            if !f.files.is_empty() {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    for file in &f.files {
                        code_chip(ui, file);
                    }
                });
            }
            if !f.mitigation.is_empty() {
                callout(ui, "✔", "Verify", &f.mitigation);
            }
        });
    }
}

/// The three dimensions the risk score is computed from.
fn risk_breakdown(ui: &mut egui::Ui, r: &RiskReview) {
    let p = pal(ui);
    let complexity = r.complexity.score.clamp(0.0, 10.0);
    let impact = r.worst_case.impact().unwrap_or(0.0);
    let exposure = r.audience.factor().unwrap_or(0.0) * 10.0;
    let rows = [
        ("Complexity", format!("{complexity:.0}/10"), complexity, r.complexity.reason.as_str()),
        ("Worst case", humanize(&r.worst_case.tier), impact, r.worst_case.scenario.as_str()),
        ("Audience", format!("{}-facing", humanize(&r.audience.facing)), exposure, r.audience.reason.as_str()),
    ];
    card(ui, |ui| {
        section_title(ui, "How the score is computed");
        for (label, value, level, reason) in rows {
            let c = severity_color(scoring::level_for(level));
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(150.0);
                    ui.spacing_mut().item_spacing.y = 4.0;
                    ui.label(RichText::new(label).size(12.0).strong().color(p.muted));
                    pill(ui, value, c);
                    meter(ui, level / 10.0, c, 120.0);
                });
                ui.add(egui::Label::new(RichText::new(reason).color(p.text).line_height(Some(20.0))).wrap());
            });
            ui.add_space(6.0);
        }
        ui.label(muted(
            ui,
            format!(
                "Score = worst case × audience × ({:.1} + {:.1} × complexity / 10)",
                scoring::BASE_LIKELIHOOD,
                1.0 - scoring::BASE_LIKELIHOOD
            ),
        ).size(12.0));
    });
}
