//! GitHub access through the `gh` CLI, which already handles authentication.

use std::collections::BTreeMap;
use std::process::Command;

use anyhow::{Context, Result, anyhow, bail};
use serde::Deserialize;
use serde_json::Value;

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

#[derive(Debug, Clone)]
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
}

impl PullRequest {
    pub fn key(&self) -> String {
        format!("{}#{}", self.repo, self.number)
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SearchItem {
    number: u64,
    title: String,
    url: String,
    #[serde(default)]
    is_draft: bool,
    #[serde(default)]
    updated_at: String,
    repository: SearchRepo,
    author: Option<Login>,
    #[serde(default)]
    labels: Vec<Label>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SearchRepo {
    name_with_owner: String,
}

#[derive(Deserialize)]
struct Login {
    login: String,
}

#[derive(Deserialize)]
struct Label {
    name: String,
}

fn gh(args: &[&str]) -> Result<String> {
    let out = Command::new("gh")
        .args(args)
        .output()
        .context("failed to run `gh` — is the GitHub CLI installed and on PATH?")?;
    if !out.status.success() {
        bail!(
            "gh {} failed: {}",
            args.first().unwrap_or(&""),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

pub fn current_user() -> Result<String> {
    Ok(gh(&["api", "user", "--jq", ".login"])?.trim().to_string())
}

/// Open PRs where the current user is assignee, requested reviewer, or has already reviewed.
pub fn fetch_prs(limit: u32) -> Result<Vec<PullRequest>> {
    let limit = limit.to_string();
    let queries = [
        (Role::Assignee, "--assignee=@me"),
        (Role::ReviewRequested, "--review-requested=@me"),
        (Role::Reviewed, "--reviewed-by=@me"),
    ];

    let mut by_url: BTreeMap<String, PullRequest> = BTreeMap::new();
    for (role, filter) in queries {
        let json = gh(&[
            "search",
            "prs",
            filter,
            "--state=open",
            "--limit",
            &limit,
            "--json",
            "number,title,url,isDraft,updatedAt,repository,author,labels",
        ])?;
        let items: Vec<SearchItem> =
            serde_json::from_str(&json).context("parsing `gh search prs` output")?;
        for item in items {
            let pr = by_url
                .entry(item.url.clone())
                .or_insert_with(|| PullRequest {
                    repo: item.repository.name_with_owner,
                    number: item.number,
                    title: item.title,
                    url: item.url,
                    author: item.author.map(|a| a.login).unwrap_or_default(),
                    is_draft: item.is_draft,
                    updated_at: item.updated_at,
                    labels: item.labels.into_iter().map(|l| l.name).collect(),
                    roles: vec![],
                });
            if !pr.roles.contains(&role) {
                pr.roles.push(role);
            }
        }
    }

    let mut prs: Vec<_> = by_url.into_values().collect();
    for pr in &mut prs {
        pr.roles.sort();
    }
    prs.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    Ok(prs)
}

/// Everything the LLM needs to know about a PR, rendered as a single text document.
pub struct PrContext {
    pub head_sha: String,
    pub text: String,
}

pub fn fetch_context(repo: &str, number: u64, max_diff_chars: usize) -> Result<PrContext> {
    let n = number.to_string();
    let view = gh(&[
        "pr",
        "view",
        &n,
        "-R",
        repo,
        "--json",
        "title,body,author,baseRefName,headRefName,headRefOid,additions,deletions,changedFiles,files,commits,comments,reviews,labels",
    ])?;
    let v: Value = serde_json::from_str(&view).context("parsing `gh pr view` output")?;

    let diff = gh(&["pr", "diff", &n, "-R", repo])?;
    // Inline review comments are where most design discussion happens; tolerate failure.
    let review_comments: Value = gh(&[
        "api",
        &format!("repos/{repo}/pulls/{number}/comments?per_page=100"),
    ])
    .ok()
    .and_then(|s| serde_json::from_str(&s).ok())
    .unwrap_or(Value::Null);

    let s = |val: &Value| val.as_str().unwrap_or("").to_string();
    let login = |val: &Value| s(&val["login"]);

    let mut t = String::new();
    let mut push = |line: String| {
        t.push_str(&line);
        t.push('\n');
    };

    push(format!("# PR {repo}#{number}: {}", s(&v["title"])));
    push(format!("Author: {}", login(&v["author"])));
    push(format!(
        "Branch: {} -> {}",
        s(&v["headRefName"]),
        s(&v["baseRefName"])
    ));
    push(format!(
        "Size: {} files, +{} / -{}",
        v["changedFiles"], v["additions"], v["deletions"]
    ));
    let labels: Vec<String> = v["labels"]
        .as_array()
        .map(|a| a.iter().map(|l| s(&l["name"])).collect())
        .unwrap_or_default();
    if !labels.is_empty() {
        push(format!("Labels: {}", labels.join(", ")));
    }

    push("\n## Description".into());
    let body = s(&v["body"]);
    push(if body.trim().is_empty() {
        "(no description)".into()
    } else {
        body
    });

    push("\n## Commits".into());
    for c in v["commits"].as_array().into_iter().flatten() {
        let oid = s(&c["oid"]);
        push(format!(
            "- {} {}",
            &oid[..oid.len().min(8)],
            s(&c["messageHeadline"])
        ));
        let body = s(&c["messageBody"]);
        if !body.trim().is_empty() {
            push(format!("    {}", body.trim().replace('\n', "\n    ")));
        }
    }

    push("\n## Changed files".into());
    for f in v["files"].as_array().into_iter().flatten() {
        push(format!(
            "- {} (+{} / -{})",
            s(&f["path"]),
            f["additions"],
            f["deletions"]
        ));
    }

    push("\n## Conversation".into());
    for c in v["comments"].as_array().into_iter().flatten() {
        push(format!("- {}: {}", login(&c["author"]), s(&c["body"]).trim()));
    }
    for r in v["reviews"].as_array().into_iter().flatten() {
        let body = s(&r["body"]);
        push(format!(
            "- review by {} [{}]: {}",
            login(&r["author"]),
            s(&r["state"]),
            body.trim()
        ));
    }
    for c in review_comments.as_array().into_iter().flatten() {
        push(format!(
            "- inline on {}:{} by {}: {}",
            s(&c["path"]),
            c["line"],
            login(&c["user"]),
            s(&c["body"]).trim()
        ));
    }

    push("\n## Diff".into());
    push(truncate(&diff, max_diff_chars));

    let head_sha = s(&v["headRefOid"]);
    if head_sha.is_empty() {
        return Err(anyhow!("PR has no head commit"));
    }
    Ok(PrContext { head_sha, text: t })
}

fn truncate(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_string();
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!(
        "{}\n\n[... diff truncated: {} of {} characters shown ...]",
        &text[..end],
        end,
        text.len()
    )
}
