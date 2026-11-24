//! Reads a folder (a workspace of git repos, or a single repo) into the text the LLM maps.
//! Only a few files per repo are read: docs, compose files and manifests.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Result, bail};

/// Characters sent in total, and per file.
const BUDGET: usize = 120_000;
const MAX_FILE: usize = 6_000;
/// Entries listed per folder.
const MAX_LISTED: usize = 60;

/// Files worth reading, matched without case on the start of the name.
const DOC_PREFIXES: &[&str] = &["readme", "architecture", "docker-compose", "compose."];
const MANIFESTS: &[&str] = &[
    "package.json",
    "cargo.toml",
    "pyproject.toml",
    "requirements.txt",
    "go.mod",
    "pom.xml",
    "build.gradle",
    "build.gradle.kts",
    "gemfile",
    "composer.json",
    "serverless.yml",
    "template.yaml",
    "dockerfile",
    ".env.example",
];

pub struct Repo {
    /// `owner/name` from the `origin` remote, else the folder name.
    pub name: String,
    pub dir: PathBuf,
}

pub struct Scan {
    pub repos: Vec<Repo>,
    /// Folder listings and file contents, ready for the prompt.
    pub text: String,
}

fn is_repo(dir: &Path) -> bool {
    dir.join(".git").exists()
}

fn dir_name(dir: &Path) -> String {
    dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| dir.display().to_string())
}

/// `owner/name` from a git remote URL (https, ssh or scp-like).
pub fn parse_remote(url: &str) -> Option<String> {
    let url = url.trim().trim_end_matches('/').trim_end_matches(".git");
    let path = match url.split_once("://") {
        Some((_, rest)) => rest.split_once('/')?.1,
        None => url.split_once(':')?.1,
    };
    let mut parts = path.rsplit('/');
    let name = parts.next().filter(|s| !s.is_empty())?;
    let owner = parts.next().filter(|s| !s.is_empty())?;
    Some(format!("{owner}/{name}"))
}

fn remote_name(dir: &Path) -> Option<String> {
    let out = Command::new("git").arg("-C").arg(dir).args(["remote", "get-url", "origin"]).output().ok()?;
    out.status.success().then(|| parse_remote(&String::from_utf8_lossy(&out.stdout))).flatten()
}

/// The repos in `folder`: itself if it is one, plus each direct subfolder that is one.
pub fn find_repos(folder: &Path) -> Vec<Repo> {
    let mut dirs: Vec<PathBuf> = vec![];
    if is_repo(folder) {
        dirs.push(folder.to_path_buf());
    }
    if let Ok(entries) = std::fs::read_dir(folder) {
        let mut subs: Vec<PathBuf> = entries.flatten().map(|e| e.path()).filter(|p| p.is_dir() && is_repo(p)).collect();
        subs.sort();
        dirs.extend(subs);
    }
    dirs.into_iter()
        .map(|dir| Repo { name: remote_name(&dir).unwrap_or_else(|| dir_name(&dir)), dir })
        .collect()
}

fn wanted(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    DOC_PREFIXES.iter().any(|p| lower.starts_with(p)) || MANIFESTS.contains(&lower.as_str())
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

/// Appends `dir`'s listing and wanted files to `out`, within the budget. Returns the number of
/// files read.
fn describe(dir: &Path, title: &str, out: &mut String) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else { return 0 };
    let mut entries: Vec<(String, bool)> = entries
        .flatten()
        .map(|e| (e.file_name().to_string_lossy().into_owned(), e.path().is_dir()))
        .filter(|(n, _)| n != ".git" && n != ".DS_Store" && n != "node_modules" && n != "target")
        .collect();
    entries.sort();
    out.push_str(&format!("==================== {title} ====================\n"));
    let listed: Vec<String> = entries
        .iter()
        .take(MAX_LISTED)
        .map(|(n, is_dir)| if *is_dir { format!("{n}/") } else { n.clone() })
        .collect();
    out.push_str(&format!("Files: {}{}\n\n", listed.join(", "), if entries.len() > MAX_LISTED { ", …" } else { "" }));

    let mut read = 0;
    for (name, _) in entries.iter().filter(|(n, is_dir)| !is_dir && wanted(n)) {
        let left = BUDGET.saturating_sub(out.len());
        if left < 500 {
            break;
        }
        let Ok(text) = std::fs::read_to_string(dir.join(name)) else { continue };
        let text = text.trim();
        if text.is_empty() {
            continue;
        }
        let cut = truncate(text, MAX_FILE.min(left - 200));
        out.push_str(&format!("--- {name} ---\n{cut}{}\n\n", if cut.len() < text.len() { "\n[…]" } else { "" }));
        read += 1;
    }
    read
}

/// Reads `folder`. Fails when it holds no repo and no file worth reading.
pub fn scan(folder: &Path) -> Result<Scan> {
    if !folder.is_dir() {
        bail!("{} is not a folder", folder.display());
    }
    let repos = find_repos(folder);
    let mut text = String::new();
    let mut read = 0;
    let root_is_repo = repos.first().is_some_and(|r| r.dir == folder);
    if !root_is_repo {
        read += describe(folder, &format!("FOLDER {}", dir_name(folder)), &mut text);
    }
    for repo in &repos {
        read += describe(&repo.dir, &format!("REPOSITORY {}", repo.name), &mut text);
    }
    if repos.is_empty() && read == 0 {
        bail!("{} holds no git repository and no README, compose file or manifest", folder.display());
    }
    Ok(Scan { repos, text })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_remotes() {
        assert_eq!(parse_remote("git@github.com:acme/api.git").as_deref(), Some("acme/api"));
        assert_eq!(parse_remote("https://github.com/acme/web\n").as_deref(), Some("acme/web"));
        assert_eq!(parse_remote("ssh://git@gitlab.com/group/sub/svc.git").as_deref(), Some("sub/svc"));
        assert_eq!(parse_remote("nonsense"), None);
    }

    #[test]
    fn scans_a_workspace_of_repos() {
        let root = std::env::temp_dir().join(format!("memento-scan-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for repo in ["api", "web"] {
            std::fs::create_dir_all(root.join(repo).join(".git")).unwrap();
            std::fs::write(root.join(repo).join("README.md"), format!("# {repo}\nThe {repo} service.")).unwrap();
            std::fs::write(root.join(repo).join("main.rs"), "fn main() {}").unwrap();
        }
        std::fs::create_dir_all(root.join("notes")).unwrap();
        std::fs::write(root.join("docker-compose.yml"), "services:\n  api: {}\n").unwrap();

        let scan = scan(&root).unwrap();
        let names: Vec<&str> = scan.repos.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, ["api", "web"]);
        assert!(scan.text.contains("REPOSITORY api"));
        assert!(scan.text.contains("The web service."));
        assert!(scan.text.contains("--- docker-compose.yml ---"));
        assert!(!scan.text.contains("fn main"), "source files are listed, not read");

        let empty = root.join("notes");
        assert!(super::scan(&empty).is_err());
        let _ = std::fs::remove_dir_all(&root);
    }
}
