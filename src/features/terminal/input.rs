//! Keyboard, scroll and mouse-selection input, turned into the bytes a terminal sends.


use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Column, Point, Side};
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::term::{TermMode, viewport_to_point};
use eframe::egui::{
    self, Key, Modifiers, Rect, Vec2,
};

use super::Terminal;

impl Terminal {
    pub(super) fn handle_keyboard(&mut self, ui: &egui::Ui) {
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

    pub(super) fn handle_scroll(&mut self, ui: &egui::Ui, line_height: f32) {
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

    pub(super) fn handle_selection(&mut self, ui: &egui::Ui, response: &egui::Response, inner: Rect, cell: Vec2) {
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
}
