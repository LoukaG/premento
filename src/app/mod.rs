//! The app shell: holds each feature's state, routes background events to it and lays out
//! the three panels.

mod details;
mod messages;
mod top_bar;

#[cfg(test)]
mod snapshots;

use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Duration;

use eframe::egui::{self, Margin};

pub use details::Tab;
use messages::Msg;

use crate::core::config::Config;
use crate::features::analysis::{Analyses, StoredAnalysis};
use crate::features::ask::AskSessions;
use crate::features::providers::ProviderPicker;
use crate::features::pull_requests::{PrList, PullRequest, list_view};
use crate::features::related::Relations;
use crate::ui::theme::{self, pal};

pub struct PRementoApp {
    logo: Option<egui::TextureHandle>,
    config: Config,
    config_error: Option<String>,
    picker: ProviderPicker,
    prs: PrList,
    analyses: Analyses,
    asks: AskSessions,
    relations: Relations,
    pub tab: Tab,

    tx: Sender<Msg>,
    rx: Receiver<Msg>,
}

impl PRementoApp {
    pub fn new(cc: &eframe::CreationContext<'_>, config: Config, config_error: Option<String>) -> Self {
        theme::install(&cc.egui_ctx);
        egui_extras::install_image_loaders(&cc.egui_ctx);
        let mut app = Self::build(config, config_error);
        app.refresh(&cc.egui_ctx);
        app
    }

    fn build(config: Config, config_error: Option<String>) -> Self {
        let picker = ProviderPicker::new(config.providers.clone(), config.default_provider.as_deref());
        let (tx, rx) = channel();
        Self {
            logo: None,
            config,
            config_error,
            picker,
            prs: PrList::default(),
            analyses: Analyses::default(),
            asks: AskSessions::default(),
            relations: Relations::default(),
            tab: Tab::Overview,
            tx,
            rx,
        }
    }

    /// A callback for a feature's background thread: wraps its event in a [`Msg`], sends it to
    /// the UI thread and wakes the UI up.
    fn emitter<E: 'static>(&self, ctx: &egui::Context, wrap: fn(E) -> Msg) -> impl Fn(E) + Send + 'static {
        let (tx, ctx) = (self.tx.clone(), ctx.clone());
        move |event| {
            let _ = tx.send(wrap(event));
            ctx.request_repaint();
        }
    }

    fn refresh(&mut self, ctx: &egui::Context) {
        let emit = self.emitter(ctx, Msg::Prs);
        self.prs.refresh(self.config.search_limit, emit);
    }

    fn start_analysis(&mut self, ctx: &egui::Context, pr: &PullRequest) {
        let emit = self.emitter(ctx, Msg::Analysis);
        let timeout = Duration::from_secs(self.config.timeout_secs);
        self.analyses.start(pr, self.picker.selection(), self.config.max_diff_chars, timeout, emit);
    }

    /// Asks the picked provider which listed PRs belong together, or cancels that.
    fn start_relations(&mut self, ctx: &egui::Context) {
        let emit = self.emitter(ctx, Msg::Related);
        let timeout = Duration::from_secs(self.config.timeout_secs);
        let briefs = self
            .analyses
            .results
            .iter()
            .filter_map(|(k, s)| Some((k.clone(), s.analysis.as_ref()?.brief.what.clone())))
            .collect();
        self.relations.start_or_cancel(&self.prs.prs, briefs, self.picker.selection(), timeout, emit);
    }

    fn start_ask(&mut self, ctx: &egui::Context, pr: &PullRequest, review: &StoredAnalysis, external: bool) {
        let emit = self.emitter(ctx, Msg::Ask);
        self.asks.start(self.picker.selection(), pr, review, external, self.config.max_diff_chars, emit);
    }
}

impl eframe::App for PRementoApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.handle_messages(&ctx);
        self.asks.pump(&ctx);
        self.relations.update(&self.prs.prs);
        if !self.analyses.jobs.is_empty() || self.relations.job.is_some() {
            ui.ctx().request_repaint_after(Duration::from_millis(500));
        }
        let p = pal(ui);

        egui::Panel::top("top")
            .frame(
                egui::Frame::new()
                    .fill(p.sidebar)
                    .inner_margin(Margin::symmetric(18, 12)),
            )
            .show(ui, |ui| self.top_bar(ui));

        egui::Panel::left("prs")
            .resizable(true)
            .default_size(420.0)
            .size_range(300.0..=640.0)
            .frame(
                egui::Frame::new()
                    .fill(p.sidebar)
                    .inner_margin(Margin::symmetric(12, 14)),
            )
            .show(ui, |ui| {
                if list_view::show(ui, &mut self.prs, &self.analyses, &self.relations) {
                    self.start_relations(ui.ctx());
                }
            });

        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(p.bg)
                    .inner_margin(Margin::symmetric(32, 24)),
            )
            .show(ui, |ui| self.details(ui));
    }
}
