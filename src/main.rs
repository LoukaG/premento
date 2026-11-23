mod app;
mod cli;
mod core;
mod features;
mod ui;

use eframe::egui;

fn main() -> eframe::Result {
    let (config, config_error) = match core::config::Config::load_or_init() {
        Ok(c) => (c, None),
        Err(e) => (core::config::Config::default(), Some(format!("{e:#}"))),
    };

    // Headless mode: `premento analyze owner/repo#123 [provider name] [model]` prints the analysis JSON.
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("analyze") {
        if let Err(e) = cli::analyze(&config, &args[2..]) {
            eprintln!("error: {e:#}");
            std::process::exit(1);
        }
        return Ok(());
    }

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("PRemento — PR review copilot")
            .with_icon(ui::icon::window_icon())
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
