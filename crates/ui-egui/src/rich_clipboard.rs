//! The clipboard's rich-text side (#154). Copy verses puts HTML on the clipboard together
//! with its plain-text form, so word processors paste formatted verses and plain editors the
//! text. egui copies plain text only; the desktop app supplies the system clipboard here.

/// A clipboard that takes HTML with its plain-text form.
pub trait RichClipboard: Send + Sync {
    /// Put `html` on the clipboard as rich text, with `plain` for pastes that take text only.
    fn copy_html(&self, html: &str, plain: &str) -> Result<(), String>;
}
