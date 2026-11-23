//! On-disk cache of analyses, one JSON file per PR.

use std::path::PathBuf;

use anyhow::Result;

use super::model::StoredAnalysis;
use crate::core::paths::app_cache_dir;

fn cache_path(pr_key: &str) -> PathBuf {
    let safe: String = pr_key
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '.' { c } else { '_' })
        .collect();
    app_cache_dir().join("analyses").join(format!("{safe}.json"))
}

pub fn load_cached(pr_key: &str) -> Option<StoredAnalysis> {
    let text = std::fs::read_to_string(cache_path(pr_key)).ok()?;
    let mut stored: StoredAnalysis = serde_json::from_str(&text).ok()?;
    // Re-score with the current formula; the dimensions are stored, the score is derived.
    if let Some(a) = &mut stored.analysis {
        a.risk.compute();
    }
    Some(stored)
}

pub fn save_cached(pr_key: &str, stored: &StoredAnalysis) -> Result<()> {
    let path = cache_path(pr_key);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, serde_json::to_string_pretty(stored)?)?;
    Ok(())
}
