//! GitHub access through the `gh` CLI, which already handles authentication.

use std::collections::BTreeMap;
use std::process::Command;

use anyhow::{Context, Result, anyhow, bail};
use serde::Deserialize;
use serde_json::Value;

use super::model::{PullRequest, Role};

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
                    head_sha: None,
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
    fetch_head_shas(&mut prs);
    Ok(prs)
}

/// Fills in each PR's head commit, which `gh search prs` does not return, with one GraphQL
/// query per 100 PRs. Best effort: a PR whose head cannot be read keeps `None`.
fn fetch_head_shas(prs: &mut [PullRequest]) {
    for chunk in prs.chunks_mut(100) {
        let Some(query) = head_query(chunk) else { continue };
        // Not `gh()`: when some repositories cannot be read, gh exits non-zero but still prints
        // the data of the others.
        let Ok(out) = Command::new("gh").args(["api", "graphql", "-f", &format!("query={query}")]).output() else {
            return;
        };
        let Ok(v) = serde_json::from_slice::<Value>(&out.stdout) else { continue };
        for (i, pr) in chunk.iter_mut().enumerate() {
            pr.head_sha = v["data"][format!("p{i}")]["pullRequest"]["headRefOid"]
                .as_str()
                .filter(|sha| !sha.is_empty())
                .map(str::to_owned);
        }
    }
}

/// `query { p0: repository(owner: "o", name: "r") { pullRequest(number: 1) { headRefOid } } ... }`,
/// with `p{i}` for the i-th PR. `None` when no PR has a usable repository name.
fn head_query(prs: &[PullRequest]) -> Option<String> {
    let safe = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c));
    let fields: Vec<String> = prs
        .iter()
        .enumerate()
        .filter_map(|(i, pr)| {
            let (owner, name) = pr.repo.split_once('/').filter(|(o, n)| safe(o) && safe(n))?;
            Some(format!(
                r#"p{i}: repository(owner: "{owner}", name: "{name}") {{ pullRequest(number: {}) {{ headRefOid }} }}"#,
                pr.number
            ))
        })
        .collect();
    (!fields.is_empty()).then(|| format!("query {{ {} }}", fields.join(" ")))
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

#[cfg(test)]
mod tests {
    use super::*;

    fn pr(repo: &str, number: u64) -> PullRequest {
        PullRequest {
            repo: repo.into(),
            number,
            title: String::new(),
            url: String::new(),
            author: String::new(),
            is_draft: false,
            updated_at: String::new(),
            labels: vec![],
            roles: vec![],
            head_sha: None,
        }
    }

    #[test]
    fn head_query_aliases_by_position_and_skips_odd_repo_names() {
        let q = head_query(&[pr("o/a", 1), pr("o/\"x", 2), pr("o/b.c", 3)]).unwrap();
        assert_eq!(
            q,
            r#"query { p0: repository(owner: "o", name: "a") { pullRequest(number: 1) { headRefOid } } p2: repository(owner: "o", name: "b.c") { pullRequest(number: 3) { headRefOid } } }"#
        );
        assert!(head_query(&[pr("no-slash", 1)]).is_none());
    }
}
