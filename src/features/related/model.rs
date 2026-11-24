//! Links between PRs, the PRs they reach outside the user's list, and the AI's cached result.

use serde::{Deserialize, Serialize};

/// Where a link comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Source {
    /// Written in a PR description, or two PRs closing the same issue.
    Body,
    /// Suggested by the LLM.
    Ai,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LinkKind {
    /// `from` must merge after `to`.
    DependsOn,
    /// Same piece of work, no order.
    Related,
}

/// A link between two PRs, by key (`owner/repo#N`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Link {
    pub from: String,
    pub to: String,
    pub kind: LinkKind,
    pub source: Source,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrState {
    Open,
    Merged,
    Closed,
}

/// A linked PR that is not in the user's list.
#[derive(Debug, Clone)]
pub struct OutsidePr {
    pub key: String,
    pub title: String,
    pub url: String,
    pub state: PrState,
}

/// The AI's links, as cached on disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredRelations {
    /// The PRs (and their heads) the AI saw; see [`super::cache::fingerprint`].
    pub fingerprint: String,
    pub provider: String,
    pub created_at: i64,
    pub links: Vec<Link>,
}
