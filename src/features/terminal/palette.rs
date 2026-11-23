//! Terminal colors, matched to the app theme.


use alacritty_terminal::vte::ansi::{NamedColor, Rgb};
use eframe::egui::Color32;

use crate::ui::theme;

pub(super) fn to_color32(rgb: Rgb) -> Color32 {
    Color32::from_rgb(rgb.r, rgb.g, rgb.b)
}

/// Terminal colors, matched to the app's light or dark theme.
pub(super) struct Palette {
    pub(super) fg: Color32,
    pub(super) bg: Color32,
    pub(super) cursor: Color32,
    pub(super) selection: Color32,
    pub(super) ansi: [Color32; 16],
}

impl Palette {
    pub(super) fn new(dark: bool) -> Self {
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
    pub(super) fn indexed(&self, i: usize) -> Color32 {
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
