//! Ask sessions per PR: being prepared, running in the embedded terminal, or failed.

use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::Result;
use chrono::Local;
use eframe::egui;

use super::session::{self, Session};
use crate::features::analysis::StoredAnalysis;
use crate::features::providers::Selection;
use crate::features::pull_requests::PullRequest;
use crate::features::terminal::Terminal;

/// State of the Ask tab's embedded terminal for one PR.
pub enum AskState {
    Preparing(String),
    Running { label: String, dir: PathBuf, term: Terminal },
    Failed(String),
}

/// A prepared Ask session; `external` when it goes to a separate terminal app.
pub struct Ready {
    pub key: String,
    session: Result<Session>,
    external: bool,
}

#[derive(Default)]
pub struct AskSessions {
    pub sessions: HashMap<String, AskState>,
    /// Status of the last "Open in Terminal app" per PR.
    pub notes: HashMap<String, String>,
}

impl AskSessions {
    /// Prepares the PR's Ask folder in the background, then (in [`Self::on_ready`]) starts the
    /// session in the embedded terminal or, with `external`, in a separate terminal app.
    pub fn start(
        &mut self,
        selection: Selection,
        pr: &PullRequest,
        review: &StoredAnalysis,
        external: bool,
        max_diff: usize,
        emit: impl FnOnce(Ready) + Send + 'static,
    ) {
        let key = pr.key();
        let Selection { provider, model, label } = selection;
        if external {
            self.notes.insert(key.clone(), format!("Opening {label} in a terminal app…"));
        } else {
            self.sessions.insert(key.clone(), AskState::Preparing(label));
        }
        let (pr, review) = (pr.clone(), review.clone());
        std::thread::spawn(move || {
            let session = session::prepare(&provider, model.as_deref(), &pr, &review, max_diff);
            emit(Ready { key, session, external });
        });
    }

    /// `pr` is the PR the session is for, if it is still listed.
    pub fn on_ready(&mut self, ctx: &egui::Context, ready: Ready, pr: Option<&PullRequest>, terminal: &[String]) {
        let Ready { key, session, external } = ready;
        if external {
            let note = session
                .and_then(|s| {
                    let pr = pr.ok_or_else(|| anyhow::anyhow!("PR {key} is no longer listed"))?;
                    session::open_external(&s, pr, terminal)?;
                    Ok(format!("{} opened in a terminal app at {}.", s.label, Local::now().format("%H:%M")))
                })
                .unwrap_or_else(|e| format!("Could not open a terminal app: {e:#}"));
            self.notes.insert(key, note);
            return;
        }
        // Started, then ended or restarted before it was ready: ignore the stale result.
        if !matches!(self.sessions.get(&key), Some(AskState::Preparing(_))) {
            return;
        }
        let state = session
            .and_then(|s| {
                let term = Terminal::spawn(ctx, &s.program, s.args, &s.dir)?;
                term.request_focus(ctx);
                Ok(AskState::Running { label: s.label, dir: s.dir, term })
            })
            .unwrap_or_else(|e| AskState::Failed(format!("{e:#}")));
        self.sessions.insert(key, state);
    }

    /// Processes the output of every running terminal, including ones not on screen.
    pub fn pump(&mut self, ctx: &egui::Context) {
        for state in self.sessions.values_mut() {
            if let AskState::Running { term, .. } = state {
                term.pump(ctx);
            }
        }
    }
}
