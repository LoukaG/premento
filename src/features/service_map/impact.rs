//! Which services a PR impacts, and the service map as LLM prompt text.

use super::graph::{AUDIENCE_NOTES, EDGES, NODES, Node, ORG};
use crate::features::analysis::ServiceImpact;

pub fn applies(repo: &str) -> bool {
    repo.split('/')
        .next()
        .is_some_and(|org| org.eq_ignore_ascii_case(ORG))
}

pub fn node(id: &str) -> Option<&'static Node> {
    NODES.iter().find(|n| n.id == id)
}

/// Services whose code lives in `owner/repo`.
pub fn nodes_for_repo(repo: &str) -> Vec<&'static Node> {
    let name = repo.rsplit('/').next().unwrap_or(repo);
    NODES
        .iter()
        .filter(|n| n.repos.iter().any(|r| r.eq_ignore_ascii_case(name)))
        .collect()
}

/// Nodes that call / depend on `id` (incoming edges).
pub fn dependents(id: &str) -> impl Iterator<Item = &'static str> + '_ {
    EDGES.iter().filter(move |e| e.to == id).map(|e| e.from)
}

/// Best-effort impact without an LLM: the repo's own services, plus their callers.
pub fn heuristic_impact(repo: &str) -> Vec<ServiceImpact> {
    let mut out: Vec<ServiceImpact> = vec![];
    for n in nodes_for_repo(repo) {
        out.push(ServiceImpact {
            service: n.id.into(),
            level: "direct".into(),
            severity: "medium".into(),
            reason: format!("The PR changes the {} repository.", n.name),
        });
    }
    let direct: Vec<String> = out.iter().map(|i| i.service.clone()).collect();
    for d in &direct {
        for caller in dependents(d) {
            if !out.iter().any(|i| i.service == caller) {
                out.push(ServiceImpact {
                    service: caller.into(),
                    level: "indirect".into(),
                    severity: "low".into(),
                    reason: format!("Calls {} — may be affected by its changes.", node(d).map_or(d.as_str(), |n| n.name)),
                });
            }
        }
    }
    out
}

/// The impact to display: the LLM's answer when present (plus the PR's own services, which are
/// always directly impacted), otherwise the repository heuristic.
pub fn resolve_impact(repo: &str, from_llm: &[ServiceImpact]) -> (Vec<ServiceImpact>, bool) {
    let known: Vec<ServiceImpact> = from_llm
        .iter()
        .filter(|i| node(&i.service).is_some())
        .cloned()
        .collect();
    if known.is_empty() {
        return (heuristic_impact(repo), false);
    }
    let mut out = known;
    for n in nodes_for_repo(repo) {
        if !out.iter().any(|i| i.service == n.id) {
            out.push(ServiceImpact {
                service: n.id.into(),
                level: "direct".into(),
                severity: "medium".into(),
                reason: format!("The PR changes the {} repository.", n.name),
            });
        }
    }
    (out, true)
}

/// Architecture description handed to the LLM.
pub fn prompt_section(repo: &str) -> String {
    let mut s = String::from(
        "The PR belongs to the attraction-inc system. Its services and main runtime connections are:\n\nSERVICES (id: name — repos — role)\n",
    );
    for n in NODES {
        let repos = if n.repos.is_empty() {
            "external".to_string()
        } else {
            n.repos.join(", ")
        };
        s.push_str(&format!("- {}: {} — {} — {}\n", n.id, n.name, repos, n.description));
    }
    s.push_str("\nCONNECTIONS (caller -> callee: what)\n");
    for e in EDGES {
        s.push_str(&format!("- {} -> {}: {}\n", e.from, e.to, e.label));
    }
    s.push_str("\nAUDIENCE (use for audience.facing)\n");
    for note in AUDIENCE_NOTES {
        s.push_str(&format!("- {note}\n"));
    }
    let own: Vec<&str> = nodes_for_repo(repo).iter().map(|n| n.id).collect();
    s.push_str(&format!(
        "\nThis PR's repository ({repo}) maps to: {}.\n",
        if own.is_empty() { "no single service (decide from the changed files)".to_string() } else { own.join(", ") }
    ));
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::service_map::graph::LANES;

    #[test]
    fn graph_is_consistent() {
        for e in EDGES {
            assert!(node(e.from).is_some(), "unknown edge source {}", e.from);
            assert!(node(e.to).is_some(), "unknown edge target {}", e.to);
        }
        for n in NODES {
            assert!(n.lane < LANES.len());
        }
    }

    #[test]
    fn heuristic_marks_repo_and_callers() {
        let impact = heuristic_impact("attraction-inc/fibra");
        assert!(impact.iter().any(|i| i.service == "fibra" && i.level == "direct"));
        assert!(impact.iter().any(|i| i.service == "website" && i.level == "indirect"));
        assert!(applies("attraction-inc/fibra"));
        assert!(!applies("someone/fibra"));
    }
}
