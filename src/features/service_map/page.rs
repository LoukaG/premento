//! The Service maps page, shown when no PR is selected: the saved maps, and generating one
//! from a folder.

use std::path::PathBuf;

use eframe::egui::{self, Align, Color32, CornerRadius, Layout, RichText, Stroke, vec2};

use super::state::ServiceMaps;
use super::view;
use crate::core::format::ago_str;
use crate::features::providers::ProviderPicker;
use crate::ui::blocks::banner;
use crate::ui::theme::pal;
use crate::ui::widgets::{card, ghost_button, muted, primary_button};

/// What the page asks the app to do.
pub enum Action {
    /// Generate (or regenerate) the map of this folder.
    Generate(PathBuf),
    Cancel,
}

fn pick_folder() -> Option<PathBuf> {
    rfd::FileDialog::new().set_title("Folder holding your repositories").pick_folder()
}

pub fn show(ui: &mut egui::Ui, maps: &mut ServiceMaps, picker: &mut ProviderPicker) -> Option<Action> {
    let p = pal(ui);
    let mut action = None;

    ui.label(RichText::new("Service maps").size(24.0).strong().color(p.text));
    ui.label(muted(
        ui,
        "Pick a folder holding your repositories: the selected tool maps their services and connections. \
PRs from a repo on a map get a Service Map tab showing what they impact.",
    ));
    ui.add_space(8.0);
    if maps.job.is_none() {
        ui.horizontal(|ui| {
            picker.show(ui);
            ui.add_space(6.0);
            if primary_button(ui, "  Generate from folder…  ", picker.is_available()).clicked()
                && let Some(folder) = pick_folder()
            {
                action = Some(Action::Generate(folder));
            }
        });
    }
    ui.add_space(10.0);

    if let Some(job) = &maps.job {
        card(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(RichText::new(&job.stage).color(p.accent_text));
                ui.label(RichText::new(format!("{}s", job.started.elapsed().as_secs())).color(p.faint));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ghost_button(ui, "Cancel").on_hover_text(format!("Stop {} mapping the folder.", job.provider)).clicked() {
                        action = Some(Action::Cancel);
                    }
                });
            });
            ui.label(RichText::new(job.folder.display().to_string()).monospace().size(11.5).color(p.muted));
        });
        ui.add_space(8.0);
    }
    if let Some(err) = &maps.error {
        banner(ui, err);
        ui.add_space(8.0);
    }

    if maps.maps.is_empty() {
        if maps.job.is_none() {
            empty_state(ui);
        }
        return action;
    }

    if maps.maps.len() > 1 {
        ui.horizontal_wrapped(|ui| {
            for (i, m) in maps.maps.iter().enumerate() {
                let selected = i == maps.selected;
                let text = RichText::new(&m.name).size(13.0).color(if selected { p.text } else { p.muted });
                let btn = egui::Button::new(if selected { text.strong() } else { text })
                    .fill(if selected { p.surface_hover } else { Color32::TRANSPARENT })
                    .stroke(Stroke::new(1.0, if selected { p.border } else { Color32::TRANSPARENT }))
                    .corner_radius(CornerRadius::same(7))
                    .min_size(vec2(0.0, 26.0));
                if ui.add(btn).clicked() {
                    maps.selected = i;
                }
            }
        });
        ui.add_space(6.0);
    }

    let Some(map) = maps.selected().cloned() else { return action };
    let confirm_id = ui.scope_id().with(("delete-map", &map.id));
    let confirming = ui.data(|d| d.get_temp::<bool>(confirm_id)).unwrap_or(false);
    let mut delete = false;
    ui.horizontal(|ui| {
        let when = chrono::DateTime::from_timestamp(map.created_at, 0).map(|t| ago_str(&t.to_rfc3339()));
        ui.label(
            RichText::new(format!(
                "{} services · {} connections · by {}{}",
                map.nodes.len(),
                map.edges.len(),
                map.provider,
                when.map(|w| format!(", {w}")).unwrap_or_default()
            ))
            .size(12.5)
            .color(p.muted),
        );
        ui.label(RichText::new(map.folder.display().to_string()).monospace().size(11.5).color(p.faint));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if confirming {
                if ghost_button(ui, RichText::new("Confirm delete").color(p.danger)).clicked() {
                    delete = true;
                }
                if ghost_button(ui, "Keep").clicked() {
                    ui.data_mut(|d| d.remove::<bool>(confirm_id));
                }
            } else if ghost_button(ui, "Delete").on_hover_text("Remove this map from PRemento.").clicked() {
                ui.data_mut(|d| d.insert_temp(confirm_id, true));
            }
            let can_regenerate = maps.job.is_none() && picker.is_available() && map.folder.is_dir();
            if ui
                .add_enabled_ui(can_regenerate, |ui| ghost_button(ui, "⟳  Regenerate"))
                .inner
                .on_hover_text("Map the same folder again with the selected tool.")
                .clicked()
            {
                action = Some(Action::Generate(map.folder.clone()));
            }
        });
    });
    if delete {
        ui.data_mut(|d| d.remove::<bool>(confirm_id));
        maps.remove(&map.id);
        return action;
    }
    ui.add_space(6.0);

    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        card(ui, |ui| view::draw(ui, &map, &[]));
        if !map.notes.is_empty() {
            ui.add_space(6.0);
            for note in &map.notes {
                ui.label(muted(ui, format!("› {note}")).size(12.5));
            }
        }
    });
    action
}

fn empty_state(ui: &mut egui::Ui) {
    let p = pal(ui);
    ui.add_space(40.0);
    ui.vertical_centered(|ui| {
        ui.label(RichText::new("🗺").size(32.0).color(p.accent_text));
        ui.label(RichText::new("No service map yet").size(18.0).strong().color(p.text));
        ui.label(muted(
            ui,
            "Pick a folder with your repos to map its services. A workspace with one subfolder per repo works best.",
        ));
    });
}
