//! Asks the LLM which of the listed PRs belong together, and in which order they merge.

use std::collections::HashMap;

use anyhow::{Context, Result, anyhow};
use serde::Deserialize;

use super::model::{Link, LinkKind, Source};
use crate::features::pull_requests::PullRequest;

/// Description characters sent per PR.
const MAX_BODY: usize = 1200;

/// `briefs`: the "what" line of each analyzed PR, by key.
pub fn build_prompt(prs: &[PullRequest], briefs: &HashMap<String, String>) -> String {
    let mut list = String::new();
    for pr in prs {
        let key = pr.key();
        list.push_str(&format!("### {key} — {}\n", pr.title));
        let mut facts = vec![format!("Author: {}", pr.author)];
        if !pr.labels.is_empty() {
            facts.push(format!("Labels: {}", pr.labels.join(", ")));
        }
        if let (Some(head), Some(base)) = (&pr.head_ref, &pr.base_ref) {
            facts.push(format!("Branch: {head} -> {base}"));
        }
        if !pr.closes.is_empty() {
            facts.push(format!("Closes: {}", pr.closes.join(", ")));
        }
        if pr.is_draft {
            facts.push("Draft".into());
        }
        list.push_str(&facts.join(" · "));
        list.push('\n');
        if let Some(what) = briefs.get(&key).filter(|w| !w.trim().is_empty()) {
            list.push_str(&format!("Summary: {}\n", what.trim()));
        }
        let body = pr.body.trim();
        if !body.is_empty() {
            list.push_str("Description:\n");
            list.push_str(truncate(body, MAX_BODY));
            if body.len() > MAX_BODY {
                list.push_str(" […]");
            }
            list.push('\n');
        }
        list.push('\n');
    }

    format!(
        "You help a code reviewer see which of their open pull requests belong together.\n\
Below is their list. Find the PRs that are part of the same piece of work: for example a backend PR and the \
frontend PR that uses it, PRs on stacked branches (one PR's base branch is another's head branch), a migration \
and the code that needs it, or a feature split into parts. When one PR must be merged before another, say so.\n\
Link PRs only on concrete evidence: branches, titles, descriptions, a shared issue, the same feature named. \
Being in the same repository or touching similar areas is not enough. Returning no links is fine.\n\
Do not run tools or modify files; everything you need is below.\n\n\
Respond with ONLY a single JSON object (no markdown fences, no prose before or after) of this shape:\n\
{{\"links\": [{{\"from\": \"owner/repo#2\", \"to\": \"owner/repo#1\", \"kind\": \"depends_on\", \"reason\": \"at most 15 words\"}}]}}\n\
\"depends_on\" means `from` must be merged after `to`. Use \"related\" when the PRs belong together without an order. \
Use the exact keys from the headings below.\n\n\
==================== PULL REQUESTS ====================\n\n\
{list}"
    )
}

fn truncate(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

#[derive(Deserialize)]
struct Response {
    #[serde(default)]
    links: Vec<RawLink>,
}

#[derive(Deserialize)]
struct RawLink {
    from: String,
    to: String,
    #[serde(default)]
    kind: String,
    #[serde(default)]
    reason: String,
}

/// The links in an LLM response that may include fences or chatter. Links to keys that are not
/// in `prs` are dropped.
pub fn parse(raw: &str, prs: &[PullRequest]) -> Result<Vec<Link>> {
    let start = raw.find('{').ok_or_else(|| anyhow!("no JSON object in response"))?;
    let end = raw.rfind('}').ok_or_else(|| anyhow!("no JSON object in response"))?;
    if end < start {
        return Err(anyhow!("malformed JSON in response"));
    }
    let response: Response =
        serde_json::from_str(&raw[start..=end]).context("response JSON did not match the schema")?;
    let keys: HashMap<String, String> = prs.iter().map(|pr| (pr.key().to_ascii_lowercase(), pr.key())).collect();
    let resolve = |k: &str| keys.get(&k.trim().trim_start_matches("https://github.com/").to_ascii_lowercase()).cloned();
    Ok(response
        .links
        .into_iter()
        .filter_map(|l| {
            let (from, to) = (resolve(&l.from)?, resolve(&l.to)?);
            let kind = match l.kind.trim().to_ascii_lowercase().replace([' ', '-'], "_").as_str() {
                "depends_on" => LinkKind::DependsOn,
                _ => LinkKind::Related,
            };
            let reason = Some(l.reason.trim().to_owned()).filter(|r| !r.is_empty());
            (from != to).then_some(Link { from, to, kind, source: Source::Ai, reason })
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pr(repo: &str, number: u64) -> PullRequest {
        PullRequest { repo: repo.into(), number, title: format!("PR {number}"), ..Default::default() }
    }

    #[test]
    fn parses_links_and_drops_unknown_keys() {
        let prs = [pr("Acme/api", 1), pr("acme/web", 2)];
        let raw = "Sure:\n```json\n{\"links\":[\
            {\"from\":\"acme/web#2\",\"to\":\"acme/api#1\",\"kind\":\"depends_on\",\"reason\":\"uses the new endpoint\"},\
            {\"from\":\"acme/web#2\",\"to\":\"acme/web#99\",\"kind\":\"related\"},\
            {\"from\":\"acme/web#2\",\"to\":\"acme/web#2\",\"kind\":\"related\"}]}\n```";
        let links = parse(raw, &prs).unwrap();
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].from, "acme/web#2");
        assert_eq!(links[0].to, "Acme/api#1");
        assert_eq!(links[0].kind, LinkKind::DependsOn);
        assert_eq!(links[0].reason.as_deref(), Some("uses the new endpoint"));
        assert!(parse("{\"links\": []}", &prs).unwrap().is_empty());
        assert!(parse("nothing", &prs).is_err());
    }

    #[test]
    fn prompt_lists_every_pr() {
        let mut a = pr("acme/api", 1);
        a.body = "x".repeat(5000);
        a.head_ref = Some("feat".into());
        a.base_ref = Some("main".into());
        let p = build_prompt(&[a, pr("acme/web", 2)], &HashMap::from([("acme/web#2".into(), "Adds a page".into())]));
        assert!(p.contains("### acme/api#1 — PR 1"));
        assert!(p.contains("Branch: feat -> main"));
        assert!(p.contains("Summary: Adds a page"));
        assert!(p.len() < 4000, "bodies are truncated");
    }
}
