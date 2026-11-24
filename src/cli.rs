//! Headless mode: analyze one PR from the command line, without the UI.

use anyhow::{Context, Result, anyhow};

use crate::core::config::Config;
use crate::features::analysis::prompt;
use crate::features::providers::runner;
use crate::features::pull_requests::github;
use crate::features::service_map;

/// `premento analyze owner/repo#123 [provider name] [model]`: prints the analysis JSON.
pub fn analyze(config: &Config, args: &[String]) -> Result<()> {
    let target = args
        .first()
        .ok_or_else(|| anyhow!("usage: premento analyze owner/repo#123 [provider name] [model]"))?;
    let (repo, number) = target
        .split_once('#')
        .ok_or_else(|| anyhow!("expected owner/repo#number"))?;
    let number: u64 = number.parse().context("invalid PR number")?;
    let provider_name = args
        .get(1)
        .cloned()
        .or(config.default_provider.clone())
        .unwrap_or_default();
    let provider = config
        .providers
        .iter()
        .find(|p| p.name.eq_ignore_ascii_case(&provider_name))
        .ok_or_else(|| anyhow!("unknown provider `{provider_name}`"))?;

    let model = provider.effective_model(args.get(2).map(String::as_str).unwrap_or(""));
    if args.get(2).is_some() && !provider.supports_models() {
        return Err(anyhow!("`{}` has no `model_args` in the config, so no model can be chosen", provider.name));
    }
    let label = provider.label(model);

    eprintln!("Fetching {repo}#{number}…");
    let ctx = github::fetch_context(repo, number, config.max_diff_chars)?;
    eprintln!("Analyzing with {label} ({} chars of context)…", ctx.text.len());
    let raw = runner::run(
        provider,
        model,
        &prompt::build_prompt(
            &ctx.text,
            service_map::ServiceMaps::load()
                .for_repo(repo)
                .map(|m| service_map::prompt_section(m, repo))
                .as_deref(),
        ),
        std::time::Duration::from_secs(config.timeout_secs),
        Default::default(),
    )?;
    let parsed = prompt::parse(&raw).with_context(|| format!("raw response:\n{raw}"))?;
    println!("{}", serde_json::to_string_pretty(&parsed)?);
    Ok(())
}
