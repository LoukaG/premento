use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// A local LLM CLI that can be driven non-interactively.
///
/// `args` may contain three placeholders:
/// - `{prompt_file}`: path to a temp file holding the full prompt
/// - `{output_file}`: path the tool writes its final answer to (otherwise stdout is used)
/// - `{model_args}`: where `model_args` go (otherwise they are put before `args`)
///
/// When `stdin` is true the prompt is also piped on stdin.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProviderConfig {
    pub name: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default = "yes")]
    pub stdin: bool,
    /// Args that select a model, with `{model}` replaced by its name. Empty: the tool has no
    /// model choice. Left out entirely when no model is chosen, so the tool uses its own default.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub model_args: Vec<String>,
    /// Models offered in the picker. Any other name can be typed in.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub models: Vec<String>,
    /// Model used when none is picked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_model: Option<String>,
    /// Args for an interactive session (the Ask tab), with `{prompt}` replaced by the opening
    /// message and `{model_args}` placed as in `args`. Left out: the built-in default for this
    /// command, if any. `[]`: the tool is not offered for questions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chat_args: Option<Vec<String>>,
}

impl ProviderConfig {
    pub fn supports_models(&self) -> bool {
        !self.model_args.is_empty()
    }

    /// The model actually used: the one picked, else `default_model`, else none (tool default).
    pub fn effective_model<'a>(&'a self, picked: &'a str) -> Option<&'a str> {
        let picked = picked.trim();
        if !self.supports_models() {
            return None;
        }
        if picked.is_empty() { self.default_model.as_deref() } else { Some(picked) }
    }

    /// "Claude Code · haiku", or just the provider name when the tool picks its model.
    pub fn label(&self, model: Option<&str>) -> String {
        match model {
            Some(m) => format!("{} · {m}", self.name),
            None => self.name.clone(),
        }
    }

    /// `args` with the model args inserted (`{prompt_file}`/`{output_file}` are left as is).
    pub fn command_args(&self, model: Option<&str>) -> Vec<String> {
        self.with_model_args(&self.args, model)
    }

    /// Interactive session args, or `None` if this tool cannot be used for questions.
    pub fn chat_command_args(&self, model: Option<&str>, prompt: &str) -> Option<Vec<String>> {
        let template = self.chat_args.clone().or_else(|| {
            default_providers()
                .into_iter()
                .find(|d| d.command == self.command)
                .and_then(|d| d.chat_args)
        })?;
        if template.is_empty() {
            return None;
        }
        let args = self.with_model_args(&template, model);
        Some(args.into_iter().map(|a| a.replace("{prompt}", prompt)).collect())
    }

    pub fn supports_chat(&self) -> bool {
        self.chat_command_args(None, "").is_some()
    }

    /// `template` with `{model_args}` replaced by the model args, or the model args put first.
    fn with_model_args(&self, template: &[String], model: Option<&str>) -> Vec<String> {
        let model_args: Vec<String> = match model {
            Some(m) => self.model_args.iter().map(|a| a.replace("{model}", m)).collect(),
            None => vec![],
        };
        match template.iter().position(|a| a == "{model_args}") {
            Some(i) => {
                let mut args = template.to_vec();
                args.splice(i..=i, model_args);
                args
            }
            None => model_args.into_iter().chain(template.iter().cloned()).collect(),
        }
    }
}

fn yes() -> bool {
    true
}

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

fn strings(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

fn provider(name: &str, command: &str, args: &[&str]) -> ProviderConfig {
    ProviderConfig {
        name: name.into(),
        command: command.into(),
        args: strings(args),
        stdin: true,
        model_args: vec![],
        models: vec![],
        default_model: None,
        chat_args: None,
    }
}

fn with_models(mut p: ProviderConfig, model_args: &[&str], models: &[&str]) -> ProviderConfig {
    p.model_args = strings(model_args);
    p.models = strings(models);
    p
}

fn with_chat(mut p: ProviderConfig, chat_args: &[&str]) -> ProviderConfig {
    p.chat_args = Some(strings(chat_args));
    p
}

fn default_providers() -> Vec<ProviderConfig> {
    vec![
        with_chat(
            with_models(
                provider("Claude Code", "claude", &["-p", "--output-format", "text"]),
                &["--model", "{model}"],
                &["fable", "opus", "sonnet", "haiku"],
            ),
            &["{prompt}"],
        ),
        with_chat(with_models(provider(
            "Codex",
            "codex",
            &[
                "exec",
                "{model_args}",
                "--skip-git-repo-check",
                "--sandbox",
                "read-only",
                "--ephemeral",
                "-o",
                "{output_file}",
                "-",
            ],
        ), &["-m", "{model}"], &[]), &["{model_args}", "--sandbox", "read-only", "{prompt}"]),
        with_models(
            provider(
                "Gemini CLI",
                "gemini",
                &["-p", "Follow the instructions provided on stdin exactly."],
            ),
            &["-m", "{model}"],
            &[],
        ),
        ProviderConfig {
            default_model: Some("llama3.1".into()),
            ..with_models(provider("Ollama", "ollama", &["run", "{model_args}"]), &["{model}"], &["llama3.1"])
        },
    ]
}

/// Provider args written by versions without model choice. A provider still on them is
/// replaced by the current default, so existing configs get the model picker.
fn legacy_args(command: &str) -> Option<Vec<String>> {
    Some(strings(match command {
        "claude" => &["-p", "--output-format", "text"],
        "codex" => &[
            "exec",
            "--skip-git-repo-check",
            "--sandbox",
            "read-only",
            "--ephemeral",
            "-o",
            "{output_file}",
            "-",
        ],
        "gemini" => &["-p", "Follow the instructions provided on stdin exactly."],
        "ollama" => &["run", "llama3.1"],
        _ => return None,
    }))
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

    fn by_command(c: &Config, command: &str) -> ProviderConfig {
        c.providers.iter().find(|p| p.command == command).unwrap().clone()
    }

    #[test]
    fn model_args_are_optional_and_placed() {
        let c = Config::default();
        let claude = by_command(&c, "claude");
        assert_eq!(claude.command_args(None), ["-p", "--output-format", "text"]);
        assert_eq!(claude.command_args(Some("haiku"))[..2], ["--model", "haiku"]);
        let codex = by_command(&c, "codex");
        assert_eq!(codex.command_args(Some("x"))[..3], ["exec", "-m", "x"]);
        assert!(!codex.command_args(None).iter().any(|a| a.contains('{') && a != "{output_file}"));
        let ollama = by_command(&c, "ollama");
        assert_eq!(ollama.effective_model(""), Some("llama3.1"));
        assert_eq!(ollama.command_args(ollama.effective_model("")), ["run", "llama3.1"]);
        assert_eq!(claude.effective_model("  "), None);
    }

    #[test]
    fn chat_args_default_by_command() {
        let c = Config::default();
        let mut claude = by_command(&c, "claude");
        assert_eq!(claude.chat_command_args(Some("opus"), "hi").unwrap(), ["--model", "opus", "hi"]);
        claude.chat_args = None; // an older config file: falls back to the built-in default
        assert_eq!(claude.chat_command_args(None, "hi").unwrap(), ["hi"]);
        let codex = by_command(&c, "codex");
        assert_eq!(codex.chat_command_args(Some("x"), "hi").unwrap(), ["-m", "x", "--sandbox", "read-only", "hi"]);
        assert!(!by_command(&c, "ollama").supports_chat());
        claude.chat_args = Some(vec![]);
        assert!(!claude.supports_chat());
    }

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
