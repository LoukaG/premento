//! Header of the selected PR: repo, labels, title, author and a link to GitHub.

use eframe::egui::{self, Align, Color32, Layout, RichText};

use super::PullRequest;
use super::list_view::role_color;
use crate::core::format::ago_str;
use crate::ui::theme::pal;
use crate::ui::widgets::{avatar, ghost_button, pill};

pub fn show(ui: &mut egui::Ui, pr: &PullRequest) {
    let p = pal(ui);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        ui.label(RichText::new(&pr.repo).color(p.muted));
        ui.label(RichText::new(format!("#{}", pr.number)).color(p.faint));
        if pr.is_draft {
            pill(ui, "Draft", p.muted);
        }
        for role in &pr.roles {
            pill(ui, role.label(), role_color(*role));
        }
        for l in &pr.labels {
            pill(ui, l, Color32::from_rgb(120, 130, 160));
        }
    });
    ui.add(egui::Label::new(RichText::new(&pr.title).size(24.0).strong().color(p.text)).wrap());
    ui.horizontal(|ui| {
        avatar(ui, &pr.author, 20.0);
        ui.label(RichText::new(&pr.author).color(p.text));
        ui.label(RichText::new(format!("· updated {}", ago_str(&pr.updated_at))).color(p.muted));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ghost_button(ui, "Open on GitHub  ↗").clicked() {
                ui.ctx().open_url(egui::OpenUrl::new_tab(&pr.url));
            }
        });
    });
}
