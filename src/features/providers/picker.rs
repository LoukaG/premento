//! The Tool and Model pickers: which local CLI (and model) runs analyses and Ask sessions.

use eframe::egui::{self, RichText};

use super::{ProviderConfig, runner};
use crate::ui::theme::pal;

/// The provider picked, with its model resolved.
pub struct Selection {
    pub provider: ProviderConfig,
    pub model: Option<String>,
    /// E.g. "Claude Code · haiku".
    pub label: String,
}

pub struct ProviderPicker {
    providers: Vec<ProviderConfig>,
    available: Vec<bool>,
    idx: usize,
    /// Model picked for each provider; empty means its default.
    models: Vec<String>,
    /// Whether each provider's model is typed in rather than picked from its list.
    custom: Vec<bool>,
}

impl ProviderPicker {
    /// Starts on `default` if it is installed, else on the first installed provider.
    pub fn new(providers: Vec<ProviderConfig>, default: Option<&str>) -> Self {
        let available: Vec<bool> = providers.iter().map(runner::is_available).collect();
        let idx = default
            .and_then(|name| providers.iter().position(|p| p.name == name))
            .filter(|&i| available[i])
            .or_else(|| available.iter().position(|&a| a))
            .unwrap_or(0);
        let n = providers.len();
        Self { providers, available, idx, models: vec![String::new(); n], custom: vec![false; n] }
    }

    pub fn provider(&self) -> &ProviderConfig {
        &self.providers[self.idx]
    }

    /// Whether the picked provider's CLI is installed.
    pub fn is_available(&self) -> bool {
        self.available[self.idx]
    }

    pub fn selection(&self) -> Selection {
        let provider = self.provider().clone();
        let model = provider.effective_model(&self.models[self.idx]).map(str::to_owned);
        let label = provider.label(model.as_deref());
        Selection { provider, model, label }
    }

    /// The Tool combo box, followed by the model picker when the tool has models.
    pub fn show(&mut self, ui: &mut egui::Ui) {
        let p = pal(ui);
        ui.label(RichText::new("Tool").color(p.muted));
        let current = self.provider().name.clone();
        egui::ComboBox::from_id_salt("provider")
            .selected_text(RichText::new(current).color(p.text))
            .width(190.0)
            .height(300.0)
            .show_ui(ui, |ui| {
                for (i, prov) in self.providers.iter().enumerate() {
                    let label = if self.available[i] {
                        prov.name.clone()
                    } else {
                        format!("{}  — not installed", prov.name)
                    };
                    ui.add_enabled_ui(self.available[i], |ui| {
                        ui.selectable_value(&mut self.idx, i, label)
                            .on_hover_text(format!("{} {}", prov.command, prov.args.join(" ")));
                    });
                }
            });
        self.model_picker(ui);
    }

    /// Model picker for the selected provider: its default, its listed models, or a typed name.
    fn model_picker(&mut self, ui: &mut egui::Ui) {
        let p = pal(ui);
        let i = self.idx;
        let provider = &self.providers[i];
        if !provider.supports_models() {
            return;
        }
        let default = match &provider.default_model {
            Some(m) => format!("Default ({m})"),
            None => "Default".to_owned(),
        };
        let (picked, custom) = (&mut self.models[i], &mut self.custom[i]);
        let selected = if *custom {
            "Other…".to_owned()
        } else if picked.is_empty() {
            default.clone()
        } else {
            picked.clone()
        };
        ui.label(RichText::new("Model").color(p.muted));
        egui::ComboBox::from_id_salt(("model", i))
            .selected_text(RichText::new(selected).color(p.text))
            .width(130.0)
            .show_ui(ui, |ui| {
                if ui.selectable_label(!*custom && picked.is_empty(), &default).clicked() {
                    picked.clear();
                    *custom = false;
                }
                for m in &provider.models {
                    if ui.selectable_label(!*custom && picked == m, m).clicked() {
                        *picked = m.clone();
                        *custom = false;
                    }
                }
                if ui.selectable_label(*custom, "Other…").clicked() {
                    *custom = true;
                }
            });
        if *custom {
            ui.add(
                egui::TextEdit::singleline(picked)
                    .hint_text("model name")
                    .desired_width(140.0),
            )
            .on_hover_text(format!("Passed as: {}", provider.model_args.join(" ")));
        }
    }
}
