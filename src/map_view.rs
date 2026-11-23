//! Renders the service map, optionally highlighting the services a PR impacts.

use std::collections::HashMap;

use eframe::egui::{
    self, Align2, Color32, CornerRadius, FontId, Pos2, Rect, RichText, Sense, Shape, Stroke, Ui,
    epaint::CubicBezierShape, pos2, vec2,
};

use crate::analysis::ServiceImpact;
use crate::service_map::{self, EDGES, Kind, LANES, NODES, Node};
use crate::theme::{accent_card, body, muted, pal, pill, section_title, severity_color, severity_rank};

const LANE_H: f32 = 84.0;
const BOX_H: f32 = 42.0;
const MARGIN: f32 = 14.0;

/// Draws the map. With an empty `impacts`, every service is shown neutrally.
pub fn draw(ui: &mut Ui, impacts: &[ServiceImpact]) {
    let p = pal(ui);
    let dark = ui.visuals().dark_mode;
    let by_id: HashMap<&str, &ServiceImpact> =
        impacts.iter().map(|i| (i.service.as_str(), i)).collect();
    let highlighting = !by_id.is_empty();

    let width = ui.available_width().max(600.0);
    let (rect, _) = ui.allocate_exact_size(vec2(width, LANES.len() as f32 * LANE_H), Sense::hover());
    let painter = ui.painter_at(rect.expand(2.0));

    // Lanes
    for (i, name) in LANES.iter().enumerate() {
        let band = Rect::from_min_size(
            pos2(rect.left(), rect.top() + i as f32 * LANE_H),
            vec2(width, LANE_H),
        );
        if i % 2 == 0 {
            painter.rect_filled(band.shrink2(vec2(0.0, 2.0)), CornerRadius::same(10), p.surface_sunken);
        }
        painter.text(
            band.left_top() + vec2(12.0, 8.0),
            Align2::LEFT_TOP,
            name.to_uppercase(),
            FontId::proportional(10.0),
            p.faint,
        );
    }

    let box_w = (width / 7.6).clamp(96.0, 150.0);
    let usable = width - 2.0 * MARGIN - box_w;
    let max_col = NODES.iter().map(|n| n.col).fold(1.0, f32::max);
    let rects: HashMap<&str, Rect> = NODES
        .iter()
        .map(|n| {
            let x = rect.left() + MARGIN + n.col / max_col * usable;
            let y = rect.top() + n.lane as f32 * LANE_H + 28.0;
            (n.id, Rect::from_min_size(pos2(x, y), vec2(box_w, BOX_H)))
        })
        .collect();

    // Edges: faint ones first, highlighted ones on top.
    let mut edges: Vec<(u8, &service_map::Edge)> = EDGES
        .iter()
        .map(|e| {
            let n = by_id.contains_key(e.from) as u8 + by_id.contains_key(e.to) as u8;
            (n, e)
        })
        .collect();
    edges.sort_by_key(|(n, _)| *n);
    for (n_impacted, e) in edges {
        let (a, b) = (rects[e.from], rects[e.to]);
        let reverse = EDGES.iter().any(|r| r.from == e.to && r.to == e.from);
        let package = service_map::node(e.to).is_some_and(|n| n.kind == Kind::Package);
        let (color, width) = match (highlighting, n_impacted) {
            (false, _) if package => (p.muted.gamma_multiply(0.3), 1.0),
            (false, _) => (p.muted.gamma_multiply(0.55), 1.2),
            (true, 2) => (p.accent_text, 2.0),
            (true, 1) => (p.muted.gamma_multiply(0.45), 1.1),
            (true, _) => (p.border.gamma_multiply(0.6), 1.0),
        };
        draw_edge(&painter, a, b, e.from < e.to, reverse, package, Stroke::new(width, color));
    }

    // Nodes
    for n in NODES {
        let r = rects[n.id];
        let impact = by_id.get(n.id).copied();
        draw_node(ui, &painter, n, r, impact, highlighting, dark);
    }
}

fn impact_color(i: &ServiceImpact) -> Color32 {
    if severity_rank(&i.severity) == 0 {
        severity_color("medium")
    } else {
        severity_color(&i.severity)
    }
}

fn draw_edge(
    painter: &egui::Painter,
    a: Rect,
    b: Rect,
    forward: bool,
    reverse: bool,
    dashed: bool,
    stroke: Stroke,
) {
    // Separate the two directions of a bidirectional pair.
    let shift = if reverse { if forward { 6.0 } else { -6.0 } } else { 0.0 };
    let same_lane = (a.center().y - b.center().y).abs() < 1.0;

    let (p0, d0, p3, d3) = if same_lane {
        // Arc over the lane (one direction above, the other below).
        let up = a.center().x < b.center().x;
        let (pa, pb) = if up {
            (a.center_top(), b.center_top())
        } else {
            (a.center_bottom(), b.center_bottom())
        };
        let dir = if up { vec2(0.0, -1.0) } else { vec2(0.0, 1.0) };
        (pa + vec2(shift, 0.0), dir, pb + vec2(shift, 0.0), dir)
    } else {
        let down = b.center().y > a.center().y;
        // Spread anchors along the box edge toward the other end.
        let spread = |from: Rect, to: Rect| ((to.center().x - from.center().x) * 0.12).clamp(-from.width() * 0.38, from.width() * 0.38);
        let (pa, pb) = if down {
            (a.center_bottom(), b.center_top())
        } else {
            (a.center_top(), b.center_bottom())
        };
        let dy = if down { 1.0 } else { -1.0 };
        (
            pa + vec2(spread(a, b) + shift, 0.0),
            vec2(0.0, dy),
            pb + vec2(spread(b, a) + shift, 0.0),
            vec2(0.0, -dy),
        )
    };

    let dist = p0.distance(p3);
    let k = if same_lane { 24.0 + dist * 0.12 } else { (dist * 0.45).max(26.0) };
    let c1 = p0 + d0 * k;
    let c2 = p3 + d3 * k;
    let curve = CubicBezierShape::from_points_stroke([p0, c1, c2, p3], false, Color32::TRANSPARENT, stroke);
    if dashed {
        painter.extend(Shape::dashed_line(&curve.flatten(Some(0.5)), stroke, 4.0, 3.0));
    } else {
        painter.add(curve);
    }
    arrow_head(painter, c2, p3, stroke.color);
}

fn arrow_head(painter: &egui::Painter, from: Pos2, tip: Pos2, color: Color32) {
    let dir = (tip - from).normalized();
    let perp = vec2(-dir.y, dir.x);
    let base = tip - dir * 7.0;
    painter.add(Shape::convex_polygon(
        vec![tip, base + perp * 3.5, base - perp * 3.5],
        color,
        Stroke::NONE,
    ));
}

fn draw_node(
    ui: &mut Ui,
    painter: &egui::Painter,
    n: &Node,
    r: Rect,
    impact: Option<&ServiceImpact>,
    highlighting: bool,
    dark: bool,
) {
    let p = pal(ui);
    let radius = CornerRadius::same(if n.kind == Kind::Package { 6 } else { 10 });
    let direct = impact.is_some_and(|i| i.level.eq_ignore_ascii_case("direct"));

    let (fill, stroke, text, sub) = match impact {
        Some(i) => {
            let c = impact_color(i);
            let alpha = match (direct, dark) {
                (true, true) => 0.28,
                (true, false) => 0.16,
                (false, true) => 0.12,
                (false, false) => 0.07,
            };
            (
                p.surface.blend(c.gamma_multiply(alpha)),
                Stroke::new(if direct { 2.0 } else { 1.4 }, c),
                p.text,
                p.muted,
            )
        }
        None if highlighting => (p.surface, Stroke::new(1.0, p.border), p.faint, p.faint),
        None => (p.surface, Stroke::new(1.0, p.border), p.text, p.muted),
    };

    painter.rect_filled(r, radius, fill);
    let dashed = n.kind == Kind::External || impact.is_some_and(|_| !direct);
    if dashed {
        let pts = [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()];
        painter.extend(Shape::dashed_line(&pts, stroke, 5.0, 3.0));
    } else {
        painter.rect_stroke(r, radius, stroke, egui::StrokeKind::Inside);
    }

    let name_size = (r.width() / 10.5).min(12.5);
    painter.text(
        r.center() - vec2(0.0, 7.0),
        Align2::CENTER_CENTER,
        n.name,
        FontId::proportional(name_size),
        text,
    );
    let subtitle = match n.kind {
        Kind::External => "external".to_string(),
        Kind::Package => "package".to_string(),
        _ => n.repos.first().copied().unwrap_or("").to_string(),
    };
    painter.text(
        r.center() + vec2(0.0, 9.0),
        Align2::CENTER_CENTER,
        subtitle,
        FontId::proportional(10.0),
        sub,
    );

    if let Some(i) = impact {
        let c = impact_color(i);
        let dot = r.right_top() + vec2(-8.0, 8.0);
        if direct {
            painter.circle_filled(dot, 3.5, c);
        } else {
            painter.circle_stroke(dot, 3.0, Stroke::new(1.3, c));
        }
    }

    let resp = ui.interact(r, ui.scope_id().with(("svc", n.id)), Sense::hover());
    resp.on_hover_ui(|ui| {
        ui.set_max_width(320.0);
        ui.label(RichText::new(n.name).strong().color(p.text));
        ui.label(muted(ui, n.description));
        if !n.repos.is_empty() {
            ui.label(RichText::new(n.repos.join(", ")).monospace().size(11.0).color(p.muted));
        }
        if let Some(i) = impact {
            ui.separator();
            ui.horizontal(|ui| {
                pill(ui, i.level.to_uppercase(), impact_color(i));
                pill(ui, i.severity.to_uppercase(), impact_color(i));
            });
            ui.label(RichText::new(&i.reason).color(p.text));
        }
        let calls: Vec<String> = EDGES
            .iter()
            .filter(|e| e.from == n.id)
            .map(|e| format!("{} ({})", service_map::node(e.to).map_or(e.to, |t| t.name), e.label))
            .collect();
        if !calls.is_empty() {
            ui.separator();
            ui.label(RichText::new("Calls").size(11.0).strong().color(p.muted));
            for c in calls {
                ui.label(RichText::new(format!("› {c}")).size(12.0).color(p.text));
            }
        }
    });
}

pub fn legend(ui: &mut Ui, estimated: bool) {
    let p = pal(ui);
    ui.horizontal_wrapped(|ui| {
        let (r, _) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
        ui.painter().circle_filled(r.center(), 4.0, severity_color("high"));
        ui.label(muted(ui, "Direct (code changes)"));
        ui.add_space(8.0);
        let (r, _) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
        ui.painter().circle_stroke(r.center(), 3.5, Stroke::new(1.3, severity_color("high")));
        ui.label(muted(ui, "Indirect (through a connection)"));
        ui.add_space(8.0);
        ui.label(muted(ui, "Color = severity · Hover a service for details"));
        if estimated {
            ui.add_space(8.0);
            pill(ui, "estimated from the repository · re-analyze for AI impact", p.muted);
        }
    });
}

/// Cards listing each impacted service, direct first then by severity.
pub fn impact_list(ui: &mut Ui, impacts: &[ServiceImpact]) {
    let p = pal(ui);
    let mut items: Vec<&ServiceImpact> = impacts.iter().collect();
    items.sort_by_key(|i| {
        (
            !i.level.eq_ignore_ascii_case("direct"),
            std::cmp::Reverse(severity_rank(&i.severity)),
        )
    });
    section_title(ui, &format!("Impacted services · {}", items.len()));
    if items.is_empty() {
        ui.label(muted(ui, "No service impact identified."));
    }
    for i in items {
        let Some(n) = service_map::node(&i.service) else { continue };
        let c = impact_color(i);
        accent_card(ui, c, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(n.name).size(15.0).strong().color(p.text));
                ui.label(RichText::new(n.repos.join(", ")).monospace().size(11.0).color(p.faint));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    pill(ui, i.severity.to_uppercase(), c);
                    pill(ui, i.level.to_uppercase(), if i.level.eq_ignore_ascii_case("direct") { p.accent_text } else { p.muted });
                });
            });
            body(ui, &i.reason);
        });
    }
}
