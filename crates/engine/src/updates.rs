//! The Update check and Update now (#276, ADR 0017).
//!
//! The one exception to the app's offline rule: it may ask this repository's GitHub releases
//! for the latest published release, and download it when the user says so. Nothing else is
//! ever sent or fetched for it.
//!
//! - **Update check**: a plain GET of public release metadata through the app's [`Fetch`]
//!   (`{releases}/latest`, or `{releases}?per_page=20` with pre-releases), on the job queue.
//!   At launch it runs once, a few seconds after the first window ([`LAUNCH_CHECK_DELAY_SECS`],
//!   started by the UI) when "Check for updates automatically" is on; Help → Check for
//!   updates… runs it any time. [`newest_release`] compares tags with [`APP_VERSION`] as
//!   semver ([`Version`]), never offering a draft, an older or the same version, or (unless
//!   the user opted in) a pre-release.
//! - **Update now** ([`Session::start_app_update`]): the asset for this platform and
//!   [`CHECKSUMS_ASSET`] (and [`SIGNATURE_ASSET`] once a release key is compiled in,
//!   [`RELEASE_PUBLIC_KEY`]) are downloaded on the job queue, verified ([`verify_archive`])
//!   and unpacked beside the installed copy ([`stage_update`]). Only then, on the UI thread, a
//!   Before-update backup of the User database is taken and the new files are renamed into
//!   place ([`swap_in`]); the old ones wait in [`UPDATE_DIR`] until the next launch removes
//!   them ([`clean_up_update_folder`]). Restart now relaunches the new executable. Any
//!   failure leaves the installed copy as it was and says why in plain words
//!   ([`UpdateError::message`]), with the download page as the way on.
//! - Only a Release archive install ([`INSTALL_MARKER`] beside the executable) can replace
//!   itself; the installers of #272 plug in as further [`InstallKind`]s. Any other copy (a
//!   build from source, a distribution's package, a folder the user cannot write) is only
//!   told about the new version and pointed to the download page.
//!
//! The engine never opens a connection: the app hands it its transport
//! ([`Session::set_fetch`]), and tests hand it a mock release source.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::command::{CommandError, CommandSpec, ParamKind, ParamSpec, Params, always};
use crate::jobs::{JobEvent, JobEventKind, JobId};
use crate::session::Session;
use crate::workspace::Effect;
use obs_formats::sword_repo::FetchError;

/// This build's version: the workspace version, or `OBS_BUILD_VERSION` when the build set
/// one (the end-to-end update test builds an older and a newer app from one tree).
pub const APP_VERSION: &str = match option_env!("OBS_BUILD_VERSION") {
    Some(v) => v,
    None => env!("CARGO_PKG_VERSION"),
};

/// The GitHub releases API of this repository.
pub const RELEASES_API: &str = "https://api.github.com/repos/inhesrom/openbiblesoftware/releases";
/// Where this repository's release assets are downloaded from; no other URL is fetched.
pub const RELEASE_DOWNLOADS: &str =
    "https://github.com/inhesrom/openbiblesoftware/releases/download/";
/// The download page offered whenever the app cannot update itself.
pub const RELEASES_PAGE: &str = "https://github.com/inhesrom/openbiblesoftware/releases";
/// The release-signing public key (minisign, the base64 line of `minisign.pub`), compiled
/// into the app. `None` until the maintainer creates the key (docs/dev/releasing.md): until
/// then downloads are checked against the release's SHA256SUMS only, and the app says so.
pub const RELEASE_PUBLIC_KEY: Option<&str> = None;
/// The release asset listing every asset's SHA-256.
pub const CHECKSUMS_ASSET: &str = "SHA256SUMS";
/// The detached minisign signature of [`CHECKSUMS_ASSET`].
pub const SIGNATURE_ASSET: &str = "SHA256SUMS.minisig";
/// The file a Release archive puts beside the executable, naming how it was installed
/// (`{"kind": "archive"}`).
pub const INSTALL_MARKER: &str = "openbible-install.json";
/// The folder beside the executable where an update is unpacked and the replaced files wait
/// until the next launch removes them.
pub const UPDATE_DIR: &str = ".openbible-update";
/// Written into [`UPDATE_DIR`] when a failed update could not put every replaced file back:
/// `old/` then holds the only copy of them, and [`clean_up_update_folder`] keeps it.
pub const ROLLBACK_INCOMPLETE: &str = "rollback-incomplete.txt";
/// How long after the first window appears the launch Update check starts, in seconds.
pub const LAUNCH_CHECK_DELAY_SECS: f64 = 3.0;
/// How many releases the pre-release check reads (newest first).
const RELEASES_PER_PAGE: u32 = 20;
/// Preference key of the Update settings.
const UPDATES_PREF: &str = "updates";

// ------------------------------------------------------------------------------- Versions

/// A semantic version (`1.2.3`, `1.2.3-rc.1`), as a release tag (`v1.2.3`) or this build
/// names it. Build metadata (`+…`) is accepted and ignored.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
    /// Pre-release identifiers (`rc`, `1`); empty for a release.
    pub pre: Vec<String>,
}

impl Version {
    /// Parse `1.2.3`, `v1.2.3`, `1.2.3-beta.2` or `1.2.3+build`. `None` for anything else.
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        let text = text
            .strip_prefix('v')
            .or_else(|| text.strip_prefix('V'))
            .unwrap_or(text);
        let text = text.split_once('+').map_or(text, |(v, _)| v);
        let (core, pre) = match text.split_once('-') {
            Some((core, pre)) => (core, Some(pre)),
            None => (text, None),
        };
        let number = |s: &str| -> Option<u64> {
            let ok = !s.is_empty()
                && s.bytes().all(|b| b.is_ascii_digit())
                && (s == "0" || !s.starts_with('0'));
            if ok { s.parse().ok() } else { None }
        };
        let mut parts = core.split('.');
        let major = number(parts.next()?)?;
        let minor = number(parts.next()?)?;
        let patch = number(parts.next()?)?;
        if parts.next().is_some() {
            return None;
        }
        let pre = match pre {
            None => Vec::new(),
            Some(pre) => {
                let ids: Vec<String> = pre.split('.').map(str::to_string).collect();
                let valid = ids.iter().all(|id| {
                    !id.is_empty() && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
                });
                if !valid {
                    return None;
                }
                ids
            }
        };
        Some(Self {
            major,
            minor,
            patch,
            pre,
        })
    }

    /// This build's version.
    pub fn current() -> Self {
        Self::parse(APP_VERSION).unwrap_or(Self {
            major: 0,
            minor: 0,
            patch: 0,
            pre: Vec::new(),
        })
    }

    pub fn is_prerelease(&self) -> bool {
        !self.pre.is_empty()
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)?;
        if !self.pre.is_empty() {
            write!(f, "-{}", self.pre.join("."))?;
        }
        Ok(())
    }
}

impl Ord for Version {
    /// Semver precedence: numbers first; a pre-release sorts before its release; identifiers
    /// compare numerically when both are numbers, else as text, numbers before text; a shorter
    /// list of equal identifiers sorts first.
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        use std::cmp::Ordering;
        (self.major, self.minor, self.patch)
            .cmp(&(other.major, other.minor, other.patch))
            .then_with(|| match (self.pre.is_empty(), other.pre.is_empty()) {
                (true, true) => Ordering::Equal,
                (true, false) => Ordering::Greater,
                (false, true) => Ordering::Less,
                (false, false) => {
                    for (a, b) in self.pre.iter().zip(&other.pre) {
                        let order = match (a.parse::<u64>(), b.parse::<u64>()) {
                            (Ok(a), Ok(b)) => a.cmp(&b),
                            (Ok(_), Err(_)) => Ordering::Less,
                            (Err(_), Ok(_)) => Ordering::Greater,
                            (Err(_), Err(_)) => a.cmp(b),
                        };
                        if order != Ordering::Equal {
                            return order;
                        }
                    }
                    self.pre.len().cmp(&other.pre.len())
                }
            })
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

// ------------------------------------------------------------------------------- Releases

/// One file attached to a release.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseAsset {
    pub name: String,
    pub url: String,
    pub size: u64,
}

/// One block of a release's notes (Markdown), as the update notice draws it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotesBlock {
    Heading(String),
    Paragraph(String),
    /// A list item.
    Item(String),
    Code(String),
}

/// A GitHub release, as the releases API describes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    pub tag: String,
    pub version: Version,
    /// The release's title (its tag when it has none).
    pub name: String,
    /// The release notes, as written (Markdown).
    pub notes: String,
    /// The notes in blocks to draw ([`release_notes_blocks`]).
    pub notes_blocks: Vec<NotesBlock>,
    /// The release's own page.
    pub page: String,
    pub draft: bool,
    pub prerelease: bool,
    pub assets: Vec<ReleaseAsset>,
}

impl Release {
    pub fn asset(&self, name: &str) -> Option<&ReleaseAsset> {
        self.assets.iter().find(|a| a.name == name)
    }
}

/// Parse the releases API's answer: one release (`/releases/latest`) or a list
/// (`/releases`). Releases whose tag is not a version are left out.
pub fn parse_releases(json: &str) -> Result<Vec<Release>, String> {
    let value: Value =
        serde_json::from_str(json).map_err(|e| format!("the release list is not JSON ({e})"))?;
    let items = match value {
        Value::Array(items) => items,
        object @ Value::Object(_) => vec![object],
        _ => return Err("the release list is not a release or a list of them".into()),
    };
    Ok(items.iter().filter_map(parse_release).collect())
}

fn parse_release(v: &Value) -> Option<Release> {
    let text = |key: &str| v.get(key).and_then(Value::as_str).unwrap_or_default();
    let tag = text("tag_name").to_string();
    let version = Version::parse(&tag)?;
    let notes = text("body").to_string();
    let assets = (v.get("assets").and_then(Value::as_array))
        .map(|assets| {
            assets
                .iter()
                .filter_map(|a| {
                    Some(ReleaseAsset {
                        name: a.get("name")?.as_str()?.to_string(),
                        url: a.get("browser_download_url")?.as_str()?.to_string(),
                        size: a.get("size").and_then(Value::as_u64).unwrap_or(0),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    Some(Release {
        name: match text("name") {
            "" => tag.clone(),
            name => name.to_string(),
        },
        notes_blocks: release_notes_blocks(&notes),
        notes,
        page: text("html_url").to_string(),
        draft: v.get("draft").and_then(Value::as_bool).unwrap_or(false),
        prerelease: v
            .get("prerelease")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        assets,
        version,
        tag,
    })
}

/// The newest release newer than `current`: never a draft, and a pre-release (by GitHub's
/// flag or by its version) only with `prereleases`. Never an older or the same version.
pub fn newest_release<'a>(
    releases: &'a [Release],
    current: &Version,
    prereleases: bool,
) -> Option<&'a Release> {
    releases
        .iter()
        .filter(|r| !r.draft)
        .filter(|r| prereleases || !(r.prerelease || r.version.is_prerelease()))
        .filter(|r| r.version > *current)
        .max_by(|a, b| a.version.cmp(&b.version))
}

/// Release notes (Markdown) as headings, paragraphs, list items and code blocks of plain
/// text: emphasis is dropped, a link keeps its text, an image its description.
pub fn release_notes_blocks(markdown: &str) -> Vec<NotesBlock> {
    use pulldown_cmark::{Event, Parser, Tag, TagEnd};
    let mut out = Vec::new();
    let mut text = String::new();
    let flush = |text: &mut String, make: fn(String) -> NotesBlock, out: &mut Vec<_>| {
        let t = text.trim().to_string();
        text.clear();
        if !t.is_empty() {
            out.push(make(t));
        }
    };
    // Paragraphs inside a list item belong to the item.
    let mut items = 0usize;
    for event in Parser::new(markdown) {
        match event {
            Event::Start(Tag::Item) => {
                flush(&mut text, NotesBlock::Paragraph, &mut out);
                items += 1;
            }
            Event::End(TagEnd::Item) => {
                flush(&mut text, NotesBlock::Item, &mut out);
                items = items.saturating_sub(1);
            }
            Event::Start(Tag::List(_)) => flush(
                &mut text,
                if items > 0 {
                    NotesBlock::Item
                } else {
                    NotesBlock::Paragraph
                },
                &mut out,
            ),
            Event::End(TagEnd::Heading(_)) => flush(&mut text, NotesBlock::Heading, &mut out),
            Event::End(TagEnd::Paragraph) if items == 0 => {
                flush(&mut text, NotesBlock::Paragraph, &mut out);
            }
            Event::End(TagEnd::Paragraph) => text.push(' '),
            Event::End(TagEnd::CodeBlock) => flush(&mut text, NotesBlock::Code, &mut out),
            Event::End(TagEnd::TableCell) => text.push_str("  "),
            Event::End(TagEnd::TableRow | TagEnd::TableHead) => {
                flush(&mut text, NotesBlock::Paragraph, &mut out);
            }
            Event::Text(t) | Event::Code(t) => text.push_str(&t),
            Event::SoftBreak => text.push(' '),
            Event::HardBreak => text.push('\n'),
            Event::Html(_) | Event::InlineHtml(_) => {}
            _ => {}
        }
    }
    flush(&mut text, NotesBlock::Paragraph, &mut out);
    out
}

// ------------------------------------------------------------------------------- Sources

/// Where releases come from: this repository's GitHub releases, or a mock release source in
/// tests (`OBS_UPDATE_URL`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateSource {
    /// The releases API: `{releases}/latest` and `{releases}?per_page=N`.
    pub releases: String,
    /// Every asset downloaded must start with this.
    pub downloads: String,
    /// The download page.
    pub page: String,
    /// The release-signing public key; `None`: SHA256SUMS only.
    pub public_key: Option<String>,
}

impl Default for UpdateSource {
    fn default() -> Self {
        Self {
            releases: RELEASES_API.into(),
            downloads: RELEASE_DOWNLOADS.into(),
            page: RELEASES_PAGE.into(),
            public_key: RELEASE_PUBLIC_KEY.map(str::to_string),
        }
    }
}

impl UpdateSource {
    /// A mock release source at `base`: `{base}/releases/latest`, `{base}/releases?per_page=N`
    /// and assets under `{base}/download/`. The compiled-in public key still applies.
    pub fn at(base: &str) -> Self {
        let base = base.trim_end_matches('/');
        Self {
            releases: format!("{base}/releases"),
            downloads: format!("{base}/download/"),
            page: format!("{base}/releases"),
            ..Self::default()
        }
    }

    /// This repository's releases, or the mock source `OBS_UPDATE_URL` names (tests and the
    /// end-to-end update check). A debug build also takes `OBS_UPDATE_PUBLIC_KEY`, so a test
    /// can sign its mock release with a key of its own; a release build never does.
    pub fn from_env() -> Self {
        let mut source = match std::env::var("OBS_UPDATE_URL") {
            Ok(base) if !base.trim().is_empty() => Self::at(&base),
            _ => Self::default(),
        };
        #[cfg(debug_assertions)]
        if let Ok(key) = std::env::var("OBS_UPDATE_PUBLIC_KEY") {
            source.public_key = Some(key).filter(|k| !k.trim().is_empty());
        }
        #[cfg(not(debug_assertions))]
        let _ = &mut source;
        source
    }

    /// The one URL an Update check fetches.
    pub fn check_url(&self, prereleases: bool) -> String {
        if prereleases {
            format!("{}?per_page={RELEASES_PER_PAGE}", self.releases)
        } else {
            format!("{}/latest", self.releases)
        }
    }

    /// Whether `url` is an asset of this source's releases (and so may be downloaded).
    pub fn serves(&self, url: &str) -> bool {
        url.starts_with(&self.downloads) && !url[self.downloads.len()..].contains("..")
    }
}

// ------------------------------------------------------------------------------- Errors

/// Why an Update check or Update now did not work. Every case leaves the app as it was.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateError {
    /// No connection, or the server could not be reached.
    Offline,
    /// GitHub is limiting how often this computer may ask.
    RateLimited,
    /// The repository has no published release yet.
    NoRelease,
    /// The release has no download for this platform.
    NoDownload,
    /// The release has no SHA256SUMS (or no line for the download).
    NoChecksums,
    ChecksumMismatch,
    /// The SHA256SUMS signature is missing or does not verify with the compiled-in key.
    BadSignature,
    /// The folder the app is installed in cannot be written.
    NotWritable(PathBuf),
    /// This copy was not installed from a Release archive.
    NotUpdatable,
    /// The download is not a Release archive this app can install.
    BadArchive(String),
    /// Putting the new files in place failed, and so did putting some of the replaced ones
    /// back: they are kept in this folder ([`ROLLBACK_INCOMPLETE`]).
    RollbackIncomplete(PathBuf),
    Cancelled,
    /// Anything else, in a few words.
    Failed(String),
}

impl UpdateError {
    /// What went wrong, in plain words, for the update notice.
    pub fn message(&self) -> String {
        match self {
            Self::Offline => {
                "Could not reach GitHub. Check your internet connection and try again.".into()
            }
            Self::RateLimited => "GitHub is limiting how often this computer can ask for updates. \
                 Try again in an hour, or get the new version from the download page."
                .into(),
            Self::NoRelease => "No version of Open Bible Software has been published yet.".into(),
            Self::NoDownload => {
                "This release has no download for this computer. The download page lists \
                 every download."
                    .into()
            }
            Self::NoChecksums => "The release has no checksums to check the download against, \
                 so it was not installed. Nothing was changed."
                .into(),
            Self::ChecksumMismatch => "The download did not match the release's checksum, so it \
                 was not installed. Nothing was changed."
                .into(),
            Self::BadSignature => "The download's signature did not match the release key, so \
                 it was not installed. Nothing was changed."
                .into(),
            Self::NotWritable(dir) => format!(
                "Open Bible Software cannot update itself here: you do not have permission to \
                 change its folder ({}). Get the new version from the download page.",
                dir.display()
            ),
            Self::NotUpdatable => "This copy of Open Bible Software was not installed from a \
                 release archive, so it cannot update itself. Get the new version from the \
                 download page."
                .into(),
            Self::BadArchive(why) => format!(
                "The download is not a release archive this app can install ({why}). Nothing \
                 was changed."
            ),
            Self::RollbackIncomplete(dir) => format!(
                "The update did not work, and some of the files it replaced could not be put \
                 back. They are kept in {}; your Library and Notes were not changed. Get the \
                 new version from the download page.",
                dir.display()
            ),
            Self::Cancelled => "Update cancelled. Nothing was changed.".into(),
            Self::Failed(why) => {
                format!("The update did not work ({why}). Nothing was changed.")
            }
        }
    }

    fn from_fetch(e: FetchError) -> Self {
        match e {
            FetchError::Cancelled => Self::Cancelled,
            FetchError::RateLimited => Self::RateLimited,
            FetchError::NotFound => Self::Failed("a release file is missing".into()),
            FetchError::Network(_) => Self::Offline,
        }
    }
}

// ------------------------------------------------------------------------- Verification

/// How a download was checked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verification {
    /// SHA256SUMS signed with the compiled-in release key, and the download's SHA-256 in it.
    Signed,
    /// The download's SHA-256 in the release's SHA256SUMS; no release key yet.
    ChecksumOnly,
}

impl Verification {
    /// What the update notice says about it.
    pub fn describe(self) -> &'static str {
        match self {
            Self::Signed => "The download was checked against the release's signed checksums.",
            Self::ChecksumOnly => {
                "The download was checked against the release's SHA-256 checksums \
                 (SHA256SUMS). Release signing is not set up yet."
            }
        }
    }
}

/// The `(sha256, file name)` lines of a SHA256SUMS file (`sha256sum`'s `hex  name` or
/// `hex *name`).
pub fn parse_sha256sums(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|line| {
            let (hex, name) = line.trim_end().split_once(char::is_whitespace)?;
            let name = name.trim_start().trim_start_matches('*');
            (hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_hexdigit()) && !name.is_empty())
                .then(|| (hex.to_ascii_lowercase(), name.to_string()))
        })
        .collect()
}

/// Check the download `archive`, named `name`, against the release's `sums` (SHA256SUMS).
/// With a `public_key`, `sums` must carry a valid minisign `signature` by it first.
pub fn verify_archive(
    name: &str,
    archive: &[u8],
    sums: &[u8],
    signature: Option<&[u8]>,
    public_key: Option<&str>,
) -> Result<Verification, UpdateError> {
    let verification = match public_key {
        Some(key) => {
            let signature = signature.ok_or(UpdateError::BadSignature)?;
            let key = minisign_verify::PublicKey::from_base64(key.trim())
                .map_err(|_| UpdateError::BadSignature)?;
            let signature = std::str::from_utf8(signature)
                .ok()
                .and_then(|s| minisign_verify::Signature::decode(s).ok())
                .ok_or(UpdateError::BadSignature)?;
            key.verify(sums, &signature, false)
                .map_err(|_| UpdateError::BadSignature)?;
            Verification::Signed
        }
        None => Verification::ChecksumOnly,
    };
    let sums = std::str::from_utf8(sums).map_err(|_| UpdateError::NoChecksums)?;
    let expected = parse_sha256sums(sums)
        .into_iter()
        .find(|(_, n)| n == name)
        .map(|(hex, _)| hex)
        .ok_or(UpdateError::NoChecksums)?;
    if crate::starter::sha256_hex(archive) != expected {
        return Err(UpdateError::ChecksumMismatch);
    }
    Ok(verification)
}

// ------------------------------------------------------------------------------- Installs

/// This platform's target triple, as the release workflow names its archives; `None` where
/// no archive is published.
pub fn platform_target() -> Option<&'static str> {
    if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        Some("x86_64-unknown-linux-gnu")
    } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        Some("aarch64-apple-darwin")
    } else if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
        Some("x86_64-pc-windows-msvc")
    } else {
        None
    }
}

/// The Release archive of `tag` for `target`: `openbible-<tag>-<target>.tar.gz`, or `.zip`
/// for Windows.
pub fn archive_asset_name(tag: &str, target: &str) -> String {
    let ext = if target.contains("windows") {
        "zip"
    } else {
        "tar.gz"
    };
    format!("openbible-{tag}-{target}.{ext}")
}

/// How the running copy was installed, as its [`INSTALL_MARKER`] says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallKind {
    /// Unpacked from a Release archive: the executable, `starter/` and the licence files in
    /// one folder the app replaces in place.
    Archive,
    /// Anything else (a build from source, a distribution's package, an installer from #272
    /// this build cannot update yet): Update now is not offered, the download page is.
    Other,
}

/// The installed copy: its executable and the folder it was unpacked in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Install {
    pub dir: PathBuf,
    pub exe: PathBuf,
    pub kind: InstallKind,
}

impl Install {
    /// The install `exe` belongs to, by the marker beside it.
    pub fn of_exe(exe: &Path) -> Self {
        let dir = exe.parent().map(Path::to_path_buf).unwrap_or_default();
        let kind = std::fs::read_to_string(dir.join(INSTALL_MARKER))
            .ok()
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
            .filter(|v| v.get("kind").and_then(Value::as_str) == Some("archive"))
            .map_or(InstallKind::Other, |_| InstallKind::Archive);
        Self {
            dir,
            exe: exe.to_path_buf(),
            kind,
        }
    }

    /// The running app's install, by its executable's real path.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn current() -> Option<Self> {
        let exe = std::env::current_exe().ok()?;
        Some(Self::of_exe(&std::fs::canonicalize(&exe).unwrap_or(exe)))
    }

    /// Whether Update now can replace this copy in place: a Release archive install, on a
    /// platform releases have archives for, in a folder this user may change.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn check_updatable(&self) -> Result<(), UpdateError> {
        if self.kind != InstallKind::Archive {
            return Err(UpdateError::NotUpdatable);
        }
        platform_target().ok_or(UpdateError::NoDownload)?;
        // The files a failed update could not put back are not risked on another one.
        let work = self.dir.join(UPDATE_DIR);
        if work.join(ROLLBACK_INCOMPLETE).exists() {
            return Err(UpdateError::RollbackIncomplete(work.join("old")));
        }
        let probe = self.dir.join(format!("{UPDATE_DIR}-probe"));
        match std::fs::write(&probe, b"") {
            Ok(()) => {
                let _ = std::fs::remove_file(&probe);
                Ok(())
            }
            Err(_) => Err(UpdateError::NotWritable(self.dir.clone())),
        }
    }
}

/// Unpack the verified Release archive `bytes` (named `name`) into [`UPDATE_DIR`]`/new`
/// beside the installed copy and return its top folder. The installed copy is not touched.
/// Entries that would land outside the folder are refused; the archive must hold one folder
/// with the app's executable in it.
#[cfg(not(target_arch = "wasm32"))]
pub fn stage_update(install: &Install, name: &str, bytes: &[u8]) -> Result<PathBuf, UpdateError> {
    let work = install.dir.join(UPDATE_DIR);
    let failed = |e: std::io::Error| match e.kind() {
        std::io::ErrorKind::PermissionDenied => UpdateError::NotWritable(install.dir.clone()),
        _ => UpdateError::Failed(format!("unpacking: {e}")),
    };
    if work.exists() {
        std::fs::remove_dir_all(&work).map_err(failed)?;
    }
    let new = work.join("new");
    std::fs::create_dir_all(&new).map_err(failed)?;
    let unpacked = if name.ends_with(".zip") {
        unpack_zip(bytes, &new)
    } else if name.ends_with(".tar.gz") {
        unpack_tar_gz(bytes, &new)
    } else {
        Err(UpdateError::BadArchive("not a .tar.gz or .zip".into()))
    };
    if let Err(e) = unpacked {
        let _ = std::fs::remove_dir_all(&work);
        return Err(e);
    }
    let tops: Vec<PathBuf> = std::fs::read_dir(&new)
        .map_err(failed)?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .collect();
    let top = match tops.as_slice() {
        [top] if top.is_dir() => top.clone(),
        _ => {
            let _ = std::fs::remove_dir_all(&work);
            return Err(UpdateError::BadArchive(
                "it does not hold one folder".into(),
            ));
        }
    };
    let exe_name = install.exe.file_name().unwrap_or_default();
    if !top.join(exe_name).is_file() {
        let _ = std::fs::remove_dir_all(&work);
        return Err(UpdateError::BadArchive(format!(
            "it has no {}",
            exe_name.to_string_lossy()
        )));
    }
    Ok(top)
}

#[cfg(not(target_arch = "wasm32"))]
fn unpack_tar_gz(bytes: &[u8], dest: &Path) -> Result<(), UpdateError> {
    let bad = |e: std::io::Error| UpdateError::BadArchive(e.to_string());
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(bytes));
    for entry in archive.entries().map_err(bad)? {
        let mut entry = entry.map_err(bad)?;
        // `unpack_in` refuses (returns false for) paths that would leave `dest`.
        if !entry.unpack_in(dest).map_err(bad)? {
            return Err(UpdateError::BadArchive(
                "a file in it would land outside its folder".into(),
            ));
        }
    }
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
fn unpack_zip(bytes: &[u8], dest: &Path) -> Result<(), UpdateError> {
    let bad = |e: &dyn std::fmt::Display| UpdateError::BadArchive(e.to_string());
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| bad(&e))?;
    for i in 0..zip.len() {
        let mut file = zip.by_index(i).map_err(|e| bad(&e))?;
        let Some(rel) = file.enclosed_name() else {
            return Err(UpdateError::BadArchive(
                "a file in it would land outside its folder".into(),
            ));
        };
        let path = dest.join(rel);
        if file.is_dir() {
            std::fs::create_dir_all(&path).map_err(|e| bad(&e))?;
            continue;
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| bad(&e))?;
        }
        let mut out = std::fs::File::create(&path).map_err(|e| bad(&e))?;
        std::io::copy(&mut file, &mut out).map_err(|e| bad(&e))?;
        #[cfg(unix)]
        if let Some(mode) = file.unix_mode() {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode & 0o777))
                .map_err(|e| bad(&e))?;
        }
    }
    Ok(())
}

/// Replace the installed copy with the unpacked one at `staged`: each of its top-level
/// entries (the executable, `starter/`, the licence files, the marker) is renamed into the
/// install folder, the entry it replaces first moved to [`UPDATE_DIR`]`/old`. Renaming works
/// on a running executable on every platform (Windows refuses only to overwrite or delete
/// it). On any failure everything moved so far is moved back. Returns the new executable.
#[cfg(not(target_arch = "wasm32"))]
pub fn swap_in(install: &Install, staged: &Path) -> Result<PathBuf, UpdateError> {
    let old = install.dir.join(UPDATE_DIR).join("old");
    let io = |e: std::io::Error| match e.kind() {
        std::io::ErrorKind::PermissionDenied => UpdateError::NotWritable(install.dir.clone()),
        _ => UpdateError::Failed(e.to_string()),
    };
    if old.exists() {
        std::fs::remove_dir_all(&old).map_err(io)?;
    }
    std::fs::create_dir_all(&old).map_err(io)?;
    let mut names: Vec<std::ffi::OsString> = std::fs::read_dir(staged)
        .map_err(io)?
        .filter_map(Result::ok)
        .map(|e| e.file_name())
        .collect();
    names.sort();
    // (name, whether an old entry was moved aside, whether the new one is in place)
    let mut done: Vec<(std::ffi::OsString, bool, bool)> = Vec::new();
    let mut failure = None;
    for name in &names {
        let target = install.dir.join(name);
        let had_old = std::fs::symlink_metadata(&target).is_ok();
        if had_old && let Err(e) = std::fs::rename(&target, old.join(name)) {
            failure = Some(io(e));
            break;
        }
        done.push((name.clone(), had_old, false));
        if let Err(e) = std::fs::rename(staged.join(name), &target) {
            failure = Some(io(e));
            break;
        }
        if let Some(last) = done.last_mut() {
            last.2 = true;
        }
    }
    if let Some(e) = failure {
        let mut stuck = Vec::new();
        for (name, had_old, placed) in done.into_iter().rev() {
            if placed && let Err(err) = std::fs::rename(install.dir.join(&name), staged.join(&name))
            {
                stuck.push(format!("{}: {err}", name.to_string_lossy()));
            }
            if had_old && let Err(err) = std::fs::rename(old.join(&name), install.dir.join(&name)) {
                stuck.push(format!("{}: {err}", name.to_string_lossy()));
            }
        }
        if stuck.is_empty() {
            return Err(e);
        }
        // The replaced files left in `old/` are the only copy: keep them past the clean-up.
        log::error!(
            "update failed ({e:?}) and could not put back {}; the replaced files are kept in {}",
            stuck.join(", "),
            old.display()
        );
        let note = format!(
            "An update of Open Bible Software failed and could not put these back:\n{}\n\
             The files it replaced are in the old folder beside this note.\n",
            stuck.join("\n")
        );
        let _ = std::fs::write(install.dir.join(UPDATE_DIR).join(ROLLBACK_INCOMPLETE), note);
        return Err(UpdateError::RollbackIncomplete(old));
    }
    let exe = install
        .dir
        .join(install.exe.file_name().unwrap_or_default());
    log::info!("update installed; the replaced files wait for the next launch to remove them");
    Ok(exe)
}

/// Remove what an update left beside the executable in `dir`: the replaced files (on
/// Windows the old executable, which could not be deleted while it ran) and an unpacked
/// download that was never installed. Called at launch, off the UI thread. Errors are
/// ignored: whatever is still in use goes at a later launch. After a failed update that
/// could not put everything back ([`ROLLBACK_INCOMPLETE`]) the replaced files in `old/` are
/// kept; only the unpacked download goes.
#[cfg(not(target_arch = "wasm32"))]
pub fn clean_up_update_folder(dir: &Path) {
    let work = dir.join(UPDATE_DIR);
    if work.join(ROLLBACK_INCOMPLETE).exists() {
        log::warn!(
            "keeping the files a failed update could not put back, in {}",
            work.join("old").display()
        );
        let _ = std::fs::remove_dir_all(work.join("new"));
        return;
    }
    if work.exists() {
        match std::fs::remove_dir_all(&work) {
            Ok(()) => log::info!("removed the files the last update replaced"),
            Err(e) => log::warn!("could not remove the files the last update replaced: {e}"),
        }
    }
}

// ---------------------------------------------------------------------- Session and state

/// Where the Update check and Update now are.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateStatus {
    Idle,
    Checking {
        /// Help → Check for updates… (the result is always shown) rather than at launch.
        manual: bool,
    },
    /// The last check found nothing newer.
    UpToDate,
    Available(Release),
    Downloading {
        release: Release,
        done: u64,
        total: u64,
    },
    /// Installed; Restart now runs it.
    Installed {
        release: Release,
        exe: PathBuf,
        /// The Before-update backup of the User database, when there is one.
        backup: Option<PathBuf>,
        verification: Verification,
    },
    Failed {
        error: UpdateError,
        /// The release Update now was installing, to try again.
        release: Option<Release>,
    },
}

/// What an Update download job hands back.
struct Staged {
    top: PathBuf,
    verification: Verification,
}

/// The Update settings and where the Update check is (#276).
#[derive(Debug, Clone)]
pub struct Updates {
    pub source: UpdateSource,
    current: Version,
    install: Option<Install>,
    /// "Check for updates automatically" (on by default, ADR 0017).
    auto_check: bool,
    /// "Include pre-releases" (off by default).
    prereleases: bool,
    /// The tag Skip this version was clicked for.
    skipped: Option<String>,
    /// The first-run note about automatic checks was answered.
    explained: bool,
    status: UpdateStatus,
    /// The update notice is showing.
    notice: bool,
    job: Option<JobId>,
    /// This launch's automatic check has started.
    launch_checked: bool,
    /// A script ran Update now (or Restart now) before the check (or install) finished:
    /// it goes on when it can.
    queued_install: bool,
    queued_restart: bool,
}

impl Default for Updates {
    fn default() -> Self {
        Self {
            source: UpdateSource::default(),
            current: Version::current(),
            install: None,
            auto_check: true,
            prereleases: false,
            skipped: None,
            explained: false,
            status: UpdateStatus::Idle,
            notice: false,
            job: None,
            launch_checked: false,
            queued_install: false,
            queued_restart: false,
        }
    }
}

impl Updates {
    pub fn status(&self) -> &UpdateStatus {
        &self.status
    }

    /// Whether a check or a download is running.
    pub fn is_busy(&self) -> bool {
        self.job.is_some()
    }

    /// Whether an update is downloading (its progress bar wants frames).
    pub fn is_downloading(&self) -> bool {
        matches!(self.status, UpdateStatus::Downloading { .. })
    }

    /// Whether the update notice is showing.
    pub fn notice_shown(&self) -> bool {
        self.notice
    }

    pub fn current(&self) -> &Version {
        &self.current
    }

    pub fn auto_check(&self) -> bool {
        self.auto_check
    }

    pub fn prereleases(&self) -> bool {
        self.prereleases
    }

    pub fn skipped(&self) -> Option<&str> {
        self.skipped.as_deref()
    }

    pub fn install(&self) -> Option<&Install> {
        self.install.as_ref()
    }

    /// The first-run note ("checks for updates automatically; turn it off in Settings") is
    /// due: this launch checked automatically and the note was never answered.
    pub fn first_run_note(&self) -> bool {
        self.launch_checked && self.auto_check && !self.explained
    }

    /// Whether this launch's automatic check is still to come.
    pub fn launch_check_pending(&self) -> bool {
        self.auto_check && !self.launch_checked
    }

    /// Why Update now cannot replace this copy, if it cannot (the notice then offers the
    /// download page instead).
    pub fn cannot_update(&self) -> Option<UpdateError> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            match &self.install {
                Some(install) => install.check_updatable().err(),
                None => Some(UpdateError::NotUpdatable),
            }
        }
        #[cfg(target_arch = "wasm32")]
        Some(UpdateError::NotUpdatable)
    }

    /// The page to send the user to for `release` (its own page), else the releases page.
    pub fn download_page(&self) -> String {
        match &self.status {
            UpdateStatus::Available(r)
            | UpdateStatus::Downloading { release: r, .. }
            | UpdateStatus::Failed {
                release: Some(r), ..
            } if !r.page.is_empty() => r.page.clone(),
            _ => self.source.page.clone(),
        }
    }

    fn release(&self) -> Option<&Release> {
        match &self.status {
            UpdateStatus::Available(r)
            | UpdateStatus::Downloading { release: r, .. }
            | UpdateStatus::Installed { release: r, .. }
            | UpdateStatus::Failed {
                release: Some(r), ..
            } => Some(r),
            _ => None,
        }
    }

    fn to_json(&self) -> Value {
        json!({
            "auto_check": self.auto_check,
            "prereleases": self.prereleases,
            "skipped": self.skipped,
            "explained": self.explained,
        })
    }
}

impl Session {
    pub fn updates(&self) -> &Updates {
        &self.updates
    }

    /// Where releases come from (the app sets [`UpdateSource::from_env`]; tests a mock).
    pub fn set_update_source(&mut self, source: UpdateSource) {
        self.updates.source = source;
    }

    /// The installed copy Update now replaces (the app sets [`Install::current`]).
    pub fn set_install(&mut self, install: Install) {
        self.updates.install = Some(install);
    }

    /// Pretend to be another version (tests).
    pub fn set_current_version(&mut self, version: Version) {
        self.updates.current = version;
    }

    /// Read the Update settings from the User database (defaults where it has none).
    pub(crate) fn load_update_settings(&mut self) {
        let saved = (self.user.as_ref())
            .and_then(|u| u.preference(UPDATES_PREF).ok().flatten())
            .and_then(|v| serde_json::from_str::<Value>(&v).ok())
            .unwrap_or(Value::Null);
        let flag =
            |key: &str, default: bool| saved.get(key).and_then(Value::as_bool).unwrap_or(default);
        self.updates.auto_check = flag("auto_check", true);
        self.updates.prereleases = flag("prereleases", false);
        self.updates.explained = flag("explained", false);
        self.updates.skipped = saved
            .get("skipped")
            .and_then(Value::as_str)
            .map(str::to_string);
    }

    fn save_update_settings(&mut self, command: &'static str) -> Result<(), CommandError> {
        if let Some(user) = &self.user {
            user.set_preference(UPDATES_PREF, &self.updates.to_json().to_string())
                .map_err(|e| failed(command, format!("saving the Update settings: {e}")))?;
        }
        Ok(())
    }

    /// This launch's automatic Update check, when "Check for updates automatically" is on;
    /// the UI calls it [`LAUNCH_CHECK_DELAY_SECS`] after the first window appears. It runs at
    /// most once per launch. Returns its job.
    pub fn launch_update_check(&mut self) -> Option<JobId> {
        if !self.updates.launch_check_pending() {
            return None;
        }
        self.updates.launch_checked = true;
        self.check_for_updates(false)
    }

    /// Ask the release source for the newest release, on the job queue. `manual`: the user
    /// asked (Help → Check for updates…), so the answer is shown even when there is nothing
    /// new, and a failure is explained; a launch check speaks only when there is a release
    /// to offer that was not skipped. Returns the job, or `None` while one is running.
    pub fn check_for_updates(&mut self, manual: bool) -> Option<JobId> {
        // While a check or download runs, or an installed update waits for Restart now, a
        // new check would only hide where it is: the notice shows it instead. A launch check
        // still running becomes the user's, so its answer (or failure) is shown.
        if self.updates.job.is_some()
            || matches!(self.updates.status, UpdateStatus::Installed { .. })
        {
            if manual {
                if let UpdateStatus::Checking { manual: m } = &mut self.updates.status {
                    *m = true;
                }
                self.updates.notice = true;
            }
            return None;
        }
        let url = self.updates.source.check_url(self.updates.prereleases);
        let fetch = self.fetch();
        let current = self.updates.current.clone();
        let prereleases = self.updates.prereleases;
        log::info!("checking for updates");
        let job = self.jobs().submit_fn("Update check", move |ctx| {
            let found = fetch
                .get(&url, &mut |_, _| !ctx.is_cancelled())
                .map_err(|e| match e {
                    FetchError::NotFound => UpdateError::NoRelease,
                    e => UpdateError::from_fetch(e),
                })
                .and_then(|bytes| {
                    parse_releases(&String::from_utf8_lossy(&bytes)).map_err(UpdateError::Failed)
                })
                .map(|releases| newest_release(&releases, &current, prereleases).cloned());
            Ok(found)
        });
        self.updates.job = Some(job);
        self.updates.status = UpdateStatus::Checking { manual };
        if manual {
            self.updates.notice = true;
        }
        Some(job)
    }

    /// Update now: download the release on offer for this platform, verify it and unpack it
    /// beside the installed copy, on the job queue. The swap happens when the job is done
    /// ([`Self::poll_updates`]). Returns the job.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn start_app_update(&mut self) -> Result<JobId, UpdateError> {
        let release = match &self.updates.status {
            UpdateStatus::Available(r)
            | UpdateStatus::Failed {
                release: Some(r), ..
            } => r.clone(),
            UpdateStatus::Downloading { .. } => {
                return Err(UpdateError::Failed(
                    "an update is already downloading".into(),
                ));
            }
            _ => return Err(UpdateError::Failed("there is no update to install".into())),
        };
        let result = self.download_job(&release);
        match result {
            Ok(job) => {
                self.updates.job = Some(job);
                self.updates.status = UpdateStatus::Downloading {
                    release,
                    done: 0,
                    total: 0,
                };
                self.updates.notice = true;
                Ok(job)
            }
            Err(error) => {
                log::warn!("update not started: {error:?}");
                self.updates.status = UpdateStatus::Failed {
                    error: error.clone(),
                    release: Some(release),
                };
                self.updates.notice = true;
                Err(error)
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn download_job(&mut self, release: &Release) -> Result<JobId, UpdateError> {
        // Never a downgrade, nor the version that is running.
        if release.version <= self.updates.current || release.draft {
            return Err(UpdateError::Failed("that version is not newer".into()));
        }
        let install = self
            .updates
            .install
            .clone()
            .ok_or(UpdateError::NotUpdatable)?;
        install.check_updatable()?;
        let target = platform_target().ok_or(UpdateError::NoDownload)?;
        let name = archive_asset_name(&release.tag, target);
        let source = self.updates.source.clone();
        let asset = |name: &str| -> Option<ReleaseAsset> {
            release
                .asset(name)
                .filter(|a| source.serves(&a.url))
                .cloned()
        };
        let archive = asset(&name).ok_or(UpdateError::NoDownload)?;
        let sums = asset(CHECKSUMS_ASSET).ok_or(UpdateError::NoChecksums)?;
        let key = source.public_key.clone();
        let signature = match key {
            Some(_) => Some(asset(SIGNATURE_ASSET).ok_or(UpdateError::BadSignature)?),
            None => None,
        };
        let fetch = self.fetch();
        log::info!("downloading an update");
        Ok(self.jobs().submit_fn("Update download", move |ctx| {
            let staged = (|| {
                let small = |a: &ReleaseAsset| {
                    fetch
                        .get(&a.url, &mut |_, _| !ctx.is_cancelled())
                        .map_err(UpdateError::from_fetch)
                };
                let sums_bytes = small(&sums)?;
                let signature_bytes = signature.as_ref().map(small).transpose()?;
                let total = archive.size;
                let bytes = fetch
                    .get(&archive.url, &mut |done, size| {
                        ctx.progress(done, size.unwrap_or(total).max(done));
                        !ctx.is_cancelled()
                    })
                    .map_err(UpdateError::from_fetch)?;
                let verification = verify_archive(
                    &archive.name,
                    &bytes,
                    &sums_bytes,
                    signature_bytes.as_deref(),
                    key.as_deref(),
                )?;
                if ctx.is_cancelled() {
                    return Err(UpdateError::Cancelled);
                }
                let top = stage_update(&install, &archive.name, &bytes)?;
                Ok(Staged { top, verification })
            })();
            Ok(staged)
        }))
    }

    /// The download is verified and unpacked: take the Before-update backup, then put the
    /// new files in place. Nothing in the Library, the User database or the settings is
    /// changed.
    #[cfg(not(target_arch = "wasm32"))]
    fn install_staged(&mut self, release: Release, staged: Staged) {
        let fail = |s: &mut Self, error: UpdateError| {
            log::warn!("update not installed: {error:?}");
            if let Some(install) = &s.updates.install {
                clean_up_update_folder(&install.dir);
            }
            s.updates.status = UpdateStatus::Failed {
                error,
                release: Some(release.clone()),
            };
            s.updates.queued_restart = false;
        };
        let Some(install) = self.updates.install.clone() else {
            return fail(self, UpdateError::NotUpdatable);
        };
        let backup = match &self.user {
            Some(user) => match user.update_backup() {
                Ok(path) => Some(path),
                Err(e) => {
                    return fail(
                        self,
                        UpdateError::Failed(format!("could not back up your data first: {e}")),
                    );
                }
            },
            None => None,
        };
        match swap_in(&install, &staged.top) {
            Ok(exe) => {
                self.updates.status = UpdateStatus::Installed {
                    release,
                    exe: exe.clone(),
                    backup,
                    verification: staged.verification,
                };
                if std::mem::take(&mut self.updates.queued_restart) {
                    self.workspace.push_effect(Effect::Restart(exe));
                }
            }
            Err(e) => fail(self, e),
        }
    }

    /// Apply Update job events; return the rest.
    pub(crate) fn poll_updates(&mut self, events: Vec<JobEvent>) -> Vec<JobEvent> {
        let mut other = Vec::new();
        for event in events {
            if self.updates.job != Some(event.id) {
                other.push(event);
                continue;
            }
            match event.kind {
                JobEventKind::Progress { done, total } => {
                    if let UpdateStatus::Downloading {
                        done: d, total: t, ..
                    } = &mut self.updates.status
                    {
                        *d = done;
                        *t = total;
                    }
                }
                JobEventKind::Finished(out) => {
                    self.updates.job = None;
                    match out.downcast::<Result<Option<Release>, UpdateError>>() {
                        Ok(found) => self.update_checked(*found),
                        Err(out) => {
                            if let Ok(staged) = out.downcast::<Result<Staged, UpdateError>>() {
                                self.update_downloaded(*staged);
                            }
                        }
                    }
                }
                JobEventKind::Failed(e) => {
                    self.updates.job = None;
                    self.update_failed(UpdateError::Failed(e));
                }
                JobEventKind::Cancelled => {
                    self.updates.job = None;
                    self.update_failed(UpdateError::Cancelled);
                }
                _ => {}
            }
        }
        other
    }

    fn update_checked(&mut self, found: Result<Option<Release>, UpdateError>) {
        let manual = matches!(self.updates.status, UpdateStatus::Checking { manual: true });
        match found {
            Ok(Some(release)) => {
                log::info!("an update is available");
                let skipped = self.updates.skipped.as_deref() == Some(release.tag.as_str());
                self.updates.status = UpdateStatus::Available(release);
                if manual || !skipped {
                    self.updates.notice = true;
                }
                if std::mem::take(&mut self.updates.queued_install) {
                    #[cfg(not(target_arch = "wasm32"))]
                    let _ = self.start_app_update();
                }
            }
            Ok(None) => {
                log::info!("no update available");
                self.updates.status = UpdateStatus::UpToDate;
                self.updates.queued_install = false;
                self.updates.queued_restart = false;
            }
            Err(error) => {
                log::warn!("update check failed: {error:?}");
                self.updates.queued_install = false;
                self.updates.queued_restart = false;
                if manual {
                    self.updates.status = UpdateStatus::Failed {
                        error,
                        release: None,
                    };
                } else {
                    // A launch check stays quiet when it cannot reach GitHub.
                    self.updates.status = UpdateStatus::Idle;
                    self.updates.notice = false;
                }
            }
        }
    }

    fn update_downloaded(&mut self, staged: Result<Staged, UpdateError>) {
        let release = self.updates.release().cloned();
        match (staged, release) {
            #[cfg(not(target_arch = "wasm32"))]
            (Ok(staged), Some(release)) => self.install_staged(release, staged),
            (Err(error), _) => self.update_failed(error),
            #[allow(unreachable_patterns)]
            _ => self.update_failed(UpdateError::Failed("the download went missing".into())),
        }
    }

    fn update_failed(&mut self, error: UpdateError) {
        log::warn!("update failed: {error:?}");
        let release = self.updates.release().cloned();
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(install) = &self.updates.install {
            clean_up_update_folder(&install.dir);
        }
        self.updates.queued_install = false;
        self.updates.queued_restart = false;
        self.updates.status = UpdateStatus::Failed { error, release };
        self.updates.notice = true;
    }
}

// ------------------------------------------------------------------------------ Commands

fn failed(command: &'static str, message: impl ToString) -> CommandError {
    CommandError::Failed {
        command,
        message: message.to_string(),
    }
}

fn check(s: &mut Session, _: Params<'_>) -> Result<Value, CommandError> {
    let job = s.check_for_updates(true);
    Ok(json!({ "job": job.map(|j| j.0) }))
}

/// Update now. Run before the check has answered (a script), it waits for the answer.
fn install(s: &mut Session, _: Params<'_>) -> Result<Value, CommandError> {
    const ID: &str = "update.install";
    match s.updates.status {
        UpdateStatus::Checking { .. } => {
            s.updates.queued_install = true;
            return Ok(json!({ "queued": true }));
        }
        UpdateStatus::Available(_)
        | UpdateStatus::Failed {
            release: Some(_), ..
        } => {}
        UpdateStatus::Downloading { .. } | UpdateStatus::Installed { .. } => {
            return Ok(json!({ "running": true }));
        }
        _ => return Err(failed(ID, "There is no update to install")),
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        // A failure is shown in the update notice.
        let job = s.start_app_update().ok();
        Ok(json!({ "job": job.map(|j| j.0) }))
    }
    #[cfg(target_arch = "wasm32")]
    Err(failed(ID, "Updates are not available here"))
}

fn cancel(s: &mut Session, _: Params<'_>) -> Result<Value, CommandError> {
    let job = match (&s.updates.status, s.updates.job) {
        (UpdateStatus::Downloading { .. }, Some(job)) => job,
        _ => return Err(failed("update.cancel", "No update is downloading")),
    };
    s.jobs().cancel(job);
    Ok(json!({ "job": job.0 }))
}

/// Later: close the notice. A release on offer stays known (Help → Check for updates…
/// shows it again); a download or an installed update goes on.
fn later(s: &mut Session, _: Params<'_>) -> Result<Value, CommandError> {
    s.updates.notice = false;
    if matches!(
        s.updates.status,
        UpdateStatus::UpToDate | UpdateStatus::Failed { .. }
    ) {
        s.updates.status = match s.updates.release() {
            Some(r) if r.version > s.updates.current => UpdateStatus::Available(r.clone()),
            _ => UpdateStatus::Idle,
        };
    }
    Ok(Value::Null)
}

fn skip(s: &mut Session, _: Params<'_>) -> Result<Value, CommandError> {
    const ID: &str = "update.skip";
    let tag = match &s.updates.status {
        UpdateStatus::Available(r)
        | UpdateStatus::Failed {
            release: Some(r), ..
        } => r.tag.clone(),
        _ => return Err(failed(ID, "There is no update to skip")),
    };
    s.updates.skipped = Some(tag.clone());
    s.updates.notice = false;
    s.save_update_settings(ID)?;
    Ok(json!({ "skipped": tag }))
}

/// Restart now. Run before the install has finished (a script), it waits for it.
fn restart(s: &mut Session, _: Params<'_>) -> Result<Value, CommandError> {
    match &s.updates.status {
        UpdateStatus::Installed { exe, .. } => {
            let exe = exe.clone();
            s.workspace.push_effect(Effect::Restart(exe.clone()));
            Ok(json!({ "exe": exe.display().to_string() }))
        }
        UpdateStatus::Downloading { .. } => {
            s.updates.queued_restart = true;
            Ok(json!({ "queued": true }))
        }
        UpdateStatus::Checking { .. } if s.updates.queued_install => {
            s.updates.queued_restart = true;
            Ok(json!({ "queued": true }))
        }
        _ => Err(failed("update.restart", "No update has been installed")),
    }
}

fn set_flag(
    s: &mut Session,
    p: Params<'_>,
    command: &'static str,
    set: fn(&mut Updates, bool),
) -> Result<Value, CommandError> {
    let on = p
        .bool("on")
        .ok_or_else(|| failed(command, "`on` must be true or false"))?;
    set(&mut s.updates, on);
    s.save_update_settings(command)?;
    Ok(json!({ "on": on }))
}

fn auto_check(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    set_flag(s, p, "updates.auto_check", |u, on| {
        u.auto_check = on;
        u.explained = true;
    })
}

fn prereleases(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    set_flag(s, p, "updates.prereleases", |u, on| u.prereleases = on)
}

fn explained(s: &mut Session, _: Params<'_>) -> Result<Value, CommandError> {
    s.updates.explained = true;
    s.save_update_settings("updates.explained")?;
    Ok(Value::Null)
}

const ON: &[ParamSpec] = &[ParamSpec {
    name: "on",
    kind: ParamKind::Boolean,
    required: true,
    help: "true or false",
}];

pub(crate) const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        id: "help.check_for_updates",
        label: "Check for updates…",
        shortcut: None,
        params: &[],
        enabled: always,
        run: check,
    },
    CommandSpec {
        id: "update.install",
        label: "Update now",
        shortcut: None,
        params: &[],
        enabled: always,
        run: install,
    },
    CommandSpec {
        id: "update.cancel",
        label: "Cancel the update download",
        shortcut: None,
        params: &[],
        enabled: always,
        run: cancel,
    },
    CommandSpec {
        id: "update.later",
        label: "Update later",
        shortcut: None,
        params: &[],
        enabled: always,
        run: later,
    },
    CommandSpec {
        id: "update.skip",
        label: "Skip this version",
        shortcut: None,
        params: &[],
        enabled: always,
        run: skip,
    },
    CommandSpec {
        id: "update.restart",
        label: "Restart now",
        shortcut: None,
        params: &[],
        enabled: always,
        run: restart,
    },
    CommandSpec {
        id: "updates.auto_check",
        label: "Check for updates automatically",
        shortcut: None,
        params: ON,
        enabled: always,
        run: auto_check,
    },
    CommandSpec {
        id: "updates.prereleases",
        label: "Include pre-releases",
        shortcut: None,
        params: ON,
        enabled: always,
        run: prereleases,
    },
    CommandSpec {
        id: "updates.explained",
        label: "Keep automatic update checks",
        shortcut: None,
        params: &[],
        enabled: always,
        run: explained,
    },
];
