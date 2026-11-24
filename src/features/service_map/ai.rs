//! Asks the LLM for a simplified service map of a scanned folder.

use std::collections::HashSet;

use anyhow::{Context, Result, anyhow, bail};
use serde::Deserialize;

use super::graph::{Edge, Node, ServiceMap};
use super::scan::Scan;

pub fn build_prompt(folder_name: &str, scan: &Scan) -> String {
    let repos = if scan.repos.is_empty() {
        "(none found: the folder is not made of git repositories)".to_string()
    } else {
        scan.repos.iter().map(|r| format!("- {}", r.name)).collect::<Vec<_>>().join("\n")
    };
    format!(
        "You help a code reviewer understand a software system. Below are the repositories of the \"{folder_name}\" \
folder, with their file listings, docs, compose files and manifests.\n\
Draw a SIMPLIFIED service map: the runtime services, the apps users touch, the data stores and external systems \
they talk to, the shared packages, and the infra/tooling repos, with the main connections between them.\n\
Rules:\n\
- At most 20 nodes. Merge small libraries or helper repos into the service they belong to; skip repos that are \
irrelevant at runtime (docs only, archived experiments) unless they are infra or tooling.\n\
- `id`: short kebab-case, unique. `name`: 1 to 3 words. `description`: one line, max 15 words.\n\
- `kind`: `surface` (an app or site people use), `service` (a backend or worker), `data` (a database, bucket or \
data file), `external` (a third-party or company system outside these repos), `package` (a shared library used by \
other repos), `tooling` (infra, CI, deploy, local stack, tests).\n\
- `repos`: names copied exactly from the REPOSITORIES list. A repo may belong to one node only. External systems \
and data stores have no repos.\n\
- `edges`: `from` calls, uses or depends on `to`; `label` at most 5 words (\"REST API\", \"events\", \"package\"). \
Only connections you have evidence for (config, env vars, compose links, dependencies, docs).\n\
- `notes`: up to 3 short lines on who is exposed to which part (customers, partners, internal staff), if the docs \
say so.\n\
Do not run tools or modify files; everything you need is below.\n\n\
Respond with ONLY a single JSON object (no markdown fences, no prose before or after) of this shape:\n\
{{\"name\": \"system name\", \"nodes\": [{{\"id\": \"api\", \"name\": \"Public API\", \"kind\": \"service\", \
\"repos\": [\"owner/api\"], \"description\": \"...\"}}], \"edges\": [{{\"from\": \"web\", \"to\": \"api\", \
\"label\": \"REST API\"}}], \"notes\": [\"...\"]}}\n\n\
==================== REPOSITORIES ====================\n{repos}\n\n{}",
        scan.text
    )
}

#[derive(Deserialize)]
struct Response {
    #[serde(default)]
    name: String,
    #[serde(default)]
    nodes: Vec<Node>,
    #[serde(default)]
    edges: Vec<Edge>,
    #[serde(default)]
    notes: Vec<String>,
}

/// The map in an LLM response that may include fences or chatter. Duplicate nodes, and edges
/// to unknown nodes or to themselves, are dropped. Only `name`, `nodes`, `edges` and `notes`
/// are set.
pub fn parse(raw: &str) -> Result<ServiceMap> {
    let start = raw.find('{').ok_or_else(|| anyhow!("no JSON object in response"))?;
    let end = raw.rfind('}').ok_or_else(|| anyhow!("no JSON object in response"))?;
    if end < start {
        bail!("malformed JSON in response");
    }
    let r: Response = serde_json::from_str(&raw[start..=end]).context("response JSON did not match the map schema")?;

    let mut seen = HashSet::new();
    let nodes: Vec<Node> = r
        .nodes
        .into_iter()
        .map(|mut n| {
            n.id = n.id.trim().to_string();
            n.name = Some(n.name.trim().to_string()).filter(|s| !s.is_empty()).unwrap_or_else(|| n.id.clone());
            n
        })
        .filter(|n| !n.id.is_empty() && seen.insert(n.id.clone()))
        .collect();
    if nodes.is_empty() {
        bail!("the model returned no services");
    }
    let mut seen_edges = HashSet::new();
    let edges = r
        .edges
        .into_iter()
        .filter(|e| e.from != e.to && seen.contains(&e.from) && seen.contains(&e.to))
        .filter(|e| seen_edges.insert((e.from.clone(), e.to.clone())))
        .collect();
    Ok(ServiceMap {
        name: r.name.trim().to_string(),
        nodes,
        edges,
        notes: r.notes.into_iter().map(|n| n.trim().to_string()).filter(|n| !n.is_empty()).collect(),
        ..Default::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::service_map::graph::Kind;
    use crate::features::service_map::scan::Repo;

    #[test]
    fn parses_and_cleans_the_map() {
        let raw = "Here:\n```json\n{\"name\":\"Acme\",\"nodes\":[\
            {\"id\":\"web\",\"name\":\"Web\",\"kind\":\"surface\",\"repos\":[\"acme/web\"]},\
            {\"id\":\"api\",\"name\":\"\",\"kind\":\"service\"},\
            {\"id\":\"api\",\"name\":\"Dup\"}],\
            \"edges\":[{\"from\":\"web\",\"to\":\"api\",\"label\":\"REST\"},\
            {\"from\":\"web\",\"to\":\"api\",\"label\":\"again\"},\
            {\"from\":\"web\",\"to\":\"ghost\"},{\"from\":\"api\",\"to\":\"api\"}],\
            \"notes\":[\" web is public \",\"\"]}\n```";
        let map = parse(raw).unwrap();
        assert_eq!(map.name, "Acme");
        assert_eq!(map.nodes.len(), 2);
        assert_eq!(map.nodes[0].kind, Kind::Surface);
        assert_eq!(map.nodes[1].name, "api", "empty name falls back to the id");
        assert_eq!(map.edges.len(), 1);
        assert_eq!(map.edges[0].label, "REST");
        assert_eq!(map.notes, ["web is public"]);
    }

    #[test]
    fn rejects_empty_or_non_json() {
        assert!(parse("{\"nodes\": []}").is_err());
        assert!(parse("sorry").is_err());
    }

    #[test]
    fn prompt_lists_repos() {
        let scan = Scan { repos: vec![Repo { name: "acme/api".into(), dir: "/x/api".into() }], text: "BODY".into() };
        let p = build_prompt("acme", &scan);
        assert!(p.contains("- acme/api"));
        assert!(p.ends_with("BODY"));
    }
}
