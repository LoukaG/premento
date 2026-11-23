mod analysis;
mod ask;
mod app;
mod config;
mod github;
mod icon;
mod llm;
mod map_view;
mod service_map;
mod terminal;
mod theme;

use eframe::egui;

fn main() -> eframe::Result {
    let (config, config_error) = match config::Config::load_or_init() {
        Ok(c) => (c, None),
        Err(e) => (config::Config::default(), Some(format!("{e:#}"))),
    };

    // Headless mode: `premento analyze owner/repo#123 [provider name] [model]` prints the analysis JSON.
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("analyze") {
        if let Err(e) = headless(&config, &args[2..]) {
            eprintln!("error: {e:#}");
            std::process::exit(1);
        }
        return Ok(());
    }

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("PRemento — PR review copilot")
            .with_icon(icon::window_icon())
            .with_inner_size([1360.0, 860.0])
            .with_min_inner_size([900.0, 500.0]),
        ..Default::default()
    };
    eframe::run_native(
        "PRemento",
        options,
        Box::new(move |cc| Ok(Box::new(app::PRementoApp::new(cc, config, config_error)))),
    )
}

fn headless(config: &config::Config, args: &[String]) -> anyhow::Result<()> {
    use anyhow::{Context, anyhow};
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
    let raw = llm::run(
        provider,
        model,
        &analysis::build_prompt(
            &ctx.text,
            service_map::applies(repo)
                .then(|| service_map::prompt_section(repo))
                .as_deref(),
        ),
        std::time::Duration::from_secs(config.timeout_secs),
        Default::default(),
    )?;
    let parsed = analysis::parse(&raw).with_context(|| format!("raw response:\n{raw}"))?;
    println!("{}", serde_json::to_string_pretty(&parsed)?);
    Ok(())
}
