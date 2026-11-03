//! Open Bible Software desktop app.

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title(obs_ui_egui::APP_TITLE)
            .with_inner_size([1280.0, 800.0]),
        ..Default::default()
    };
    eframe::run_native(
        obs_ui_egui::APP_TITLE,
        options,
        Box::new(|_cc| {
            Ok(Box::new(obs_ui_egui::App::with_file(
                std::env::args_os().nth(1).map(Into::into),
            )))
        }),
    )
}
