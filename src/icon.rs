use std::sync::{Arc, LazyLock};

use eframe::egui;

static ICON: LazyLock<Arc<egui::IconData>> = LazyLock::new(|| {
    Arc::new(
        eframe::icon_data::from_png_bytes(include_bytes!("../assets/premento-icon.png"))
            .expect("the embedded PRemento icon must be a valid PNG"),
    )
});

// Give the Dock icon breathing room and a rounded tile. Keep the in-app logo transparent.
static WINDOW_ICON: LazyLock<Arc<egui::IconData>> = LazyLock::new(|| {
    let source = &*ICON;
    let padding = source.width.max(source.height) / 8;
    let mut icon = egui::IconData {
        width: source.width + padding * 2,
        height: source.height + padding * 2,
        ..Default::default()
    };
    icon.rgba.resize((icon.width * icon.height * 4) as usize, 0);
    let background = crate::theme::DARK.surface.to_array();
    let inset = icon.width.min(icon.height) as f32 * 0.08;
    let half_width = icon.width as f32 / 2.0 - inset;
    let half_height = icon.height as f32 / 2.0 - inset;
    let radius = half_width.min(half_height) * 0.44;

    for y in 0..icon.height {
        for x in 0..icon.width {
            // A one-pixel coverage ramp smooths the rounded silhouette.
            let dx =
                ((x as f32 + 0.5 - icon.width as f32 / 2.0).abs() - (half_width - radius)).max(0.0);
            let dy = ((y as f32 + 0.5 - icon.height as f32 / 2.0).abs() - (half_height - radius))
                .max(0.0);
            let coverage = (radius + 0.5 - dx.hypot(dy)).clamp(0.0, 1.0);
            if coverage == 0.0 {
                continue;
            }

            let offset = ((y * icon.width + x) * 4) as usize;
            let pixel = &mut icon.rgba[offset..offset + 4];
            pixel.copy_from_slice(&background);
            if x >= padding
                && x < padding + source.width
                && y >= padding
                && y < padding + source.height
            {
                let source_offset = (((y - padding) * source.width + x - padding) * 4) as usize;
                let foreground = &source.rgba[source_offset..source_offset + 4];
                let alpha = u32::from(foreground[3]);
                for channel in 0..3 {
                    pixel[channel] = ((u32::from(foreground[channel]) * alpha
                        + u32::from(background[channel]) * (255 - alpha)
                        + 127)
                        / 255) as u8;
                }
            }
            pixel[3] = (coverage * 255.0).round() as u8;
        }
    }
    Arc::new(icon)
});

pub fn window_icon() -> Arc<egui::IconData> {
    Arc::clone(&WINDOW_ICON)
}

pub fn texture(ctx: &egui::Context) -> egui::TextureHandle {
    let icon = &*ICON;
    ctx.load_texture(
        "premento-logo",
        egui::ColorImage::from_rgba_unmultiplied(
            [icon.width as usize, icon.height as usize],
            &icon.rgba,
        ),
        egui::TextureOptions::LINEAR,
    )
}
