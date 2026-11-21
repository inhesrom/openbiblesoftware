//! The Mobile app (CONTEXT.md § Mobile) below its platform shells: what each screen shows and
//! what each action does, as plain Rust types (ADR 0021 rule 3). The shells reach it through
//! `apps/obs-ffi` only; this crate never depends on an FFI crate and compiles for wasm32
//! (`docs/dev/mobile-core.md`).

use std::path::PathBuf;

/// Where the Mobile app keeps its files. The shell picks them (on iOS, under Application
/// Support) because the engine's desktop defaults (`obs_engine::default_library_dir`) do not
/// apply on a phone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    /// The library folder of `.obr` Resources.
    pub library: PathBuf,
    /// The User database file.
    pub user_db: PathBuf,
    /// The Search index cache.
    pub search_cache: PathBuf,
    /// The alias cache.
    pub alias_cache: PathBuf,
    /// The Log folder.
    pub logs: PathBuf,
    /// Where finished downloads wait to be installed.
    pub downloads: PathBuf,
}

/// The running Mobile app. A placeholder until S13 of `docs/plans/mobile-ios.md` gives it a
/// Session, readers and snapshots.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MobileApp {
    paths: Paths,
}

impl MobileApp {
    pub fn new(paths: Paths) -> Self {
        Self { paths }
    }

    pub fn paths(&self) -> &Paths {
        &self.paths
    }

    /// The engine's version, as the desktop's About shows it.
    pub fn engine_version() -> &'static str {
        obs_engine::updates::APP_VERSION
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mobile_app_keeps_the_paths_it_was_given() {
        let paths = Paths {
            library: "lib".into(),
            user_db: "user.db".into(),
            search_cache: "search".into(),
            alias_cache: "aliases".into(),
            logs: "logs".into(),
            downloads: "downloads".into(),
        };
        assert_eq!(MobileApp::new(paths.clone()).paths(), &paths);
        assert_eq!(
            MobileApp::engine_version(),
            obs_engine::updates::APP_VERSION
        );
    }
}
