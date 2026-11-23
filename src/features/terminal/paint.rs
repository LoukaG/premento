//! Draws the terminal grid, cursor and selection.


use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::vte::ansi::{Color as AnsiColor, CursorShape};
use eframe::egui::{
    self, Align2, Color32, CornerRadius, FontId, Rect, Stroke, Vec2,
    pos2, vec2,
};

use super::palette::{Palette, to_color32};
use super::Terminal;
use crate::ui::theme::pal;

impl Terminal {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn paint(&self, ui: &egui::Ui, rect: Rect, inner: Rect, cell: Vec2, font: &FontId, colors: &Palette, focused: bool) {
        let p = pal(ui);
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, CornerRadius::same(10), colors.bg);
        painter.rect_stroke(
            rect,
            CornerRadius::same(10),
            Stroke::new(1.0, if focused { p.accent } else { p.border }),
            egui::StrokeKind::Inside,
        );

        let term = self.term.lock();
        let content = term.renderable_content();
        let offset = content.display_offset as i32;
        let overrides = content.colors;
        let resolve = |c: AnsiColor, bold: bool| -> Color32 {
            let c = match c {
                // Bold text in one of the 8 base colors is shown in its bright variant.
                AnsiColor::Named(n) if bold && (n as usize) < 8 => AnsiColor::Indexed(n as u8 + 8),
                c => c,
            };
            match c {
                AnsiColor::Spec(rgb) => to_color32(rgb),
                AnsiColor::Indexed(i) => overrides[i as usize].map(to_color32).unwrap_or_else(|| colors.indexed(i as usize)),
                AnsiColor::Named(n) => overrides[n].map(to_color32).unwrap_or_else(|| colors.indexed(n as usize)),
            }
        };
        let cell_rect = |line: i32, col: usize, width: f32| {
            Rect::from_min_size(
                pos2(inner.left() + col as f32 * cell.x, inner.top() + (line + offset) as f32 * cell.y),
                vec2(width, cell.y),
            )
        };

        for indexed in content.display_iter {
            let flags = indexed.flags;
            if flags.intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER) {
                continue;
            }
            let bold = flags.contains(Flags::BOLD);
            let mut fg = resolve(indexed.fg, bold);
            let mut bg = resolve(indexed.bg, false);
            if flags.contains(Flags::INVERSE) {
                std::mem::swap(&mut fg, &mut bg);
            }
            if flags.contains(Flags::DIM) {
                fg = fg.gamma_multiply(0.66);
            }
            let selected = content.selection.is_some_and(|s| s.contains(indexed.point));
            if selected {
                bg = colors.selection;
            }
            let width = if flags.contains(Flags::WIDE_CHAR) { cell.x * 2.0 } else { cell.x };
            let r = cell_rect(indexed.point.line.0, indexed.point.column.0, width);
            if bg != colors.bg {
                painter.rect_filled(r.expand2(vec2(0.0, 0.5)), CornerRadius::ZERO, bg);
            }
            if indexed.c != ' ' && indexed.c != '\t' && !flags.contains(Flags::HIDDEN) {
                painter.text(r.left_top(), Align2::LEFT_TOP, self.drawable(ui, indexed.c, font), font.clone(), fg);
            }
            if flags.intersects(Flags::ALL_UNDERLINES) {
                let y = r.bottom() - 1.0;
                painter.hline(r.x_range(), y, Stroke::new(1.0, fg));
            }
            if flags.contains(Flags::STRIKEOUT) {
                painter.hline(r.x_range(), r.center().y, Stroke::new(1.0, fg));
            }
        }

        let cursor = content.cursor;
        if cursor.shape != CursorShape::Hidden && self.exit.is_none() {
            let r = cell_rect(cursor.point.line.0, cursor.point.column.0, cell.x);
            if inner.expand(1.0).contains_rect(r) {
                match (cursor.shape, focused) {
                    (CursorShape::Beam, _) => {
                        painter.rect_filled(Rect::from_min_size(r.min, vec2(2.0, cell.y)), CornerRadius::ZERO, colors.cursor);
                    }
                    (CursorShape::Underline, _) => {
                        painter.rect_filled(
                            Rect::from_min_size(pos2(r.left(), r.bottom() - 2.0), vec2(cell.x, 2.0)),
                            CornerRadius::ZERO,
                            colors.cursor,
                        );
                    }
                    (CursorShape::Block, true) => {
                        painter.rect_filled(r, CornerRadius::ZERO, colors.cursor);
                        let c = term.grid()[cursor.point].c;
                        if c != ' ' {
                            painter.text(r.left_top(), Align2::LEFT_TOP, self.drawable(ui, c, font), font.clone(), colors.bg);
                        }
                    }
                    _ => {
                        painter.rect_stroke(r, CornerRadius::ZERO, Stroke::new(1.0, colors.cursor), egui::StrokeKind::Inside);
                    }
                }
            }
        }
    }
}

impl Terminal {
    /// `c`, or a stand-in when no installed font has it (otherwise egui draws an empty box).
    fn drawable(&self, ui: &egui::Ui, c: char, font: &FontId) -> char {
        if c.is_ascii() {
            return c;
        }
        *self.glyphs.borrow_mut().entry(c).or_insert_with(|| {
            let has = |c: char| ui.ctx().fonts_mut(|f| f.has_glyph(font, c));
            if has(c) {
                c
            } else {
                Some(substitute(c)).filter(|&s| has(s)).unwrap_or('·')
            }
        })
    }
}

/// A look-alike for symbols TUIs use that the bundled fonts lack.
fn substitute(c: char) -> char {
    match c {
        // Dingbat stars and asterisks (Claude Code's spinner).
        '\u{2722}'..='\u{274B}' => '*',
        '⏺' | '⬤' | '⚫' => '●',
        '⎿' => '└',
        '⏵' => '▸',
        '⏴' => '◂',
        _ => '·',
    }
}
