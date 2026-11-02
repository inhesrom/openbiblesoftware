//! eframe app, egui_tiles Workspace, the Resource-text widget and the Tools.
//!
//! Layer L6: the only layer allowed to depend on egui, eframe, winit or rfd.

/// Window title used until the product is named.
pub const APP_TITLE: &str = "Open Bible Software";

/// The desktop application state.
#[derive(Debug, Default)]
pub struct App;

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ui, |ui| {
            ui.centered_and_justified(|ui| {
                ui.heading(APP_TITLE);
            });
        });
    }
}
