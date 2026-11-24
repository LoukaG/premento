//! The bar above the tabs: tool and model pickers, Analyze button, progress of a running analysis.

use std::sync::atomic::Ordering;

use chrono::DateTime;
use eframe::egui::{self, Align, Layout, RichText};

use crate::core::format::ago;
use crate::features::analysis::Analyses;
use crate::features::providers::ProviderPicker;
use crate::ui::blocks::indeterminate_bar;
use crate::ui::theme::{OUTDATED, pal};
use crate::ui::widgets::{card, ghost_button, pill, primary_button};

/// Returns true when Analyze (or Re-analyze) was clicked.
/// `current_head` is the PR's head commit, when known: an analysis of another commit is flagged.
pub fn show(
    ui: &mut egui::Ui,
    picker: &mut ProviderPicker,
    analyses: &Analyses,
    key: &str,
    current_head: Option<&str>,
) -> bool {
    let p = pal(ui);
    let mut analyze = false;
    card(ui, |ui| {
        ui.horizontal(|ui| {
            picker.show(ui);

            let running = analyses.jobs.contains_key(key);
            let has_result = analyses.results.contains_key(key);
            let can_run = !running && picker.is_available();
            let label = if has_result { "↻  Re-analyze" } else { "✨  Analyze PR" };
            if primary_button(ui, label, can_run).clicked() {
                analyze = true;
            }

            if let Some(stored) = analyses.results.get(key) {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(
                        RichText::new(format!(
                            "{} · {} · {}s · {}",
                            stored.provider,
                            DateTime::from_timestamp(stored.created_at, 0)
                                .map(ago)
                                .unwrap_or_default(),
                            stored.duration_secs,
                            &stored.head_sha[..stored.head_sha.len().min(7)]
                        ))
                        .size(12.0)
                        .color(p.faint),
                    )
                    .on_hover_text("Tool · model · analyzed · duration · head commit");
                    if !running && stored.is_outdated(current_head) {
                        pill(ui, "Outdated", OUTDATED).on_hover_text(format!(
                            "The PR has new commits since this analysis (now at {}). Re-analyze to review them.",
                            current_head.map(|h| &h[..h.len().min(7)]).unwrap_or_default()
                        ));
                    }
                });
            }
        });

        if let Some(job) = analyses.jobs.get(key) {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.add(egui::Spinner::new().size(16.0).color(p.accent_text));
                ui.label(RichText::new(&job.stage).color(p.text));
                ui.label(
                    RichText::new(format!("{} · {}s", job.provider, job.started.elapsed().as_secs()))
                        .color(p.muted),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ghost_button(ui, "Cancel").clicked() {
                        job.cancel.store(true, Ordering::Relaxed);
                    }
                });
            });
            indeterminate_bar(ui);
        }
    });
    analyze
}
