//! The central panel: the selected PR with its analysis tabs, or the service map when none is.

use eframe::egui::{self, RichText};

use super::PRementoApp;
use crate::features::analysis::views::{self, action_bar};
use crate::features::ask::{self, Launch};
use crate::features::pull_requests::header;
use crate::features::related;
use crate::features::service_map;
use crate::ui::blocks::banner;
use crate::ui::theme::pal;
use crate::ui::widgets::{self, card, muted};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tab {
    Overview,
    Risk,
    Intent,
    Impact,
    Map,
    Ask,
}

impl Tab {
    /// attraction-inc PRs get the service map in place of the generic impact analysis.
    pub fn for_pr(on_map: bool) -> [(Tab, &'static str); 5] {
        [
            (Tab::Overview, "Overview"),
            (Tab::Risk, "Risk Review"),
            (Tab::Intent, "Intent & Decisions"),
            if on_map { (Tab::Map, "Service Map") } else { (Tab::Impact, "Impact") },
            (Tab::Ask, "Ask"),
        ]
    }
}

impl PRementoApp {
    pub(super) fn details(&mut self, ui: &mut egui::Ui) {
        let p = pal(ui);
        let Some(pr) = self.prs.selected_pr().cloned() else {
            ui.label(RichText::new("Service map").size(24.0).strong().color(p.text));
            ui.label(muted(
                ui,
                "attraction-inc services and their main connections. Select a PR to see which services it impacts.",
            ));
            ui.add_space(10.0);
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                card(ui, |ui| service_map::view::draw(ui, &[]));
            });
            return;
        };
        let key = pr.key();

        header::show(ui, &pr);
        if let Some(group) = self.relations.group_of(&key) {
            ui.add_space(4.0);
            let prs = &self.prs;
            if let Some(k) = related::view::header_line(ui, group, &key, &|k| prs.find(k).map(|p| p.title.clone())) {
                self.prs.selected = Some(k);
            }
        }
        ui.add_space(10.0);

        if action_bar::show(ui, &mut self.picker, &self.analyses, &key, pr.head_sha.as_deref()) {
            self.start_analysis(ui.ctx(), &pr);
        }

        if let Some(err) = self.analyses.errors.get(&key) {
            ui.add_space(8.0);
            banner(ui, err);
        }
        ui.add_space(14.0);

        let Some(stored) = self.analyses.results.get(&key).cloned() else {
            if !self.analyses.jobs.contains_key(&key) {
                views::intro(ui);
            }
            return;
        };

        let on_map = service_map::applies(&pr.repo);
        match (self.tab, on_map) {
            (Tab::Impact, true) => self.tab = Tab::Map,
            (Tab::Map, false) => self.tab = Tab::Impact,
            _ => {}
        }
        widgets::tabs(ui, &mut self.tab, &Tab::for_pr(on_map));
        if self.tab == Tab::Ask {
            if let Some(launch) = ask::view::show(ui, &mut self.asks, &self.picker, &key) {
                self.start_ask(ui.ctx(), &pr, &stored, launch == Launch::TerminalApp);
            }
            return;
        }

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 10.0;
                match (&stored.analysis, self.tab) {
                    (None, _) => views::raw_response(ui, &stored.raw),
                    (Some(a), Tab::Overview) => views::overview(ui, a, &pr.repo),
                    (Some(a), Tab::Risk) => views::risk(ui, a),
                    (Some(a), Tab::Intent) => views::intent(ui, a),
                    (Some(a), Tab::Impact) => views::impact(ui, a),
                    (Some(_), Tab::Ask) => {} // drawn before the scroll area
                    (Some(a), Tab::Map) => {
                        let (impacts, from_ai) = service_map::resolve_impact(&pr.repo, &a.service_impact);
                        card(ui, |ui| {
                            service_map::view::legend(ui, !from_ai);
                            ui.add_space(4.0);
                            service_map::view::draw(ui, &impacts);
                        });
                        service_map::view::impact_list(ui, &impacts);
                    }
                }
                ui.add_space(20.0);
            });
    }
}
