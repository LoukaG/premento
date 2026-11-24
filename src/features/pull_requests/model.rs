//! A pull request as listed in the sidebar, and the user's role on it.

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Role {
    Assignee,
    ReviewRequested,
    Reviewed,
}

impl Role {
    pub fn label(self) -> &'static str {
        match self {
            Role::Assignee => "assigned",
            Role::ReviewRequested => "review requested",
            Role::Reviewed => "reviewed",
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct PullRequest {
    pub repo: String,
    pub number: u64,
    pub title: String,
    pub url: String,
    pub author: String,
    pub is_draft: bool,
    pub updated_at: String,
    pub labels: Vec<String>,
    pub roles: Vec<Role>,
    /// The PR's current head commit; `None` when GitHub could not be asked for it.
    pub head_sha: Option<String>,
    /// The description; empty when GitHub could not be asked for it.
    pub body: String,
    /// Branch the PR merges into, and its own branch.
    pub base_ref: Option<String>,
    pub head_ref: Option<String>,
    /// Issues the PR closes when merged, as `owner/repo#N`.
    pub closes: Vec<String>,
}

impl PullRequest {
    pub fn key(&self) -> String {
        format!("{}#{}", self.repo, self.number)
    }
}
