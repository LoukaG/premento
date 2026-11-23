//! The PR list: loading it from GitHub, filtering, searching and the selected PR.

use anyhow::Result;
use chrono::{DateTime, Local};

use super::{PullRequest, Role, github};
use crate::features::analysis::{StoredAnalysis, cache};

/// Result of a refresh, with the cached analyses of the PRs found.
pub struct Loaded {
    user: Option<String>,
    prs: Vec<PullRequest>,
    cached: Vec<(String, StoredAnalysis)>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Filter {
    All,
    Assigned,
    Reviewer,
}

pub struct PrList {
    pub user: Option<String>,
    pub prs: Vec<PullRequest>,
    pub loading: bool,
    pub error: Option<String>,
    pub last_refresh: Option<DateTime<Local>>,
    pub filter: Filter,
    pub search: String,
    /// Key of the selected PR.
    pub selected: Option<String>,
}

impl Default for PrList {
    fn default() -> Self {
        Self {
            user: None,
            prs: vec![],
            loading: false,
            error: None,
            last_refresh: None,
            filter: Filter::All,
            search: String::new(),
            selected: None,
        }
    }
}

impl PrList {
    /// Reloads the PRs in the background; `emit` receives the result on that thread.
    pub fn refresh(&mut self, limit: u32, emit: impl FnOnce(Result<Loaded>) + Send + 'static) {
        if self.loading {
            return;
        }
        self.loading = true;
        self.error = None;
        let need_user = self.user.is_none();
        std::thread::spawn(move || {
            let result = (|| -> Result<Loaded> {
                let user = if need_user { github::current_user().ok() } else { None };
                let prs = github::fetch_prs(limit)?;
                let cached = prs
                    .iter()
                    .filter_map(|pr| cache::load_cached(&pr.key()).map(|a| (pr.key(), a)))
                    .collect();
                Ok(Loaded { user, prs, cached })
            })();
            emit(result);
        });
    }

    /// Applies a finished refresh and returns the cached analyses it found.
    pub fn on_loaded(&mut self, result: Result<Loaded>) -> Vec<(String, StoredAnalysis)> {
        self.loading = false;
        match result {
            Ok(loaded) => {
                if loaded.user.is_some() {
                    self.user = loaded.user;
                }
                self.prs = loaded.prs;
                self.last_refresh = Some(Local::now());
                loaded.cached
            }
            Err(e) => {
                self.error = Some(format!("{e:#}"));
                vec![]
            }
        }
    }

    pub fn find(&self, key: &str) -> Option<&PullRequest> {
        self.prs.iter().find(|pr| pr.key() == key)
    }

    pub fn selected_pr(&self) -> Option<&PullRequest> {
        self.selected.as_deref().and_then(|k| self.find(k))
    }

    /// The PRs matching the filter and the search text.
    pub fn visible(&self) -> Vec<&PullRequest> {
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
}
