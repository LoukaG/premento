//! The sidebar: filter, search and the list of PR cards.

use eframe::egui::{self, Align, Color32, Layout, Margin, RichText};

use super::{Filter, PrList, PullRequest, Role};
use crate::core::format::ago_str;
use crate::features::analysis::Analyses;
use crate::features::related::{self, Entry, Relations};
use crate::features::service_map;
use crate::ui::theme::{OUTDATED, pal, severity_color};
use crate::ui::widgets::{avatar, clickable_card, muted, paint_stripe, pill, segmented, solid_pill};

/// Returns true when "Group related" was clicked.
pub fn show(ui: &mut egui::Ui, prs: &mut PrList, analyses: &Analyses, relations: &Relations) -> bool {
    let p = pal(ui);
    let visible_count = prs.visible().len();
    let mut group_clicked = false;

    ui.horizontal(|ui| {
        ui.label(RichText::new("Pull requests").size(16.0).strong().color(p.text));
        pill(ui, visible_count.to_string(), p.muted);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            group_clicked = related::view::group_button(ui, relations, prs.prs.len());
        });
    });
    ui.add_space(2.0);
    segmented(
        ui,
        &mut prs.filter,
        &[
            (Filter::All, "All"),
            (Filter::Assigned, "Assigned"),
            (Filter::Reviewer, "Reviewer"),
        ],
    );
    ui.add(
        egui::TextEdit::singleline(&mut prs.search)
            .hint_text("🔎  Search title, repo or author")
            .desired_width(f32::INFINITY)
            .margin(Margin::symmetric(10, 7)),
    );
    if let Some(err) = &relations.error {
        ui.add(egui::Label::new(muted(ui, err).size(11.5)).wrap());
    }
    ui.add_space(4.0);

    let visible = prs.visible();
    let entries = relations.entries(&visible);
    let mut clicked = None;

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            if visible.is_empty() {
                ui.add_space(40.0);
                ui.vertical_centered(|ui| {
                    if prs.loading {
                        ui.spinner();
                        ui.label(muted(ui, "Loading pull requests…"));
                    } else {
                        ui.label(RichText::new("✔").size(28.0).color(p.faint));
                        ui.label(RichText::new("All caught up").strong().color(p.text));
                        ui.label(muted(ui, "No open PRs match this filter."));
                    }
                });
            }
            let selected = prs.selected.as_deref();
            for entry in &entries {
                match entry {
                    Entry::Single(pr) => {
                        if pr_card(ui, pr, selected == Some(pr.key().as_str()), analyses).clicked() {
                            clicked = Some(pr.key());
                        }
                    }
                    Entry::Group(g) => {
                        let group = &relations.groups[*g];
                        let shown = |key: &str| visible.iter().any(|pr| pr.key() == key);
                        let picked = related::view::group(ui, group, &shown, |ui, key| {
                            let pr = prs.find(key)?;
                            Some(pr_card(ui, pr, selected == Some(key), analyses))
                        });
                        if picked.is_some() {
                            clicked = picked;
                        }
                    }
                }
            }
        });

    if let Some(key) = clicked {
        prs.selected = Some(key);
    }
    group_clicked
}

/// The card of one PR in the list.
pub fn pr_card(ui: &mut egui::Ui, pr: &PullRequest, selected: bool, analyses: &Analyses) -> egui::Response {
    let p = pal(ui);
    let key = pr.key();
    let stored = analyses.results.get(&key);
    let analysis = stored.and_then(|s| s.analysis.as_ref());
    let outdated = stored.is_some_and(|s| s.is_outdated(pr.head_sha.as_deref()));
    let running = analyses.jobs.contains_key(&key);

    let resp = clickable_card(ui, ui.scope_id().with(("pr", &key)), selected, |ui| {
        ui.spacing_mut().item_spacing.y = 5.0;
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!("{}  #{}", pr.repo, pr.number))
                    .size(11.5)
                    .color(p.muted),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if running {
                    ui.add(egui::Spinner::new().size(14.0).color(p.accent_text));
                    ui.label(RichText::new("Analyzing").size(11.5).color(p.accent_text));
                } else if let Some(a) = analysis {
                    solid_pill(ui, a.risk.level.to_uppercase(), severity_color(&a.risk.level));
                }
                if outdated && !running {
                    pill(ui, "Outdated", OUTDATED)
                        .on_hover_text("The PR has new commits since it was analyzed.");
                }
                if pr.is_draft {
                    pill(ui, "Draft", p.muted);
                }
            });
        });
        ui.add(
            egui::Label::new(RichText::new(&pr.title).size(14.0).strong().color(p.text))
                .wrap()
                .selectable(false),
        );
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 5.0;
            for role in &pr.roles {
                pill(ui, role.label(), role_color(*role));
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(RichText::new(ago_str(&pr.updated_at)).size(11.5).color(p.faint));
                ui.label(RichText::new(&pr.author).size(11.5).color(p.muted));
                avatar(ui, &pr.author, 16.0);
            });
        });
        if service_map::applies(&pr.repo) {
            let from_ai = analysis.map(|a| a.service_impact.as_slice()).unwrap_or(&[]);
            let (impacts, _) = service_map::resolve_impact(&pr.repo, from_ai);
            service_map::view::service_chips(ui, &impacts);
        }
    });
    if let Some(a) = analysis {
        paint_stripe(ui, resp.rect, severity_color(&a.risk.level));
    }
    resp
}

pub fn role_color(role: Role) -> Color32 {
    match role {
        Role::Assignee => Color32::from_rgb(59, 130, 246),
        Role::ReviewRequested => Color32::from_rgb(168, 85, 247),
        Role::Reviewed => Color32::from_rgb(20, 184, 166),
    }
}
