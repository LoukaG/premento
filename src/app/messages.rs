//! Events from the features' background threads, handled on the UI thread.

use anyhow::Result;
use eframe::egui;

use super::PRementoApp;
use crate::features::{analysis, ask, pull_requests, related};

pub enum Msg {
    Prs(Result<pull_requests::Loaded>),
    Analysis(analysis::Event),
    Ask(ask::Ready),
    Related(related::Event),
}

impl PRementoApp {
    pub(super) fn handle_messages(&mut self, ctx: &egui::Context) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                Msg::Prs(result) => {
                    let found = self.prs.on_loaded(result);
                    self.analyses.add_cached(found.analyses);
                    if let Some(fetched) = found.related {
                        self.relations.on_fetched(fetched);
                    }
                }
                Msg::Analysis(event) => {
                    if let analysis::Event::Done(key, result) = &event
                        && let Ok(stored) = result.as_ref()
                    {
                        self.prs.set_head(key, &stored.head_sha);
                    }
                    self.analyses.on_event(event);
                }
                Msg::Related(event) => self.relations.on_event(event),
                Msg::Ask(ready) => {
                    let pr = self.prs.find(&ready.key);
                    self.asks.on_ready(ctx, ready, pr, &self.config.terminal);
                }
            }
        }
    }
}
