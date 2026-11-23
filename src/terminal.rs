//! A terminal emulator widget: runs a program in a PTY and draws it in egui.
//!
//! `alacritty_terminal` does the PTY, the escape-sequence parsing and the screen grid, and answers
//! the queries TUIs send (cursor position, device attributes, colors). This module draws the grid
//! and turns egui input into the bytes a terminal would send.

use std::path::Path;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};

use alacritty_terminal::event::{Event, EventListener, Notify, OnResize, WindowSize};
use alacritty_terminal::event_loop::{EventLoop, Msg, Notifier};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Column, Point, Side};
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::test::TermSize;
use alacritty_terminal::term::{self, Term, TermMode, viewport_to_point};
use alacritty_terminal::tty;
use alacritty_terminal::vte::ansi::{Color as AnsiColor, CursorShape, NamedColor, Rgb};
use anyhow::{Context as _, Result};
use eframe::egui::{
    self, Align2, Color32, CornerRadius, FontId, Key, Modifiers, Rect, Sense, Stroke, Vec2,
    pos2, vec2,
};

use crate::theme::{self, pal};

const FONT_SIZE: f32 = 13.0;
const PADDING: f32 = 8.0;

#[derive(Clone)]
struct Listener {
    tx: Sender<Event>,
    ctx: egui::Context,
}

impl EventListener for Listener {
    fn send_event(&self, event: Event) {
        let _ = self.tx.send(event);
        self.ctx.request_repaint();
    }
}

pub struct Terminal {
    term: Arc<FairMutex<Term<Listener>>>,
    notifier: Notifier,
    events: Receiver<Event>,
    /// Columns and rows of the PTY.
    size: (u16, u16),
    /// `Some(code)` once the program has exited (`code` is `None` when killed by a signal).
    exit: Option<Option<i32>>,
    scroll_lines: f32,
    id: egui::Id,
    /// Characters as drawn: those the fonts lack are replaced (see [`substitute`]).
    glyphs: std::cell::RefCell<std::collections::HashMap<char, char>>,
}

impl Terminal {
    /// Starts `program` with `args` in `cwd`.
    pub fn spawn(ctx: &egui::Context, program: &Path, args: Vec<String>, cwd: &Path) -> Result<Self> {
        static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let id = egui::Id::unique(("memento-terminal", NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
        let (cols, rows) = (100u16, 30u16);
        #[allow(clippy::needless_update)] // Windows has an extra field
        let options = tty::Options {
            shell: Some(tty::Shell::new(program.to_string_lossy().into_owned(), args)),
            working_directory: Some(cwd.to_path_buf()),
            drain_on_exit: true,
            env: [("TERM", "xterm-256color"), ("COLORTERM", "truecolor")]
                .into_iter()
                .map(|(k, v)| (k.to_owned(), v.to_owned()))
                .collect(),
            ..Default::default()
        };
        let (tx, events) = channel();
        let listener = Listener { tx, ctx: ctx.clone() };
        let config = term::Config { scrolling_history: 10_000, ..Default::default() };
        let term = Arc::new(FairMutex::new(Term::new(
            config,
            &TermSize::new(cols as usize, rows as usize),
            listener.clone(),
        )));
        let pty = tty::new(&options, window_size(cols, rows, vec2(8.0, 16.0)), 0)
            .with_context(|| format!("starting {}", program.display()))?;
        let event_loop = EventLoop::new(term.clone(), listener, pty, true, false)?;
        let notifier = Notifier(event_loop.channel());
        event_loop.spawn();
        Ok(Self {
            term,
            notifier,
            events,
            size: (cols, rows),
            exit: None,
            scroll_lines: 0.0,
            id,
            glyphs: Default::default(),
        })
    }

    /// `Some(exit code)` once the program has ended.
    pub fn exit(&self) -> Option<Option<i32>> {
        self.exit
    }

    pub fn request_focus(&self, ctx: &egui::Context) {
        ctx.memory_mut(|m| m.request_focus(self.id));
    }

    /// Handles output-side events (replies to the program's queries, exit). Call every frame,
    /// also while the terminal is not shown, so programs waiting on a reply are not stalled.
    pub fn pump(&mut self, ctx: &egui::Context) {
        let colors = Palette::new(ctx.global_style().visuals.dark_mode);
        self.handle_events(ctx, &colors);
    }

    /// Draws the terminal in all the space left in `ui` and handles its input.
    pub fn show(&mut self, ui: &mut egui::Ui) {
        let colors = Palette::new(ui.visuals().dark_mode);
        self.handle_events(ui.ctx(), &colors);

        let font = FontId::monospace(FONT_SIZE);
        let cell = ui.ctx().fonts_mut(|f| vec2(f.glyph_width(&font, 'M'), f.row_height(&font)));
        let height = ui.available_height().max(240.0);
        let rect = Rect::from_min_size(ui.cursor().min, vec2(ui.available_width(), height));
        let response = ui.interact(rect, self.id, Sense::click_and_drag());
        ui.advance_cursor_after_rect(rect);
        let inner = rect.shrink(PADDING);

        let cols = ((inner.width() / cell.x).floor() as u16).max(10);
        let rows = ((inner.height() / cell.y).floor() as u16).max(4);
        if (cols, rows) != self.size {
            self.size = (cols, rows);
            self.term.lock().resize(TermSize::new(cols as usize, rows as usize));
            self.notifier.on_resize(window_size(cols, rows, cell));
        }

        if response.clicked() || response.drag_started() {
            response.request_focus();
        }
        let focused = response.has_focus();
        if focused {
            ui.memory_mut(|m| {
                m.set_focus_lock_filter(
                    self.id,
                    egui::EventFilter { tab: true, horizontal_arrows: true, vertical_arrows: true, escape: true },
                )
            });
            self.handle_keyboard(ui);
        }
        if response.hovered() {
            self.handle_scroll(ui, cell.y);
        }
        self.handle_selection(ui, &response, inner, cell);

        self.paint(ui, rect, inner, cell, &font, &colors, focused);
    }

    fn handle_events(&mut self, ctx: &egui::Context, colors: &Palette) {
        while let Ok(event) = self.events.try_recv() {
            match event {
                Event::PtyWrite(text) => self.write(text.into_bytes()),
                Event::ColorRequest(index, format) => {
                    let c = self.term.lock().colors()[index].map(to_color32).unwrap_or_else(|| colors.indexed(index));
                    self.write(format(Rgb { r: c.r(), g: c.g(), b: c.b() }).into_bytes());
                }
                Event::TextAreaSizeRequest(format) => {
                    let cell = ctx.fonts_mut(|f| {
                        let font = FontId::monospace(FONT_SIZE);
                        vec2(f.glyph_width(&font, 'M'), f.row_height(&font))
                    });
                    self.write(format(window_size(self.size.0, self.size.1, cell)).into_bytes());
                }
                // Reading the clipboard is not possible from here; answer with nothing.
                Event::ClipboardLoad(_, format) => self.write(format("").into_bytes()),
                Event::ClipboardStore(_, text) => ctx.copy_text(text),
                Event::ChildExit(status) => self.exit = Some(status.code()),
                Event::Exit => {
                    self.exit.get_or_insert(None);
                }
                _ => {}
            }
        }
    }

    fn write(&self, bytes: impl Into<std::borrow::Cow<'static, [u8]>>) {
        self.notifier.notify(bytes);
    }

    /// Sends typed input, which also jumps back to the bottom and clears the selection.
    fn input(&self, bytes: Vec<u8>) {
        if self.exit.is_some() {
            return;
        }
        {
            let mut term = self.term.lock();
            term.scroll_display(Scroll::Bottom);
            term.selection = None;
        }
        self.write(bytes);
    }

    fn handle_keyboard(&mut self, ui: &egui::Ui) {
        let mode = *self.term.lock().mode();
        for event in ui.input(|i| i.events.clone()) {
            match event {
                egui::Event::Text(text) => self.input(text.into_bytes()),
                egui::Event::Paste(text) => {
                    let text = text.replace("\r\n", "\r").replace('\n', "\r");
                    let bytes = if mode.contains(TermMode::BRACKETED_PASTE) {
                        format!("\x1b[200~{}\x1b[201~", text.replace('\x1b', ""))
                    } else {
                        text
                    };
                    self.input(bytes.into_bytes());
                }
                egui::Event::Copy => {
                    let selected = self.term.lock().selection_to_string().filter(|s| !s.is_empty());
                    match selected {
                        Some(text) => ui.ctx().copy_text(text),
                        // Off macOS, Ctrl+C arrives as Copy: send it as an interrupt.
                        None if !cfg!(target_os = "macos") => self.input(vec![0x03]),
                        None => {}
                    }
                }
                egui::Event::Cut if !cfg!(target_os = "macos") => self.input(vec![0x18]),
                egui::Event::Key { key, pressed: true, modifiers, .. } => {
                    if let Some(bytes) = key_bytes(key, modifiers, mode.contains(TermMode::APP_CURSOR)) {
                        self.input(bytes);
                    }
                }
                _ => {}
            }
        }
    }

    fn handle_scroll(&mut self, ui: &egui::Ui, line_height: f32) {
        let mut lines = 0.0;
        for event in ui.input(|i| i.events.clone()) {
            if let egui::Event::MouseWheel { unit, delta, .. } = event {
                lines += match unit {
                    egui::MouseWheelUnit::Point => delta.y / line_height,
                    egui::MouseWheelUnit::Line => delta.y,
                    egui::MouseWheelUnit::Page => delta.y * self.size.1 as f32,
                };
            }
        }
        self.scroll_lines += lines;
        let whole = self.scroll_lines.trunc() as i32;
        if whole == 0 {
            return;
        }
        self.scroll_lines -= whole as f32;
        let mut term = self.term.lock();
        let mode = *term.mode();
        if mode.contains(TermMode::ALT_SCREEN | TermMode::ALTERNATE_SCROLL) {
            // Full-screen apps have no scrollback: send arrow keys like other terminals do.
            drop(term);
            let arrow: &[u8] = match (whole > 0, mode.contains(TermMode::APP_CURSOR)) {
                (true, true) => b"\x1bOA",
                (true, false) => b"\x1b[A",
                (false, true) => b"\x1bOB",
                (false, false) => b"\x1b[B",
            };
            self.write(arrow.repeat(whole.unsigned_abs() as usize));
        } else {
            term.scroll_display(Scroll::Delta(whole));
        }
    }

    fn handle_selection(&mut self, ui: &egui::Ui, response: &egui::Response, inner: Rect, cell: Vec2) {
        let Some(pos) = response.interact_pointer_pos() else { return };
        let mut term = self.term.lock();
        let offset = term.grid().display_offset();
        let x = ((pos.x - inner.left()) / cell.x).max(0.0);
        let col = (x as usize).min(term.columns().saturating_sub(1));
        let row = (((pos.y - inner.top()) / cell.y).max(0.0) as usize).min(term.screen_lines().saturating_sub(1));
        let side = if x.fract() < 0.5 { Side::Left } else { Side::Right };
        let point = viewport_to_point(offset, Point::new(row, Column(col)));

        if response.double_clicked() {
            term.selection = Some(Selection::new(SelectionType::Semantic, point, side));
        } else if response.drag_started_by(egui::PointerButton::Primary) {
            term.selection = Some(Selection::new(SelectionType::Simple, point, side));
        } else if response.dragged_by(egui::PointerButton::Primary) {
            if let Some(selection) = term.selection.as_mut() {
                selection.update(point, side);
            }
        } else if response.clicked() && !ui.input(|i| i.modifiers.shift) {
            term.selection = None;
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn paint(&self, ui: &egui::Ui, rect: Rect, inner: Rect, cell: Vec2, font: &FontId, colors: &Palette, focused: bool) {
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

impl Drop for Terminal {
    fn drop(&mut self) {
        // Stops the I/O thread, which drops the PTY and hangs up the program.
        let _ = self.notifier.0.send(Msg::Shutdown);
    }
}

fn window_size(cols: u16, rows: u16, cell: Vec2) -> WindowSize {
    WindowSize { num_cols: cols, num_lines: rows, cell_width: cell.x as u16, cell_height: cell.y as u16 }
}

fn to_color32(rgb: Rgb) -> Color32 {
    Color32::from_rgb(rgb.r, rgb.g, rgb.b)
}

/// The bytes a terminal sends for a key press that is not plain text.
fn key_bytes(key: Key, m: Modifiers, app_cursor: bool) -> Option<Vec<u8>> {
    // Cmd shortcuts belong to the app (copy/paste arrive as their own events).
    if m.mac_cmd {
        return None;
    }
    let modifier_param = 1 + m.shift as u8 + 2 * m.alt as u8 + 4 * m.ctrl as u8;
    let csi_letter = |letter: char| -> Vec<u8> {
        if modifier_param > 1 {
            format!("\x1b[1;{modifier_param}{letter}").into_bytes()
        } else if app_cursor {
            format!("\x1bO{letter}").into_bytes()
        } else {
            format!("\x1b[{letter}").into_bytes()
        }
    };
    let tilde = |code: u8| -> Vec<u8> {
        if modifier_param > 1 {
            format!("\x1b[{code};{modifier_param}~").into_bytes()
        } else {
            format!("\x1b[{code}~").into_bytes()
        }
    };
    let bytes = match key {
        Key::Enter if m.shift || m.alt => b"\x1b\r".to_vec(),
        Key::Enter => b"\r".to_vec(),
        Key::Tab if m.shift => b"\x1b[Z".to_vec(),
        Key::Tab => b"\t".to_vec(),
        Key::Backspace if m.alt => b"\x1b\x7f".to_vec(),
        Key::Backspace if m.ctrl => b"\x08".to_vec(),
        Key::Backspace => b"\x7f".to_vec(),
        Key::Escape => b"\x1b".to_vec(),
        Key::ArrowUp => csi_letter('A'),
        Key::ArrowDown => csi_letter('B'),
        Key::ArrowRight => csi_letter('C'),
        Key::ArrowLeft => csi_letter('D'),
        Key::Home => csi_letter('H'),
        Key::End => csi_letter('F'),
        Key::Insert => tilde(2),
        Key::Delete => tilde(3),
        Key::PageUp => tilde(5),
        Key::PageDown => tilde(6),
        Key::F1 => b"\x1bOP".to_vec(),
        Key::F2 => b"\x1bOQ".to_vec(),
        Key::F3 => b"\x1bOR".to_vec(),
        Key::F4 => b"\x1bOS".to_vec(),
        Key::F5 => tilde(15),
        Key::F6 => tilde(17),
        Key::F7 => tilde(18),
        Key::F8 => tilde(19),
        Key::F9 => tilde(20),
        Key::F10 => tilde(21),
        Key::F11 => tilde(23),
        Key::F12 => tilde(24),
        _ if m.ctrl => {
            let byte = match key {
                Key::Space => 0x00,
                Key::OpenBracket => 0x1b,
                Key::Backslash => 0x1c,
                Key::CloseBracket => 0x1d,
                _ => {
                    let name = key.name();
                    let c = name.chars().next().filter(|c| name.len() == 1 && c.is_ascii_alphabetic())?;
                    c.to_ascii_lowercase() as u8 & 0x1f
                }
            };
            if m.alt { vec![0x1b, byte] } else { vec![byte] }
        }
        _ => return None,
    };
    Some(bytes)
}

/// Terminal colors, matched to the app's light or dark theme.
struct Palette {
    fg: Color32,
    bg: Color32,
    cursor: Color32,
    selection: Color32,
    ansi: [Color32; 16],
}

impl Palette {
    fn new(dark: bool) -> Self {
        let p = if dark { theme::DARK } else { theme::LIGHT };
        let hex = |v: u32| Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8);
        let ansi = if dark {
            [
                0x1d2029, 0xef6b73, 0x7ccf8a, 0xe5c07b, 0x61afef, 0xc678dd, 0x56b6c2, 0xc8ccd4,
                0x5c6370, 0xff7b85, 0x98e0a4, 0xf0d08f, 0x7cc0ff, 0xd896ec, 0x6fd0dc, 0xffffff,
            ]
        } else {
            [
                0x2b2f38, 0xc8323c, 0x2f8f46, 0x9a6b00, 0x2f6fd6, 0x9b3fc4, 0x1d8592, 0xd4d7de,
                0x6b7280, 0xe0444e, 0x3aa957, 0xb88200, 0x4a86e8, 0xb054d8, 0x23a0b0, 0xf4f5f7,
            ]
        }
        .map(hex);
        Self {
            fg: p.text,
            bg: p.surface_sunken,
            cursor: p.accent_text,
            selection: p.accent.gamma_multiply(0.35),
            ansi,
        }
    }

    /// Color for a palette index: 0-15 ANSI, 16-255 xterm, 256+ the named colors.
    fn indexed(&self, i: usize) -> Color32 {
        const CUBE: [u8; 6] = [0, 95, 135, 175, 215, 255];
        let dim = |c: Color32| c.gamma_multiply(0.66);
        match i {
            0..=15 => self.ansi[i],
            16..=231 => {
                let i = i - 16;
                Color32::from_rgb(CUBE[i / 36], CUBE[(i / 6) % 6], CUBE[i % 6])
            }
            232..=255 => {
                let v = (8 + 10 * (i - 232)) as u8;
                Color32::from_rgb(v, v, v)
            }
            _ if i == NamedColor::Foreground as usize || i == NamedColor::BrightForeground as usize => self.fg,
            _ if i == NamedColor::Background as usize => self.bg,
            _ if i == NamedColor::Cursor as usize => self.cursor,
            _ if i == NamedColor::DimForeground as usize => dim(self.fg),
            _ if (NamedColor::DimBlack as usize..=NamedColor::DimWhite as usize).contains(&i) => {
                dim(self.ansi[i - NamedColor::DimBlack as usize])
            }
            _ => self.fg,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_map_to_terminal_bytes() {
        let none = Modifiers::NONE;
        let ctrl = Modifiers { ctrl: true, ..Default::default() };
        let shift = Modifiers { shift: true, ..Default::default() };
        assert_eq!(key_bytes(Key::C, ctrl, false), Some(vec![0x03]));
        assert_eq!(key_bytes(Key::Enter, none, false), Some(b"\r".to_vec()));
        assert_eq!(key_bytes(Key::Tab, shift, false), Some(b"\x1b[Z".to_vec()));
        assert_eq!(key_bytes(Key::ArrowUp, none, true), Some(b"\x1bOA".to_vec()));
        assert_eq!(key_bytes(Key::ArrowLeft, ctrl, false), Some(b"\x1b[1;5D".to_vec()));
        assert_eq!(key_bytes(Key::A, none, false), None, "plain letters come as Text events");
    }

    /// Runs a real program in a PTY and checks its output lands in the grid.
    #[test]
    fn runs_a_program() {
        let ctx = egui::Context::default();
        let mut t = Terminal::spawn(
            &ctx,
            Path::new("/bin/sh"),
            vec!["-c".into(), "printf 'hello \\033[31mred\\033[0m'; sleep 0.2".into()],
            Path::new("/"),
        )
        .unwrap();
        let start = std::time::Instant::now();
        while t.exit.is_none() && start.elapsed().as_secs() < 5 {
            if let Ok(Event::ChildExit(status)) = t.events.recv_timeout(std::time::Duration::from_millis(50)) {
                t.exit = Some(status.code());
            }
        }
        assert_eq!(t.exit, Some(Some(0)));
        let term = t.term.lock();
        let line: String = (0..9).map(|c| term.grid()[Point::new(alacritty_terminal::index::Line(0), Column(c))].c).collect();
        assert_eq!(line, "hello red");
        let red = term.grid()[Point::new(alacritty_terminal::index::Line(0), Column(6))].fg;
        assert_eq!(red, AnsiColor::Named(NamedColor::Red));
    }
}

