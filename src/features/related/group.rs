//! Turns links into groups of PRs, each with its merge order when the links give one.

use std::collections::{BTreeSet, HashMap};

use super::model::{Link, LinkKind, OutsidePr, Source};
use crate::features::pull_requests::PullRequest;

#[derive(Debug, Clone)]
pub enum MemberKind {
    /// In the user's list.
    Listed,
    /// Linked from a listed PR but not in the user's list.
    Outside(OutsidePr),
}

#[derive(Debug, Clone)]
pub struct Member {
    pub key: String,
    /// Merge step, from 1. PRs sharing a step can merge in any order. `None` when the group
    /// has no order, or this PR has no order relative to the others.
    pub step: Option<usize>,
    pub kind: MemberKind,
}

#[derive(Debug, Clone)]
pub struct Group {
    /// Sorted by step, then repo and number.
    pub members: Vec<Member>,
    /// The links between the members.
    pub links: Vec<Link>,
    /// Some members must merge before others.
    pub ordered: bool,
    /// The "depends on" links go in a circle, so there is no order.
    pub cycle: bool,
    pub sources: Vec<Source>,
}

impl Group {
    pub fn member(&self, key: &str) -> Option<&Member> {
        self.members.iter().find(|m| m.key == key)
    }

    pub fn last_step(&self) -> usize {
        self.members.iter().filter_map(|m| m.step).max().unwrap_or(0)
    }

    /// The PRs `key` must merge after.
    pub fn after(&self, key: &str) -> Vec<&Member> {
        self.ordered_neighbours(|l| (l.from == key).then_some(&l.to))
    }

    /// The PRs that must merge after `key`.
    pub fn before(&self, key: &str) -> Vec<&Member> {
        self.ordered_neighbours(|l| (l.to == key).then_some(&l.from))
    }

    fn ordered_neighbours<'a>(&'a self, other: impl Fn(&'a Link) -> Option<&'a String>) -> Vec<&'a Member> {
        if self.cycle {
            return vec![];
        }
        let keys: BTreeSet<&String> =
            self.links.iter().filter(|l| l.kind == LinkKind::DependsOn).filter_map(other).collect();
        self.members.iter().filter(|m| keys.contains(&m.key)).collect()
    }
}

/// Sort key for `owner/repo#N`: repo, then number.
pub fn sort_key(key: &str) -> (String, u64) {
    match key.rsplit_once('#') {
        Some((repo, n)) => (repo.to_ascii_lowercase(), n.parse().unwrap_or(0)),
        None => (key.to_ascii_lowercase(), 0),
    }
}

/// Groups the PRs that `links` connect. Links to a PR that is neither listed nor in `outside`
/// (an issue, or a PR that could not be read) are dropped. Every group has at least one
/// listed PR.
pub fn build(prs: &[PullRequest], outside: &HashMap<String, OutsidePr>, links: &[Link]) -> Vec<Group> {
    // GitHub names are case-insensitive: resolve every key to the listed or fetched spelling.
    let mut canonical: HashMap<String, String> = HashMap::new();
    for key in outside.keys() {
        canonical.insert(key.to_ascii_lowercase(), key.clone());
    }
    for pr in prs {
        canonical.insert(pr.key().to_ascii_lowercase(), pr.key());
    }
    let resolve = |k: &str| canonical.get(&k.to_ascii_lowercase()).cloned();

    let mut all: Vec<Link> = vec![];
    let mut push = |l: Link| {
        let same = |o: &Link| {
            o.kind == l.kind
                && o.source == l.source
                && ((o.from == l.from && o.to == l.to) || (l.kind == LinkKind::Related && o.from == l.to && o.to == l.from))
        };
        if l.from != l.to && !all.iter().any(same) {
            all.push(l);
        }
    };
    for l in links {
        if let (Some(from), Some(to)) = (resolve(&l.from), resolve(&l.to)) {
            push(Link { from, to, ..l.clone() });
        }
    }
    // PRs closing the same issue are the same piece of work.
    let mut by_issue: HashMap<String, Vec<String>> = HashMap::new();
    for pr in prs {
        for issue in &pr.closes {
            by_issue.entry(issue.to_ascii_lowercase()).or_default().push(pr.key());
        }
    }
    let mut issues: Vec<_> = by_issue.into_iter().filter(|(_, keys)| keys.len() > 1).collect();
    issues.sort();
    for (issue, keys) in issues {
        for pair in keys.windows(2) {
            push(Link {
                from: pair[0].clone(),
                to: pair[1].clone(),
                kind: LinkKind::Related,
                source: Source::Body,
                reason: Some(format!("Both close {issue}")),
            });
        }
    }

    // Union-find over the linked keys.
    let mut nodes: Vec<String> = vec![];
    let mut index: HashMap<String, usize> = HashMap::new();
    let mut id = |k: &String, nodes: &mut Vec<String>| {
        *index.entry(k.clone()).or_insert_with(|| {
            nodes.push(k.clone());
            nodes.len() - 1
        })
    };
    let edges: Vec<(usize, usize)> = all.iter().map(|l| (id(&l.from, &mut nodes), id(&l.to, &mut nodes))).collect();
    let mut parent: Vec<usize> = (0..nodes.len()).collect();
    fn root(parent: &mut [usize], mut i: usize) -> usize {
        while parent[i] != i {
            parent[i] = parent[parent[i]];
            i = parent[i];
        }
        i
    }
    for &(a, b) in &edges {
        let (ra, rb) = (root(&mut parent, a), root(&mut parent, b));
        if ra != rb {
            parent[ra] = rb;
        }
    }
    let mut components: HashMap<usize, Vec<usize>> = HashMap::new();
    for i in 0..nodes.len() {
        components.entry(root(&mut parent, i)).or_default().push(i);
    }

    let listed: HashMap<String, ()> = prs.iter().map(|pr| (pr.key(), ())).collect();
    let mut groups: Vec<Group> = components
        .into_values()
        .filter(|ids| ids.iter().any(|&i| listed.contains_key(&nodes[i])))
        .map(|ids| {
            let keys: BTreeSet<&String> = ids.iter().map(|&i| &nodes[i]).collect();
            let links: Vec<Link> = all.iter().filter(|l| keys.contains(&l.from)).cloned().collect();
            let (steps, cycle) = steps(&keys, &links);
            let mut members: Vec<Member> = keys
                .iter()
                .map(|&k| Member {
                    key: k.clone(),
                    step: steps.get(k).copied(),
                    kind: match outside.get(k) {
                        Some(o) if !listed.contains_key(k) => MemberKind::Outside(o.clone()),
                        _ => MemberKind::Listed,
                    },
                })
                .collect();
            members.sort_by_key(|m| (m.step.is_none(), m.step, sort_key(&m.key)));
            let sources: BTreeSet<Source> = links.iter().map(|l| l.source).collect();
            Group { ordered: !steps.is_empty(), cycle, members, links, sources: sources.into_iter().collect() }
        })
        .collect();
    groups.sort_by_key(|g| sort_key(&g.members[0].key));
    groups
}

/// The merge step of each PR in a "depends on" link: 1 for PRs that depend on nothing, else
/// one more than their latest dependency. Empty, with `true`, when the links form a cycle.
fn steps(keys: &BTreeSet<&String>, links: &[Link]) -> (HashMap<String, usize>, bool) {
    let deps: Vec<(&String, &String)> = links
        .iter()
        .filter(|l| l.kind == LinkKind::DependsOn && keys.contains(&l.from) && keys.contains(&l.to))
        .map(|l| (&l.from, &l.to))
        .collect();
    let ordered: BTreeSet<&String> = deps.iter().flat_map(|&(a, b)| [a, b]).collect();
    let mut step: HashMap<String, usize> = HashMap::new();
    // Kahn's algorithm, one layer at a time.
    let mut left: Vec<&String> = ordered.into_iter().collect();
    let mut level = 1;
    while !left.is_empty() {
        let ready: Vec<&String> = left
            .iter()
            .copied()
            .filter(|k| deps.iter().filter(|(a, _)| a == k).all(|(_, b)| step.contains_key(*b)))
            .collect();
        if ready.is_empty() {
            return (HashMap::new(), true);
        }
        for k in &ready {
            step.insert((*k).clone(), level);
        }
        left.retain(|k| !ready.contains(k));
        level += 1;
    }
    (step, false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::related::model::PrState;

    fn pr(key: &str) -> PullRequest {
        let (repo, n) = key.split_once('#').unwrap();
        PullRequest { repo: repo.into(), number: n.parse().unwrap(), ..Default::default() }
    }

    fn dep(from: &str, to: &str) -> Link {
        Link { from: from.into(), to: to.into(), kind: LinkKind::DependsOn, source: Source::Body, reason: None }
    }

    fn rel(from: &str, to: &str, source: Source) -> Link {
        Link { from: from.into(), to: to.into(), kind: LinkKind::Related, source, reason: None }
    }

    fn steps_of(g: &Group) -> Vec<(&str, Option<usize>)> {
        g.members.iter().map(|m| (m.key.as_str(), m.step)).collect()
    }

    #[test]
    fn chain_and_diamond_steps() {
        let prs: Vec<_> = ["o/r#1", "o/r#2", "o/r#3", "o/r#4", "o/r#9"].map(pr).into();
        // 1 <- 2, 1 <- 3, {2,3} <- 4
        let links = [dep("o/r#2", "o/r#1"), dep("o/r#3", "o/r#1"), dep("o/r#4", "o/r#2"), dep("o/r#4", "o/r#3")];
        let groups = build(&prs, &HashMap::new(), &links);
        assert_eq!(groups.len(), 1, "#9 has no links");
        let g = &groups[0];
        assert!(g.ordered && !g.cycle);
        assert_eq!(steps_of(g), vec![("o/r#1", Some(1)), ("o/r#2", Some(2)), ("o/r#3", Some(2)), ("o/r#4", Some(3))]);
        assert_eq!(g.last_step(), 3);
        assert_eq!(g.after("o/r#4").len(), 2);
        assert_eq!(g.before("o/r#1").len(), 2);
    }

    #[test]
    fn cycle_has_no_order() {
        let prs: Vec<_> = ["o/r#1", "o/r#2"].map(pr).into();
        let g = &build(&prs, &HashMap::new(), &[dep("o/r#1", "o/r#2"), dep("o/r#2", "o/r#1")])[0];
        assert!(g.cycle && !g.ordered);
        assert!(g.members.iter().all(|m| m.step.is_none()));
    }

    #[test]
    fn outside_prs_join_and_unknown_refs_drop() {
        let prs: Vec<_> = ["o/r#2"].map(pr).into();
        let outside = HashMap::from([(
            "O/R#1".to_owned(),
            OutsidePr { key: "O/R#1".into(), title: "base".into(), url: String::new(), state: PrState::Merged },
        )]);
        let groups = build(&prs, &outside, &[dep("o/r#2", "o/r#1"), dep("o/r#2", "o/r#77")]);
        assert_eq!(groups.len(), 1);
        let g = &groups[0];
        assert_eq!(steps_of(g), vec![("O/R#1", Some(1)), ("o/r#2", Some(2))]);
        assert!(matches!(g.members[0].kind, MemberKind::Outside(_)));
        // A group made only of outside PRs is never shown.
        assert!(build(&[], &outside, &[]).is_empty());
    }

    #[test]
    fn mixed_sources_and_shared_issue() {
        let mut a = pr("o/r#1");
        a.closes = vec!["o/r#50".into()];
        let mut b = pr("x/y#2");
        b.closes = vec!["O/r#50".into()];
        let prs = vec![a, b, pr("o/r#3")];
        let groups = build(&prs, &HashMap::new(), &[rel("o/r#3", "o/r#1", Source::Ai)]);
        assert_eq!(groups.len(), 1);
        let g = &groups[0];
        assert_eq!(g.members.len(), 3);
        assert!(!g.ordered);
        assert_eq!(g.sources, vec![Source::Body, Source::Ai]);
    }

    #[test]
    fn unordered_member_of_ordered_group_goes_last() {
        let prs: Vec<_> = ["o/r#1", "o/r#2", "o/r#3"].map(pr).into();
        let g = &build(&prs, &HashMap::new(), &[dep("o/r#3", "o/r#2"), rel("o/r#1", "o/r#2", Source::Ai)])[0];
        assert_eq!(steps_of(g), vec![("o/r#2", Some(1)), ("o/r#3", Some(2)), ("o/r#1", None)]);
    }
}
