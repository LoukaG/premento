//! The top bar: logo, signed-in user, theme toggle and refresh.

use eframe::egui::{self, Align, Layout, RichText, ThemePreference, vec2};

use super::PRementoApp;
use crate::ui::blocks::banner;
use crate::ui::icon;
use crate::ui::theme::pal;
use crate::ui::widgets::{avatar, ghost_button};

impl PRementoApp {
    pub(super) fn top_bar(&mut self, ui: &mut egui::Ui) {
        let p = pal(ui);
        let logo = self.logo.get_or_insert_with(|| icon::texture(ui.ctx())).id();
        ui.horizontal(|ui| {
            ui.add(egui::Image::new((logo, vec2(44.0, 44.0))).corner_radius(9));
            ui.add_space(2.0);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                ui.label(RichText::new("PRemento").size(15.0).strong().color(p.text));
                ui.label(RichText::new("AI pull request review").size(11.5).color(p.muted));
            });

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if let Some(user) = self.prs.user.clone() {
                    ui.label(RichText::new(&user).color(p.text));
                    avatar(ui, &user, 26.0);
                    ui.add_space(6.0);
                }

                let dark = ui.visuals().dark_mode;
                if ghost_button(ui, if dark { "☀" } else { "🌙" })
                    .on_hover_text(if dark { "Light mode" } else { "Dark mode" })
                    .clicked()
                {
                    ui.ctx().set_theme(if dark {
                        ThemePreference::Light
                    } else {
                        ThemePreference::Dark
                    });
                }

                if self.prs.loading {
                    ui.add_enabled(false, egui::Button::new("Refreshing…"));
                    ui.spinner();
                } else {
                    if ghost_button(ui, "⟳  Refresh").clicked() {
                        self.refresh(ui.ctx());
                    }
                    if let Some(t) = self.prs.last_refresh {
                        ui.label(
                            RichText::new(format!("Updated {}", t.format("%H:%M")))
                                .size(12.0)
                                .color(p.faint),
                        );
                    }
                }
            });
        });

        for err in [self.config_error.as_ref(), self.prs.error.as_ref()]
            .into_iter()
            .flatten()
        {
            ui.add_space(8.0);
            banner(ui, err);
        }
    }
}
