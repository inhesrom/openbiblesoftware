//! Log files, Crash reports and Report a problem (#219).
//!
//! - The app writes a **Log** to the **Log folder** ([`default_log_dir`]): `openbible.log`,
//!   rotated at [`LOG_FILE_BYTES`] into `openbible.1.log` … (the last [`LOG_FILES_KEPT`]
//!   kept), at the **Log level** the user chooses in Settings → Privacy and data
//!   (`app.log_level`). [`start_logging`] installs it as the process's `log` logger; log lines
//!   name commands, jobs and problems, never Notes, Highlights or Resource text: another
//!   library's message is withheld unless it is vetted ([`log_line`]).
//! - A panic writes a **Crash report** there ([`install_panic_hook`]): what panicked and
//!   where (quoted text in the message withheld, [`withhold_quoted`]), a backtrace, the app
//!   version, the OS and the recent log lines, with the home folder written as `~`. It is a
//!   plain text file; nothing is sent automatically.
//! - At the next launch the app offers **Report a problem** for a new Crash report
//!   ([`CrashReport::is_new`]): the user reads it, then may copy it or open GitHub's new-issue
//!   form with it in the web address ([`issue_url`]; opening it sends the report to GitHub,
//!   submitting the issue publishes it). Help → Report a problem… shows the newest report, or
//!   a problem report made now ([`problem_report`]).
//! - Help → Open log folder (`help.open_log_folder`) shows the folder in the file manager.

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
/// A panic message longer than this is cut.
const PANIC_MESSAGE_CHARS: usize = 300;

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
/// checking first.
pub fn write_crash_report(dir: &Path, when: u64, text: &str) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let stamp = file_stamp(when);
    let mut n = 1;
    let (path, mut file) = loop {
        let path = if n == 1 {
            dir.join(format!("{CRASH_PREFIX}{stamp}{NEW_SUFFIX}"))
        } else {
            dir.join(format!("{CRASH_PREFIX}{stamp}-{n}{NEW_SUFFIX}"))
        };
        n += 1;
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
        let _ = std::fs::remove_file(&old.path);
    }
    Ok(path)
}

/// Write a Crash report for every panic into the Log folder `dir`, then do what the
/// previous hook did (print it). Installed by the desktop app at launch.
pub fn install_panic_hook(dir: PathBuf) {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let facts = PanicFacts {
            message: withhold_quoted(&panic_message(info.payload())),
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

/// A panic message without the text it quotes, which may be the user's: the standard
/// library quotes a whole string it could not slice (`` of `…` ``), and an error's details
/// are often in double quotes. Only the standard library's own opening phrases keep their
/// backticks ([`STD_PANIC_PHRASES`], e.g. ``called `Option::unwrap()` on a `None` value``);
/// any other backtick span runs to the message's last backtick, since the quoted string may
/// hold backticks itself, and is written `…`. A span in double quotes (up to the first
/// unescaped `"`), or a character's debug form in single quotes, is written `…` too. The
/// message is cut at [`PANIC_MESSAGE_CHARS`].
pub fn withhold_quoted(message: &str) -> String {
    let (mut out, rest) = match STD_PANIC_PHRASES
        .iter()
        .find(|phrase| message.starts_with(*phrase))
    {
        Some(phrase) => (phrase.to_string(), &message[phrase.len()..]),
        None => (String::new(), message),
    };
    let chars: Vec<char> = rest.chars().collect();
    let last_backtick = chars.iter().rposition(|&c| c == '`');
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            '`' => {
                out.push_str("`…");
                match last_backtick.filter(|&end| end > i) {
                    Some(end) => {
                        out.push('`');
                        i = end + 1;
                    }
                    None => i = chars.len(),
                }
            }
            '"' => {
                out.push_str("\"…");
                match closing_quote(&chars, i + 1, '"') {
                    Some(end) => {
                        out.push('"');
                        i = end + 1;
                    }
                    None => i = chars.len(),
                }
            }
            // A char's debug form: 'é', '\n', '\'', '\u{301}' (not an apostrophe in words).
            '\'' => match closing_quote(&chars, i + 1, '\'') {
                Some(end) if end == i + 2 || (chars.get(i + 1) == Some(&'\\') && end - i <= 11) => {
                    out.push_str("'…'");
                    i = end + 1;
                }
                _ => {
                    out.push(c);
                    i += 1;
                }
            },
            _ => {
                out.push(c);
                i += 1;
            }
        }
    }
    if out.chars().count() > PANIC_MESSAGE_CHARS {
        out = out.chars().take(PANIC_MESSAGE_CHARS).collect();
        out.push('…');
    }
    out
}

/// The opening phrases of the standard library's panic messages whose backticks name code,
/// kept as they are at the start of a panic message by [`withhold_quoted`].
const STD_PANIC_PHRASES: &[&str] = &[
    "called `Result::unwrap()` on an `Err` value",
    "called `Result::unwrap_err()` on an `Ok` value",
    "called `Option::unwrap()` on a `None` value",
    "assertion `left == right` failed",
    "assertion `left != right` failed",
    "assertion `left matches right` failed",
];

/// The index of the `quote` closing a span that starts at `from`, skipping a debug string's
/// escapes (`\"`, `\\`).
fn closing_quote(chars: &[char], from: usize, quote: char) -> Option<usize> {
    let mut i = from;
    while i < chars.len() {
        match chars[i] {
            '\\' => i += 2,
            c if c == quote => return Some(i),
            _ => i += 1,
        }
    }
    None
}

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
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let stem = name
        .strip_suffix(SEEN_SUFFIX)
        .or_else(|| name.strip_suffix(NEW_SUFFIX))
        .unwrap_or(name);
    let stamp: String = stem.chars().skip(CRASH_PREFIX.len()).take(15).collect();
    let n = stem
        .rsplit_once('-')
        .filter(|(_, n)| n.len() < 4)
        .and_then(|(_, n)| n.parse().ok())
        .unwrap_or(1);
    (stamp, n)
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
    fn quoted_text_in_a_panic_message_is_withheld() {
        // The standard library quotes the whole string it could not slice.
        assert_eq!(
            withhold_quoted(
                "byte index 5 is not a char boundary; it is inside 'é' (bytes 4..6) of \
                 `my PRIVATE_NOTE text`"
            ),
            "byte index 5 is not a char boundary; it is inside '…' (bytes 4..6) of `…`"
        );
        assert_eq!(
            withhold_quoted(
                "called `Result::unwrap()` on an `Err` value: Error { msg: \"PRIVATE_NOTE\" }"
            ),
            "called `Result::unwrap()` on an `Err` value: Error { msg: \"…\" }"
        );
        assert_eq!(
            withhold_quoted("char '\\u{301}' and \"open"),
            "char '…' and \"…"
        );
        assert_eq!(
            withhold_quoted("index out of bounds: the len is 3 but the index is 7"),
            "index out of bounds: the len is 3 but the index is 7"
        );
        assert_eq!(withhold_quoted("can't open it's"), "can't open it's");
        // Quoted text that looks like code is still withheld; only the standard library's
        // own phrases keep their backticks, and only at the start of the message.
        assert_eq!(withhold_quoted("of `PRIVATE::NOTE`"), "of `…`");
        assert_eq!(withhold_quoted("of `Err`"), "of `…`");
        assert_eq!(
            withhold_quoted("of `Err` PRIVATE_NOTE `Option::unwrap()` [...]"),
            "of `…` [...]"
        );
        assert_eq!(
            withhold_quoted("called `Option::unwrap()` on a `None` value"),
            "called `Option::unwrap()` on a `None` value"
        );
        assert_eq!(
            withhold_quoted("assertion `left == right` failed\n  left: \"PRIVATE\"\n right: 2"),
            "assertion `left == right` failed\n  left: \"…\"\n right: 2"
        );
        // Escaped quotes in a debug string do not end it.
        assert_eq!(
            withhold_quoted(r#"Error { msg: "a \"PRIVATE_NOTE\" \\" }"#),
            r#"Error { msg: "…" }"#
        );
        assert_eq!(
            withhold_quoted(r"char '\'' PRIVATE '\\'"),
            "char '…' PRIVATE '…'"
        );
        let long = withhold_quoted(&"x".repeat(1000));
        assert_eq!(long.chars().count(), PANIC_MESSAGE_CHARS + 1);
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
