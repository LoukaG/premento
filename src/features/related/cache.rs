//! On-disk cache of the AI's links, a single JSON file for the whole list.

use std::path::PathBuf;

use anyhow::Result;

use super::model::StoredRelations;
use crate::core::paths::app_cache_dir;
use crate::features::pull_requests::PullRequest;

fn cache_path() -> PathBuf {
    app_cache_dir().join("related.json")
}

/// Identifies a list of PRs at their current heads: the AI's links are stale once it changes.
pub fn fingerprint(prs: &[PullRequest]) -> String {
    let mut keys: Vec<String> = prs
        .iter()
        .map(|pr| format!("{}@{}", pr.key(), pr.head_sha.as_deref().unwrap_or("")))
        .collect();
    keys.sort();
    // FNV-1a: stable across runs and Rust versions, unlike `DefaultHasher`.
    let hash = keys.join("\n").bytes().fold(0xcbf2_9ce4_8422_2325_u64, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    });
    format!("{hash:016x}")
}

pub fn load_cached() -> Option<StoredRelations> {
    serde_json::from_str(&std::fs::read_to_string(cache_path()).ok()?).ok()
}

pub fn save_cached(stored: &StoredRelations) -> Result<()> {
    let path = cache_path();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, serde_json::to_string_pretty(stored)?)?;
    Ok(())
}
