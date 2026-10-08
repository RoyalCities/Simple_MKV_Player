mod app;
mod export;
mod media;

use app::SimpleMkvPlayer;
use eframe::egui;

fn main() -> eframe::Result<()> {
    let icon = eframe::icon_data::from_png_bytes(include_bytes!("assets/smkv_logo.png"))
        .expect("Failed to load src/assets/smkv_logo.png");

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Simple MKV Player")
            .with_inner_size([1100.0, 760.0])
            .with_min_inner_size([800.0, 260.0])
            .with_icon(icon),
        ..Default::default()
    };

    eframe::run_native(
        "Simple MKV Player",
        options,
        Box::new(|cc| Ok(Box::new(SimpleMkvPlayer::new(cc)))),
    )
}
