//! SWORD repositories (Get Bibles, #91): catalogs and module installs over a transport the
//! app hands in.
//!
//! The engine and everything below it stay offline-first: they never open a network
//! connection themselves. The app implements [`Fetch`] (HTTPS with its own client) and the
//! engine wraps it with [`installer`]; [`LocalFetch`] reads `file://` repositories only and
//! is what tests and builds without a network transport use.

use std::fmt;
use std::sync::Arc;

use sword_modules::install::{DefaultTransport, DirEntry, Transport};

pub use sword_modules::ModuleKind;
pub use sword_modules::install::{
    Catalog, Installer, MASTER_REPO_LIST_URL, ModuleStatus, Protocol, RemoteModule, Repository,
    Source, parse_master_repo_list,
};

/// Why a fetch failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FetchError {
    /// The server answered that there is nothing at this URL (HTTP 404/410, missing file).
    NotFound,
    /// The progress callback asked to stop.
    Cancelled,
    /// Anything else: no network, refused, timed out, bad status, unsupported scheme.
    Network(String),
}

impl fmt::Display for FetchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound => f.write_str("not found"),
            Self::Cancelled => f.write_str("cancelled"),
            Self::Network(m) => f.write_str(m),
        }
    }
}

impl std::error::Error for FetchError {}

/// Called as bytes arrive: `(received, total if known)`. Return `false` to cancel.
pub type Progress<'a> = &'a mut dyn FnMut(u64, Option<u64>) -> bool;

/// Downloads whole files by URL. Implemented by the app (it owns the network client).
pub trait Fetch: fmt::Debug + Send + Sync {
    fn get(&self, url: &str, progress: Progress<'_>) -> Result<Vec<u8>, FetchError>;

    /// The size of the file at `url` without downloading it (an HTTP `HEAD`), when the
    /// server says. `Ok(None)`: it exists but its size is unknown.
    fn size(&self, url: &str) -> Result<Option<u64>, FetchError> {
        let _ = url;
        Ok(None)
    }
}

/// `file://` only. The default when no app transport is installed, and for tests against
/// a local mock repository; every other scheme fails without touching the network.
#[derive(Debug, Default, Clone, Copy)]
pub struct LocalFetch;

impl Fetch for LocalFetch {
    fn get(&self, url: &str, progress: Progress<'_>) -> Result<Vec<u8>, FetchError> {
        if !is_file_url(url) {
            return Err(FetchError::Network(format!(
                "{url}: no network transport in this build"
            )));
        }
        let bytes = builtin_get(url)?;
        let n = bytes.len() as u64;
        if !progress(n, Some(n)) {
            return Err(FetchError::Cancelled);
        }
        Ok(bytes)
    }

    fn size(&self, url: &str) -> Result<Option<u64>, FetchError> {
        if !is_file_url(url) {
            return Err(FetchError::Network(format!(
                "{url}: no network transport in this build"
            )));
        }
        local_size(url)
    }
}

/// The size of a `file://` URL's file.
pub fn local_size(url: &str) -> Result<Option<u64>, FetchError> {
    let path = url
        .get(7..)
        .map(|p| p.strip_prefix("localhost").unwrap_or(p))
        .map(|p| {
            // `file:///C:/x` → `C:/x`
            if p.len() > 2 && p.as_bytes()[2] == b':' {
                &p[1..]
            } else {
                p
            }
        })
        .ok_or(FetchError::NotFound)?;
    match std::fs::metadata(path) {
        Ok(m) if m.is_file() => Ok(Some(m.len())),
        Ok(_) => Err(FetchError::NotFound),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(FetchError::NotFound),
        Err(e) => Err(FetchError::Network(format!("{url}: {e}"))),
    }
}

/// Read a `file://` or `ftp://` URL with the SWORD crate's own transport, which needs no
/// networking crate (FTP is plain `std::net`). Apps use it for the schemes their HTTP
/// client does not speak; libraries reach it only through [`LocalFetch`] (`file://`).
pub fn builtin_get(url: &str) -> Result<Vec<u8>, FetchError> {
    DefaultTransport::new().get(url).map_err(from_sword)
}

/// A `file://` URL for a local folder or file, on every platform (`file:///C:/x` on
/// Windows, `file:///tmp/x` elsewhere).
pub fn file_url(path: &std::path::Path) -> String {
    let p = path.display().to_string().replace('\\', "/");
    if p.starts_with('/') {
        format!("file://{p}")
    } else {
        format!("file:///{p}")
    }
}

fn is_file_url(url: &str) -> bool {
    url.get(..7)
        .is_some_and(|s| s.eq_ignore_ascii_case("file://"))
}

fn from_sword(e: sword_modules::Error) -> FetchError {
    match e {
        sword_modules::Error::NotFound(_) => FetchError::NotFound,
        other => FetchError::Network(other.to_string()),
    }
}

fn to_sword(url: &str, e: FetchError) -> sword_modules::Error {
    match e {
        FetchError::NotFound => sword_modules::Error::NotFound(url.to_string()),
        FetchError::Cancelled => sword_modules::Error::Network {
            url: url.to_string(),
            message: CANCELLED.into(),
        },
        FetchError::Network(message) => sword_modules::Error::Network {
            url: url.to_string(),
            message,
        },
    }
}

/// The message a cancelled transfer surfaces as, inside SWORD install errors.
pub const CANCELLED: &str = "download cancelled";

/// Progress for a whole install or catalog fetch: `(received, total if known)` of the file
/// being fetched. Return `false` to cancel.
pub type OnProgress = Arc<dyn Fn(u64, Option<u64>) -> bool + Send + Sync>;

/// Bridges [`Fetch`] to the SWORD installer's transport.
struct Bridge {
    fetch: Arc<dyn Fetch>,
    on_progress: OnProgress,
}

impl Transport for Bridge {
    fn get(&self, url: &str) -> sword_modules::Result<Vec<u8>> {
        let on_progress = &self.on_progress;
        self.fetch
            .get(url, &mut |done, total| on_progress(done, total))
            .map_err(|e| to_sword(url, e))
    }

    fn list(&self, url: &str) -> sword_modules::Result<Vec<DirEntry>> {
        // Directory listings of local repositories come from the file system; remote ones
        // are the server's HTML index, fetched through the app's transport.
        if is_file_url(url) {
            return DefaultTransport::new().list(url);
        }
        let html = self.get(url)?;
        Ok(parse_listing(&String::from_utf8_lossy(&html), url))
    }
}

/// An HTML directory index as entries (the crate's default `Transport::list`).
fn parse_listing(html: &str, url: &str) -> Vec<DirEntry> {
    struct Html<'a>(&'a str);
    impl Transport for Html<'_> {
        fn get(&self, _: &str) -> sword_modules::Result<Vec<u8>> {
            Ok(self.0.as_bytes().to_vec())
        }
    }
    Html(html).list(url).unwrap_or_default()
}

/// A SWORD installer whose every byte goes through `fetch`.
pub fn installer(fetch: Arc<dyn Fetch>, on_progress: OnProgress) -> Installer {
    Installer::with_transport(Bridge { fetch, on_progress })
}

/// Whether an installer error came from a cancelled transfer.
pub fn is_cancelled(e: &sword_modules::Error) -> bool {
    matches!(e, sword_modules::Error::Network { message, .. } if message == CANCELLED)
}

/// SWORD installer and catalog errors, as shown to the user.
pub type RepoError = sword_modules::Error;
