//! Analyses per PR: the ones running in the background, their results and their errors.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use anyhow::Result;
use chrono::Utc;

use super::{StoredAnalysis, cache, prompt};
use crate::features::providers::{Selection, runner};
use crate::features::pull_requests::{PullRequest, github};

/// An analysis running in the background.
pub struct Job {
    pub started: Instant,
    pub provider: String,
    pub stage: String,
    pub cancel: Arc<AtomicBool>,
}

/// Progress of a background analysis, keyed by PR.
pub enum Event {
    Stage(String, String),
    Done(String, Box<Result<StoredAnalysis>>),
}

#[derive(Default)]
pub struct Analyses {
    pub jobs: HashMap<String, Job>,
    pub results: HashMap<String, StoredAnalysis>,
    pub errors: HashMap<String, String>,
}

impl Analyses {
    /// Analyzes `pr` in the background with the picked provider. `service_map`: the
    /// architecture of the system the PR belongs to, if a map lists its repo. `emit` receives
    /// the progress on the background thread.
    pub fn start(
        &mut self,
        pr: &PullRequest,
        selection: Selection,
        service_map: Option<String>,
        max_diff: usize,
        timeout: Duration,
        emit: impl Fn(Event) + Send + 'static,
    ) {
        let key = pr.key();
        if self.jobs.contains_key(&key) {
            return;
        }
        let Selection { provider, model, label } = selection;
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

        let repo = pr.repo.clone();
        let number = pr.number;
        std::thread::spawn(move || {
            let stage = |s: String| emit(Event::Stage(key.clone(), s));
            let result = (|| -> Result<StoredAnalysis> {
                let pr_ctx = github::fetch_context(&repo, number, max_diff)?;
                stage(format!("{label} is reviewing the changes…"));
                let started = Instant::now();
                let prompt = prompt::build_prompt(&pr_ctx.text, service_map.as_deref());
                let raw = runner::run(&provider, model.as_deref(), &prompt, timeout, cancel)?;
                let (analysis, parse_error) = match prompt::parse(&raw) {
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
                let _ = cache::save_cached(&key, &stored);
                Ok(stored)
            })();
            emit(Event::Done(key.clone(), Box::new(result)));
        });
    }

    pub fn on_event(&mut self, event: Event) {
        match event {
            Event::Stage(key, stage) => {
                if let Some(job) = self.jobs.get_mut(&key) {
                    job.stage = stage;
                }
            }
            Event::Done(key, result) => {
                self.jobs.remove(&key);
                match *result {
                    Ok(stored) => {
                        if let Some(err) = &stored.parse_error {
                            self.errors.insert(
                                key.clone(),
                                format!("Could not parse the model's response ({err}). Try running the analysis again."),
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

    /// Adds analyses found in the cache, keeping any newer result already in memory.
    pub fn add_cached(&mut self, cached: Vec<(String, StoredAnalysis)>) {
        for (key, stored) in cached {
            self.results.entry(key).or_insert(stored);
        }
    }
}
