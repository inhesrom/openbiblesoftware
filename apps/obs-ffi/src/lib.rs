//! The FFI surface of the Mobile app (ADR 0021, `docs/dev/mobile-core.md`): UniFFI exports over
//! `obs-mobile`. Rules stay in `obs-mobile`; this crate only converts and forwards.
//!
//! Everything but [`engine_version`] is the S03 spike of `docs/plans/mobile-ios.md` ("Hello
//! engine"): boot, open a reader, lay out a tile. S13 replaces it with the designed surface.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

uniffi::setup_scaffolding!();

/// The engine's version.
#[uniffi::export]
pub fn engine_version() -> String {
    obs_mobile::MobileApp::engine_version().to_string()
}

// S03 spike: replaced in S13.
/// Where the app keeps its files: `obs_mobile::Paths`, as absolute paths.
#[doc(hidden)]
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct Paths {
    pub library: String,
    pub user_db: String,
    pub search_cache: String,
    pub alias_cache: String,
    pub logs: String,
    pub downloads: String,
}

impl From<Paths> for obs_mobile::Paths {
    fn from(p: Paths) -> Self {
        Self {
            library: PathBuf::from(p.library),
            user_db: PathBuf::from(p.user_db),
            search_cache: PathBuf::from(p.search_cache),
            alias_cache: PathBuf::from(p.alias_cache),
            logs: PathBuf::from(p.logs),
            downloads: PathBuf::from(p.downloads),
        }
    }
}

// S03 spike: replaced in S13.
/// A tile's pixels: `width × height`, row-major RGBA8, premultiplied alpha.
#[doc(hidden)]
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct Tile {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl From<obs_mobile::Tile> for Tile {
    fn from(t: obs_mobile::Tile) -> Self {
        Self {
            width: t.width,
            height: t.height,
            rgba: t.rgba,
        }
    }
}

// S03 spike: replaced in S13.
/// Why a call failed, in words.
#[doc(hidden)]
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Error)]
pub enum MobileError {
    Failed { message: String },
}

impl std::fmt::Display for MobileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MobileError::Failed { message } => f.write_str(message),
        }
    }
}

impl std::error::Error for MobileError {}

impl From<obs_mobile::MobileError> for MobileError {
    fn from(e: obs_mobile::MobileError) -> Self {
        MobileError::Failed {
            message: e.to_string(),
        }
    }
}

// S03 spike: replaced in S13.
/// The running Mobile app (`obs_mobile::MobileApp`) behind one lock (plan A3): every call
/// locks, acts and returns.
#[doc(hidden)]
#[derive(uniffi::Object)]
pub struct MobileApp {
    app: Mutex<obs_mobile::MobileApp>,
}

impl MobileApp {
    fn with<T>(
        &self,
        f: impl FnOnce(&mut obs_mobile::MobileApp) -> Result<T, obs_mobile::MobileError>,
    ) -> Result<T, MobileError> {
        let mut app = self.app.lock().map_err(|_| MobileError::Failed {
            message: "the app stopped after an internal error".into(),
        })?;
        f(&mut app).map_err(MobileError::from)
    }
}

#[uniffi::export]
impl MobileApp {
    /// Start the app over `paths`, adding the Starter library from `bundle` (the Bundled set
    /// folder) on the first run, with the first Bible open as reader 0.
    #[uniffi::constructor]
    pub fn boot(paths: Paths, bundle: Option<String>) -> Result<Arc<Self>, MobileError> {
        let app = obs_mobile::MobileApp::boot(paths.into(), bundle.as_deref().map(Path::new))?;
        Ok(Arc::new(Self {
            app: Mutex::new(app),
        }))
    }

    /// What went wrong at boot without stopping it.
    pub fn problems(&self) -> Vec<String> {
        self.with(|a| Ok(a.problems().to_vec())).unwrap_or_default()
    }

    /// The ids of the library's Resources, in Preferred order.
    pub fn resource_ids(&self) -> Vec<String> {
        self.with(|a| Ok(a.resource_ids())).unwrap_or_default()
    }

    /// The Resource reader `reader` shows, if there is such a reader.
    pub fn reader_resource(&self, reader: u32) -> Option<String> {
        (self.with(|a| {
            Ok(a.reader_resource(obs_mobile::ReaderId(reader))
                .map(String::from))
        }))
        .ok()
        .flatten()
    }

    /// A new reader of `resource_id` at `reference` (or its start); returns its id.
    pub fn open_reader(
        &self,
        resource_id: String,
        reference: Option<String>,
    ) -> Result<u32, MobileError> {
        self.with(|a| a.open_reader(&resource_id, reference.as_deref()))
            .map(|r| r.0)
    }

    /// The tile of `reader`'s page, `width` points wide, `height` points tall from `top`
    /// points below its Current location, at `scale` pixels per point.
    pub fn layout_tile(
        &self,
        reader: u32,
        top: f32,
        width: f32,
        height: f32,
        scale: f32,
    ) -> Result<Tile, MobileError> {
        self.with(|a| a.layout_tile(obs_mobile::ReaderId(reader), top, width, height, scale))
            .map(Tile::from)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn engine_version_is_the_engines() {
        assert_eq!(
            super::engine_version(),
            obs_mobile::MobileApp::engine_version()
        );
    }
}
