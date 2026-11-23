use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

use anyhow::Result;
use chrono::{DateTime, Local, Utc};
use eframe::egui::{
    self, Align, Color32, CornerRadius, Layout, Margin, RichText, Sense, Stroke, ThemePreference,
    pos2, vec2,
};

use crate::analysis::{self, Analysis, StoredAnalysis};
use crate::ask;
use crate::config::{Config, ProviderConfig, config_path};
use crate::github::{self, PullRequest, Role};
use crate::llm;
use crate::map_view;
use crate::service_map;
use crate::terminal::Terminal;
use crate::theme::{
    self, accent_card, body, avatar, card, clickable_card, ghost_button, meter, muted, pal, pill,
    primary_button, section_title, segmented, severity_color, severity_rank, solid_pill,
};

enum Msg {
    Prs(Result<PrsLoaded>),
    Stage(String, String),
    Analysis(String, Box<Result<StoredAnalysis>>),
    /// A prepared Ask session; `external` when it goes to a separate terminal app.
    Ask { key: String, session: Result<ask::Session>, external: bool },
}

/// State of the Ask tab's embedded terminal for one PR.
enum AskState {
    Preparing(String),
    Running { label: String, dir: std::path::PathBuf, term: Terminal },
    Failed(String),
}

struct PrsLoaded {
    user: Option<String>,
    prs: Vec<PullRequest>,
    cached: Vec<(String, StoredAnalysis)>,
}

struct Job {
    started: Instant,
    provider: String,
    stage: String,
    cancel: Arc<AtomicBool>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Filter {
    All,
    Assigned,
    Reviewer,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tab {
    Overview,
    Risk,
    Intent,
    Impact,
    Map,
    Ask,
    Raw,
}

impl Tab {
    /// attraction-inc PRs get the service map in place of the generic impact analysis.
    fn for_pr(on_map: bool) -> [(Tab, &'static str); 6] {
        [
            (Tab::Overview, "Overview"),
            (Tab::Risk, "Risk Review"),
            (Tab::Intent, "Intent & Decisions"),
            if on_map { (Tab::Map, "Service Map") } else { (Tab::Impact, "Impact") },
            (Tab::Ask, "Ask"),
            (Tab::Raw, "Raw"),
        ]
    }
}

pub struct PRementoApp {
    logo: Option<egui::TextureHandle>,
    config: Config,
    config_error: Option<String>,
    available: Vec<bool>,
    provider_idx: usize,
    /// Model picked for each provider; empty means its default.
    models: Vec<String>,
    /// Whether each provider's model is typed in rather than picked from its list.
    custom_model: Vec<bool>,

    user: Option<String>,
    prs: Vec<PullRequest>,
    loading_prs: bool,
    list_error: Option<String>,
    last_refresh: Option<DateTime<Local>>,
    filter: Filter,
    search: String,

    selected: Option<String>,
    pub tab: Tab,
    jobs: HashMap<String, Job>,
    results: HashMap<String, StoredAnalysis>,
    errors: HashMap<String, String>,
    asks: HashMap<String, AskState>,
    /// Status of the last "Open in Terminal app" per PR.
    ask_notes: HashMap<String, String>,

    tx: Sender<Msg>,
    rx: Receiver<Msg>,
}

impl PRementoApp {
    pub fn new(cc: &eframe::CreationContext<'_>, config: Config, config_error: Option<String>) -> Self {
        theme::install(&cc.egui_ctx);
        let mut app = Self::build(config, config_error);
        app.refresh(&cc.egui_ctx);
        app
    }

    fn build(config: Config, config_error: Option<String>) -> Self {
        let available: Vec<bool> = config.providers.iter().map(llm::is_available).collect();
        let provider_idx = config
            .default_provider
            .as_ref()
            .and_then(|name| config.providers.iter().position(|p| &p.name == name))
            .filter(|&i| available[i])
            .or_else(|| available.iter().position(|&a| a))
            .unwrap_or(0);

        let (tx, rx) = channel();
        let n = config.providers.len();
        Self {
            logo: None,
            config,
            config_error,
            available,
            provider_idx,
            models: vec![String::new(); n],
            custom_model: vec![false; n],
            user: None,
            prs: vec![],
            loading_prs: false,
            list_error: None,
            last_refresh: None,
            filter: Filter::All,
            search: String::new(),
            selected: None,
            tab: Tab::Overview,
            jobs: HashMap::new(),
            results: HashMap::new(),
            errors: HashMap::new(),
            asks: HashMap::new(),
            ask_notes: HashMap::new(),
            tx,
            rx,
        }
    }

    fn refresh(&mut self, ctx: &egui::Context) {
        if self.loading_prs {
            return;
        }
        self.loading_prs = true;
        self.list_error = None;
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        let limit = self.config.search_limit;
        let need_user = self.user.is_none();
        std::thread::spawn(move || {
            let result = (|| -> Result<PrsLoaded> {
                let user = if need_user { github::current_user().ok() } else { None };
                let prs = github::fetch_prs(limit)?;
                let cached = prs
                    .iter()
                    .filter_map(|pr| analysis::load_cached(&pr.key()).map(|a| (pr.key(), a)))
                    .collect();
                Ok(PrsLoaded { user, prs, cached })
            })();
            let _ = tx.send(Msg::Prs(result));
            ctx.request_repaint();
        });
    }

    fn start_analysis(&mut self, ctx: &egui::Context, pr: &PullRequest) {
        let key = pr.key();
        if self.jobs.contains_key(&key) {
            return;
        }
        let provider: ProviderConfig = self.config.providers[self.provider_idx].clone();
        let model = provider.effective_model(&self.models[self.provider_idx]).map(str::to_owned);
        let label = provider.label(model.as_deref());
        let cancel = Arc::new(AtomicBool::new(false));
        self.errors.remove(&key);
        self.jobs.insert(
            key.clone(),
            Job {
                started: Instant::now(),
                provider: label.clone(),
                stage: "Fetching PR from GitHub…".into(),
                cancel: cancel.clone(),
            },
        );

        let tx = self.tx.clone();
        let ctx = ctx.clone();
        let repo = pr.repo.clone();
        let number = pr.number;
        let max_diff = self.config.max_diff_chars;
        let timeout = Duration::from_secs(self.config.timeout_secs);
        std::thread::spawn(move || {
            let stage = |s: String| {
                let _ = tx.send(Msg::Stage(key.clone(), s));
                ctx.request_repaint();
            };
            let result = (|| -> Result<StoredAnalysis> {
                let pr_ctx = github::fetch_context(&repo, number, max_diff)?;
                stage(format!("{label} is reviewing the changes…"));
                let started = Instant::now();
                let map = service_map::applies(&repo).then(|| service_map::prompt_section(&repo));
                let prompt = analysis::build_prompt(&pr_ctx.text, map.as_deref());
                let raw = llm::run(&provider, model.as_deref(), &prompt, timeout, cancel)?;
                let (analysis, parse_error) = match analysis::parse(&raw) {
                    Ok(a) => (Some(a), None),
                    Err(e) => (None, Some(format!("{e:#}"))),
                };
                let stored = StoredAnalysis {
                    provider: label.clone(),
                    head_sha: pr_ctx.head_sha,
                    created_at: Utc::now().timestamp(),
                    duration_secs: started.elapsed().as_secs(),
                    analysis,
                    parse_error,
                    raw,
                };
                let _ = analysis::save_cached(&key, &stored);
                Ok(stored)
            })();
            let _ = tx.send(Msg::Analysis(key.clone(), Box::new(result)));
            ctx.request_repaint();
        });
    }

    /// Prepares the PR's Ask folder in the background, then starts the session in the embedded
    /// terminal or, with `external`, in a separate terminal app.
    fn start_ask(&mut self, ctx: &egui::Context, pr: &PullRequest, review: &StoredAnalysis, external: bool) {
        let key = pr.key();
        let provider = self.config.providers[self.provider_idx].clone();
        let model = provider.effective_model(&self.models[self.provider_idx]).map(str::to_owned);
        let label = provider.label(model.as_deref());
        if external {
            self.ask_notes.insert(key.clone(), format!("Opening {label} in a terminal app…"));
        } else {
            self.asks.insert(key.clone(), AskState::Preparing(label));
        }
        let (tx, ctx, pr, review) = (self.tx.clone(), ctx.clone(), pr.clone(), review.clone());
        let max_diff = self.config.max_diff_chars;
        std::thread::spawn(move || {
            let session = ask::prepare(&provider, model.as_deref(), &pr, &review, max_diff);
            let _ = tx.send(Msg::Ask { key, session, external });
            ctx.request_repaint();
        });
    }

    fn on_ask_ready(&mut self, ctx: &egui::Context, key: String, session: Result<ask::Session>, external: bool) {
        if external {
            let note = session
                .and_then(|s| {
                    let pr = self.prs.iter().find(|p| p.key() == key).cloned();
                    let pr = pr.ok_or_else(|| anyhow::anyhow!("PR {key} is no longer listed"))?;
                    ask::open_external(&s, &pr, &self.config.terminal)?;
                    Ok(format!("{} opened in a terminal app at {}.", s.label, Local::now().format("%H:%M")))
                })
                .unwrap_or_else(|e| format!("Could not open a terminal app: {e:#}"));
            self.ask_notes.insert(key, note);
            return;
        }
        // Started, then ended or restarted before it was ready: ignore the stale result.
        if !matches!(self.asks.get(&key), Some(AskState::Preparing(_))) {
            return;
        }
        let state = session
            .and_then(|s| {
                let term = Terminal::spawn(ctx, &s.program, s.args, &s.dir)?;
                term.request_focus(ctx);
                Ok(AskState::Running { label: s.label, dir: s.dir, term })
            })
            .unwrap_or_else(|e| AskState::Failed(format!("{e:#}")));
        self.asks.insert(key, state);
    }

    fn ask_tab(&mut self, ui: &mut egui::Ui, pr: &PullRequest, review: &StoredAnalysis) {
        let p = pal(ui);
        let key = pr.key();
        let provider = &self.config.providers[self.provider_idx];
        let label = provider.label(provider.effective_model(&self.models[self.provider_idx]));
        let can_start = provider.supports_chat() && self.available[self.provider_idx];
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

        match self.asks.get_mut(&key) {
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
                if let Some(note) = self.ask_notes.get(&key) {
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
                    if let Some(note) = self.ask_notes.get(&key) {
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
            Action::Start => self.start_ask(ui.ctx(), pr, review, false),
            Action::External => self.start_ask(ui.ctx(), pr, review, true),
            Action::End => {
                self.asks.remove(&key);
            }
            Action::Reveal => ask::reveal(&ask::folder(&key)),
            Action::None => {}
        }
    }

    /// Model picker for the selected provider: its default, its listed models, or a typed name.
    fn model_picker(&mut self, ui: &mut egui::Ui) {
        let p = pal(ui);
        let i = self.provider_idx;
        let provider = &self.config.providers[i];
        if !provider.supports_models() {
            return;
        }
        let default = match &provider.default_model {
            Some(m) => format!("Default ({m})"),
            None => "Default".to_owned(),
        };
        let (picked, custom) = (&mut self.models[i], &mut self.custom_model[i]);
        let selected = if *custom {
            "Other…".to_owned()
        } else if picked.is_empty() {
            default.clone()
        } else {
            picked.clone()
        };
        ui.label(RichText::new("Model").color(p.muted));
        egui::ComboBox::from_id_salt(("model", i))
            .selected_text(RichText::new(selected).color(p.text))
            .width(130.0)
            .show_ui(ui, |ui| {
                if ui.selectable_label(!*custom && picked.is_empty(), &default).clicked() {
                    picked.clear();
                    *custom = false;
                }
                for m in &provider.models {
                    if ui.selectable_label(!*custom && picked == m, m).clicked() {
                        *picked = m.clone();
                        *custom = false;
                    }
                }
                if ui.selectable_label(*custom, "Other…").clicked() {
                    *custom = true;
                }
            });
        if *custom {
            ui.add(
                egui::TextEdit::singleline(picked)
                    .hint_text("model name")
                    .desired_width(140.0),
            )
            .on_hover_text(format!("Passed as: {}", provider.model_args.join(" ")));
        }
    }

    fn handle_messages(&mut self, ctx: &egui::Context) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                Msg::Prs(Ok(loaded)) => {
                    self.loading_prs = false;
                    if loaded.user.is_some() {
                        self.user = loaded.user;
                    }
                    self.prs = loaded.prs;
                    for (key, stored) in loaded.cached {
                        self.results.entry(key).or_insert(stored);
                    }
                    self.last_refresh = Some(Local::now());
                }
                Msg::Prs(Err(e)) => {
                    self.loading_prs = false;
                    self.list_error = Some(format!("{e:#}"));
                }
                Msg::Stage(key, stage) => {
                    if let Some(job) = self.jobs.get_mut(&key) {
                        job.stage = stage;
                    }
                }
                Msg::Ask { key, session, external } => self.on_ask_ready(ctx, key, session, external),
                Msg::Analysis(key, result) => {
                    self.jobs.remove(&key);
                    match *result {
                        Ok(stored) => {
                            if let Some(err) = &stored.parse_error {
                                self.errors.insert(
                                    key.clone(),
                                    format!("Could not parse the model's response ({err}). See the Raw tab."),
                                );
                            }
                            self.results.insert(key, stored);
                        }
                        Err(e) => {
                            self.errors.insert(key, format!("{e:#}"));
                        }
                    }
                }
            }
        }
    }

    fn visible_prs(&self) -> Vec<&PullRequest> {
        let needle = self.search.to_lowercase();
        self.prs
            .iter()
            .filter(|pr| match self.filter {
                Filter::All => true,
                Filter::Assigned => pr.roles.contains(&Role::Assignee),
                Filter::Reviewer => pr
                    .roles
                    .iter()
                    .any(|r| matches!(r, Role::ReviewRequested | Role::Reviewed)),
            })
            .filter(|pr| {
                needle.is_empty()
                    || pr.title.to_lowercase().contains(&needle)
                    || pr.key().to_lowercase().contains(&needle)
                    || pr.author.to_lowercase().contains(&needle)
            })
            .collect()
    }

    // ---------------------------------------------------------------- top bar

    fn top_bar(&mut self, ui: &mut egui::Ui) {
        let p = pal(ui);
        let logo = self.logo.get_or_insert_with(|| crate::icon::texture(ui.ctx())).id();
        ui.horizontal(|ui| {
            ui.add(egui::Image::new((logo, vec2(44.0, 44.0))).corner_radius(9));
            ui.add_space(2.0);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                ui.label(RichText::new("PRemento").size(15.0).strong().color(p.text));
                ui.label(RichText::new("AI pull request review").size(11.5).color(p.muted));
            });

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if let Some(user) = self.user.clone() {
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

                if ghost_button(ui, "⚙")
                    .on_hover_text(format!(
                        "Providers & settings live in\n{}",
                        config_path().display()
                    ))
                    .clicked()
                {
                    ui.ctx().copy_text(config_path().display().to_string());
                }

                if self.loading_prs {
                    ui.add_enabled(false, egui::Button::new("Refreshing…"));
                    ui.spinner();
                } else {
                    if ghost_button(ui, "⟳  Refresh").clicked() {
                        self.refresh(ui.ctx());
                    }
                    if let Some(t) = self.last_refresh {
                        ui.label(
                            RichText::new(format!("Updated {}", t.format("%H:%M")))
                                .size(12.0)
                                .color(p.faint),
                        );
                    }
                }
            });
        });

        for err in [self.config_error.as_ref(), self.list_error.as_ref()]
            .into_iter()
            .flatten()
        {
            ui.add_space(8.0);
            banner(ui, err);
        }
    }

    // ---------------------------------------------------------------- sidebar

    fn pr_list(&mut self, ui: &mut egui::Ui) {
        let p = pal(ui);
        let visible_count = self.visible_prs().len();

        ui.horizontal(|ui| {
            ui.label(RichText::new("Pull requests").size(16.0).strong().color(p.text));
            pill(ui, visible_count.to_string(), p.muted);
        });
        ui.add_space(2.0);
        segmented(
            ui,
            &mut self.filter,
            &[
                (Filter::All, "All"),
                (Filter::Assigned, "Assigned"),
                (Filter::Reviewer, "Reviewer"),
            ],
        );
        ui.add(
            egui::TextEdit::singleline(&mut self.search)
                .hint_text("🔎  Search title, repo or author")
                .desired_width(f32::INFINITY)
                .margin(Margin::symmetric(10, 7)),
        );
        ui.add_space(4.0);

        let visible: Vec<PullRequest> = self.visible_prs().into_iter().cloned().collect();
        let mut clicked = None;

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 4.0;
                if visible.is_empty() {
                    ui.add_space(40.0);
                    ui.vertical_centered(|ui| {
                        if self.loading_prs {
                            ui.spinner();
                            ui.label(muted(ui, "Loading pull requests…"));
                        } else {
                            ui.label(RichText::new("✔").size(28.0).color(p.faint));
                            ui.label(RichText::new("All caught up").strong().color(p.text));
                            ui.label(muted(ui, "No open PRs match this filter."));
                        }
                    });
                }
                for pr in &visible {
                    let key = pr.key();
                    let selected = self.selected.as_deref() == Some(key.as_str());
                    let analysis = self.results.get(&key).and_then(|s| s.analysis.as_ref());
                    let running = self.jobs.contains_key(&key);

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
                            service_chips(ui, &impacts);
                        }
                    });
                    if let Some(a) = analysis {
                        theme::paint_stripe(ui, resp.rect, severity_color(&a.risk.level));
                    }
                    if resp.clicked() {
                        clicked = Some(key);
                    }
                }
            });

        if let Some(key) = clicked {
            self.selected = Some(key);
        }
    }

    // ---------------------------------------------------------------- details

    fn details(&mut self, ui: &mut egui::Ui) {
        let p = pal(ui);
        let Some(pr) = self
            .selected
            .as_ref()
            .and_then(|k| self.prs.iter().find(|pr| &pr.key() == k))
            .cloned()
        else {
            ui.label(RichText::new("Service map").size(24.0).strong().color(p.text));
            ui.label(muted(
                ui,
                "attraction-inc services and their main connections. Select a PR to see which services it impacts.",
            ));
            ui.add_space(10.0);
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                card(ui, |ui| map_view::draw(ui, &[]));
            });
            return;
        };
        let key = pr.key();

        // Header
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
        ui.add_space(10.0);

        // Action bar
        card(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Tool").color(p.muted));
                let current = self.config.providers[self.provider_idx].name.clone();
                egui::ComboBox::from_id_salt("provider")
                    .selected_text(RichText::new(current).color(p.text))
                    .width(190.0)
                    .height(300.0)
                    .show_ui(ui, |ui| {
                        for (i, prov) in self.config.providers.iter().enumerate() {
                            let label = if self.available[i] {
                                prov.name.clone()
                            } else {
                                format!("{}  — not installed", prov.name)
                            };
                            ui.add_enabled_ui(self.available[i], |ui| {
                                ui.selectable_value(&mut self.provider_idx, i, label)
                                    .on_hover_text(format!("{} {}", prov.command, prov.args.join(" ")));
                            });
                        }
                    });
                self.model_picker(ui);

                let running = self.jobs.contains_key(&key);
                let has_result = self.results.contains_key(&key);
                let can_run = !running && self.available[self.provider_idx];
                let label = if has_result { "↻  Re-analyze" } else { "✨  Analyze PR" };
                if primary_button(ui, label, can_run).clicked() {
                    self.start_analysis(ui.ctx(), &pr);
                }

                if let Some(stored) = self.results.get(&key) {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(
                            RichText::new(format!(
                                "{} · {} · {}s · {}",
                                stored.provider,
                                DateTime::from_timestamp(stored.created_at, 0)
                                    .map(ago)
                                    .unwrap_or_default(),
                                stored.duration_secs,
                                &stored.head_sha[..stored.head_sha.len().min(7)]
                            ))
                            .size(12.0)
                            .color(p.faint),
                        )
                        .on_hover_text("Tool · model · analyzed · duration · head commit");
                    });
                }
            });

            if let Some(job) = self.jobs.get(&key) {
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.add(egui::Spinner::new().size(16.0).color(p.accent_text));
                    ui.label(RichText::new(&job.stage).color(p.text));
                    ui.label(
                        RichText::new(format!("{} · {}s", job.provider, job.started.elapsed().as_secs()))
                            .color(p.muted),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ghost_button(ui, "Cancel").clicked() {
                            job.cancel.store(true, Ordering::Relaxed);
                        }
                    });
                });
                indeterminate_bar(ui);
            }
        });

        if let Some(err) = self.errors.get(&key) {
            ui.add_space(8.0);
            banner(ui, err);
        }
        ui.add_space(14.0);

        let Some(stored) = self.results.get(&key).cloned() else {
            if !self.jobs.contains_key(&key) {
                intro(ui);
            }
            return;
        };

        let on_map = service_map::applies(&pr.repo);
        match (self.tab, on_map) {
            (Tab::Impact, true) => self.tab = Tab::Map,
            (Tab::Map, false) => self.tab = Tab::Impact,
            _ => {}
        }
        theme::tabs(ui, &mut self.tab, &Tab::for_pr(on_map));
        if self.tab == Tab::Ask {
            self.ask_tab(ui, &pr, &stored);
            return;
        }

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 10.0;
                match (&stored.analysis, self.tab) {
                    (_, Tab::Raw) | (None, _) => {
                        card(ui, |ui| {
                            let mut raw = stored.raw.as_str();
                            ui.add(
                                egui::TextEdit::multiline(&mut raw)
                                    .code_editor()
                                    .frame(egui::Frame::NONE)
                                    .desired_width(f32::INFINITY),
                            );
                        });
                    }
                    (Some(a), Tab::Overview) => overview(ui, a, &pr.repo),
                    (Some(a), Tab::Risk) => risk(ui, a),
                    (Some(a), Tab::Intent) => intent(ui, a),
                    (Some(a), Tab::Impact) => impact(ui, a),
                    (Some(_), Tab::Ask) => {} // drawn before the scroll area
                    (Some(a), Tab::Map) => {
                        let (impacts, from_ai) = service_map::resolve_impact(&pr.repo, &a.service_impact);
                        card(ui, |ui| {
                            map_view::legend(ui, !from_ai);
                            ui.add_space(4.0);
                            map_view::draw(ui, &impacts);
                        });
                        map_view::impact_list(ui, &impacts);
                    }
                }
                ui.add_space(20.0);
            });
    }
}

impl eframe::App for PRementoApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.handle_messages(&ctx);
        for state in self.asks.values_mut() {
            if let AskState::Running { term, .. } = state {
                term.pump(&ctx);
            }
        }
        if !self.jobs.is_empty() {
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
            .show(ui, |ui| self.pr_list(ui));

        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(p.bg)
                    .inner_margin(Margin::symmetric(32, 24)),
            )
            .show(ui, |ui| self.details(ui));
    }
}

// ---------------------------------------------------------------- analysis views

fn overview(ui: &mut egui::Ui, a: &Analysis, repo: &str) {
    let p = pal(ui);
    let services = service_map::applies(repo).then(|| service_map::resolve_impact(repo, &a.service_impact).0);
    // High and critical risk factors, most severe first.
    let mut serious: Vec<_> = a.risk.factors.iter().filter(|f| severity_rank(&f.severity) >= 3).collect();
    serious.sort_by_key(|f| std::cmp::Reverse(severity_rank(&f.severity)));

    ui.columns(4, |cols| {
        stat_tile(&mut cols[0], "Risk", |ui| {
            let c = severity_color(&a.risk.level);
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("{:.1}", a.risk.score)).size(30.0).strong().color(c));
                ui.label(RichText::new("/10").size(15.0).color(p.muted));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    solid_pill(ui, a.risk.level.to_uppercase(), c);
                });
            });
            meter(ui, a.risk.score / 10.0, c, ui.available_width());
        });
        stat_tile(&mut cols[1], "Serious risks", |ui| {
            big_number(ui, serious.len(), a.risk.factors.len(), "factors");
        });
        if let Some(services) = &services {
            let direct = services.iter().filter(|i| i.level.eq_ignore_ascii_case("direct")).count();
            stat_tile(&mut cols[2], "Services impacted", |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(services.len().to_string()).size(30.0).strong().color(p.text));
                    ui.label(RichText::new(format!("{direct} direct")).size(14.0).color(p.muted));
                });
            });
        } else {
            let breaking = a.impact.breaking_changes.len();
            stat_tile(&mut cols[2], "Breaking changes", |ui| {
                let c = if breaking > 0 { severity_color("high") } else { p.text };
                ui.label(RichText::new(breaking.to_string()).size(30.0).strong().color(c));
            });
        }
        stat_tile(&mut cols[3], "Open questions", |ui| {
            ui.label(
                RichText::new(a.intent.open_questions.len().to_string())
                    .size(30.0)
                    .strong()
                    .color(p.text),
            );
        });
    });

    card(ui, |ui| {
        section_title(ui, "Summary");
        body(ui, &a.summary);
        if !a.risk.recommended_review_depth.is_empty() {
            ui.add_space(6.0);
            callout(ui, "🎯", "Recommended review", &a.risk.recommended_review_depth);
        }
    });

    card(ui, |ui| {
        section_title(ui, "Look here first");
        if serious.is_empty() {
            ui.label(muted(ui, "No high or critical risks identified."));
        }
        for f in serious {
            ui.horizontal_top(|ui| {
                ui.allocate_ui_with_layout(
                    vec2(76.0, 20.0),
                    Layout::left_to_right(Align::Center),
                    |ui| {
                        ui.set_min_width(76.0);
                        solid_pill(ui, f.severity.to_uppercase(), severity_color(&f.severity))
                    },
                );
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    ui.label(RichText::new(&f.area).strong().color(p.text));
                    if !f.files.is_empty() {
                        ui.label(RichText::new(f.files.join(", ")).monospace().size(12.0).color(p.muted));
                    }
                });
            });
            ui.add_space(4.0);
        }
    });

    ui.columns(2, |cols| {
        match &services {
            Some(services) => {
                let names: Vec<String> = services
                    .iter()
                    .filter_map(|i| {
                        service_map::node(&i.service).map(|n| format!("{} ({}): {}", n.name, i.level, i.reason))
                    })
                    .collect();
                list_card(&mut cols[0], "🗺", "Impacted services", &names, p.accent_text);
            }
            None => list_card(&mut cols[0], "⚠", "Breaking changes", &a.impact.breaking_changes, severity_color("high")),
        }
        list_card(&mut cols[1], "❓", "Questions for the author", &a.intent.open_questions, p.accent_text);
    });
}

fn risk(ui: &mut egui::Ui, a: &Analysis) {
    let p = pal(ui);
    let c = severity_color(&a.risk.level);
    card(ui, |ui| {
        ui.horizontal(|ui| {
            solid_pill(ui, format!("{} RISK", a.risk.level.to_uppercase()), c);
            ui.label(RichText::new(format!("{:.1}/10", a.risk.score)).size(18.0).strong().color(c));
            meter(ui, a.risk.score / 10.0, c, 160.0);
        });
        ui.add_space(4.0);
        body(ui, &a.risk.rationale);
        if !a.risk.recommended_review_depth.is_empty() {
            ui.add_space(4.0);
            callout(ui, "🎯", "Recommended review", &a.risk.recommended_review_depth);
        }
    });

    if a.risk.has_breakdown() {
        risk_breakdown(ui, &a.risk);
    }

    ui.add_space(4.0);
    section_title(ui, &format!("Risk factors · {}", a.risk.factors.len()));
    if a.risk.factors.is_empty() {
        ui.label(muted(ui, "None identified."));
    }
    let mut factors = a.risk.factors.clone();
    factors.sort_by_key(|f| std::cmp::Reverse(severity_rank(&f.severity)));
    for f in &factors {
        accent_card(ui, severity_color(&f.severity), |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(&f.area).size(15.0).strong().color(p.text));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    pill(ui, f.severity.to_uppercase(), severity_color(&f.severity));
                });
            });
            body(ui, &f.description);
            if !f.files.is_empty() {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    for file in &f.files {
                        code_chip(ui, file);
                    }
                });
            }
            if !f.mitigation.is_empty() {
                callout(ui, "✔", "Verify", &f.mitigation);
            }
        });
    }
}

/// The three dimensions the risk score is computed from.
fn risk_breakdown(ui: &mut egui::Ui, r: &analysis::RiskReview) {
    let p = pal(ui);
    let complexity = r.complexity.score.clamp(0.0, 10.0);
    let impact = r.worst_case.impact().unwrap_or(0.0);
    let exposure = r.audience.factor().unwrap_or(0.0) * 10.0;
    let rows = [
        ("Complexity", format!("{complexity:.0}/10"), complexity, r.complexity.reason.as_str()),
        ("Worst case", humanize(&r.worst_case.tier), impact, r.worst_case.scenario.as_str()),
        ("Audience", format!("{}-facing", humanize(&r.audience.facing)), exposure, r.audience.reason.as_str()),
    ];
    card(ui, |ui| {
        section_title(ui, "How the score is computed");
        for (label, value, level, reason) in rows {
            let c = severity_color(analysis::level_for(level));
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(150.0);
                    ui.spacing_mut().item_spacing.y = 4.0;
                    ui.label(RichText::new(label).size(12.0).strong().color(p.muted));
                    pill(ui, value, c);
                    meter(ui, level / 10.0, c, 120.0);
                });
                ui.add(egui::Label::new(RichText::new(reason).color(p.text).line_height(Some(20.0))).wrap());
            });
            ui.add_space(6.0);
        }
        ui.label(muted(
            ui,
            format!(
                "Score = worst case × audience × ({:.1} + {:.1} × complexity / 10)",
                analysis::BASE_LIKELIHOOD,
                1.0 - analysis::BASE_LIKELIHOOD
            ),
        ).size(12.0));
    });
}

/// `core_flow` becomes `Core flow`.
fn humanize(s: &str) -> String {
    let s = s.trim().replace(['_', '-'], " ");
    let mut chars = s.chars();
    chars.next().map(|c| c.to_uppercase().chain(chars).collect()).unwrap_or_default()
}

fn intent(ui: &mut egui::Ui, a: &Analysis) {
    let p = pal(ui);
    let i = &a.intent;
    ui.columns(2, |cols| {
        card(&mut cols[0], |ui| {
            section_title(ui, "📝  What the PR says");
            body(ui, &i.stated_intent);
        });
        card(&mut cols[1], |ui| {
            section_title(ui, "🔬  What the code does");
            body(ui, &i.inferred_intent);
        });
    });
    card(ui, |ui| {
        section_title(ui, "⚖  Alignment");
        body(ui, &i.alignment);
    });

    ui.add_space(4.0);
    section_title(ui, &format!("Decision log · {}", i.decisions.len()));
    if i.decisions.is_empty() {
        ui.label(muted(ui, "No decisions recorded."));
    }
    for (n, d) in i.decisions.iter().enumerate() {
        ui.horizontal_top(|ui| {
            // Timeline marker
            let (rect, _) = ui.allocate_exact_size(vec2(26.0, 26.0), Sense::hover());
            ui.painter().circle_filled(rect.center(), 13.0, p.accent.gamma_multiply(0.2));
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                (n + 1).to_string(),
                egui::FontId::proportional(12.5),
                p.accent_text,
            );
            ui.vertical(|ui| {
                card(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::Label::new(RichText::new(&d.decision).size(15.0).strong().color(p.text))
                                .wrap(),
                        );
                    });
                    if !d.source.is_empty() {
                        pill(ui, &d.source, p.accent_text);
                    }
                    if !d.rationale.is_empty() {
                        labeled(ui, "Why", &d.rationale);
                    }
                    if !d.alternatives.is_empty() {
                        labeled(ui, "Alternatives", &d.alternatives);
                    }
                });
            });
        });
    }

    ui.add_space(4.0);
    list_card(ui, "❓", "Open questions", &i.open_questions, p.accent_text);
}

fn impact(ui: &mut egui::Ui, a: &Analysis) {
    let p = pal(ui);
    let i = &a.impact;

    card(ui, |ui| {
        section_title(ui, &format!("🗺  Affected components · {}", i.affected_components.len()));
        if i.affected_components.is_empty() {
            ui.label(muted(ui, "None identified."));
        }
        for c in &i.affected_components {
            ui.horizontal_top(|ui| {
                ui.allocate_ui_with_layout(
                    vec2(180.0, 20.0),
                    Layout::left_to_right(Align::Min),
                    |ui| {
                        ui.set_min_width(180.0);
                        ui.add(egui::Label::new(RichText::new(&c.name).strong().color(p.text)).truncate())
                    },
                );
                ui.add(egui::Label::new(RichText::new(&c.description).color(p.muted)).wrap());
            });
        }
    });

    let groups: [(&str, &str, &Vec<String>, Color32); 5] = [
        ("⚠", "Breaking changes", &i.breaking_changes, severity_color("critical")),
        ("🔗", "Downstream effects", &i.downstream_effects, Color32::from_rgb(56, 189, 248)),
        ("💾", "Data & migrations", &i.data_and_migrations, Color32::from_rgb(168, 85, 247)),
        ("🔒", "Security", &i.security, severity_color("high")),
        ("⚡", "Performance", &i.performance, severity_color("medium")),
    ];
    ui.columns(2, |cols| {
        for (n, (icon, title, items, color)) in groups.iter().enumerate() {
            list_card(&mut cols[n % 2], icon, title, items, *color);
            cols[n % 2].add_space(2.0);
        }
        card(&mut cols[1], |ui| {
            section_title(ui, "✅  Test coverage");
            body(ui, &i.test_coverage);
        });
    });
    list_card(ui, "🔬", "Suggested tests", &i.suggested_tests, severity_color("low"));
}

// ---------------------------------------------------------------- view helpers


fn labeled(ui: &mut egui::Ui, label: &str, text: &str) {
    let p = pal(ui);
    ui.label(RichText::new(label).size(12.0).strong().color(p.muted));
    body(ui, text);
}

fn stat_tile(ui: &mut egui::Ui, title: &str, add: impl FnOnce(&mut egui::Ui)) {
    card(ui, |ui| {
        ui.set_min_height(72.0);
        section_title(ui, title);
        add(ui);
    });
}

fn big_number(ui: &mut egui::Ui, n: usize, total: usize, unit: &str) {
    let p = pal(ui);
    ui.horizontal(|ui| {
        ui.label(RichText::new(n.to_string()).size(30.0).strong().color(p.text));
        ui.label(RichText::new(format!("/ {total} {unit}")).size(14.0).color(p.muted));
    });
}

fn list_card(ui: &mut egui::Ui, icon: &str, title: &str, items: &[String], color: Color32) {
    let p = pal(ui);
    card(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new(icon).color(color));
            section_title(ui, title);
            if !items.is_empty() {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    pill(ui, items.len().to_string(), color);
                });
            }
        });
        if items.is_empty() {
            ui.label(RichText::new("None identified").color(p.faint));
        }
        for item in items {
            ui.horizontal_top(|ui| {
                let (rect, _) = ui.allocate_exact_size(vec2(6.0, 20.0), Sense::hover());
                ui.painter().circle_filled(pos2(rect.center().x, rect.top() + 10.0), 2.5, color);
                ui.add(egui::Label::new(RichText::new(item).color(p.text).line_height(Some(20.0))).wrap());
            });
        }
    });
}

fn callout(ui: &mut egui::Ui, icon: &str, title: &str, text: &str) {
    let p = pal(ui);
    egui::Frame::new()
        .fill(p.accent.gamma_multiply(if ui.visuals().dark_mode { 0.12 } else { 0.07 }))
        .corner_radius(CornerRadius::same(8))
        .inner_margin(Margin::symmetric(12, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_top(|ui| {
                ui.label(RichText::new(icon).color(p.accent_text));
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    ui.label(RichText::new(title).size(12.0).strong().color(p.accent_text));
                    ui.add(egui::Label::new(RichText::new(text).color(p.text)).wrap());
                });
            });
        });
}

fn code_chip(ui: &mut egui::Ui, text: &str) {
    let p = pal(ui);
    egui::Frame::new()
        .fill(p.surface_sunken)
        .stroke(Stroke::new(1.0, p.border))
        .corner_radius(CornerRadius::same(6))
        .inner_margin(Margin::symmetric(6, 2))
        .show(ui, |ui| {
            ui.label(RichText::new(text).monospace().size(11.5).color(p.muted));
        });
}

fn indeterminate_bar(ui: &mut egui::Ui) {
    let p = pal(ui);
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 3.0), Sense::hover());
    ui.painter().rect_filled(rect, CornerRadius::same(2), p.surface_sunken);
    let t = ui.input(|i| i.time) as f32;
    let w = rect.width() * 0.3;
    let x = rect.left() + ((t * 0.7).fract() * 1.3 - 0.3) * rect.width();
    let seg = egui::Rect::from_min_size(pos2(x, rect.top()), vec2(w, rect.height()));
    ui.painter()
        .with_clip_rect(rect)
        .rect_filled(seg, CornerRadius::same(2), p.accent);
    ui.ctx().request_repaint();
}

fn banner(ui: &mut egui::Ui, text: &str) {
    let p = pal(ui);
    egui::Frame::new()
        .fill(p.danger.gamma_multiply(0.12))
        .stroke(Stroke::new(1.0, p.danger.gamma_multiply(0.4)))
        .corner_radius(CornerRadius::same(8))
        .inner_margin(Margin::symmetric(12, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_top(|ui| {
                ui.label(RichText::new("⚠").color(p.danger));
                ui.add(egui::Label::new(RichText::new(text).color(p.danger)).wrap());
            });
        });
}


fn intro(ui: &mut egui::Ui) {
    let p = pal(ui);
    ui.vertical_centered(|ui| {
        ui.add_space(20.0);
        ui.label(RichText::new("✨").size(32.0).color(p.accent_text));
        ui.label(RichText::new("Get an AI review of this pull request").size(18.0).strong().color(p.text));
        ui.label(RichText::new("Choose a model and click Analyze PR. Your local CLI reads the diff, commits and discussion.").color(p.muted));
    });
    ui.add_space(16.0);
    let items = [
        ("🛡", "Risk Review", "How risky is it, where, and how deep to review"),
        ("📝", "Intent & Decisions", "What it tries to do and the choices it makes"),
        ("🗺", "Impact", "What else in the system is affected"),
    ];
    ui.columns(3, |cols| {
        for (col, (icon, title, text)) in cols.iter_mut().zip(items) {
            card(col, |ui| {
                ui.label(RichText::new(icon).size(20.0).color(p.accent_text));
                ui.label(RichText::new(title).strong().color(p.text));
                ui.add(egui::Label::new(RichText::new(text).size(12.5).color(p.muted)).wrap());
            });
        }
    });
}

/// Compact line of impacted services for a PR card: direct ones first.
fn service_chips(ui: &mut egui::Ui, impacts: &[analysis::ServiceImpact]) {
    let p = pal(ui);
    let mut sorted: Vec<&analysis::ServiceImpact> = impacts.iter().collect();
    sorted.sort_by_key(|i| !i.level.eq_ignore_ascii_case("direct"));
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        ui.label(RichText::new("Impacts  ").size(11.0).color(p.faint));
        const MAX: usize = 3;
        let names: Vec<(&str, bool)> = sorted
            .iter()
            .filter_map(|i| service_map::node(&i.service).map(|n| (n.name, i.level.eq_ignore_ascii_case("direct"))))
            .collect();
        for (k, (name, direct)) in names.iter().take(MAX).enumerate() {
            if k > 0 {
                ui.label(RichText::new("  ·  ").size(11.0).color(p.faint));
            }
            let text = RichText::new(*name).size(11.0);
            ui.label(if *direct { text.color(p.accent_text).strong() } else { text.color(p.muted) });
        }
        if names.len() > MAX {
            ui.label(RichText::new(format!("  +{}", names.len() - MAX)).size(11.0).color(p.faint));
        }
    });
}

fn role_color(role: Role) -> Color32 {
    match role {
        Role::Assignee => Color32::from_rgb(59, 130, 246),
        Role::ReviewRequested => Color32::from_rgb(168, 85, 247),
        Role::Reviewed => Color32::from_rgb(20, 184, 166),
    }
}

fn ago_str(rfc3339: &str) -> String {
    DateTime::parse_from_rfc3339(rfc3339)
        .map(|t| ago(t.with_timezone(&Utc)))
        .unwrap_or_else(|_| rfc3339.to_string())
}

fn ago(t: DateTime<Utc>) -> String {
    let secs = (Utc::now() - t).num_seconds().max(0);
    match secs {
        0..60 => "just now".into(),
        60..3600 => format!("{}m ago", secs / 60),
        3600..86400 => format!("{}h ago", secs / 3600),
        _ => format!("{}d ago", secs / 86400),
    }
}

// ---------------------------------------------------------------- visual snapshots

#[cfg(test)]
mod snapshots {
    //! Renders the UI with demo data to PNGs. Run with:
    //! `MEMENTO_SNAPSHOT_DIR=/some/dir cargo test snapshots -- --nocapture`
    use super::*;
    use egui_kittest::Harness;

    fn demo_app() -> PRementoApp {
        let mut app = PRementoApp::build(Config::default(), None);
        app.user = Some("octocat".into());
        app.last_refresh = Some(Local::now());
        let now = Utc::now();
        let pr = |repo: &str, n: u64, title: &str, author: &str, hours: i64, roles: Vec<Role>, draft: bool| PullRequest {
            repo: repo.into(),
            number: n,
            title: title.into(),
            url: format!("https://github.com/{repo}/pull/{n}"),
            author: author.into(),
            is_draft: draft,
            updated_at: (now - chrono::Duration::hours(hours)).to_rfc3339(),
            labels: if n == 412 { vec!["auth".into(), "backend".into()] } else { vec![] },
            roles,
        };
        app.prs = vec![
            pr("attraction-inc/fibra", 412, "Rotate refresh tokens on every use and revoke reused tokens", "mira", 2, vec![Role::ReviewRequested], false),
            pr("attraction-inc/studio-frontend", 1288, "Migrate settings page to the new design system", "jonas", 5, vec![Role::Assignee, Role::Reviewed], false),
            pr("attraction-inc/workspace", 77, "Bump Node to 24 LTS across services", "sam", 26, vec![Role::Assignee], false),
            pr("attraction-inc/fibra", 415, "WIP: batch webhook deliveries", "lee", 50, vec![Role::ReviewRequested], true),
            pr("octo-org/mobile", 903, "Fix crash when opening deep links while logged out", "ana", 72, vec![Role::Reviewed], false),
        ];
        let analysis: Analysis = serde_json::from_str(DEMO).unwrap();
        let mut stored = |key: &str, complexity: f32, tier: &str, facing: &str| {
            let mut a = analysis.clone();
            a.risk.complexity.score = complexity;
            a.risk.worst_case.tier = tier.into();
            a.risk.audience.facing = facing.into();
            a.risk.compute();
            app.results.insert(key.into(), StoredAnalysis {
                provider: "Claude Code".into(),
                head_sha: "9f2c1e7ab".into(),
                created_at: (now - chrono::Duration::minutes(12)).timestamp(),
                duration_secs: 94,
                analysis: Some(a),
                parse_error: None,
                raw: DEMO.into(),
            });
        };
        stored("attraction-inc/fibra#412", 7.0, "system_wide", "customer");
        stored("attraction-inc/workspace#77", 4.0, "core_flow", "internal");
        stored("octo-org/mobile#903", 2.0, "feature", "customer");
        app.jobs.insert("attraction-inc/studio-frontend#1288".into(), Job {
            started: Instant::now(),
            provider: "Codex".into(),
            stage: "Codex is reviewing the changes…".into(),
            cancel: Default::default(),
        });
        app.selected = Some("attraction-inc/fibra#412".into());
        app
    }

    /// Every non-ASCII character in the UI source must exist in the bundled fonts,
    /// otherwise it renders as an empty box.
    #[test]
    fn all_ui_glyphs_exist() {
        let h = Harness::new_ui(|_ui| {});
        let src = concat!(include_str!("theme.rs"), include_str!("map_view.rs"), include_str!("service_map.rs"), include_str!("app.rs"));
        let demo_start = src.find("mod snapshots").unwrap_or(src.len());
        let mut missing: Vec<char> = src[..demo_start]
            .chars()
            .filter(|c| !c.is_ascii())
            .filter(|&c| {
                !h.ctx
                    .fonts_mut(|f| f.has_glyph(&egui::FontId::proportional(14.0), c))
            })
            .collect();
        missing.sort();
        missing.dedup();
        assert!(missing.is_empty(), "missing glyphs: {missing:?}");
    }

    #[test]
    fn render_screens() {
        let Ok(dir) = std::env::var("MEMENTO_SNAPSHOT_DIR") else { return };
        for (theme, tname) in [(egui::Theme::Dark, "dark"), (egui::Theme::Light, "light")] {
            let mut h = Harness::builder()
                .with_size(vec2(1440.0, 900.0))
                .with_pixels_per_point(1.0)
                .with_theme(theme)
                .wgpu()
                .build_eframe(|cc| {
                    theme::install(&cc.egui_ctx);
                    demo_app()
                });
            for (tab, name) in Tab::for_pr(true) {
                if tab == Tab::Raw { continue; }
                h.state_mut().tab = tab;
                h.run_steps(4);
                let img = h.render().unwrap();
                img.save(format!("{dir}/{tname}-{name}.png").replace(' ', "_")).unwrap();
            }
            h.state_mut().selected = Some("attraction-inc/studio-frontend#1288".into());
            h.run_steps(4);
            h.render().unwrap().save(format!("{dir}/{tname}-running.png")).unwrap();
            h.state_mut().selected = None;
            h.run_steps(4);
            h.render().unwrap().save(format!("{dir}/{tname}-empty.png")).unwrap();
            // Non attraction-inc PR keeps the generic Impact tab.
            h.state_mut().selected = Some("octo-org/mobile#903".into());
            h.state_mut().tab = Tab::Impact;
            h.run_steps(4);
            h.render().unwrap().save(format!("{dir}/{tname}-other-org-impact.png")).unwrap();
            // Ask tab with a session running (a shell standing in for Claude Code).
            let key = "attraction-inc/fibra#412".to_owned();
            let term = Terminal::spawn(
                &h.ctx,
                std::path::Path::new("/bin/sh"),
                vec!["-c".into(), "printf '\\033[1;35m> Claude Code\\033[0m\\n\\nThe PR rotates refresh tokens. Risk: \\033[31mcritical\\033[0m.\\n\\n> '; cat".into()],
                std::path::Path::new("/"),
            )
            .unwrap();
            h.state_mut().asks.insert(key.clone(), AskState::Running { label: "Claude Code · haiku".into(), dir: ask::folder(&key), term });
            h.state_mut().selected = Some(key);
            h.state_mut().tab = Tab::Ask;
            for _ in 0..10 {
                h.run_steps(1);
                std::thread::sleep(Duration::from_millis(50));
            }
            h.render().unwrap().save(format!("{dir}/{tname}-ask-running.png")).unwrap();
            h.state_mut().asks.clear();
        }
    }

    const DEMO: &str = r#"{
      "summary": "Introduces refresh-token rotation: every refresh issues a new token and invalidates the previous one. If a revoked token is presented again, the whole token family is revoked to contain stolen tokens. Adds a token_family column and a background job that prunes expired families.",
      "risk": {
        "complexity": {"score": 7, "reason": "Token rotation with reuse detection, a schema migration on a hot table and a new background job; concurrent refreshes need careful transaction handling."},
        "worst_case": {"tier": "system_wide", "scenario": "A bug in rotation or reuse detection logs every customer out, or silently weakens session security."},
        "audience": {"facing": "customer", "reason": "The refresh endpoint is called by the public website and mobile apps for every signed-in customer."},
        "rationale": "Touches the authentication hot path and changes the database schema. A bug here either logs every user out or silently weakens session security. Concurrency between parallel refresh requests from the same client is not obviously handled.",
        "recommended_review_depth": "Careful line-by-line review of the rotation logic plus a local test of two concurrent refreshes.",
        "factors": [
          {"area": "Race between concurrent refreshes", "severity": "critical", "description": "Two tabs refreshing at once will both present the same token; the second is treated as reuse and revokes the family, logging the user out.", "files": ["src/auth/refresh.rs"], "mitigation": "Ask for a short grace window or row-level lock on the token row."},
          {"area": "Migration on a large table", "severity": "high", "description": "Adding a NOT NULL column with a default to `sessions` rewrites the table on older Postgres versions.", "files": ["migrations/0042_token_family.sql"], "mitigation": "Confirm the production Postgres version or split into add-nullable + backfill."},
          {"area": "Pruning job deletes by created_at", "severity": "medium", "description": "Families are pruned by creation date rather than last use, which may delete active long-lived sessions.", "files": ["src/jobs/prune_tokens.rs"], "mitigation": "Prune on last_used_at instead."}
        ]
      },
      "intent": {
        "stated_intent": "Harden session security by rotating refresh tokens and detecting token reuse, as required by the security audit.",
        "inferred_intent": "Implements rotation and family revocation, and also changes the session TTL from 30 to 14 days, which is not mentioned in the description.",
        "alignment": "Mostly aligned. The TTL change is scope creep and has user-facing impact; it should be called out or split out.",
        "decisions": [
          {"decision": "Revoke the entire token family on reuse", "rationale": "Follows the OAuth 2.0 Security BCP recommendation (quoted in the description).", "alternatives": "Revoke only the reused token; alert without revoking.", "source": "description"},
          {"decision": "Store token hashes instead of raw tokens", "rationale": "Inferred: limits damage if the sessions table leaks.", "alternatives": "Encrypt tokens at rest.", "source": "code (inferred)"}
        ],
        "open_questions": ["How are concurrent refreshes from multiple tabs handled?", "Was the TTL change from 30 to 14 days intentional?", "Is there a rollout plan for already-issued tokens?"]
      },
      "impact": {
        "affected_components": [{"name": "Auth service", "description": "Refresh endpoint behaviour changes; clients must store the new token on every refresh."}, {"name": "Web & mobile clients", "description": "Must replace the stored refresh token after each call."}],
        "breaking_changes": ["Clients that reuse the old refresh token will be logged out.", "Session lifetime reduced from 30 to 14 days."],
        "downstream_effects": ["Mobile app versions older than 5.2 do not persist rotated tokens."],
        "data_and_migrations": ["New token_family column on sessions; backfill required."],
        "security": ["Detects and contains stolen refresh tokens."],
        "performance": [],
        "test_coverage": "Unit tests cover rotation and reuse, but there is no test for concurrent refreshes or for the migration.",
        "suggested_tests": ["Two concurrent refreshes with the same token", "Migration on a copy of production data"]
      },
      "service_impact": [
        {"service": "fibra", "level": "direct", "severity": "high", "reason": "Refresh endpoint and token storage change in the public API."},
        {"service": "backend", "level": "indirect", "severity": "medium", "reason": "Customer authentication relayed by Fibra now expects rotated tokens."},
        {"service": "website", "level": "indirect", "severity": "high", "reason": "Must store the new refresh token after every call or users get logged out."},
        {"service": "frontend", "level": "indirect", "severity": "low", "reason": "Product modal reads the catalogue with the same session."},
        {"service": "distributors", "level": "indirect", "severity": "low", "reason": "PromoStandards credentials unaffected; listed for awareness."}
      ]
    }"#;
}
