//! The operating system's reduce motion setting (#225), read with the system's own settings
//! tool so the app needs no platform bindings: `defaults` on macOS (Accessibility → Display →
//! Reduce motion), `reg` on Windows (Accessibility → Visual effects → Animation effects, whose
//! switch also turns off window animations, `MinAnimate`), `gsettings` on Linux desktops that
//! follow GNOME's `enable-animations` (GNOME, Cinnamon, Budgie, Pantheon …).
//!
//! It is read on a worker thread at launch and again whenever the main window comes back to
//! the front ([`SystemMotion::refresh`]); until the first read finishes, motion is not
//! reduced. Where the tool or the setting is missing, it reads as not reduced.

use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use obs_ui_egui::SystemMotion;

pub struct OsMotion {
    reduce: Arc<AtomicBool>,
    reading: Arc<AtomicBool>,
}

impl OsMotion {
    /// Start reading the setting.
    pub fn start() -> Self {
        let motion = Self {
            reduce: Arc::new(AtomicBool::new(false)),
            reading: Arc::new(AtomicBool::new(false)),
        };
        motion.refresh();
        motion
    }
}

impl SystemMotion for OsMotion {
    fn reduce_motion(&self) -> bool {
        self.reduce.load(Ordering::Relaxed)
    }

    fn refresh(&self) {
        if self.reading.swap(true, Ordering::AcqRel) {
            return;
        }
        let (reduce, reading) = (Arc::clone(&self.reduce), Arc::clone(&self.reading));
        let spawned = std::thread::Builder::new()
            .name("reduce-motion".into())
            .spawn(move || {
                if let Some(on) = read() {
                    reduce.store(on, Ordering::Relaxed);
                }
                reading.store(false, Ordering::Release);
            });
        if spawned.is_err() {
            self.reading.store(false, Ordering::Release);
        }
    }
}

/// Run a settings tool and return what it printed, if it ran and succeeded.
fn output(program: &str, args: &[&str]) -> Option<String> {
    let mut command = Command::new(program);
    command.args(args);
    #[cfg(windows)]
    {
        // No console window flashing up behind the app.
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let out = command.output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// The system's setting: `Some(true)` when it asks for less motion, `None` when it cannot be
/// read (an unset key reads as the default, motion on).
fn read() -> Option<bool> {
    #[cfg(target_os = "macos")]
    {
        // An unset key makes `defaults` fail: Reduce motion was never turned on.
        Some(
            output(
                "defaults",
                &["read", "com.apple.universalaccess", "reduceMotion"],
            )
            .is_some_and(|v| macos_reduces(&v)),
        )
    }
    #[cfg(windows)]
    {
        let out = output(
            "reg",
            &[
                "query",
                r"HKCU\Control Panel\Desktop\WindowMetrics",
                "/v",
                "MinAnimate",
            ],
        );
        Some(out.is_some_and(|v| windows_reduces(&v)))
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        output(
            "gsettings",
            &["get", "org.gnome.desktop.interface", "enable-animations"],
        )
        .map(|v| gnome_reduces(&v))
    }
    #[cfg(not(any(unix, windows)))]
    {
        None
    }
}

/// `defaults read com.apple.universalaccess reduceMotion` prints `1` when it is on.
#[cfg_attr(not(any(target_os = "macos", test)), allow(dead_code))]
fn macos_reduces(printed: &str) -> bool {
    printed.trim() == "1"
}

/// `reg query … /v MinAnimate` prints `MinAnimate    REG_SZ    0` when animations are off.
#[cfg_attr(not(any(windows, test)), allow(dead_code))]
fn windows_reduces(printed: &str) -> bool {
    printed
        .lines()
        .find(|l| l.trim_start().starts_with("MinAnimate"))
        .and_then(|l| l.split_whitespace().last())
        == Some("0")
}

/// `gsettings get org.gnome.desktop.interface enable-animations` prints `false` when off.
#[cfg_attr(not(any(all(unix, not(target_os = "macos")), test)), allow(dead_code))]
fn gnome_reduces(printed: &str) -> bool {
    printed.trim() == "false"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_systems_answer_reads_as_reduce_motion_on_or_off() {
        assert!(macos_reduces("1\n"));
        assert!(!macos_reduces("0\n"));
        let off = "\nHKEY_CURRENT_USER\\Control Panel\\Desktop\\WindowMetrics\n    MinAnimate    REG_SZ    0\n\n";
        assert!(windows_reduces(off));
        assert!(!windows_reduces(&off.replace("REG_SZ    0", "REG_SZ    1")));
        assert!(!windows_reduces(""));
        assert!(gnome_reduces("false\n"));
        assert!(!gnome_reduces("true\n"));
    }

    /// Reads this computer's setting without failing, whatever it is.
    #[test]
    fn the_setting_is_read_on_a_worker_and_can_be_read_again() {
        let motion = OsMotion::start();
        for _ in 0..200 {
            if !motion.reading.load(Ordering::Acquire) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let first = motion.reduce_motion();
        motion.refresh();
        motion.refresh();
        assert_eq!(first, read().unwrap_or(false));
    }
}
