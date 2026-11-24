//! Finds the links a PR description states, like "Depends on #12" or
//! "Blocks owner/repo#34".
//!
//! A phrase only counts when references follow it right away ("Depends on #12, #13 and
//! o/r#4"), or, when nothing follows it, on the list items of the next lines:
//!
//! ```text
//! Depends on:
//! - #12
//! - [ ] https://github.com/o/r/pull/4
//! ```
//!
//! Code blocks, quotes and HTML comments are skipped.

use super::model::{Link, LinkKind, Source};

#[derive(Clone, Copy)]
enum Dir {
    /// This PR merges after the referenced one.
    After,
    /// This PR merges before the referenced one.
    Before,
    Related,
}

const PHRASES: &[(&str, Dir)] = &[
    ("depends on", Dir::After),
    ("depend on", Dir::After),
    ("dependent on", Dir::After),
    ("blocked by", Dir::After),
    ("requires", Dir::After),
    ("merge after", Dir::After),
    ("merged after", Dir::After),
    ("land after", Dir::After),
    ("lands after", Dir::After),
    ("stacked on", Dir::After),
    ("on top of", Dir::After),
    ("based on", Dir::After),
    ("builds on", Dir::After),
    ("follow-up to", Dir::After),
    ("follow-up of", Dir::After),
    ("follow up to", Dir::After),
    ("blocks", Dir::Before),
    ("blocking", Dir::Before),
    ("required by", Dir::Before),
    ("needed by", Dir::Before),
    ("merge before", Dir::Before),
    ("land before", Dir::Before),
    ("related to", Dir::Related),
    ("related pr", Dir::Related),
    ("related", Dir::Related),
    ("see also", Dir::Related),
    ("companion", Dir::Related),
    ("goes with", Dir::Related),
];

/// Words that may sit between a phrase and its references: "depends on PR #12 and #13".
const FILLERS: &[&str] = &["pull requests", "pull request", "prs", "pr", "and", "the", "to", "of", "&"];

/// The links stated in the description of the PR `pr_key` of `repo`.
pub fn parse_links(pr_key: &str, repo: &str, body: &str) -> Vec<Link> {
    let mut found: Vec<(Dir, String)> = vec![];
    let mut in_fence = false;
    let mut in_comment = false;
    // A phrase with nothing after it, waiting for a list on the next lines.
    let mut pending: Option<Dir> = None;

    for raw in body.lines() {
        let line = raw.trim();
        if line.starts_with("```") || line.starts_with("~~~") {
            in_fence = !in_fence;
            pending = None;
            continue;
        }
        if in_fence {
            continue;
        }
        let line = strip_comments(line, &mut in_comment);
        let line = line.trim();
        if line.starts_with('>') {
            pending = None;
            continue;
        }

        if let Some(dir) = pending {
            if let Some(item) = list_item(line) {
                let refs = refs_at(item, repo);
                if !refs.is_empty() {
                    found.extend(refs.into_iter().map(|r| (dir, r)));
                    continue;
                }
            }
            if !line.is_empty() {
                pending = None;
            }
        }

        let lower = line.to_ascii_lowercase();
        for &(phrase, dir) in PHRASES {
            for (at, _) in lower.match_indices(phrase) {
                let starts_word = lower[..at].chars().next_back().is_none_or(|c| !c.is_alphanumeric());
                let rest = &line[at + phrase.len()..];
                let ends_word = rest.chars().next().is_none_or(|c| !c.is_alphanumeric());
                if !starts_word || !ends_word {
                    continue;
                }
                let refs = refs_at(rest, repo);
                if refs.is_empty() {
                    if rest.trim_matches(|c: char| c.is_whitespace() || ":*_".contains(c)).is_empty() {
                        pending = Some(dir);
                    }
                } else {
                    found.extend(refs.into_iter().map(|r| (dir, r)));
                }
            }
        }
    }

    let mut links: Vec<Link> = vec![];
    for (dir, key) in found {
        if key.eq_ignore_ascii_case(pr_key) {
            continue;
        }
        let (from, to, kind) = match dir {
            Dir::After => (pr_key.to_owned(), key, LinkKind::DependsOn),
            Dir::Before => (key, pr_key.to_owned(), LinkKind::DependsOn),
            Dir::Related => (pr_key.to_owned(), key, LinkKind::Related),
        };
        let link = Link { from, to, kind, source: Source::Body, reason: None };
        if !links.contains(&link) {
            links.push(link);
        }
    }
    // An ordered link wins over a plain "related" one between the same PRs.
    let ordered: Vec<(String, String)> = links
        .iter()
        .filter(|l| l.kind == LinkKind::DependsOn)
        .map(|l| (l.from.clone(), l.to.clone()))
        .collect();
    links.retain(|l| {
        l.kind == LinkKind::DependsOn
            || !ordered.iter().any(|(a, b)| (a == &l.from && b == &l.to) || (a == &l.to && b == &l.from))
    });
    links
}

/// The line without its `<!-- ... -->` parts; `in_comment` carries an open comment over lines.
fn strip_comments(line: &str, in_comment: &mut bool) -> String {
    let mut out = String::new();
    let mut rest = line;
    loop {
        if *in_comment {
            match rest.find("-->") {
                Some(i) => {
                    rest = &rest[i + 3..];
                    *in_comment = false;
                }
                None => return out,
            }
        } else {
            match rest.find("<!--") {
                Some(i) => {
                    out.push_str(&rest[..i]);
                    rest = &rest[i + 4..];
                    *in_comment = true;
                }
                None => {
                    out.push_str(rest);
                    return out;
                }
            }
        }
    }
}

/// The text of a markdown list item ("- x", "* x", "1. x", "- [ ] x"), `None` for other lines.
fn list_item(line: &str) -> Option<&str> {
    let rest = if let Some(r) = line.strip_prefix("- ").or_else(|| line.strip_prefix("* ")).or_else(|| line.strip_prefix("+ ")) {
        r
    } else {
        let digits = line.find(|c: char| !c.is_ascii_digit())?;
        if digits == 0 {
            return None;
        }
        line[digits..].strip_prefix(". ").or_else(|| line[digits..].strip_prefix(") "))?
    };
    let rest = rest.trim_start();
    for check in ["[ ]", "[x]", "[X]"] {
        if let Some(r) = rest.strip_prefix(check) {
            return Some(r.trim_start());
        }
    }
    Some(rest)
}

/// The references at the start of `text`, separated by commas, "and" and the like. Stops at
/// the first word that is not a reference.
fn refs_at(text: &str, repo: &str) -> Vec<String> {
    let mut refs = vec![];
    let mut rest = text;
    loop {
        rest = rest.trim_start_matches(|c: char| c.is_whitespace() || ",:;*_`()[]-".contains(c));
        if let Some(filler) = FILLERS.iter().find(|f| {
            rest.len() >= f.len()
                && rest.is_char_boundary(f.len())
                && rest[..f.len()].eq_ignore_ascii_case(f)
                && rest[f.len()..].chars().next().is_none_or(|c| !c.is_alphanumeric())
        }) {
            rest = &rest[filler.len()..];
            continue;
        }
        match parse_ref(rest, repo) {
            Some((key, len)) => {
                refs.push(key);
                rest = &rest[len..];
            }
            None => return refs,
        }
    }
}

/// A reference at the start of `text`: `#N`, `owner/repo#N` or a GitHub pull/issue URL.
/// Returns its key and length.
fn parse_ref(text: &str, repo: &str) -> Option<(String, usize)> {
    fn digits(s: &str) -> Option<(u64, usize)> {
        let end = s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len());
        (end > 0).then(|| s[..end].parse().ok().map(|n| (n, end)))?
    }
    let name_len = |s: &str| s.find(|c: char| !(c.is_ascii_alphanumeric() || "-_.".contains(c))).unwrap_or(s.len());

    if let Some(r) = text.strip_prefix('#') {
        let (n, len) = digits(r)?;
        return Some((format!("{repo}#{n}"), 1 + len));
    }

    let lower = text.to_ascii_lowercase();
    for prefix in ["https://github.com/", "http://github.com/", "https://www.github.com/", "github.com/"] {
        if !lower.starts_with(prefix) {
            continue;
        }
        let r = &text[prefix.len()..];
        let o = name_len(r);
        let r2 = r[o..].strip_prefix('/')?;
        let n = name_len(r2);
        let r3 = &r2[n..];
        let kind = ["/pull/", "/issues/"].into_iter().find(|k| r3.starts_with(k))?;
        let (num, dl) = digits(&r3[kind.len()..])?;
        let mut len = prefix.len() + o + 1 + n + kind.len() + dl;
        // "/files", "/commits", "#discussion_r1" and other tails of the URL.
        len += text[len..].find(|c: char| c.is_whitespace() || ",;)]".contains(c)).unwrap_or(text.len() - len);
        let len = trim_sentence_end(text, len);
        if o == 0 || n == 0 {
            return None;
        }
        return Some((format!("{}/{}#{num}", &r[..o], &r2[..n]), len));
    }

    let o = name_len(text);
    let r = text[o..].strip_prefix('/')?;
    let n = name_len(r);
    let r2 = r[n..].strip_prefix('#')?;
    let (num, dl) = digits(r2)?;
    if o == 0 || n == 0 {
        return None;
    }
    Some((format!("{}/{}#{num}", &text[..o], &r[..n]), o + 1 + n + 1 + dl))
}

/// Leaves a sentence's final "." out of a URL.
fn trim_sentence_end(text: &str, len: usize) -> usize {
    let mut len = len;
    while len > 0 && text[..len].ends_with(['.', '!', '?']) {
        len -= 1;
    }
    len
}

#[cfg(test)]
mod tests {
    use super::*;

    fn deps(body: &str) -> Vec<(String, String, LinkKind)> {
        parse_links("o/r#5", "o/r", body).into_iter().map(|l| (l.from, l.to, l.kind)).collect()
    }

    fn after(to: &str) -> (String, String, LinkKind) {
        ("o/r#5".into(), to.into(), LinkKind::DependsOn)
    }

    #[test]
    fn depends_on_short_refs_and_lists() {
        assert_eq!(deps("Depends on #12"), vec![after("o/r#12")]);
        assert_eq!(
            deps("This depends on PR #12, #13 and x/y#4."),
            vec![after("o/r#12"), after("o/r#13"), after("x/y#4")]
        );
        assert_eq!(deps("**Blocked by:** #3"), vec![after("o/r#3")]);
        assert_eq!(deps("Stacked on top of #7"), vec![after("o/r#7")]);
    }

    #[test]
    fn urls() {
        assert_eq!(
            deps("Requires https://github.com/acme/api/pull/41/files."),
            vec![after("acme/api#41")]
        );
        assert_eq!(deps("merge after github.com/acme/api/pull/41"), vec![after("acme/api#41")]);
    }

    #[test]
    fn reverse_and_related() {
        assert_eq!(deps("Blocks #9"), vec![("o/r#9".into(), "o/r#5".into(), LinkKind::DependsOn)]);
        assert_eq!(deps("Related to acme/web#2"), vec![("o/r#5".into(), "acme/web#2".into(), LinkKind::Related)]);
        // An order between the same PRs wins over "related".
        assert_eq!(deps("Related: #9\nDepends on #9"), vec![after("o/r#9")]);
    }

    #[test]
    fn list_after_a_bare_phrase() {
        let body = "## Depends on\n\n- #12\n- [ ] https://github.com/a/b/pull/3\n1. c/d#4\n\nNot #99";
        assert_eq!(deps(body), vec![after("o/r#12"), after("a/b#3"), after("c/d#4")]);
    }

    #[test]
    fn ignores_prose_code_quotes_and_comments() {
        assert!(deps("This requires care, see #12").is_empty());
        assert!(deps("Fixes #12").is_empty());
        assert!(deps("```\ndepends on #12\n```").is_empty());
        assert!(deps("> depends on #12").is_empty());
        assert!(deps("<!-- e.g. depends on #12 -->").is_empty());
        assert!(deps("<!--\nDepends on #12\n-->").is_empty());
        assert!(deps("redepends on #12").is_empty());
        assert!(deps("Depends on #5").is_empty(), "no link to itself");
    }
}
