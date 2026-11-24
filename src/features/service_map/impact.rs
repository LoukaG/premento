//! Which services a PR impacts, and a service map as LLM prompt text.

use super::graph::ServiceMap;
use crate::features::analysis::ServiceImpact;

fn direct(id: &str, name: &str) -> ServiceImpact {
    ServiceImpact {
        service: id.into(),
        level: "direct".into(),
        severity: "medium".into(),
        reason: format!("The PR changes the {name} repository."),
    }
}

/// Best-effort impact without an LLM: the repo's own services, plus their callers.
pub fn heuristic_impact(map: &ServiceMap, repo: &str) -> Vec<ServiceImpact> {
    let mut out: Vec<ServiceImpact> = map.nodes_for_repo(repo).iter().map(|n| direct(&n.id, &n.name)).collect();
    let own: Vec<String> = out.iter().map(|i| i.service.clone()).collect();
    for d in &own {
        for caller in map.dependents(d) {
            if !out.iter().any(|i| i.service == caller) {
                out.push(ServiceImpact {
                    service: caller.into(),
                    level: "indirect".into(),
                    severity: "low".into(),
                    reason: format!("Calls {} — may be affected by its changes.", map.node(d).map_or(d.as_str(), |n| n.name.as_str())),
                });
            }
        }
    }
    out
}

/// The impact to display: the LLM's answer when present (plus the PR's own services, which are
/// always directly impacted), otherwise the repository heuristic.
pub fn resolve_impact(map: &ServiceMap, repo: &str, from_llm: &[ServiceImpact]) -> (Vec<ServiceImpact>, bool) {
    let mut out: Vec<ServiceImpact> = from_llm.iter().filter(|i| map.node(&i.service).is_some()).cloned().collect();
    if out.is_empty() {
        return (heuristic_impact(map, repo), false);
    }
    for n in map.nodes_for_repo(repo) {
        if !out.iter().any(|i| i.service == n.id) {
            out.push(direct(&n.id, &n.name));
        }
    }
    (out, true)
}

/// Architecture description handed to the LLM.
pub fn prompt_section(map: &ServiceMap, repo: &str) -> String {
    let mut s = format!(
        "The PR belongs to the \"{}\" system. Its services and main runtime connections are:\n\nSERVICES (id: name — repos — role)\n",
        map.name
    );
    for n in &map.nodes {
        let repos = if n.repos.is_empty() { "external".to_string() } else { n.repos.join(", ") };
        s.push_str(&format!("- {}: {} — {} — {}\n", n.id, n.name, repos, n.description));
    }
    s.push_str("\nCONNECTIONS (caller -> callee: what)\n");
    for e in &map.edges {
        s.push_str(&format!("- {} -> {}: {}\n", e.from, e.to, e.label));
    }
    if !map.notes.is_empty() {
        s.push_str("\nAUDIENCE (use for audience.facing)\n");
        for note in &map.notes {
            s.push_str(&format!("- {note}\n"));
        }
    }
    let own: Vec<&str> = map.nodes_for_repo(repo).iter().map(|n| n.id.as_str()).collect();
    s.push_str(&format!(
        "\nThis PR's repository ({repo}) maps to: {}.\n",
        if own.is_empty() { "no single service (decide from the changed files)".to_string() } else { own.join(", ") }
    ));
    s
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::features::service_map::graph::{Edge, Kind, Node};

    pub(crate) fn sample_map() -> ServiceMap {
        let node = |id: &str, repos: &[&str], kind| Node {
            id: id.into(),
            name: id.to_uppercase(),
            repos: repos.iter().map(|r| r.to_string()).collect(),
            kind,
            description: format!("{id} service"),
        };
        let edge = |from: &str, to: &str| Edge { from: from.into(), to: to.into(), label: "calls".into() };
        ServiceMap {
            id: "acme".into(),
            name: "acme".into(),
            nodes: vec![
                node("web", &["acme/web"], Kind::Surface),
                node("api", &["acme/api"], Kind::Service),
                node("db", &[], Kind::Data),
                node("erp", &[], Kind::External),
            ],
            edges: vec![edge("web", "api"), edge("api", "db"), edge("erp", "api")],
            notes: vec!["web is customer-facing".into()],
            ..Default::default()
        }
    }

    #[test]
    fn heuristic_marks_repo_and_callers() {
        let map = sample_map();
        let impact = heuristic_impact(&map, "acme/api");
        assert!(impact.iter().any(|i| i.service == "api" && i.level == "direct"));
        assert!(impact.iter().any(|i| i.service == "web" && i.level == "indirect"));
        assert!(!impact.iter().any(|i| i.service == "db"));
    }

    #[test]
    fn llm_impact_keeps_known_services_and_own_repo() {
        let map = sample_map();
        let llm = vec![
            ServiceImpact { service: "db".into(), level: "indirect".into(), ..Default::default() },
            ServiceImpact { service: "ghost".into(), level: "direct".into(), ..Default::default() },
        ];
        let (impact, from_ai) = resolve_impact(&map, "acme/api", &llm);
        assert!(from_ai);
        assert_eq!(impact.len(), 2);
        assert!(impact.iter().any(|i| i.service == "api" && i.level == "direct"));
        let (_, from_ai) = resolve_impact(&map, "acme/api", &llm[1..]);
        assert!(!from_ai, "only unknown ids: fall back to the heuristic");
    }

    #[test]
    fn prompt_lists_services_and_notes() {
        let s = prompt_section(&sample_map(), "acme/web");
        assert!(s.contains("- api: API — acme/api"));
        assert!(s.contains("- web -> api: calls"));
        assert!(s.contains("web is customer-facing"));
        assert!(s.contains("maps to: web."));
    }
}
