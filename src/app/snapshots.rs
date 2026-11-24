//! Renders the UI with demo data to PNGs. Run with:
//! `MEMENTO_SNAPSHOT_DIR=/some/dir cargo test snapshots -- --nocapture`

use std::time::{Duration, Instant};

use chrono::{Local, Utc};
use eframe::egui::{self, vec2};
use egui_kittest::Harness;

use super::{PRementoApp, Tab};
use crate::core::config::Config;
use crate::features::analysis::state::Job;
use crate::features::analysis::{Analysis, StoredAnalysis};
use crate::features::ask::session;
use crate::features::ask::state::AskState;
use crate::features::pull_requests::{PullRequest, Role};
use crate::features::service_map::ServiceMap;
use crate::features::related::model::{Link, LinkKind, OutsidePr, PrState, Source, StoredRelations};
use crate::features::terminal::Terminal;
use crate::ui::theme;

const DEMO: &str = include_str!("demo_analysis.json");
const DEMO_MAP: &str = include_str!("demo_service_map.json");

/// Every file that draws UI text; checked by [`all_ui_glyphs_exist`].
const UI_SOURCES: &[&str] = &[
    include_str!("mod.rs"),
    include_str!("details.rs"),
    include_str!("messages.rs"),
    include_str!("top_bar.rs"),
    include_str!("../core/format.rs"),
    include_str!("../ui/theme.rs"),
    include_str!("../ui/widgets.rs"),
    include_str!("../ui/blocks.rs"),
    include_str!("../features/analysis/state.rs"),
    include_str!("../features/analysis/views/mod.rs"),
    include_str!("../features/analysis/views/action_bar.rs"),
    include_str!("../features/analysis/views/overview.rs"),
    include_str!("../features/analysis/views/risk.rs"),
    include_str!("../features/analysis/views/intent.rs"),
    include_str!("../features/analysis/views/impact.rs"),
    include_str!("../features/analysis/views/intro.rs"),
    include_str!("../features/ask/state.rs"),
    include_str!("../features/ask/view.rs"),
    include_str!("../features/providers/picker.rs"),
    include_str!("../features/pull_requests/state.rs"),
    include_str!("../features/pull_requests/header.rs"),
    include_str!("../features/pull_requests/list_view.rs"),
    include_str!("../features/related/view.rs"),
    include_str!("../features/service_map/graph.rs"),
    include_str!("../features/service_map/impact.rs"),
    include_str!("../features/service_map/page.rs"),
    include_str!("../features/service_map/state.rs"),
    include_str!("../features/service_map/view.rs"),
];


fn demo_app() -> PRementoApp {
    let mut app = PRementoApp::build(Config::default(), None);
    app.prs.user = Some("octocat".into());

    app.prs.last_refresh = Some(Local::now());
    let now = Utc::now();
    let mut map: ServiceMap = serde_json::from_str(DEMO_MAP).unwrap();
    map.created_at = (now - chrono::Duration::days(2)).timestamp();
    app.maps.maps = vec![map];
    let pr = |repo: &str, n: u64, title: &str, author: &str, hours: i64, roles: Vec<Role>, draft: bool| PullRequest {
        repo: repo.into(),
        number: n,
        title: title.into(),
        url: format!("https://github.com/{repo}/pull/{n}"),
        author: author.into(),
        is_draft: draft,
        updated_at: (now - chrono::Duration::hours(hours)).to_rfc3339(),
        labels: if n == 412 { vec!["auth".into(), "backend".into()] } else { vec![] },
        roles,
        ..Default::default()
    };
    app.prs.prs = vec![
        pr("attraction-inc/fibra", 412, "Rotate refresh tokens on every use and revoke reused tokens", "mira", 2, vec![Role::ReviewRequested], false),
        pr("attraction-inc/studio-frontend", 1288, "Migrate settings page to the new design system", "jonas", 5, vec![Role::Assignee, Role::Reviewed], false),
        pr("attraction-inc/workspace", 77, "Bump Node to 24 LTS across services", "sam", 26, vec![Role::Assignee], false),
        pr("attraction-inc/fibra", 415, "WIP: batch webhook deliveries", "lee", 50, vec![Role::ReviewRequested], true),
        pr("octo-org/mobile", 903, "Fix crash when opening deep links while logged out", "ana", 72, vec![Role::Reviewed], false),
        pr("octo-org/mobile", 907, "Ask to log in when a deep link needs an account", "ana", 80, vec![Role::ReviewRequested], false),
    ];
    // A stack across two repos, built on a merged PR outside the list, and an AI suggestion.
    let link = |from: &str, to: &str, kind, source, reason: Option<&str>| Link {
        from: from.into(),
        to: to.into(),
        kind,
        source,
        reason: reason.map(Into::into),
    };
    app.relations.body_links = vec![
        link("attraction-inc/fibra#412", "attraction-inc/fibra#410", LinkKind::DependsOn, Source::Body, None),
        link("attraction-inc/studio-frontend#1288", "attraction-inc/fibra#412", LinkKind::DependsOn, Source::Body, None),
        link("attraction-inc/fibra#415", "attraction-inc/fibra#412", LinkKind::DependsOn, Source::Body, None),
    ];
    app.relations.outside.insert("attraction-inc/fibra#410".into(), OutsidePr {
        key: "attraction-inc/fibra#410".into(),
        title: "Add token family table and migration".into(),
        url: "https://github.com/attraction-inc/fibra/pull/410".into(),
        state: PrState::Merged,
    });
    app.relations.ai = Some(StoredRelations {
        fingerprint: String::new(),
        provider: "Claude Code".into(),
        created_at: (now - chrono::Duration::minutes(30)).timestamp(),
        links: vec![link("octo-org/mobile#907", "octo-org/mobile#903", LinkKind::DependsOn, Source::Ai, Some("Login prompt reuses the logged-out deep link fix"))],
    });
    let analysis: Analysis = serde_json::from_str(DEMO).unwrap();
    let mut stored = |key: &str, complexity: f32, tier: &str, facing: &str| {
        let mut a = analysis.clone();
        a.risk.complexity.score = complexity;
        a.risk.worst_case.tier = tier.into();
        a.risk.audience.facing = facing.into();
        a.risk.compute();
        app.analyses.results.insert(key.into(), StoredAnalysis {
            provider: "Claude Code".into(),
            head_sha: "9f2c1e7ab".into(),
            created_at: (now - chrono::Duration::minutes(12)).timestamp(),
            duration_secs: 94,
            analysis: Some(a),
            parse_error: None,
            raw: DEMO.into(),
        });
    };
    stored("attraction-inc/fibra#412", 7.0, "system_wide", "customer");
    stored("attraction-inc/workspace#77", 4.0, "core_flow", "internal");
    stored("octo-org/mobile#903", 2.0, "feature", "customer");
    app.analyses.jobs.insert("attraction-inc/studio-frontend#1288".into(), Job {
        started: Instant::now(),
        provider: "Codex".into(),
        stage: "Codex is reviewing the changes…".into(),
        cancel: Default::default(),
    });
    app.prs.selected = Some("attraction-inc/fibra#412".into());
    app
}

/// Every non-ASCII character in the UI source must exist in the bundled fonts,
/// otherwise it renders as an empty box.
#[test]
fn all_ui_glyphs_exist() {
    let h = Harness::new_ui(|_ui| {});
    let mut missing: Vec<char> = UI_SOURCES
        .concat()
        .chars()
        .filter(|c| !c.is_ascii())
        .filter(|&c| {
            !h.ctx
                .fonts_mut(|f| f.has_glyph(&egui::FontId::proportional(14.0), c))
        })
        .collect();
    missing.sort();
    missing.dedup();
    assert!(missing.is_empty(), "missing glyphs: {missing:?}");
}

#[test]
fn render_screens() {
    let Ok(dir) = std::env::var("MEMENTO_SNAPSHOT_DIR") else { return };
    for (theme, tname) in [(egui::Theme::Dark, "dark"), (egui::Theme::Light, "light")] {
        let mut h = Harness::builder()
            .with_size(vec2(1440.0, 900.0))
            .with_pixels_per_point(1.0)
            .with_theme(theme)
            .wgpu()
            .build_eframe(|cc| {
                theme::install(&cc.egui_ctx);
                demo_app()
            });
        for (tab, name) in Tab::for_pr(true) {
            h.state_mut().tab = tab;
            h.run_steps(4);
            let img = h.render().unwrap();
            img.save(format!("{dir}/{tname}-{name}.png").replace(' ', "_")).unwrap();
        }
        h.state_mut().prs.selected = Some("attraction-inc/studio-frontend#1288".into());
        h.run_steps(4);
        h.render().unwrap().save(format!("{dir}/{tname}-running.png")).unwrap();
        h.state_mut().prs.selected = None;
        h.run_steps(4);
        h.render().unwrap().save(format!("{dir}/{tname}-empty.png")).unwrap();
        let maps = std::mem::take(&mut h.state_mut().maps.maps);
        h.run_steps(4);
        h.render().unwrap().save(format!("{dir}/{tname}-no-maps.png")).unwrap();
        h.state_mut().maps.maps = maps;
        // A PR whose repo is on no map keeps the generic Impact tab.
        h.state_mut().prs.selected = Some("octo-org/mobile#903".into());
        h.state_mut().tab = Tab::Impact;
        h.run_steps(4);
        h.render().unwrap().save(format!("{dir}/{tname}-other-org-impact.png")).unwrap();
        // Ask tab with a session running (a shell standing in for Claude Code).
        let key = "attraction-inc/fibra#412".to_owned();
        let term = Terminal::spawn(
            &h.ctx,
            std::path::Path::new("/bin/sh"),
            vec!["-c".into(), "printf '\\033[1;35m> Claude Code\\033[0m\\n\\nThe PR rotates refresh tokens. Risk: \\033[31mcritical\\033[0m.\\n\\n> '; cat".into()],
            std::path::Path::new("/"),
        )
        .unwrap();
        h.state_mut().asks.sessions.insert(key.clone(), AskState::Running { label: "Claude Code · haiku".into(), dir: session::folder(&key), term });
        h.state_mut().prs.selected = Some(key);
        h.state_mut().tab = Tab::Ask;
        for _ in 0..10 {
            h.run_steps(1);
            std::thread::sleep(Duration::from_millis(50));
        }
        h.render().unwrap().save(format!("{dir}/{tname}-ask-running.png")).unwrap();
        h.state_mut().asks.sessions.clear();
        // The PR got new commits after its analysis.
        h.state_mut().prs.set_head("attraction-inc/fibra#412", "c0ffee1234");
        h.state_mut().tab = Tab::Overview;
        h.run_steps(4);
        h.render().unwrap().save(format!("{dir}/{tname}-outdated.png")).unwrap();
        // Groups under a filter: members it hides stay as slim rows, plus the AI's suggestion.
        h.state_mut().prs.filter = crate::features::pull_requests::Filter::Reviewer;
        h.state_mut().prs.selected = Some("octo-org/mobile#907".into());
        h.run_steps(4);
        h.render().unwrap().save(format!("{dir}/{tname}-related-filtered.png")).unwrap();
        h.state_mut().prs.filter = crate::features::pull_requests::Filter::All;
    }
}
