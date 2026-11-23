//! The analysis tabs and the controls above them.

pub mod action_bar;
mod impact;
mod intent;
mod intro;
mod overview;
mod risk;

use eframe::egui;

pub use impact::impact;
pub use intent::intent;
pub use intro::intro;
pub use overview::overview;
pub use risk::risk;

use crate::ui::widgets::card;

/// The model's raw response, shown when it could not be parsed.
pub fn raw_response(ui: &mut egui::Ui, raw: &str) {
    card(ui, |ui| {
        let mut raw = raw;
        ui.add(
            egui::TextEdit::multiline(&mut raw)
                .code_editor()
                .frame(egui::Frame::NONE)
                .desired_width(f32::INFINITY),
        );
    });
}
