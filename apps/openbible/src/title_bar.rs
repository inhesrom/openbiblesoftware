//! macOS: the main window's title bar is the app's menu bar (#297). This builds the main
//! window's viewport and reads what double-clicking a title bar does (System Settings →
//! Desktop & Dock → "Double-click a window's title bar to"), with the system's own settings
//! tool, as [`crate::system_motion`] does: `defaults read -g AppleActionOnDoubleClick`, or the
//! older `AppleMiniaturizeOnDoubleClick` switch when that key was never written. It is read on
//! a worker thread at launch and again whenever the main window comes back to the front; until
//! the first read finishes, and where neither key is set, a double-click zooms (the system's
//! default).

use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use obs_ui_egui::{SystemTitleBar, TitleBarDoubleClick};

/// The main window's viewport: titled, at its first size and, on macOS, with its title bar
/// merged into the menu bar. The title stays set, so the Window menu, Mission Control, the
/// Dock and VoiceOver still name the window.
pub fn main_viewport() -> eframe::egui::ViewportBuilder {
    let builder = eframe::egui::ViewportBuilder::default()
        .with_title(obs_ui_egui::APP_TITLE)
        .with_inner_size([1280.0, 800.0]);
    if cfg!(target_os = "macos") {
        obs_ui_egui::unified_title_bar(builder)
    } else {
        builder
    }
}

pub struct OsTitleBar {
    action: Arc<AtomicU8>,
    reading: Arc<AtomicBool>,
}

impl OsTitleBar {
    /// Start reading the setting.
    pub fn start() -> Self {
        let bar = Self {
            action: Arc::new(AtomicU8::new(code(TitleBarDoubleClick::Zoom))),
            reading: Arc::new(AtomicBool::new(false)),
        };
        bar.refresh();
        bar
    }
}

impl SystemTitleBar for OsTitleBar {
    fn double_click(&self) -> TitleBarDoubleClick {
        from_code(self.action.load(Ordering::Relaxed))
    }

    fn refresh(&self) {
        if self.reading.swap(true, Ordering::AcqRel) {
            return;
        }
        let (action, reading) = (Arc::clone(&self.action), Arc::clone(&self.reading));
        let spawned = std::thread::Builder::new()
            .name("title-bar-double-click".into())
            .spawn(move || {
                action.store(code(read()), Ordering::Relaxed);
                reading.store(false, Ordering::Release);
            });
        if spawned.is_err() {
            self.reading.store(false, Ordering::Release);
        }
    }
}

fn code(action: TitleBarDoubleClick) -> u8 {
    match action {
        TitleBarDoubleClick::Zoom => 0,
        TitleBarDoubleClick::Minimize => 1,
        TitleBarDoubleClick::Nothing => 2,
    }
}

fn from_code(code: u8) -> TitleBarDoubleClick {
    match code {
        1 => TitleBarDoubleClick::Minimize,
        2 => TitleBarDoubleClick::Nothing,
        _ => TitleBarDoubleClick::Zoom,
    }
}

/// What `defaults read -g <key>` printed, if the key is set.
fn global_default(key: &str) -> Option<String> {
    let out = Command::new("defaults")
        .args(["read", "-g", key])
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// This computer's setting (macOS only; elsewhere the system draws its own title bars).
fn read() -> TitleBarDoubleClick {
    if !cfg!(target_os = "macos") {
        return TitleBarDoubleClick::Zoom;
    }
    double_click_from(
        global_default("AppleActionOnDoubleClick").as_deref(),
        global_default("AppleMiniaturizeOnDoubleClick").as_deref(),
    )
}

/// The setting from what `defaults` printed for `AppleActionOnDoubleClick` (Maximize, Fill,
/// Minimize or None) and the older `AppleMiniaturizeOnDoubleClick` (1: minimize), each `None`
/// when unset. Fill (macOS 15) has no window command of its own and is taken as Zoom.
fn double_click_from(action: Option<&str>, miniaturize: Option<&str>) -> TitleBarDoubleClick {
    match action.map(str::trim) {
        Some("Minimize") => TitleBarDoubleClick::Minimize,
        Some("None") => TitleBarDoubleClick::Nothing,
        Some(_) => TitleBarDoubleClick::Zoom,
        None if miniaturize.map(str::trim) == Some("1") => TitleBarDoubleClick::Minimize,
        None => TitleBarDoubleClick::Zoom,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_double_click_setting_reads_as_its_action() {
        use TitleBarDoubleClick::*;
        assert_eq!(double_click_from(Some("Maximize\n"), None), Zoom);
        assert_eq!(double_click_from(Some("Fill\n"), None), Zoom);
        assert_eq!(double_click_from(Some("Minimize\n"), Some("0\n")), Minimize);
        assert_eq!(double_click_from(Some("None\n"), Some("1\n")), Nothing);
        // The action key wins; without it, the older switch decides.
        assert_eq!(double_click_from(Some("Maximize\n"), Some("1\n")), Zoom);
        assert_eq!(double_click_from(None, Some("1\n")), Minimize);
        assert_eq!(double_click_from(None, Some("0\n")), Zoom);
        assert_eq!(double_click_from(None, None), Zoom);
        for action in [Zoom, Minimize, Nothing] {
            assert_eq!(from_code(code(action)), action);
        }
    }

    /// The main window keeps its title, for the Window menu and VoiceOver; on macOS its
    /// content fills the title bar, which shows no title; elsewhere it is as it was.
    #[test]
    fn the_main_window_is_titled_and_its_title_bar_is_the_menu_bar_on_macos() {
        let v = main_viewport();
        assert_eq!(v.title.as_deref(), Some(obs_ui_egui::APP_TITLE));
        assert_eq!(v.inner_size, Some(eframe::egui::vec2(1280.0, 800.0)));
        if cfg!(target_os = "macos") {
            assert_eq!(v.fullsize_content_view, Some(true));
            assert_eq!(v.titlebar_shown, Some(false));
            assert_eq!(v.title_shown, Some(false));
        } else {
            let plain = eframe::egui::ViewportBuilder::default()
                .with_title(obs_ui_egui::APP_TITLE)
                .with_inner_size([1280.0, 800.0]);
            assert_eq!(v, plain);
        }
    }

    /// Reads this computer's setting without failing, whatever it is.
    #[test]
    fn the_setting_is_read_on_a_worker_and_can_be_read_again() {
        let bar = OsTitleBar::start();
        for _ in 0..200 {
            if !bar.reading.load(Ordering::Acquire) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let first = bar.double_click();
        bar.refresh();
        bar.refresh();
        assert_eq!(first, read());
    }
}
