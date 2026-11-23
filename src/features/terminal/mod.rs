//! A terminal emulator widget: runs a program in a PTY and draws it in egui.
//!
//! `alacritty_terminal` does the PTY, the escape-sequence parsing and the screen grid, and answers
//! the queries TUIs send (cursor position, device attributes, colors). This module draws the grid
//! and turns egui input into the bytes a terminal would send.

mod input;
mod paint;
mod palette;

use std::path::Path;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};

use alacritty_terminal::event::{Event, EventListener, Notify, OnResize, WindowSize};
use alacritty_terminal::event_loop::{EventLoop, Msg, Notifier};
use alacritty_terminal::grid::Scroll;
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::test::TermSize;
use alacritty_terminal::term::{self, Term};
use alacritty_terminal::tty;
use alacritty_terminal::vte::ansi::Rgb;
use anyhow::{Context as _, Result};
use eframe::egui::{
    self, FontId, Rect, Sense, Vec2, vec2,
};

use palette::{Palette, to_color32};

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
    /// Characters as drawn: those the fonts lack are replaced (see `paint::substitute`).
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

#[cfg(test)]
mod tests {
    use alacritty_terminal::index::{Column, Point};
    use alacritty_terminal::vte::ansi::{Color as AnsiColor, NamedColor};

    use super::*;

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

