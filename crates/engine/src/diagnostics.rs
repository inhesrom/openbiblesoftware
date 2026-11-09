//! Log files, Crash reports and Report a problem (#219).
//!
//! - The app writes a **Log** to the **Log folder** ([`default_log_dir`]): `openbible.log`,
//!   rotated at [`LOG_FILE_BYTES`] into `openbible.1.log` … (the last [`LOG_FILES_KEPT`]
//!   kept), at the **Log level** the user chooses in Settings → Privacy and data
//!   (`app.log_level`). [`start_logging`] installs it as the process's `log` logger; log lines
//!   name commands, jobs and problems, never Notes, Highlights or Resource text: another
//!   library's message is withheld unless it is vetted ([`log_line`]).
//! - A panic writes a **Crash report** there ([`install_panic_hook`]): what panicked and
//!   where (of the message only the standard library's own phrase, [`panic_summary`]), a
//!   backtrace, the app version, the OS and the recent log lines, with the home folder
//!   written as `~`. It is a plain text file; nothing is sent automatically.
//! - At the next launch the app offers **Report a problem** for a new Crash report
//!   ([`CrashReport::is_new`]): the user reads it, then may copy it or open GitHub's new-issue
//!   form with it in the web address ([`issue_url`]; opening it sends the report to GitHub,
//!   submitting the issue publishes it). Help → Report a problem… shows the newest report, or
//!   a problem report made now ([`problem_report`]).
//! - Help → Open log folder (`help.open_log_folder`) shows the folder in the file manager.
//! - **Save report as zip…** (in Report a problem) and Help → Save diagnostics as zip…
//!   (`help.save_diagnostics_zip`) write a **Diagnostics zip** where the user chooses: a
//!   README, the Crash report being reviewed (when there is one) and the log files, and
//!   nothing else ([`diagnostics_zip_entries`]); from a window, exactly the files it listed
//!   ([`Session::list_diagnostics_zip`]). It is written whole, then named, and never over an
//!   existing file ([`write_diagnostics_zip`]). The user sends it, if at all.

use std::collections::VecDeque;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Mutex, OnceLock};

use serde_json::{Value, json};

use crate::command::{CommandError, CommandSpec, ParamKind, ParamSpec, Params, always};
use crate::session::Session;
use crate::workspace::Effect;

/// The current log file in the Log folder.
pub const LOG_FILE: &str = "openbible.log";
/// A log file is rotated once it would grow past this.
pub const LOG_FILE_BYTES: u64 = 1 << 20;
/// Rotated log files kept beside the current one (`openbible.1.log` is the newest).
pub const LOG_FILES_KEPT: usize = 3;
/// Crash reports kept in the Log folder; older ones are deleted when a new one is written.
pub const CRASH_REPORTS_KEPT: usize = 10;
/// Log lines kept in memory for a Crash report or a problem report.
const RECENT_LINES: usize = 200;
/// Log lines a report ends with.
const REPORT_LOG_LINES: usize = 60;
/// Where Report a problem's Open GitHub issue… goes (the user submits it there).
pub const NEW_ISSUE_URL: &str = "https://github.com/inhesrom/openbiblesoftware/issues/new";
/// Browsers and GitHub cut longer URLs; the issue body is shortened to fit.
const ISSUE_URL_MAX: usize = 7_000;
/// The User database preference holding the Log level.
pub(crate) const LOG_LEVEL_PREF: &str = "log_level";

/// Libraries whose log messages are written as they are: the graphics crates, which see GPU
/// objects and pixels but never text (egui turns text into glyph images before they get it).
/// Vetted against the locked versions (#219); any other library's message is withheld.
const VETTED_TARGETS: &[&str] = &["wgpu", "naga", "egui_wgpu"];

const CRASH_PREFIX: &str = "crash-";
const SEEN_SUFFIX: &str = ".seen.txt";
const NEW_SUFFIX: &str = ".txt";

// ------------------------------------------------------------------------------- Log level

/// How much the app writes to its log files.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum LogLevel {
    /// Only errors.
    Error,
    /// Errors and warnings.
    Warn,
    /// Also what the app did: launches, commands' failures, background jobs (the default).
    #[default]
    Info,
    /// Also every command run, for tracking down a problem.
    Debug,
}

impl LogLevel {
    pub const ALL: [(LogLevel, &'static str, &'static str); 4] = [
        (LogLevel::Error, "error", "Errors only"),
        (LogLevel::Warn, "warn", "Warnings"),
        (LogLevel::Info, "info", "Normal"),
        (LogLevel::Debug, "debug", "Detailed"),
    ];

    pub fn id(self) -> &'static str {
        Self::ALL
            .iter()
            .find(|l| l.0 == self)
            .map_or("info", |l| l.1)
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.iter().find(|l| l.1 == id).map(|l| l.0)
    }

    fn filter(self) -> log::LevelFilter {
        match self {
            LogLevel::Error => log::LevelFilter::Error,
            LogLevel::Warn => log::LevelFilter::Warn,
            LogLevel::Info => log::LevelFilter::Info,
            LogLevel::Debug => log::LevelFilter::Debug,
        }
    }

    fn from_u8(v: u8) -> Self {
        match v {
            0 => LogLevel::Error,
            1 => LogLevel::Warn,
            3 => LogLevel::Debug,
            _ => LogLevel::Info,
        }
    }

    fn to_u8(self) -> u8 {
        match self {
            LogLevel::Error => 0,
            LogLevel::Warn => 1,
            LogLevel::Info => 2,
            LogLevel::Debug => 3,
        }
    }

    /// Whether a record at `level` from `target` is written. Other crates' (the windowing
    /// and graphics libraries') records are written from Warn up unless the level is Debug.
    pub fn writes(self, level: log::Level, target: &str) -> bool {
        let wanted = if is_ours(target) || self == LogLevel::Debug {
            self.filter()
        } else {
            self.filter().min(log::LevelFilter::Warn)
        };
        level <= wanted
    }
}

/// A record from this app's own crates, whose messages name commands, jobs and failures and
/// never quote the user's text.
fn is_ours(target: &str) -> bool {
    target.starts_with("obs_") || target.starts_with("openbible")
}

/// The log line for `record`, at `when` (seconds since 1970). The app's own messages and
/// those of [`VETTED_TARGETS`] are written as they are. Another library's message could
/// quote what the user typed (egui-winit logs a whole text-input event when it ignores its
/// range), so only where it was logged from is written: the source file in that library and
/// the line.
pub fn log_line(record: &log::Record<'_>, when: u64) -> String {
    let target = record.target();
    let vetted = VETTED_TARGETS.iter().any(|t| {
        target
            .strip_prefix(t)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with(['_', ':']))
    });
    let message = if is_ours(target) || vetted {
        record.args().to_string()
    } else {
        format!(
            "(message withheld; logged at {})",
            call_site(record.file(), record.line())
        )
    };
    format!(
        "{} {:<5} {target}: {message}",
        utc_stamp(when),
        record.level()
    )
}

/// A library's source file from where it was built: `egui-winit-0.37.0/src/lib.rs:809` for a
/// crates.io dependency, else the file name.
fn call_site(file: Option<&str>, line: Option<u32>) -> String {
    let file = file.unwrap_or("?").replace('\\', "/");
    let short = match file.split_once("/registry/src/") {
        // After the registry's index folder.
        Some((_, rest)) => rest.split_once('/').map_or(rest, |(_, r)| r),
        None => file.rsplit('/').next().unwrap_or(&file),
    };
    match line {
        Some(n) => format!("{short}:{n}"),
        None => short.to_string(),
    }
}

// --------------------------------------------------------------------------- Rotating log

/// A log file that rotates by size, and the recent lines it was given.
#[derive(Debug)]
pub struct RotatingLog {
    dir: PathBuf,
    max_bytes: u64,
    kept: usize,
    file: Option<File>,
    size: u64,
    recent: VecDeque<String>,
}

impl RotatingLog {
    /// Append to [`LOG_FILE`] in `dir` (made if missing), rotating at [`LOG_FILE_BYTES`].
    pub fn open(dir: &Path) -> std::io::Result<Self> {
        Self::with_limits(dir, LOG_FILE_BYTES, LOG_FILES_KEPT)
    }

    pub fn with_limits(dir: &Path, max_bytes: u64, kept: usize) -> std::io::Result<Self> {
        std::fs::create_dir_all(dir)?;
        let mut log = Self {
            dir: dir.to_path_buf(),
            max_bytes,
            kept,
            file: None,
            size: 0,
            recent: VecDeque::new(),
        };
        log.reopen()?;
        Ok(log)
    }

    fn reopen(&mut self) -> std::io::Result<()> {
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.dir.join(LOG_FILE))?;
        self.size = file.metadata().map(|m| m.len()).unwrap_or(0);
        self.file = Some(file);
        Ok(())
    }

    /// The rotated file `n` (1 is the newest): `openbible.1.log`.
    pub fn rotated_name(n: usize) -> String {
        format!("openbible.{n}.log")
    }

    /// Write one line (a newline is added), rotating first if it would not fit.
    pub fn write_line(&mut self, line: &str) {
        let len = line.len() as u64 + 1;
        if self.size > 0 && self.size + len > self.max_bytes {
            self.rotate();
        }
        if let Some(f) = &mut self.file
            && writeln!(f, "{line}").is_ok()
        {
            self.size += len;
        }
        if self.recent.len() >= RECENT_LINES {
            self.recent.pop_front();
        }
        self.recent.push_back(line.to_string());
    }

    fn rotate(&mut self) {
        self.file = None;
        if self.kept == 0 {
            let _ = std::fs::remove_file(self.dir.join(LOG_FILE));
        } else {
            let _ = std::fs::remove_file(self.dir.join(Self::rotated_name(self.kept)));
            for n in (1..self.kept).rev() {
                let _ = std::fs::rename(
                    self.dir.join(Self::rotated_name(n)),
                    self.dir.join(Self::rotated_name(n + 1)),
                );
            }
            let _ = std::fs::rename(
                self.dir.join(LOG_FILE),
                self.dir.join(Self::rotated_name(1)),
            );
        }
        let _ = self.reopen();
    }

    /// The last `n` lines written, oldest first.
    pub fn recent(&self, n: usize) -> Vec<String> {
        let skip = self.recent.len().saturating_sub(n);
        self.recent.iter().skip(skip).cloned().collect()
    }
}

// ----------------------------------------------------------------------- The app's logger

/// The process's logger: the rotating log in the Log folder at the Log level.
struct AppLog {
    log: Mutex<RotatingLog>,
    level: AtomicU8,
}

static APP_LOG: OnceLock<AppLog> = OnceLock::new();

impl log::Log for AppLog {
    fn enabled(&self, meta: &log::Metadata<'_>) -> bool {
        LogLevel::from_u8(self.level.load(Ordering::Relaxed)).writes(meta.level(), meta.target())
    }

    fn log(&self, record: &log::Record<'_>) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let line = log_line(record, now_secs());
        // A panic while the log is held must not hang the hook (or a later line): skip.
        if let Ok(mut log) = self.log.try_lock() {
            log.write_line(&line);
        }
    }

    fn flush(&self) {}
}

/// Write the process's `log` records to the Log folder `dir` at `level`. Once per process;
/// later calls only change the level. The desktop app calls it before anything else.
pub fn start_logging(dir: &Path, level: LogLevel) -> Result<(), String> {
    if APP_LOG.get().is_none() {
        let log = RotatingLog::open(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let app = APP_LOG.get_or_init(|| AppLog {
            log: Mutex::new(log),
            level: AtomicU8::new(level.to_u8()),
        });
        log::set_logger(app).map_err(|e| e.to_string())?;
    }
    set_log_level(level);
    log::info!(
        target: "openbible",
        "Open Bible Software {} started on {}; logging to {}",
        env!("CARGO_PKG_VERSION"),
        os_description(),
        dir.display()
    );
    Ok(())
}

/// Change the Log level of the process's log (a no-op until [`start_logging`]).
pub fn set_log_level(level: LogLevel) {
    if let Some(app) = APP_LOG.get() {
        app.level.store(level.to_u8(), Ordering::Relaxed);
        log::set_max_level(level.filter());
    }
}

/// The last `n` lines of the process's log, oldest first (none before [`start_logging`], or
/// while a line is being written on the panicking thread).
pub fn recent_log_lines(n: usize) -> Vec<String> {
    APP_LOG
        .get()
        .and_then(|app| app.log.try_lock().ok().map(|log| log.recent(n)))
        .unwrap_or_default()
}

// ------------------------------------------------------------------------- Crash reports

/// What a panic hook knows about one panic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PanicFacts {
    pub message: String,
    /// `file:line:column` in the source.
    pub location: Option<String>,
    pub thread: String,
    pub backtrace: String,
    /// When, in seconds since 1970 (UTC).
    pub when: u64,
}

/// The text of a Crash report for `panic`, with `log_lines` (oldest first). `home`, when
/// given, is written as `~` wherever it appears, so the report does not carry the user's
/// account name in paths.
pub fn crash_report_text(panic: &PanicFacts, log_lines: &[String], home: Option<&str>) -> String {
    let mut out = String::new();
    out.push_str("Open Bible Software crash report\n");
    out.push_str(&about_lines(panic.when));
    out.push_str(&format!("Thread: {}\n", panic.thread));
    out.push_str(&format!("Panic: {}\n", panic.message));
    if let Some(at) = &panic.location {
        out.push_str(&format!("At: {at}\n"));
    }
    out.push_str("\nBacktrace:\n");
    out.push_str(panic.backtrace.trim_end());
    out.push('\n');
    push_log_lines(&mut out, log_lines);
    redact_home(&out, home)
}

/// A report made now, with no crash: the app version, the OS and the recent log lines, for
/// Help → Report a problem… when there is no Crash report.
pub fn problem_report(log_lines: &[String], home: Option<&str>) -> String {
    let mut out = String::from("Open Bible Software problem report\n");
    out.push_str(&about_lines(now_secs()));
    push_log_lines(&mut out, log_lines);
    redact_home(&out, home)
}

fn about_lines(when: u64) -> String {
    format!(
        "Version: {}\nOS: {}\nTime: {}\n",
        env!("CARGO_PKG_VERSION"),
        os_description(),
        utc_stamp(when)
    )
}

fn push_log_lines(out: &mut String, lines: &[String]) {
    out.push_str("\nRecent log lines:\n");
    if lines.is_empty() {
        out.push_str("(none)\n");
    }
    let skip = lines.len().saturating_sub(REPORT_LOG_LINES);
    for l in &lines[skip..] {
        out.push_str(l);
        out.push('\n');
    }
}

fn redact_home(text: &str, home: Option<&str>) -> String {
    match home.map(|h| h.trim_end_matches(['/', '\\'])) {
        Some(h) if h.len() > 1 => text.replace(h, "~"),
        _ => text.to_string(),
    }
}

/// The user's home folder, to write as `~` in reports.
pub fn home_dir() -> Option<String> {
    std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .ok()
}

/// Write a Crash report into the Log folder `dir`, as a new report, keeping the newest
/// [`CRASH_REPORTS_KEPT`]. Returns its path. Each report gets a file of its own, even when
/// threads panic in the same second: the name is taken by creating the file, never by
/// checking first, and its number is above every report of that second still in the folder,
/// so it sorts newest and the reports removed to keep [`CRASH_REPORTS_KEPT`] are older ones.
pub fn write_crash_report(dir: &Path, when: u64, text: &str) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let stamp = file_stamp(when);
    let mut n = std::fs::read_dir(dir)?
        .filter_map(Result::ok)
        .filter_map(|e| {
            let name = e.file_name();
            let (s, n) = crash_name_parts(name.to_str()?)?;
            (s == stamp).then_some(n)
        })
        .max()
        .map_or(1, |n| n.saturating_add(1));
    let (path, mut file) = loop {
        let path = if n == 1 {
            dir.join(format!("{CRASH_PREFIX}{stamp}{NEW_SUFFIX}"))
        } else {
            dir.join(format!("{CRASH_PREFIX}{stamp}-{n}{NEW_SUFFIX}"))
        };
        n = n.saturating_add(1);
        if seen_path(&path).exists() {
            continue;
        }
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => break (path, file),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    };
    file.write_all(text.as_bytes())?;
    drop(file);
    for old in crash_reports(dir).into_iter().skip(CRASH_REPORTS_KEPT) {
        if old.path != path {
            let _ = std::fs::remove_file(&old.path);
        }
    }
    Ok(path)
}

/// Write a Crash report for every panic into the Log folder `dir`, then do what the
/// previous hook did (print it). Installed by the desktop app at launch.
pub fn install_panic_hook(dir: PathBuf) {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let facts = PanicFacts {
            message: panic_summary(&panic_message(info.payload())),
            location: info
                .location()
                .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column())),
            thread: std::thread::current()
                .name()
                .unwrap_or("unnamed")
                .to_string(),
            backtrace: without_panic_frames(
                &std::backtrace::Backtrace::force_capture().to_string(),
            ),
            when: now_secs(),
        };
        log::error!(
            target: "openbible",
            "panicked at {}: {}",
            facts.location.as_deref().unwrap_or("?"),
            facts.message
        );
        let text = crash_report_text(
            &facts,
            &recent_log_lines(REPORT_LOG_LINES),
            home_dir().as_deref(),
        );
        match write_crash_report(&dir, facts.when, &text) {
            Ok(path) => eprintln!("openbible: crash report written to {}", path.display()),
            Err(e) => eprintln!("openbible: could not write a crash report: {e}"),
        }
        previous(info);
    }));
}

/// A backtrace without its leading frames in this hook and the standard library's panic
/// machinery, so it starts where the panic was raised.
pub fn without_panic_frames(backtrace: &str) -> String {
    const MACHINERY: &[&str] = &[
        "std::panicking::",
        "core::panicking::",
        "std::sys::backtrace::__rust_end_short_backtrace",
        "rust_begin_unwind",
        "install_panic_hook",
        "std::panic::PanicHookInfo",
        // Capturing the backtrace itself (macOS and Windows list these frames).
        "std::backtrace",
        "Backtrace>::create",
    ];
    let mut frames: Vec<String> = Vec::new();
    for line in backtrace.lines() {
        let numbered = line
            .trim_start()
            .split_once(": ")
            .is_some_and(|(n, _)| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()));
        match frames.last_mut() {
            Some(frame) if !numbered => {
                frame.push('\n');
                frame.push_str(line);
            }
            _ => frames.push(line.to_string()),
        }
    }
    let skip = frames
        .iter()
        .take_while(|f| {
            let name = f.lines().next().unwrap_or("");
            MACHINERY.iter().any(|m| name.contains(m))
        })
        .count();
    if skip == frames.len() {
        return backtrace.to_string();
    }
    frames[skip..].join("\n")
}

/// A panic's message: its `&str` or `String` payload.
pub fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|s| (*s).to_string())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "(no message)".into())
}

/// What a Crash report says about a panic message: only the standard library's own phrase
/// it starts with ([`STD_PANIC_PHRASES`], e.g. ``called `Option::unwrap()` on a `None` value``
/// or `index out of bounds`), followed by [`DETAILS_WITHHELD`] when the message goes on; any
/// other message is [`MESSAGE_WITHHELD`]. A panic message is text the code formatted, and it
/// may hold the user's own (a Note, a search, a file name), quoted or not, so nothing past a
/// known phrase is kept. The location and the backtrace say where it happened.
pub fn panic_summary(message: &str) -> String {
    match STD_PANIC_PHRASES
        .iter()
        .find(|phrase| message.starts_with(*phrase))
    {
        Some(phrase) if message.len() == phrase.len() => (*phrase).to_string(),
        Some(phrase) => format!("{phrase} {DETAILS_WITHHELD}"),
        None => MESSAGE_WITHHELD.to_string(),
    }
}

/// A Crash report's panic message when it is not one of the standard library's.
pub const MESSAGE_WITHHELD: &str = "(message withheld)";
/// Written after a standard library phrase in place of the rest of the panic message.
pub const DETAILS_WITHHELD: &str = "(details withheld)";

/// The opening phrases of the standard library's panic messages, kept by [`panic_summary`].
/// Each is fixed text; what follows it (a value, an index, a quoted string) is not kept.
const STD_PANIC_PHRASES: &[&str] = &[
    "called `Result::unwrap()` on an `Err` value",
    "called `Result::unwrap_err()` on an `Ok` value",
    "called `Option::unwrap()` on a `None` value",
    "assertion `left == right` failed",
    "assertion `left != right` failed",
    "assertion `left matches right` failed",
    "index out of bounds",
    "byte index",
    "range start index",
    "range end index",
    "slice index starts at",
    "attempt to add with overflow",
    "attempt to subtract with overflow",
    "attempt to multiply with overflow",
    "attempt to divide by zero",
    "attempt to calculate the remainder with a divisor of zero",
    "attempt to negate with overflow",
    "attempt to shift left with overflow",
    "attempt to shift right with overflow",
    "already borrowed",
    "already mutably borrowed",
    "capacity overflow",
    "explicit panic",
    "internal error: entered unreachable code",
    "not yet implemented",
    "not implemented",
];

/// One Crash report in the Log folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrashReport {
    pub path: PathBuf,
    /// Not yet offered at a launch (or reviewed).
    pub is_new: bool,
    pub text: String,
}

fn seen_path(path: &Path) -> PathBuf {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let stem = name.strip_suffix(NEW_SUFFIX).unwrap_or(name);
    path.with_file_name(format!("{stem}{SEEN_SUFFIX}"))
}

/// The Crash reports in the Log folder `dir`, newest first.
pub fn crash_reports(dir: &Path) -> Vec<CrashReport> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<CrashReport> = entries
        .filter_map(Result::ok)
        .filter_map(|e| {
            let name = e.file_name().to_str()?.to_string();
            if !name.starts_with(CRASH_PREFIX) || !name.ends_with(NEW_SUFFIX) {
                return None;
            }
            let text = std::fs::read_to_string(e.path()).ok()?;
            Some(CrashReport {
                path: e.path(),
                is_new: !name.ends_with(SEEN_SUFFIX),
                text,
            })
        })
        .collect();
    // The names start with the time, so the newest sorts last.
    out.sort_by_key(|r| std::cmp::Reverse(crash_sort_key(&r.path)));
    out
}

fn crash_sort_key(path: &Path) -> (String, u32) {
    path.file_name()
        .and_then(|n| n.to_str())
        .and_then(crash_name_parts)
        .map_or((String::new(), 0), |(stamp, n)| (stamp.to_string(), n))
}

/// The time and number in a Crash report's file name: `crash-20231114-221320-3.txt` is
/// `("20231114-221320", 3)`. The first report of a second has no number and is 1.
fn crash_name_parts(name: &str) -> Option<(&str, u32)> {
    let stem = name
        .strip_suffix(SEEN_SUFFIX)
        .or_else(|| name.strip_suffix(NEW_SUFFIX))?;
    let rest = stem.strip_prefix(CRASH_PREFIX)?;
    let stamp = rest.get(..15)?;
    let n = match rest.get(15..)? {
        "" => 1,
        more => more.strip_prefix('-')?.parse().ok()?,
    };
    Some((stamp, n))
}

/// The report has been offered: it is not offered again at the next launch (Help → Report
/// a problem… still shows it).
pub fn mark_seen(report: &CrashReport) -> std::io::Result<PathBuf> {
    if !report.is_new {
        return Ok(report.path.clone());
    }
    let to = seen_path(&report.path);
    std::fs::rename(&report.path, &to)?;
    Ok(to)
}

/// The GitHub new-issue address with `report` as the body (shortened to fit a URL; the full
/// report is what Copy report copies). The report is in the address, so opening it in the
/// browser sends the report to GitHub; it becomes a public issue only if the user submits
/// the form.
pub fn issue_url(report: &str) -> String {
    let title = report
        .lines()
        .find_map(|l| l.strip_prefix("Panic: "))
        .map_or_else(
            || "Problem report".to_string(),
            |p| format!("Crash: {}", p.chars().take(80).collect::<String>()),
        );
    let head = "**What were you doing when it happened?**\n\n\n\n**Report**\n\n```\n";
    let tail = "```\n";
    let url = |body: &str| {
        format!(
            "{NEW_ISSUE_URL}?title={}&body={}",
            percent_encode(&title),
            percent_encode(body)
        )
    };
    let full = format!("{head}{report}{tail}");
    let whole = url(&full);
    if whole.len() <= ISSUE_URL_MAX {
        return whole;
    }
    let note = "… (shortened: paste the whole report from Copy report)\n";
    let mut kept = String::new();
    for line in report.lines() {
        let next = format!("{kept}{line}\n");
        if url(&format!("{head}{next}{note}{tail}")).len() > ISSUE_URL_MAX {
            break;
        }
        kept = next;
    }
    url(&format!("{head}{kept}{note}{tail}"))
}

/// `text` for a URL query value: unreserved bytes kept, every other byte `%XX`.
fn percent_encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len() * 3);
    for b in text.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

// ------------------------------------------------------------------- Diagnostics zip

/// The README in every Diagnostics zip.
pub const ZIP_README: &str = "README.txt";

/// One file of a Diagnostics zip: its name in the zip, what it is, and its text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZipEntry {
    pub name: String,
    pub about: &'static str,
    pub text: String,
}

/// The file name Save report as zip… suggests: `openbible-report-20260921-141320.zip` (UTC).
pub fn diagnostics_zip_name(when: u64) -> String {
    format!("openbible-report-{}.zip", file_stamp(when))
}

/// The files of a Diagnostics zip of the Log folder `dir`, in order: the README, the Crash
/// report `report` (when given; it must be one of the folder's), then the log files from the
/// newest. Nothing else is read: only [`LOG_FILE`] and its rotated files, by name, so the zip
/// holds what the Log and the Crash reports already hold and nothing extra. The log files
/// are written by [`log_line`], which withholds another library's message; here the home
/// folder is written as `~` in them, as in a report.
pub fn diagnostics_zip_entries(
    dir: &Path,
    report: Option<&CrashReport>,
    home: Option<&str>,
    when: u64,
) -> Vec<ZipEntry> {
    let mut files = Vec::new();
    if let Some(r) = report.filter(|r| is_crash_report_in(dir, &r.path)) {
        files.push(ZipEntry {
            name: crash_report_zip_name(&r.path),
            about: "the crash report",
            text: redact_home(&r.text, home),
        });
    }
    let logs = std::iter::once(LOG_FILE.to_string())
        .chain((1..=LOG_FILES_KEPT).map(RotatingLog::rotated_name));
    for (i, name) in logs.enumerate() {
        let Ok(bytes) = std::fs::read(dir.join(&name)) else {
            continue;
        };
        files.push(ZipEntry {
            about: if i == 0 {
                "the current log file"
            } else {
                "an older log file"
            },
            text: redact_home(&String::from_utf8_lossy(&bytes), home),
            name,
        });
    }
    let readme = zip_readme(&files, when);
    files.insert(
        0,
        ZipEntry {
            name: ZIP_README.into(),
            about: "what this zip holds and does not hold",
            text: readme,
        },
    );
    files
}

/// Whether `path` is a Crash report's file directly in the Log folder `dir`.
fn is_crash_report_in(dir: &Path, path: &Path) -> bool {
    path.parent() == Some(dir)
        && path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| crash_name_parts(n).is_some())
}

/// A Crash report's name in the zip: its file name, without the `.seen` of an offered one.
fn crash_report_zip_name(path: &Path) -> String {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("crash");
    match name.strip_suffix(SEEN_SUFFIX) {
        Some(stem) => format!("{stem}{NEW_SUFFIX}"),
        None => name.to_string(),
    }
}

fn zip_readme(files: &[ZipEntry], when: u64) -> String {
    let mut out = String::from("Open Bible Software diagnostics\n");
    out.push_str(&about_lines(when));
    out.push_str("\nThis zip holds:\n");
    out.push_str(&format!("- {ZIP_README}: this file\n"));
    for f in files {
        out.push_str(&format!("- {}: {}\n", f.name, f.about));
    }
    out.push_str(
        "\nIt does not hold your Notes, Highlights, Notebooks, Layouts, Workspace, settings, \
         User database, library or Resource text. Log lines name what the app did (launches, \
         commands, background jobs, problems); another library's messages are withheld, and \
         your home folder is written as ~.\n\nNothing was sent: you chose where to save this \
         file, and you choose whether to send it and to whom.\n",
    );
    out
}

/// Write `files` as a zip at `dest`, or beside it as `name (2).zip` … when `dest` exists:
/// an existing file is never replaced. The zip is written whole to a temporary file of its
/// own in the same folder, then given its name in one step (a hard link), so a zip under
/// that name is always complete. On a disk without hard links (FAT, exFAT) nothing is saved
/// and the error says so. Returns where it was written.
pub fn write_diagnostics_zip(dest: &Path, files: &[ZipEntry]) -> std::io::Result<PathBuf> {
    write_zip_linking(dest, files, |from, to| std::fs::hard_link(from, to))
}

/// [`write_diagnostics_zip`], giving the name with `link` (tests stand in a disk without
/// hard links).
fn write_zip_linking(
    dest: &Path,
    files: &[ZipEntry],
    link: impl Fn(&Path, &Path) -> std::io::Result<()>,
) -> std::io::Result<PathBuf> {
    let bytes = zip_bytes(files)?;
    let (temp, mut file) = create_temp(dest)?;
    // Only the temporary file made here is removed: another's of the same name is not ours.
    let written = (|| {
        file.write_all(&bytes)?;
        file.sync_all()?;
        drop(file);
        claim_name(&temp, dest, link)
    })();
    let _ = std::fs::remove_file(&temp);
    written
}

/// A new temporary file beside `dest`, `.name.<process>-<n>.partial` with the first `n` no
/// file has: `create_new` fails on a taken name, so two saves at once (in this process or
/// another) each get their own, and an existing file is never opened.
fn create_temp(dest: &Path) -> std::io::Result<(PathBuf, File)> {
    let folder = dest.parent().unwrap_or(Path::new("."));
    let name = dest
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("openbible-report.zip");
    for n in 0..1000 {
        let temp = folder.join(format!(".{name}.{}-{n}.partial", std::process::id()));
        match OpenOptions::new().write(true).create_new(true).open(&temp) {
            Ok(file) => return Ok((temp, file)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        format!("{}: 1000 temporary files are in the way", folder.display()),
    ))
}

fn zip_bytes(files: &[ZipEntry]) -> std::io::Result<Vec<u8>> {
    let mut out = std::io::Cursor::new(Vec::new());
    let mut zip = zip::ZipWriter::new(&mut out);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for f in files {
        zip.start_file(f.name.as_str(), options)
            .map_err(std::io::Error::other)?;
        zip.write_all(f.text.as_bytes())?;
    }
    zip.finish().map_err(std::io::Error::other)?;
    Ok(out.into_inner())
}

/// Give the complete file `temp` the first free name of `dest`, `dest (2)`, … with `link`
/// (a hard link). A link fails when the name is taken, so no existing file is replaced, even
/// one made meanwhile, and the name appears only with the whole file. Any other failure (a
/// disk without hard links) is an error: no fallback that could show the name empty or half
/// written, or replace a file.
fn claim_name(
    temp: &Path,
    dest: &Path,
    link: impl Fn(&Path, &Path) -> std::io::Result<()>,
) -> std::io::Result<PathBuf> {
    for n in 1..1000 {
        let to = numbered(dest, n);
        match link(temp, &to) {
            Ok(()) => return Ok(to),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => {
                return Err(std::io::Error::new(
                    e.kind(),
                    format!(
                        "nothing was saved: the zip could not be given its name ({e}); this \
                         disk may not support hard links, so choose a folder on another disk"
                    ),
                ));
            }
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        format!("{} and the next 998 names are taken", dest.display()),
    ))
}

/// `dest` for 1, else `stem (n).ext` beside it.
fn numbered(dest: &Path, n: u32) -> PathBuf {
    if n == 1 {
        return dest.to_path_buf();
    }
    let stem = dest.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    let name = match dest.extension().and_then(|e| e.to_str()) {
        Some(ext) => format!("{stem} ({n}).{ext}"),
        None => format!("{stem} ({n})"),
    };
    dest.with_file_name(name)
}

/// Where Save report as zip… suggests saving: Downloads, else Desktop, else the home folder.
pub fn default_zip_folder() -> Option<PathBuf> {
    let home = PathBuf::from(home_dir()?);
    ["Downloads", "Desktop"]
        .iter()
        .map(|d| home.join(d))
        .find(|d| d.is_dir())
        .or(Some(home))
}

// ------------------------------------------------------------------- Time and the OS

fn now_secs() -> u64 {
    crate::get_bibles::now_secs()
}

/// `YYYY-MM-DD HH:MM:SS UTC`.
pub fn utc_stamp(secs: u64) -> String {
    let (y, m, d, hh, mm, ss) = civil(secs);
    format!("{y:04}-{m:02}-{d:02} {hh:02}:{mm:02}:{ss:02} UTC")
}

/// `YYYYMMDD-HHMMSS`, for file names.
fn file_stamp(secs: u64) -> String {
    let (y, m, d, hh, mm, ss) = civil(secs);
    format!("{y:04}{m:02}{d:02}-{hh:02}{mm:02}{ss:02}")
}

fn civil(secs: u64) -> (i64, i64, i64, u64, u64, u64) {
    let days = i64::try_from(secs / 86_400).unwrap_or(0);
    let (y, m, d) = crate::reading_plans::PlanDate(days).civil();
    let rem = secs % 86_400;
    (y, m, d, rem / 3600, rem % 3600 / 60, rem % 60)
}

/// The OS name and version, and the CPU architecture: "Ubuntu 24.04.1 LTS (linux x86_64)".
pub fn os_description() -> String {
    static OS: OnceLock<String> = OnceLock::new();
    OS.get_or_init(|| {
        let arch = format!("{} {}", std::env::consts::OS, std::env::consts::ARCH);
        match os_version() {
            Some(v) => format!("{v} ({arch})"),
            None => arch,
        }
    })
    .clone()
}

fn os_version() -> Option<String> {
    if cfg!(target_os = "linux") {
        let text = std::fs::read_to_string("/etc/os-release").ok()?;
        let line = text.lines().find_map(|l| l.strip_prefix("PRETTY_NAME="))?;
        Some(line.trim_matches('"').to_string())
    } else if cfg!(target_os = "macos") {
        let text =
            std::fs::read_to_string("/System/Library/CoreServices/SystemVersion.plist").ok()?;
        let after = text.split("<key>ProductVersion</key>").nth(1)?;
        let v = after.split("<string>").nth(1)?.split("</string>").next()?;
        Some(format!("macOS {v}"))
    } else {
        None
    }
}

/// The Log folder: `$OBS_LOG_DIR`, else `logs` in the app's data folder.
pub fn default_log_dir() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("OBS_LOG_DIR") {
        return Some(PathBuf::from(d));
    }
    Some(crate::library::data_dir()?.join("logs"))
}

// ------------------------------------------------------------------- Session and commands

/// The Session's Log folder and Log level.
#[derive(Debug, Clone, Default)]
pub struct Diagnostics {
    log_dir: Option<PathBuf>,
    level: LogLevel,
    /// The files a zip window last listed, with their number: what saving it saves.
    listed_zip: Option<(i64, Vec<ZipEntry>)>,
    listings: i64,
}

impl Session {
    /// Where the app's logs and Crash reports are (the desktop app sets it at launch).
    pub fn set_log_dir(&mut self, dir: PathBuf) {
        self.diagnostics.log_dir = Some(dir);
    }

    pub fn log_dir(&self) -> Option<&Path> {
        self.diagnostics.log_dir.as_deref()
    }

    pub fn log_level(&self) -> LogLevel {
        self.diagnostics.level
    }

    /// The Crash reports in the Log folder, newest first.
    pub fn crash_reports(&self) -> Vec<CrashReport> {
        self.log_dir().map(crash_reports).unwrap_or_default()
    }

    /// What a Diagnostics zip made now would hold, for a window to list: the README,
    /// `report` (a Crash report in the Log folder) and the log files
    /// ([`diagnostics_zip_entries`]); none without a Log folder. The list is kept under the
    /// number returned with it, and `help.save_diagnostics_zip` with that number as `listed`
    /// saves exactly these files, however the log files have changed since. Only the last
    /// list is kept.
    pub fn list_diagnostics_zip(&mut self, report: Option<&CrashReport>) -> (i64, Vec<ZipEntry>) {
        let files = self.log_dir().map_or_else(Vec::new, |dir| {
            diagnostics_zip_entries(dir, report, home_dir().as_deref(), now_secs())
        });
        let d = &mut self.diagnostics;
        d.listings += 1;
        d.listed_zip = Some((d.listings, files.clone()));
        (d.listings, files)
    }

    /// The file name Save report as zip… suggests now ([`diagnostics_zip_name`]).
    pub fn diagnostics_zip_file_name(&self) -> String {
        diagnostics_zip_name(now_secs())
    }

    /// Apply the Log level saved in the User database (the default without one).
    pub(crate) fn load_log_level(&mut self) {
        let level = (self.user.as_ref())
            .and_then(|u| u.preference(LOG_LEVEL_PREF).ok().flatten())
            .and_then(|v| LogLevel::from_id(&v))
            .unwrap_or_default();
        self.diagnostics.level = level;
        set_log_level(level);
    }
}

/// The choices of `app.log_level`, for menus and Settings.
pub(crate) fn choices(
    s: &Session,
    command: &str,
) -> Option<(Vec<(&'static str, &'static str)>, &'static str)> {
    (command == "app.log_level").then(|| {
        (
            LogLevel::ALL.iter().map(|l| (l.1, l.2)).collect(),
            s.diagnostics.level.id(),
        )
    })
}

fn failed(command: &'static str, message: impl ToString) -> CommandError {
    CommandError::Failed {
        command,
        message: message.to_string(),
    }
}

fn set_level(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    const ID: &str = "app.log_level";
    let v = p.str("value").unwrap_or_default();
    let level = LogLevel::from_id(v)
        .ok_or_else(|| failed(ID, format!("“{v}” is not error, warn, info or debug")))?;
    s.diagnostics.level = level;
    set_log_level(level);
    if let Some(user) = &s.user {
        user.set_preference(LOG_LEVEL_PREF, level.id())
            .map_err(|e| failed(ID, format!("saving the Log level: {e}")))?;
    }
    Ok(json!({ "value": level.id() }))
}

fn has_log_dir(s: &Session) -> bool {
    s.diagnostics.log_dir.is_some()
}

fn open_log_folder(s: &mut Session, _: Params<'_>) -> Result<Value, CommandError> {
    const ID: &str = "help.open_log_folder";
    let dir = s
        .log_dir()
        .ok_or_else(|| failed(ID, "this app keeps no log folder"))?
        .to_path_buf();
    std::fs::create_dir_all(&dir).map_err(|e| failed(ID, format!("{}: {e}", dir.display())))?;
    s.workspace.push_effect(Effect::OpenFolder(dir.clone()));
    Ok(json!({ "path": dir.display().to_string() }))
}

fn report_problem(s: &mut Session, _: Params<'_>) -> Result<Value, CommandError> {
    s.workspace.push_effect(Effect::ReportProblem);
    Ok(Value::Null)
}

/// Without `path`, the app asks where (Help → Save diagnostics as zip…); with it, the zip
/// is written there (or beside it, never over a file): the files a window listed when
/// `listed` names that list ([`Session::list_diagnostics_zip`]), else the files now, with
/// the Crash report named `report` when given.
fn save_diagnostics_zip(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    const ID: &str = "help.save_diagnostics_zip";
    let Some(dest) = p.str("path").filter(|d| !d.is_empty()) else {
        s.workspace.push_effect(Effect::SaveDiagnosticsZip);
        return Ok(Value::Null);
    };
    let Some(dir) = s.log_dir().map(Path::to_path_buf) else {
        return Err(failed(ID, "this app keeps no log folder"));
    };
    let report = p.str("report").filter(|r| !r.is_empty());
    let files = match p.i64("listed") {
        Some(_) if report.is_some() => {
            return Err(failed(ID, "a listed zip already holds its crash report"));
        }
        Some(n) => match &s.diagnostics.listed_zip {
            Some((id, files)) if *id == n && !files.is_empty() => files.clone(),
            _ => {
                return Err(failed(
                    ID,
                    "those files are no longer listed: list them again",
                ));
            }
        },
        None => {
            let report = match report {
                None => None,
                Some(name) => Some(
                    s.crash_reports()
                        .into_iter()
                        .find(|r| r.path.file_name().and_then(|n| n.to_str()) == Some(name))
                        .ok_or_else(|| failed(ID, format!("there is no crash report “{name}”")))?,
                ),
            };
            diagnostics_zip_entries(&dir, report.as_ref(), home_dir().as_deref(), now_secs())
        }
    };
    let dest = PathBuf::from(dest);
    let path = write_diagnostics_zip(&dest, &files)
        .map_err(|e| failed(ID, format!("{}: {e}", dest.display())))?;
    let names: Vec<&str> = files.iter().map(|f| f.name.as_str()).collect();
    Ok(json!({ "path": path.display().to_string(), "files": names }))
}

pub(crate) const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        id: "help.report_problem",
        label: "Report a problem…",
        shortcut: None,
        params: &[],
        enabled: always,
        run: report_problem,
    },
    CommandSpec {
        id: "help.open_log_folder",
        label: "Open log folder",
        shortcut: None,
        params: &[],
        enabled: has_log_dir,
        run: open_log_folder,
    },
    CommandSpec {
        id: "help.save_diagnostics_zip",
        label: "Save diagnostics as zip…",
        shortcut: None,
        params: &[
            ParamSpec {
                name: "path",
                kind: ParamKind::String,
                required: false,
                help: "where to save the zip; without it the app asks",
            },
            ParamSpec {
                name: "report",
                kind: ParamKind::String,
                required: false,
                help: "file name of a Crash report in the Log folder to include",
            },
            ParamSpec {
                name: "listed",
                kind: ParamKind::Integer,
                required: false,
                help: "number of the list a window showed: save exactly those files",
            },
        ],
        enabled: has_log_dir,
        run: save_diagnostics_zip,
    },
    CommandSpec {
        id: "app.log_level",
        label: "Log level",
        shortcut: None,
        params: &[ParamSpec {
            name: "value",
            kind: ParamKind::String,
            required: true,
            help: "error, warn, info (the default) or debug",
        }],
        enabled: always,
        run: set_level,
    },
];

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    fn folder(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("obs-diagnostics-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn facts(message: &str, when: u64) -> PanicFacts {
        PanicFacts {
            message: message.into(),
            location: Some("crates/engine/src/x.rs:1:2".into()),
            thread: "main".into(),
            backtrace: "   0: obs_engine::x\n             at /home/reader/src/x.rs:1".into(),
            when,
        }
    }

    #[test]
    fn the_log_rotates_by_size_keeping_the_newest_files() {
        let dir = folder("rotate");
        let mut log = RotatingLog::with_limits(&dir, 100, 2).unwrap();
        for i in 0..20 {
            log.write_line(&format!("line {i:02} {}", "x".repeat(20)));
        }
        let current = std::fs::read_to_string(dir.join(LOG_FILE)).unwrap();
        assert!(current.ends_with(&format!("line 19 {}\n", "x".repeat(20))));
        assert!(current.len() <= 100, "{}", current.len());
        assert!(dir.join(RotatingLog::rotated_name(1)).is_file());
        assert!(dir.join(RotatingLog::rotated_name(2)).is_file());
        assert!(!dir.join(RotatingLog::rotated_name(3)).exists());
        let newest_old = std::fs::read_to_string(dir.join(RotatingLog::rotated_name(1))).unwrap();
        assert!(newest_old.contains("line 1"), "{newest_old}");
        assert_eq!(log.recent(2).len(), 2);
        assert!(log.recent(2)[1].starts_with("line 19"));
        // A reopened log appends.
        drop(log);
        let mut log = RotatingLog::with_limits(&dir, 1000, 2).unwrap();
        log.write_line("again");
        let current = std::fs::read_to_string(dir.join(LOG_FILE)).unwrap();
        assert!(current.starts_with("line") && current.ends_with("again\n"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_log_level_filters_our_records_and_quiets_other_crates() {
        use log::Level;
        assert!(LogLevel::Info.writes(Level::Info, "obs_engine::jobs"));
        assert!(!LogLevel::Info.writes(Level::Debug, "obs_engine::session"));
        assert!(LogLevel::Debug.writes(Level::Debug, "obs_engine::session"));
        assert!(!LogLevel::Info.writes(Level::Info, "winit::platform"));
        assert!(LogLevel::Info.writes(Level::Warn, "winit::platform"));
        assert!(LogLevel::Debug.writes(Level::Debug, "eframe"));
        assert!(!LogLevel::Error.writes(Level::Warn, "openbible"));
        for (l, id, _) in LogLevel::ALL {
            assert_eq!(LogLevel::from_id(id), Some(l));
            assert_eq!(LogLevel::from_u8(l.to_u8()), l);
        }
    }

    #[test]
    fn another_librarys_message_is_withheld_so_typed_text_never_reaches_a_log_or_report() {
        // egui-winit 0.37 logs a whole IME event (with the text being typed) when it
        // ignores the event's range: src/lib.rs:809, at Warn, so even at Normal.
        let ime = "Preedit(\"PRIVATE_NOTE\", Some((0, 99)))";
        let file = "/home/reader/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/egui-winit-0.37.0/src/lib.rs";
        let line = log_line(
            &log::Record::builder()
                .level(log::Level::Warn)
                .target("egui_winit")
                .file(Some(file))
                .line(Some(809))
                .args(format_args!("ignoring {ime}'s range because it is invalid"))
                .build(),
            1_700_000_000,
        );
        assert_eq!(
            line,
            "2023-11-14 22:13:20 UTC WARN  egui_winit: (message withheld; logged at \
             egui-winit-0.37.0/src/lib.rs:809)"
        );
        assert!(LogLevel::Info.writes(log::Level::Warn, "egui_winit"));
        let dir = folder("withheld");
        let mut log = RotatingLog::open(&dir).unwrap();
        log.write_line(&line);
        let on_disk = std::fs::read_to_string(dir.join(LOG_FILE)).unwrap();
        let report = problem_report(&log.recent(REPORT_LOG_LINES), None);
        let crash = crash_report_text(&facts("boom", 0), &log.recent(REPORT_LOG_LINES), None);
        for text in [&on_disk, &report, &crash, &issue_url(&report)] {
            assert!(!text.contains("PRIVATE_NOTE"), "{text}");
        }
        assert!(report.contains("egui_winit: (message withheld"), "{report}");
        std::fs::remove_dir_all(&dir).unwrap();

        // Our own messages, and the vetted graphics libraries', are written as they are.
        let ours = |target: &str| {
            log_line(
                &log::Record::builder()
                    .level(log::Level::Error)
                    .target(target)
                    .args(format_args!("job Import failed"))
                    .build(),
                0,
            )
        };
        assert!(ours("obs_engine::jobs").ends_with("obs_engine::jobs: job Import failed"));
        assert!(ours("wgpu_core::device").ends_with(": job Import failed"));
        assert!(ours("egui_wgpu::renderer").ends_with(": job Import failed"));
        assert!(ours("wgpuish").ends_with("(message withheld; logged at ?)"));
        assert_eq!(call_site(Some(r"C:\src\other\lib.rs"), Some(3)), "lib.rs:3");
    }

    #[test]
    fn a_panic_message_keeps_only_the_standard_librarys_phrase() {
        // Text the code put in a panic message is withheld, quoted or not.
        for private in [
            "invalid note: PRIVATE_NOTE",
            "PRIVATE_NOTE",
            "of `PRIVATE_NOTE`",
            "Error { msg: \"PRIVATE_NOTE\" }",
            "",
        ] {
            assert_eq!(panic_summary(private), MESSAGE_WITHHELD, "{private}");
        }
        // The standard library's own phrase is kept; what follows it is not.
        assert_eq!(
            panic_summary("called `Option::unwrap()` on a `None` value"),
            "called `Option::unwrap()` on a `None` value"
        );
        assert_eq!(
            panic_summary("called `Result::unwrap()` on an `Err` value: Error(PRIVATE_NOTE)"),
            "called `Result::unwrap()` on an `Err` value (details withheld)"
        );
        assert_eq!(
            panic_summary("assertion `left == right` failed\n  left: PRIVATE_NOTE\n right: 2"),
            "assertion `left == right` failed (details withheld)"
        );
        assert_eq!(
            panic_summary(
                "byte index 5 is not a char boundary; it is inside 'é' (bytes 4..6) of \
                 `my PRIVATE_NOTE text`"
            ),
            "byte index (details withheld)"
        );
        assert_eq!(
            panic_summary("index out of bounds: the len is 3 but the index is 7"),
            "index out of bounds (details withheld)"
        );
        assert_eq!(panic_summary("explicit panic"), "explicit panic");
        // A phrase later in the message does not count.
        assert_eq!(
            panic_summary("PRIVATE_NOTE: called `Option::unwrap()` on a `None` value"),
            MESSAGE_WITHHELD
        );

        // Neither the Crash report nor its GitHub issue address carries the withheld text.
        let report = crash_report_text(
            &facts(&panic_summary("invalid note: PRIVATE_NOTE"), 0),
            &[],
            None,
        );
        assert!(report.contains("Panic: (message withheld)\n"), "{report}");
        for text in [&report, &issue_url(&report)] {
            assert!(!text.contains("PRIVATE_NOTE"), "{text}");
        }
    }

    #[test]
    fn threads_panicking_in_the_same_second_each_get_a_report() {
        let dir = folder("concurrent");
        std::fs::create_dir_all(&dir).unwrap();
        let threads = 8;
        let start = std::sync::Arc::new(std::sync::Barrier::new(threads));
        let written: Vec<PathBuf> = (0..threads)
            .map(|i| {
                let (dir, start) = (dir.clone(), std::sync::Arc::clone(&start));
                std::thread::spawn(move || {
                    start.wait();
                    write_crash_report(&dir, 1_700_000_000, &format!("report {i}")).unwrap()
                })
            })
            .collect::<Vec<_>>()
            .into_iter()
            .map(|t| t.join().unwrap())
            .collect();
        let mut names = written.clone();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), threads, "{written:?}");
        let mut texts: Vec<String> = crash_reports(&dir).into_iter().map(|r| r.text).collect();
        texts.sort();
        let want: Vec<String> = (0..threads).map(|i| format!("report {i}")).collect();
        assert_eq!(texts, want);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_crash_report_has_the_panic_version_os_and_log_lines_without_the_home_folder() {
        let text = crash_report_text(
            &facts("index out of bounds", 1_700_000_000),
            &["2023-11-14 22:13:20 INFO  openbible: started".to_string()],
            Some("/home/reader"),
        );
        assert!(text.starts_with("Open Bible Software crash report\n"));
        assert!(text.contains(&format!("Version: {}\n", env!("CARGO_PKG_VERSION"))));
        assert!(text.contains(&format!("OS: {}\n", os_description())));
        assert!(text.contains("Time: 2023-11-14 22:13:20 UTC\n"));
        assert!(text.contains("Panic: index out of bounds\nAt: crates/engine/src/x.rs:1:2\n"));
        assert!(text.contains("Backtrace:\n   0: obs_engine::x\n             at ~/src/x.rs:1\n"));
        assert!(text.contains("Recent log lines:\n2023-11-14 22:13:20 INFO  openbible: started"));
        assert!(!text.contains("/home/reader"));
    }

    #[test]
    fn a_crash_reports_backtrace_starts_where_the_panic_was_raised() {
        let bt = "   0: std::backtrace_rs::backtrace::libunwind::trace
             at /rustc/x/library/std/src/../../backtrace/src/backtrace/libunwind.rs:117:9
   0: <std::backtrace::Backtrace>::create
   0: std::backtrace::Backtrace::force_capture
   0: obs_engine::diagnostics::install_panic_hook::{closure#0}
             at ./crates/engine/src/diagnostics.rs:402:24
   1: <alloc::boxed::Box<dyn for<'a, 'b> core::ops::function::Fn<(&'a std::panic::PanicHookInfo<'b>,), Output = ()>> as core::ops::function::Fn<(&std::panic::PanicHookInfo,)>>::call
             at /rustc/x/library/alloc/src/boxed.rs:2334:9
   2: std::panicking::panic_with_hook
             at /rustc/x/library/std/src/panicking.rs:823:13
   3: __rustc::rust_begin_unwind
   4: core::panicking::panic_fmt
             at /rustc/x/library/core/src/panicking.rs:80:14
   5: openbible::start_diagnostics
             at ./apps/openbible/src/main.rs:104:13
   6: openbible::main
   7: std::rt::lang_start_internal";
        let trimmed = without_panic_frames(bt);
        assert!(
            trimmed.starts_with("   5: openbible::start_diagnostics\n             at ./apps/"),
            "{trimmed}"
        );
        assert!(trimmed.ends_with("   7: std::rt::lang_start_internal"));
        assert_eq!(
            without_panic_frames("disabled backtrace"),
            "disabled backtrace"
        );
    }

    #[test]
    fn reports_are_listed_newest_first_and_a_seen_one_is_no_longer_new() {
        let dir = folder("reports");
        let older = write_crash_report(&dir, 1_700_000_000, "older").unwrap();
        let newer = write_crash_report(&dir, 1_700_000_100, "newer").unwrap();
        let same_second = write_crash_report(&dir, 1_700_000_100, "newest").unwrap();
        assert_ne!(newer, same_second);
        std::fs::write(dir.join(LOG_FILE), "not a report").unwrap();
        let reports = crash_reports(&dir);
        let texts: Vec<&str> = reports.iter().map(|r| r.text.as_str()).collect();
        assert_eq!(texts, ["newest", "newer", "older"]);
        assert!(reports.iter().all(|r| r.is_new));
        let seen = mark_seen(&reports[0]).unwrap();
        assert!(seen.to_str().unwrap().ends_with(".seen.txt"));
        let reports = crash_reports(&dir);
        assert_eq!(reports[0].text, "newest");
        assert!(!reports[0].is_new && reports[1].is_new);
        assert!(older.is_file());
        // Only the newest are kept.
        for i in 0..CRASH_REPORTS_KEPT as u64 {
            write_crash_report(&dir, 1_800_000_000 + i, "later").unwrap();
        }
        assert_eq!(crash_reports(&dir).len(), CRASH_REPORTS_KEPT);
        assert!(!older.exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn more_reports_in_one_second_than_are_kept_keep_the_newest() {
        let dir = folder("same-second");
        let total = CRASH_REPORTS_KEPT + 5;
        for i in 0..total {
            let path = write_crash_report(&dir, 1_700_000_000, &format!("report {i}")).unwrap();
            assert_eq!(
                std::fs::read_to_string(&path).unwrap(),
                format!("report {i}")
            );
            let reports = crash_reports(&dir);
            assert_eq!(reports.len(), (i + 1).min(CRASH_REPORTS_KEPT));
            assert_eq!(reports[0].path, path);
            assert_eq!(reports[0].text, format!("report {i}"));
        }
        let texts: Vec<String> = crash_reports(&dir).into_iter().map(|r| r.text).collect();
        let want: Vec<String> = (total - CRASH_REPORTS_KEPT..total)
            .rev()
            .map(|i| format!("report {i}"))
            .collect();
        assert_eq!(texts, want);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_issue_url_carries_the_report_and_fits_a_browser() {
        let text = crash_report_text(&facts("boom & bust", 0), &[], None);
        let url = issue_url(&text);
        assert!(url.starts_with(&format!(
            "{NEW_ISSUE_URL}?title=Crash%3A%20boom%20%26%20bust&body="
        )));
        assert!(url.contains("Panic%3A%20boom%20%26%20bust"));
        let long: Vec<String> = (0..100)
            .map(|i| format!("log line {i} {}", "x".repeat(100)))
            .collect();
        let text = crash_report_text(&facts("boom", 0), &long, None);
        let url = issue_url(&text);
        assert!(url.len() <= ISSUE_URL_MAX, "{}", url.len());
        assert!(url.contains("Panic%3A%20boom"));
        assert!(url.contains("shortened"));
        assert!(issue_url(&problem_report(&[], None)).contains("title=Problem%20report"));
    }

    #[test]
    fn the_log_level_is_a_saved_preference_and_a_setting() {
        let dir = folder("level");
        std::fs::create_dir_all(&dir).unwrap();
        let mut s = Session::new(crate::JobMode::Inline);
        let (store, _) = obs_store::UserStore::open(&dir.join("user.db")).unwrap();
        s.attach_user(Box::new(store));
        assert_eq!(s.log_level(), LogLevel::Info);
        assert_eq!(s.setting_is_default("log_level"), Some(true));
        s.run("app.log_level", &json!({ "value": "debug" }))
            .unwrap();
        assert_eq!(s.log_level(), LogLevel::Debug);
        assert_eq!(crate::choices(&s, "app.log_level").1, "debug");
        assert_eq!(s.setting_is_default("log_level"), Some(false));
        assert!(s.run("app.log_level", &json!({ "value": "loud" })).is_err());
        let user = s.user.take().unwrap();
        let mut again = Session::new(crate::JobMode::Inline);
        again.attach_user(user);
        assert_eq!(again.log_level(), LogLevel::Debug);
        again
            .run("settings.reset", &json!({ "setting": "log_level" }))
            .unwrap();
        assert_eq!(again.log_level(), LogLevel::Info);
        drop(again);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// The files of the zip at `path`, `(name, text)` in order.
    fn unzip(path: &Path) -> Vec<(String, String)> {
        use std::io::Read;
        let mut zip = zip::ZipArchive::new(File::open(path).unwrap()).unwrap();
        (0..zip.len())
            .map(|i| {
                let mut f = zip.by_index(i).unwrap();
                let mut text = String::new();
                f.read_to_string(&mut text).unwrap();
                (f.name().to_string(), text)
            })
            .collect()
    }

    #[test]
    fn a_diagnostics_zip_holds_the_readme_the_report_and_the_log_files_only() {
        let dir = folder("zip-entries");
        let logs = dir.join("logs");
        std::fs::create_dir_all(&logs).unwrap();
        std::fs::write(
            logs.join(LOG_FILE),
            "2023-11-14 22:13:20 UTC INFO  openbible: logging to /home/reader/logs\n",
        )
        .unwrap();
        std::fs::write(logs.join(RotatingLog::rotated_name(1)), "older\n").unwrap();
        // Not a log file of the app's, nor kept by it: not in the zip.
        for other in ["user.db", "notes.txt", "openbible.9.log", "crash-notes.txt"] {
            std::fs::write(logs.join(other), "PRIVATE_NOTE").unwrap();
        }
        let path = write_crash_report(&logs, 1_700_000_000, "Panic: boom\n").unwrap();
        let report = crash_reports(&logs)
            .into_iter()
            .find(|r| r.path == path)
            .unwrap();
        let report = CrashReport {
            path: mark_seen(&report).unwrap(),
            ..report
        };

        let files = diagnostics_zip_entries(&logs, Some(&report), Some("/home/reader"), 0);
        let names: Vec<&str> = files.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(
            names,
            [
                ZIP_README,
                "crash-20231114-221320.txt",
                "openbible.log",
                "openbible.1.log"
            ]
        );
        assert_eq!(files[1].text, "Panic: boom\n");
        assert!(
            files[2].text.contains("logging to ~/logs"),
            "{}",
            files[2].text
        );
        let readme = &files[0].text;
        assert!(readme.starts_with("Open Bible Software diagnostics\n"));
        assert!(readme.contains(&format!("Version: {}\n", env!("CARGO_PKG_VERSION"))));
        assert!(readme.contains(&format!("OS: {}\n", os_description())));
        for name in &names {
            assert!(readme.contains(&format!("- {name}: ")), "{name}: {readme}");
        }
        assert!(readme.contains("It does not hold your Notes, Highlights"));
        assert!(readme.contains("Nothing was sent"));
        for f in &files {
            assert!(!f.text.contains("PRIVATE_NOTE"), "{}", f.name);
            assert!(!f.text.contains("/home/reader"), "{}", f.name);
        }
        // Without a report, and with a report from elsewhere: the logs only.
        let elsewhere = CrashReport {
            path: dir.join("crash-20231114-221320.txt"),
            ..report.clone()
        };
        for r in [None, Some(&elsewhere)] {
            let names: Vec<String> = diagnostics_zip_entries(&logs, r, None, 0)
                .into_iter()
                .map(|f| f.name)
                .collect();
            assert_eq!(names, [ZIP_README, "openbible.log", "openbible.1.log"]);
        }
        assert_eq!(
            diagnostics_zip_name(1_790_000_000),
            "openbible-report-20260921-141320.zip"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_diagnostics_zip_is_written_whole_and_never_over_an_existing_file() {
        let dir = folder("zip-write");
        std::fs::create_dir_all(&dir).unwrap();
        let files = vec![
            ZipEntry {
                name: ZIP_README.into(),
                about: "readme",
                text: "hello\n".into(),
            },
            ZipEntry {
                name: LOG_FILE.into(),
                about: "log",
                text: "line\n".into(),
            },
        ];
        let dest = dir.join("openbible-report.zip");
        assert_eq!(write_diagnostics_zip(&dest, &files).unwrap(), dest);
        let want = vec![
            (ZIP_README.to_string(), "hello\n".to_string()),
            (LOG_FILE.to_string(), "line\n".to_string()),
        ];
        assert_eq!(unzip(&dest), want);
        // The same name again: the first is kept as it was, the new one goes beside it.
        let first = std::fs::read(&dest).unwrap();
        std::fs::write(dir.join("openbible-report (2).zip"), "someone else's").unwrap();
        let again = write_diagnostics_zip(&dest, &files).unwrap();
        assert_eq!(again, dir.join("openbible-report (3).zip"));
        assert_eq!(std::fs::read(&dest).unwrap(), first);
        assert_eq!(
            std::fs::read_to_string(dir.join("openbible-report (2).zip")).unwrap(),
            "someone else's"
        );
        assert_eq!(unzip(&again), want);
        // No temporary file is left behind.
        let mut names: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(
            names,
            [
                "openbible-report (2).zip",
                "openbible-report (3).zip",
                "openbible-report.zip"
            ]
        );
        // A folder that is not there: an error, and nothing written.
        assert!(write_diagnostics_zip(&dir.join("gone/x.zip"), &files).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    fn names_in(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    fn two_files() -> Vec<ZipEntry> {
        vec![
            ZipEntry {
                name: ZIP_README.into(),
                about: "readme",
                text: "hello\n".into(),
            },
            ZipEntry {
                name: LOG_FILE.into(),
                about: "log",
                text: "line\n".into(),
            },
        ]
    }

    #[test]
    fn a_disk_without_hard_links_saves_nothing_rather_than_show_a_name_early() {
        let dir = folder("zip-no-links");
        std::fs::create_dir_all(&dir).unwrap();
        let dest = dir.join("openbible-report.zip");
        // The name is never made, empty or otherwise, before or after the link fails.
        let no_links = |_: &Path, to: &Path| {
            assert!(!to.exists(), "{} made early", to.display());
            Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "operation not supported",
            ))
        };
        let e = write_zip_linking(&dest, &two_files(), no_links).unwrap_err();
        assert!(e.to_string().contains("nothing was saved"), "{e}");
        assert!(e.to_string().contains("hard links"), "{e}");
        assert!(names_in(&dir).is_empty(), "{:?}", names_in(&dir));
        // A taken name goes on to the next even then, and the taken file is kept as it was.
        std::fs::write(&dest, "kept").unwrap();
        let tried = Mutex::new(Vec::new());
        let e = write_zip_linking(&dest, &two_files(), |_: &Path, to: &Path| {
            tried.lock().unwrap().push(to.to_path_buf());
            if to.exists() {
                Err(std::io::ErrorKind::AlreadyExists.into())
            } else {
                Err(std::io::ErrorKind::Unsupported.into())
            }
        })
        .unwrap_err();
        assert!(e.to_string().contains("nothing was saved"), "{e}");
        assert_eq!(
            *tried.lock().unwrap(),
            [dest.clone(), dir.join("openbible-report (2).zip")]
        );
        assert_eq!(std::fs::read_to_string(&dest).unwrap(), "kept");
        assert_eq!(names_in(&dir), ["openbible-report.zip"]);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_zip_never_removes_a_temporary_file_it_did_not_make() {
        let dir = folder("zip-temp");
        std::fs::create_dir_all(&dir).unwrap();
        let dest = dir.join("openbible-report.zip");
        // Another save's temporary file under the first name tried: kept, and this save
        // takes the next.
        let theirs = format!(".openbible-report.zip.{}-0.partial", std::process::id());
        std::fs::write(dir.join(&theirs), "another save's").unwrap();
        assert_eq!(write_diagnostics_zip(&dest, &two_files()).unwrap(), dest);
        assert_eq!(
            std::fs::read_to_string(dir.join(&theirs)).unwrap(),
            "another save's"
        );
        assert_eq!(names_in(&dir), [theirs.as_str(), "openbible-report.zip"]);
        std::fs::remove_file(dir.join(&theirs)).unwrap();
        std::fs::remove_file(&dest).unwrap();

        // Saves at once to one name: each whole under a name of its own, no file lost.
        let saved: Vec<PathBuf> = std::thread::scope(|scope| {
            let handles: Vec<_> = (0..8)
                .map(|_| scope.spawn(|| write_diagnostics_zip(&dest, &two_files()).unwrap()))
                .collect();
            handles.into_iter().map(|h| h.join().unwrap()).collect()
        });
        let mut want: Vec<String> = (1..=8)
            .map(|n| {
                let p = numbered(&dest, n);
                p.file_name().unwrap().to_string_lossy().into_owned()
            })
            .collect();
        want.sort();
        assert_eq!(names_in(&dir), want, "no temporary file left, none missing");
        let mut got = saved.clone();
        got.sort();
        got.dedup();
        assert_eq!(got.len(), 8, "{saved:?}");
        for p in &saved {
            let files: Vec<(String, String)> = unzip(p);
            assert_eq!(files.len(), 2, "{}", p.display());
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn save_diagnostics_zip_asks_where_without_a_path_and_writes_with_one() {
        let dir = folder("zip-command");
        let logs = dir.join("logs");
        let mut s = Session::new(crate::JobMode::Inline);
        assert!(!s.is_enabled("help.save_diagnostics_zip"), "no Log folder");
        s.set_log_dir(logs.clone());
        std::fs::create_dir_all(&logs).unwrap();
        std::fs::write(logs.join(LOG_FILE), "a line\n").unwrap();
        let report = write_crash_report(&logs, 1_700_000_000, "Panic: boom\n").unwrap();
        let report_name = report.file_name().unwrap().to_str().unwrap();

        s.run("help.save_diagnostics_zip", &json!({})).unwrap();
        assert!(
            s.workspace
                .take_effects()
                .contains(&Effect::SaveDiagnosticsZip)
        );
        let help = crate::MENUS.iter().find(|m| m.title == "Help").unwrap();
        assert!(
            help.items
                .contains(&crate::MenuItem::Command("help.save_diagnostics_zip"))
        );

        let dest = dir.join("report.zip");
        let out = s
            .run(
                "help.save_diagnostics_zip",
                &json!({ "path": dest, "report": report_name }),
            )
            .unwrap();
        assert_eq!(out["path"], dest.display().to_string());
        assert_eq!(
            out["files"],
            json!([ZIP_README, "crash-20231114-221320.txt", LOG_FILE])
        );
        let names: Vec<String> = unzip(&dest).into_iter().map(|f| f.0).collect();
        assert_eq!(names, [ZIP_README, "crash-20231114-221320.txt", LOG_FILE]);
        // Only a Crash report in the Log folder can be named.
        for bad in ["../user.db", LOG_FILE, "crash-20990101-000000.txt"] {
            assert!(
                s.run(
                    "help.save_diagnostics_zip",
                    &json!({ "path": dir.join("bad.zip"), "report": bad }),
                )
                .is_err(),
                "{bad}"
            );
        }
        assert!(!dir.join("bad.zip").exists());

        // A listed zip saves exactly what was listed, though the log grew and rotated since.
        let report = s.crash_reports().into_iter().next().unwrap();
        let (listed, files) = s.list_diagnostics_zip(Some(&report));
        std::fs::write(logs.join(LOG_FILE), "a line\nthen another\n").unwrap();
        std::fs::write(logs.join("openbible.1.log"), "rotated\n").unwrap();
        let dest = dir.join("listed.zip");
        let out = s
            .run(
                "help.save_diagnostics_zip",
                &json!({ "path": dest, "listed": listed }),
            )
            .unwrap();
        assert_eq!(out["path"], dest.display().to_string());
        let want: Vec<(String, String)> = files
            .iter()
            .map(|f| (f.name.clone(), f.text.clone()))
            .collect();
        assert_eq!(unzip(&dest), want);
        assert_eq!(want[2], (LOG_FILE.to_string(), "a line\n".to_string()));
        // Only the last list is kept, and it holds its own Crash report.
        let (newer, _) = s.list_diagnostics_zip(None);
        for p in [
            json!({ "path": dir.join("old.zip"), "listed": listed }),
            json!({ "path": dir.join("old.zip"), "listed": newer, "report": report_name }),
        ] {
            assert!(s.run("help.save_diagnostics_zip", &p).is_err(), "{p}");
        }
        assert!(!dir.join("old.zip").exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn open_log_folder_and_report_a_problem_are_help_commands() {
        let mut s = Session::new(crate::JobMode::Inline);
        assert!(!s.is_enabled("help.open_log_folder"), "no Log folder yet");
        let dir = folder("open");
        s.set_log_dir(dir.clone());
        s.run("help.open_log_folder", &json!({})).unwrap();
        assert!(dir.is_dir(), "made so it can be shown");
        s.run("help.report_problem", &json!({})).unwrap();
        let effects = s.workspace.take_effects();
        assert!(
            effects.contains(&Effect::OpenFolder(dir.clone())),
            "{effects:?}"
        );
        assert!(effects.contains(&Effect::ReportProblem), "{effects:?}");
        let help = crate::MENUS.iter().find(|m| m.title == "Help").unwrap();
        for id in ["help.report_problem", "help.open_log_folder"] {
            assert!(help.items.contains(&crate::MenuItem::Command(id)), "{id}");
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
