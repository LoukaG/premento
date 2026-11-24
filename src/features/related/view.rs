//! Related PRs in the sidebar (a framed group with merge steps) and in the selected PR's header.

use eframe::egui::{
    self, Align, Align2, Color32, CornerRadius, FontId, Layout, Margin, Pos2, Response, RichText, Sense, Shape,
    Stroke, Ui, pos2,
};

use super::group::{Group, Member, MemberKind};
use super::model::{LinkKind, PrState, Source};
use super::state::Relations;
use crate::core::format::ago_str;
use crate::ui::theme::{OUTDATED, pal};
use crate::ui::widgets::{muted, pill};

/// Width of the column holding the step badges.
const GUTTER: f32 = 24.0;
const MERGED: Color32 = Color32::from_rgb(34, 197, 94);

/// The "Group related" button of the list header. Returns true when clicked: start the AI
/// job, or cancel it when one is running.
pub fn group_button(ui: &mut Ui, relations: &Relations, pr_count: usize) -> bool {
    let p = pal(ui);
    if let Some(job) = &relations.job {
        let cancel = ui
            .add(egui::Button::new(RichText::new("Cancel").size(11.5).color(p.muted)).frame(false))
            .on_hover_text(format!("Stop {} grouping the PRs.", job.provider));
        ui.label(
            RichText::new(format!("Grouping… {}s", job.started.elapsed().as_secs()))
                .size(11.5)
                .color(p.accent_text),
        );
        ui.add(egui::Spinner::new().size(12.0).color(p.accent_text));
        return cancel.clicked();
    }

    let text = if relations.stale { "Regroup related" } else { "Group related" };
    let resp = ui.add_enabled(
        pr_count >= 2,
        egui::Button::new(RichText::new(text).size(12.0).color(p.accent_text))
            .fill(Color32::TRANSPARENT)
            .stroke(Stroke::new(1.0, if relations.stale { OUTDATED } else { p.border }))
            .corner_radius(CornerRadius::same(7)),
    );
    let mut hover = String::from(
        "Ask the selected tool which PRs belong together and in which order to merge them.\n\
         Links written in descriptions (\"Depends on #12\") are always shown.",
    );
    if let Some(ai) = &relations.ai {
        let when = chrono::DateTime::from_timestamp(ai.created_at, 0).map(|t| ago_str(&t.to_rfc3339()));
        hover.push_str(&format!("\n\nLast grouped by {}{}.", ai.provider, when.map(|w| format!(", {w}")).unwrap_or_default()));
    }
    if relations.stale {
        hover.push_str("\nThe list changed since then.");
    }
    resp.on_hover_text(hover).clicked()
}

/// One group in the sidebar list. `shown`: whether a listed PR passes the filter and search.
/// `card` draws a listed PR's card, `None` when the PR is not found. Returns the key of the
/// listed PR clicked.
pub fn group(
    ui: &mut Ui,
    group: &Group,
    shown: &dyn Fn(&str) -> bool,
    mut card: impl FnMut(&mut Ui, &str) -> Option<Response>,
) -> Option<String> {
    let p = pal(ui);
    let mut clicked = None;
    ui.add_space(2.0);
    egui::Frame::new()
        .fill(p.surface_sunken)
        .stroke(Stroke::new(1.0, p.border))
        .corner_radius(CornerRadius::same(12))
        .inner_margin(Margin { left: 4, right: 4, top: 8, bottom: 4 })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.with_layout(Layout::top_down(Align::Min), |ui| {
                ui.spacing_mut().item_spacing.y = 4.0;
                header(ui, group);
                let connector = ui.painter().add(Shape::Noop);
                let left = ui.max_rect().left();
                let mut badges: Vec<Pos2> = vec![];
                let short = short_keys(group);

                for m in &group.members {
                    let row = ui
                        .horizontal_top(|ui| {
                            ui.add_space(GUTTER - ui.spacing().item_spacing.x);
                            ui.vertical(|ui| match &m.kind {
                                MemberKind::Listed if shown(&m.key) => {
                                    let resp = card(ui, &m.key)?;
                                    if resp.clicked() {
                                        clicked = Some(m.key.clone());
                                    }
                                    Some((resp, 19.0))
                                }
                                MemberKind::Listed => {
                                    let resp = ghost_row(ui, m, &short(&m.key), "", ("hidden by filter", p.muted));
                                    if resp.clicked() {
                                        clicked = Some(m.key.clone());
                                    }
                                    Some((resp.on_hover_text("In your list, but hidden by the filter or search."), 15.0))
                                }
                                MemberKind::Outside(o) => {
                                    let tag = match o.state {
                                        PrState::Open => ("not in your queue  ↗", p.muted),
                                        PrState::Merged => ("merged  ↗", MERGED),
                                        PrState::Closed => ("closed  ↗", p.danger),
                                    };
                                    let resp = ghost_row(ui, m, &short(&m.key), &o.title, tag);
                                    if resp.clicked() && !o.url.is_empty() {
                                        ui.ctx().open_url(egui::OpenUrl::new_tab(&o.url));
                                    }
                                    Some((resp.on_hover_text("Linked PR outside your list. Opens on GitHub."), 15.0))
                                }
                            })
                            .inner
                        })
                        .inner;
                    let Some((resp, dy)) = row else { continue };
                    let center = pos2(left + GUTTER / 2.0, resp.rect.top() + dy);
                    badge(ui, center, m);
                    if m.step.is_some() {
                        badges.push(center);
                    }
                }

                if group.ordered && badges.len() > 1 {
                    let stroke = Stroke::new(2.0, p.faint);
                    let lines = badges.windows(2).map(|w| Shape::line_segment([w[0], w[1]], stroke)).collect();
                    ui.painter().set(connector, Shape::Vec(lines));
                }
            });
        });
    ui.add_space(2.0);
    clicked
}

/// "🔗 Merge in order · 3 PRs" with where the links come from; the reasons on hover.
fn header(ui: &mut Ui, group: &Group) {
    let p = pal(ui);
    let short = short_keys(group);
    let resp = ui
        .horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 5.0;
            ui.add_space(6.0);
            ui.label(RichText::new("🔗").size(15.0).color(p.accent_text));
            let title = if group.ordered { "Merge in order" } else { "Related" };
            ui.label(RichText::new(title).size(12.5).strong().color(p.text));
            ui.label(RichText::new(format!("· {} PRs", group.members.len())).size(11.5).color(p.muted));
            if group.cycle {
                pill(ui, "order unclear", OUTDATED)
                    .on_hover_text("The links say each PR merges after another, in a circle.");
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.add_space(4.0);
                for source in group.sources.iter().rev() {
                    match source {
                        Source::Ai => pill(ui, "AI suggested", p.accent),
                        Source::Body => pill(ui, "from descriptions", p.muted),
                    };
                }
            });
        })
        .response;
    resp.on_hover_ui(|ui| {
        for l in &group.links {
            let how = match l.kind {
                LinkKind::DependsOn => "merges after",
                LinkKind::Related => "goes with",
            };
            let by = match l.source {
                Source::Ai => " (AI)",
                Source::Body => "",
            };
            let reason = l.reason.as_deref().map(|r| format!(": {r}")).unwrap_or_default();
            ui.label(format!("{} {how} {}{by}{reason}", short(&l.from), short(&l.to)));
        }
    });
}

/// A slim, dimmed row for a linked PR that has no card.
fn ghost_row(ui: &mut Ui, m: &Member, key: &str, title: &str, (tag, color): (&str, Color32)) -> Response {
    let p = pal(ui);
    let mut prepared = egui::Frame::new()
        .stroke(Stroke::new(1.0, p.border))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(Margin { left: 12, right: 12, top: 6, bottom: 6 })
        .begin(ui);
    {
        let ui = &mut prepared.content_ui;
        ui.set_width(ui.available_width());
        ui.spacing_mut().item_spacing.y = 2.0;
        ui.horizontal(|ui| {
            ui.label(RichText::new(key).size(11.5).color(p.muted));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                pill(ui, tag, color);
            });
        });
        if !title.is_empty() {
            ui.add(egui::Label::new(RichText::new(title).size(12.5).color(p.faint)).truncate().selectable(false));
        }
    }
    let rect = prepared.allocate_space(ui).rect;
    let resp = ui.interact(rect, ui.scope_id().with(("related-ghost", &m.key)), Sense::click());
    if resp.hovered() {
        prepared.frame.fill = p.surface_hover;
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    prepared.paint(ui);
    resp
}

/// The member's step in a circle: a check once merged, a dot when it has no order.
fn badge(ui: &Ui, center: Pos2, m: &Member) {
    let p = pal(ui);
    let painter = ui.painter();
    let merged = matches!(&m.kind, MemberKind::Outside(o) if o.state == PrState::Merged);
    match m.step {
        Some(step) => {
            let fill = if merged { MERGED } else { p.accent };
            painter.circle_filled(center, 9.0, fill);
            let text = if merged { "✔".to_owned() } else { step.to_string() };
            painter.text(center, Align2::CENTER_CENTER, text, FontId::proportional(11.0), Color32::WHITE);
        }
        None => {
            painter.circle_filled(center, 3.5, p.faint);
        }
    }
}

/// Shows `owner/repo#N` as `#N` when the whole group is in one repo.
fn short_keys(group: &Group) -> impl Fn(&str) -> String + use<> {
    let repo = |k: &str| k.rsplit_once('#').map(|(r, _)| r.to_ascii_lowercase());
    let first = group.members.first().and_then(|m| repo(&m.key));
    let one_repo = group.members.iter().all(|m| repo(&m.key) == first);
    move |k: &str| match k.rsplit_once('#') {
        Some((_, n)) if one_repo => format!("#{n}"),
        _ => k.to_owned(),
    }
}

/// One line under the selected PR's title: its step and the PRs to merge before and after it,
/// or the PRs it goes with. `title` gives a listed PR's title. Returns the key of the listed
/// PR clicked.
pub fn header_line(ui: &mut Ui, group: &Group, key: &str, title: &dyn Fn(&str) -> Option<String>) -> Option<String> {
    let p = pal(ui);
    let me = group.member(key)?;
    let short = short_keys(group);
    let mut clicked = None;
    let mut link = |ui: &mut Ui, m: &Member| {
        let (hover, url) = match &m.kind {
            MemberKind::Outside(o) => {
                let state = match o.state {
                    PrState::Open => "open, not in your list",
                    PrState::Merged => "merged",
                    PrState::Closed => "closed without merging",
                };
                (format!("{} ({state})", o.title), Some(o.url.clone()))
            }
            MemberKind::Listed => (title(&m.key).unwrap_or_default(), None),
        };
        let mut text = RichText::new(short(&m.key)).color(p.accent_text);
        if matches!(&m.kind, MemberKind::Outside(o) if o.state != PrState::Open) {
            text = text.strikethrough();
        }
        if ui.add(egui::Link::new(text)).on_hover_text(hover).clicked() {
            match url {
                Some(url) => ui.ctx().open_url(egui::OpenUrl::new_tab(url)),
                None => clicked = Some(m.key.clone()),
            }
        }
    };

    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 5.0;
        ui.label(RichText::new("🔗").size(16.0).color(p.accent_text));
        match me.step.filter(|_| group.ordered) {
            Some(step) => {
                ui.label(RichText::new(format!("Merge step {step} of {}", group.last_step())).strong().color(p.text));
                for (word, others) in [("after", group.after(key)), ("before", group.before(key))] {
                    if others.is_empty() {
                        continue;
                    }
                    ui.label(muted(ui, format!("· merge {word}")));
                    for m in others {
                        link(ui, m);
                    }
                }
            }
            None => {
                ui.label(RichText::new("Related to").strong().color(p.text));
                for m in group.members.iter().filter(|m| m.key != key) {
                    link(ui, m);
                }
            }
        }
        if group.sources.contains(&Source::Ai) {
            pill(ui, "AI suggested", p.accent);
        }
    });
    clicked
}
