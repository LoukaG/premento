//! How each local LLM CLI is configured, and the built-in defaults.

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

fn strings(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

pub(crate) fn provider(name: &str, command: &str, args: &[&str]) -> ProviderConfig {
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

pub(crate) fn default_providers() -> Vec<ProviderConfig> {
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
pub(crate) fn legacy_args(command: &str) -> Option<Vec<String>> {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn by_command(providers: &[ProviderConfig], command: &str) -> ProviderConfig {
        providers.iter().find(|p| p.command == command).unwrap().clone()
    }

    #[test]
    fn model_args_are_optional_and_placed() {
        let c = default_providers();
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
        let c = default_providers();
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
}
