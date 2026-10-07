mod app;
mod export;
mod media;
mod ui;

use app::SimpleMkvPlayer;
use eframe::egui;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Simple MKV Player")
            .with_inner_size([1100.0, 760.0])
            .with_min_inner_size([800.0, 550.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Simple MKV Player",
        options,
        Box::new(|cc| Ok(Box::new(SimpleMkvPlayer::new(cc)))),
    )
}
