//! Saved service maps: one JSON file per map in the config folder (they are user data, not a
//! cache).

use std::path::PathBuf;

use anyhow::Result;

use super::graph::ServiceMap;
use crate::core::paths::app_config_dir;

fn maps_dir() -> PathBuf {
    app_config_dir().join("maps")
}

fn path(id: &str) -> PathBuf {
    maps_dir().join(format!("{id}.json"))
}

/// Every saved map, sorted by name. Files that do not parse are skipped.
pub fn load_all() -> Vec<ServiceMap> {
    let Ok(entries) = std::fs::read_dir(maps_dir()) else { return vec![] };
    let mut maps: Vec<ServiceMap> = entries
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        .filter_map(|e| serde_json::from_str(&std::fs::read_to_string(e.path()).ok()?).ok())
        .filter(|m: &ServiceMap| !m.id.is_empty())
        .collect();
    maps.sort_by_key(|m| m.name.to_ascii_lowercase());
    maps
}

pub fn save(map: &ServiceMap) -> Result<()> {
    std::fs::create_dir_all(maps_dir())?;
    std::fs::write(path(&map.id), serde_json::to_string_pretty(map)?)?;
    Ok(())
}

pub fn delete(id: &str) -> Result<()> {
    std::fs::remove_file(path(id))?;
    Ok(())
}
