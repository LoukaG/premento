//! The Ask tab: start, show and control the Ask session of the selected PR.

use eframe::egui::{self, Align, Layout, RichText, Sense, vec2};

use super::session;
use super::state::{AskSessions, AskState};
use crate::features::providers::ProviderPicker;
use crate::ui::blocks::banner;
use crate::ui::theme::{pal, severity_color};
use crate::ui::widgets::{body, card, ghost_button, muted, primary_button, section_title};

/// Where the user asked to start a session.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Launch {
    Here,
    TerminalApp,
}

/// Draws the tab for PR `key`. Ending a session and showing its folder are handled here;
/// starting one is returned, since the app runs the background work.
pub fn show(ui: &mut egui::Ui, asks: &mut AskSessions, picker: &ProviderPicker, key: &str) -> Option<Launch> {
    let p = pal(ui);
    let provider = picker.provider();
    let label = picker.selection().label;
    let can_start = provider.supports_chat() && picker.is_available();
    let unsupported = (!provider.supports_chat()).then(|| {
        format!(
            "{} can't be used for questions. Pick Claude Code or Codex above, or set `chat_args` for it in config.toml.",
            provider.name
        )
    });

    #[derive(PartialEq)]
    enum Action {
        None,
        Start,
        External,
        End,
        Reveal,
    }
    let mut action = Action::None;
    ui.add_space(4.0);

    match asks.sessions.get_mut(key) {
        Some(AskState::Running { label: running, dir, term }) => {
            ui.horizontal(|ui| {
                let (dot, status) = match term.exit() {
                    None => (severity_color("low"), "running".to_owned()),
                    Some(Some(code)) => (p.faint, format!("ended (exit code {code})")),
                    Some(None) => (p.faint, "ended".to_owned()),
                };
                let (r, _) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
                ui.painter().circle_filled(r.center(), 4.0, dot);
                ui.label(RichText::new(running.as_str()).strong().color(p.text));
                ui.label(muted(ui, status));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ghost_button(ui, "End").on_hover_text("Stop the session").clicked() {
                        action = Action::End;
                    }
                    let restart = ui.add_enabled_ui(can_start, |ui| {
                        ghost_button(ui, "↻  Restart").on_hover_text(format!("Start a new session with {label}"))
                    });
                    if restart.inner.clicked() {
                        action = Action::Start;
                    }
                    if ghost_button(ui, "Show folder").on_hover_text(dir.display().to_string()).clicked() {
                        action = Action::Reveal;
                    }
                    if ghost_button(ui, "Open in Terminal app  ↗").clicked() {
                        action = Action::External;
                    }
                });
            });
            if let Some(note) = asks.notes.get(key) {
                ui.label(muted(ui, note).size(12.0));
            }
            ui.add_space(4.0);
            term.show(ui);
        }
        state => {
            let preparing = matches!(state, Some(AskState::Preparing(_)));
            card(ui, |ui| {
                section_title(ui, "Ask about this PR");
                body(
                    ui,
                    &format!(
                        "Starts {label} here, in a folder holding this PR (description, commits, discussion \
and diff, fetched now) and this review, so you can ask questions about it."
                    ),
                );
                ui.add_space(6.0);
                if let Some(msg) = &unsupported {
                    ui.label(muted(ui, msg));
                    ui.add_space(4.0);
                }
                ui.horizontal(|ui| {
                    if primary_button(ui, &format!("💬  Start {label}"), can_start && !preparing).clicked() {
                        action = Action::Start;
                    }
                    let external = ui.add_enabled_ui(can_start, |ui| ghost_button(ui, "Open in Terminal app  ↗"));
                    if external.inner.clicked() {
                        action = Action::External;
                    }
                });
                if let Some(note) = asks.notes.get(key) {
                    ui.label(muted(ui, note).size(12.0));
                }
            });
            match state {
                Some(AskState::Preparing(label)) => {
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        ui.add(egui::Spinner::new().size(16.0).color(p.accent_text));
                        ui.label(RichText::new(format!("Fetching the PR and starting {label}…")).color(p.text));
                    });
                }
                Some(AskState::Failed(err)) => {
                    ui.add_space(8.0);
                    banner(ui, err);
                }
                _ => {}
            }
        }
    }

    match action {
        Action::Start => return Some(Launch::Here),
        Action::External => return Some(Launch::TerminalApp),
        Action::End => {
            asks.sessions.remove(key);
        }
        Action::Reveal => session::reveal(&session::folder(key)),
        Action::None => {}
    }
    None
}
