use std::sync::{Arc, LazyLock};

use eframe::egui;

static ICON: LazyLock<Arc<egui::IconData>> = LazyLock::new(|| {
    Arc::new(
        eframe::icon_data::from_png_bytes(include_bytes!("../assets/premento-icon.png"))
            .expect("the embedded PRemento icon must be a valid PNG"),
    )
});

pub fn data() -> Arc<egui::IconData> {
    Arc::clone(&ICON)
}

pub fn texture(ctx: &egui::Context) -> egui::TextureHandle {
    let icon = data();
    ctx.load_texture(
        "premento-logo",
        egui::ColorImage::from_rgba_unmultiplied(
            [icon.width as usize, icon.height as usize],
            &icon.rgba,
        ),
        egui::TextureOptions::LINEAR,
    )
}
