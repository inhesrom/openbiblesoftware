//! Rich text on the system clipboard for Copy verses (#154), through `arboard` (the
//! clipboard eframe itself uses). One clipboard is kept for the app's life, made on first
//! use: on Linux the app serves what it copied for as long as it holds the clipboard.

use std::sync::Mutex;

use obs_ui_egui::RichClipboard;

#[derive(Default)]
pub struct SystemClipboard {
    clipboard: Mutex<Option<arboard::Clipboard>>,
}

impl RichClipboard for SystemClipboard {
    fn copy_html(&self, html: &str, plain: &str) -> Result<(), String> {
        let mut held = self
            .clipboard
            .lock()
            .map_err(|_| "the clipboard is unavailable".to_string())?;
        let clipboard = match held.as_mut() {
            Some(c) => c,
            None => held.insert(arboard::Clipboard::new().map_err(|e| e.to_string())?),
        };
        clipboard
            .set_html(html, Some(plain))
            .map_err(|e| e.to_string())
    }
}
