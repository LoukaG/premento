//! Events from the features' background threads, handled on the UI thread.

use anyhow::Result;
use eframe::egui;

use super::PRementoApp;
use crate::features::{analysis, ask, pull_requests};

pub enum Msg {
    Prs(Result<pull_requests::Loaded>),
    Analysis(analysis::Event),
    Ask(ask::Ready),
}

impl PRementoApp {
    pub(super) fn handle_messages(&mut self, ctx: &egui::Context) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                Msg::Prs(result) => {
                    let cached = self.prs.on_loaded(result);
                    self.analyses.add_cached(cached);
                }
                Msg::Analysis(event) => self.analyses.on_event(event),
                Msg::Ask(ready) => {
                    let pr = self.prs.find(&ready.key);
                    self.asks.on_ready(ctx, ready, pr, &self.config.terminal);
                }
            }
        }
    }
}
