//! The related-PR groups of the current list, and the AI job that suggests links.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use anyhow::Result;
use chrono::Utc;

use super::group::{self, Group};
use super::model::{Link, OutsidePr, StoredRelations};
use super::{ai, body, cache};
use crate::features::providers::{Selection, runner};
use crate::features::pull_requests::{PullRequest, github};

/// What a refresh found, gathered on its background thread.
#[derive(Default)]
pub struct Fetched {
    links: Vec<Link>,
    outside: Vec<OutsidePr>,
    cached: Option<StoredRelations>,
    fingerprint: String,
}

/// Parses the descriptions of `prs`, reads the PRs they link to outside the list, and loads
/// the AI's cached links. Blocking.
pub fn fetch(prs: &[PullRequest]) -> Fetched {
    let links: Vec<Link> = prs.iter().flat_map(|pr| body::parse_links(&pr.key(), &pr.repo, &pr.body)).collect();
    let listed: Vec<String> = prs.iter().map(|pr| pr.key().to_ascii_lowercase()).collect();
    let mut wanted: Vec<String> = links
        .iter()
        .flat_map(|l| [&l.from, &l.to])
        .filter(|k| !listed.contains(&k.to_ascii_lowercase()))
        .cloned()
        .collect();
    wanted.sort_by_key(|k| k.to_ascii_lowercase());
    wanted.dedup_by_key(|k| k.to_ascii_lowercase());
    Fetched {
        links,
        outside: github::fetch_outside(&wanted),
        cached: cache::load_cached(),
        fingerprint: cache::fingerprint(prs),
    }
}

/// The AI job running in the background.
pub struct Job {
    pub started: Instant,
    pub provider: String,
    pub cancel: Arc<AtomicBool>,
}

pub enum Event {
    Done(Box<Result<StoredRelations>>),
}

/// One item of the sidebar list.
pub enum Entry<'a> {
    Single(&'a PullRequest),
    Group(usize),
}

#[derive(Default)]
pub struct Relations {
    /// Links found in the descriptions.
    pub body_links: Vec<Link>,
    /// Linked PRs that are not in the list, by key.
    pub outside: HashMap<String, OutsidePr>,
    /// The AI's last links.
    pub ai: Option<StoredRelations>,
    /// The list changed since the AI's links were made.
    pub stale: bool,
    fingerprint: String,
    pub job: Option<Job>,
    pub error: Option<String>,
    pub groups: Vec<Group>,
    /// Index in `groups` of each member.
    group_of: HashMap<String, usize>,
    /// Inputs the groups were built from; see [`Relations::update`].
    built_for: Option<(u64, usize)>,
    generation: u64,
}

impl Relations {
    pub fn on_fetched(&mut self, fetched: Fetched) {
        self.body_links = fetched.links;
        self.outside = fetched.outside.into_iter().map(|o| (o.key.clone(), o)).collect();
        self.fingerprint = fetched.fingerprint;
        if let Some(cached) = fetched.cached {
            self.ai = Some(cached);
        }
        self.stale = self.ai.as_ref().is_some_and(|a| a.fingerprint != self.fingerprint);
        self.generation += 1;
    }

    /// Rebuilds the groups when the links or the PRs changed. Cheap when nothing did.
    pub fn update(&mut self, prs: &[PullRequest]) {
        let inputs = (self.generation, prs.len());
        if self.built_for == Some(inputs) {
            return;
        }
        self.built_for = Some(inputs);
        let mut links = self.body_links.clone();
        if let Some(ai) = &self.ai {
            links.extend(ai.links.iter().cloned());
        }
        self.groups = group::build(prs, &self.outside, &links);
        self.group_of = self
            .groups
            .iter()
            .enumerate()
            .flat_map(|(i, g)| g.members.iter().map(move |m| (m.key.clone(), i)))
            .collect();
    }

    pub fn group_of(&self, key: &str) -> Option<&Group> {
        self.group_of.get(key).map(|&i| &self.groups[i])
    }

    /// `visible` (already sorted) as list items: each group sits where its first visible
    /// member would be.
    pub fn entries<'a>(&self, visible: &[&'a PullRequest]) -> Vec<Entry<'a>> {
        let mut shown = vec![false; self.groups.len()];
        let mut entries = vec![];
        for pr in visible {
            match self.group_of.get(&pr.key()) {
                Some(&g) if !shown[g] => {
                    shown[g] = true;
                    entries.push(Entry::Group(g));
                }
                Some(_) => {}
                None => entries.push(Entry::Single(pr)),
            }
        }
        entries
    }

    /// Asks the picked provider for links between `prs` in the background, or cancels the job
    /// already running.
    pub fn start_or_cancel(
        &mut self,
        prs: &[PullRequest],
        briefs: HashMap<String, String>,
        selection: Selection,
        timeout: Duration,
        emit: impl Fn(Event) + Send + 'static,
    ) {
        if let Some(job) = &self.job {
            job.cancel.store(true, Ordering::Relaxed);
            return;
        }
        if prs.len() < 2 {
            return;
        }
        let Selection { provider, model, label } = selection;
        let cancel = Arc::new(AtomicBool::new(false));
        self.error = None;
        self.job = Some(Job { started: Instant::now(), provider: label.clone(), cancel: cancel.clone() });
        let prs = prs.to_vec();
        std::thread::spawn(move || {
            let result = (|| -> Result<StoredRelations> {
                let prompt = ai::build_prompt(&prs, &briefs);
                let raw = runner::run(&provider, model.as_deref(), &prompt, timeout, cancel)?;
                let stored = StoredRelations {
                    fingerprint: cache::fingerprint(&prs),
                    provider: label,
                    created_at: Utc::now().timestamp(),
                    links: ai::parse(&raw, &prs)?,
                };
                let _ = cache::save_cached(&stored);
                Ok(stored)
            })();
            emit(Event::Done(Box::new(result)));
        });
    }

    pub fn on_event(&mut self, event: Event) {
        match event {
            Event::Done(result) => {
                self.job = None;
                match *result {
                    Ok(stored) => {
                        self.stale = stored.fingerprint != self.fingerprint;
                        self.ai = Some(stored);
                        self.generation += 1;
                    }
                    Err(e) => self.error = Some(format!("Could not group related PRs: {e:#}")),
                }
            }
        }
    }
}
