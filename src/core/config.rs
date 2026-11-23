//! App settings, loaded from (and first written to) `config.toml`.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use super::paths::{app_config_dir, config_path};
use crate::features::providers::ProviderConfig;
use crate::features::providers::config::{default_providers, legacy_args};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Name of the provider selected at startup.
    #[serde(default)]
    pub default_provider: Option<String>,
    /// Diff is truncated beyond this many characters before being sent to the LLM.
    #[serde(default = "default_max_diff")]
    pub max_diff_chars: usize,
    /// Kill the LLM process after this many seconds.
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
    /// Maximum number of PRs returned per GitHub search query.
    #[serde(default = "default_limit")]
    pub search_limit: u32,
    /// Command that opens a terminal running `{script}` (the Ask tab). Empty: Terminal.app on
    /// macOS, `x-terminal-emulator -e` on Linux. E.g. `["open", "-a", "iTerm", "{script}"]`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub terminal: Vec<String>,
    #[serde(default = "default_providers")]
    pub providers: Vec<ProviderConfig>,
}

fn default_max_diff() -> usize {
    150_000
}
fn default_timeout() -> u64 {
    900
}
fn default_limit() -> u32 {
    100
}

impl Default for Config {
    fn default() -> Self {
        Self {
            default_provider: Some("Claude Code".into()),
            max_diff_chars: default_max_diff(),
            timeout_secs: default_timeout(),
            search_limit: default_limit(),
            terminal: vec![],
            providers: default_providers(),
        }
    }
}

impl Config {
    /// Gives untouched default providers from older versions their model options.
    /// Returns true if anything changed.
    fn migrate(&mut self) -> bool {
        let defaults = default_providers();
        let mut changed = false;
        for p in &mut self.providers {
            if p.supports_models() || legacy_args(&p.command).as_ref() != Some(&p.args) {
                continue;
            }
            if let Some(d) = defaults.iter().find(|d| d.command == p.command) {
                if self.default_provider.as_deref() == Some(p.name.as_str()) {
                    self.default_provider = Some(d.name.clone());
                }
                *p = d.clone();
                changed = true;
            }
        }
        changed
    }

    /// Loads the config file, writing the defaults to disk on first run so they can be edited.
    pub fn load_or_init() -> Result<Self> {
        let path = config_path();
        if path.exists() {
            let text = std::fs::read_to_string(&path)
                .with_context(|| format!("reading {}", path.display()))?;
            let mut config: Config =
                toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
            if config.migrate() {
                let _ = std::fs::write(&path, toml::to_string_pretty(&config)?);
            }
            return Ok(config);
        }
        let config = Config::default();
        std::fs::create_dir_all(app_config_dir())?;
        std::fs::write(&path, toml::to_string_pretty(&config)?)
            .with_context(|| format!("writing {}", path.display()))?;
        Ok(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::providers::config::provider;

    #[test]
    fn migrates_untouched_legacy_providers_only() {
        let mut c = Config::default();
        for p in &mut c.providers {
            *p = provider(&p.name, &p.command, &[]);
            p.args = legacy_args(&p.command).unwrap();
        }
        c.providers[3].name = "Ollama (llama3.1)".into();
        c.providers[2].args.push("--custom".into());
        c.default_provider = Some("Ollama (llama3.1)".into());
        assert!(c.migrate());
        assert_eq!(c.providers[0], Config::default().providers[0]);
        assert!(!c.providers[2].supports_models(), "edited provider is left alone");
        assert_eq!(c.default_provider.as_deref(), Some("Ollama"));
        assert!(!c.migrate());
    }
}
