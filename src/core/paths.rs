//! Where PRemento keeps its config and cache.

use std::path::PathBuf;

pub fn app_config_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("memento")
}

pub fn app_cache_dir() -> PathBuf {
    dirs::cache_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("memento")
}

pub fn config_path() -> PathBuf {
    app_config_dir().join("config.toml")
}
