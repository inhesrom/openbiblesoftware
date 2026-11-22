//! Where the Mobile app keeps its files (plan A7): one root folder the shell picks (on iOS,
//! `Application Support/OpenBible`), and the names of everything in it, which are the core's.

use std::path::{Path, PathBuf};

/// The Mobile app's folders and its User database, all under one root.
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
    /// Debug builds only: a Starter library folder (`dist/starter`) whose Study set Resources
    /// are added to the library read in place, so a simulator has Hebrew and Strong's tagging
    /// before the downloads S24 adds. `None` in a release build.
    pub study_set: Option<PathBuf>,
}

impl Paths {
    /// The library folder's name under the root.
    pub const LIBRARY: &'static str = "library";
    /// The User database's file name under the root.
    pub const USER_DB: &'static str = "user.db";
    /// The Search index cache's name under the root.
    pub const SEARCH: &'static str = "search";
    /// The Log folder's name under the root.
    pub const LOGS: &'static str = "logs";
    /// The downloads folder's name under the root.
    pub const DOWNLOADS: &'static str = "downloads";

    /// Every path under `root`. The alias cache is named as the desktop names it
    /// (`obs_engine::aliases::ALIAS_DIR`).
    pub fn under(root: &Path) -> Self {
        Self {
            library: root.join(Self::LIBRARY),
            user_db: root.join(Self::USER_DB),
            search_cache: root.join(Self::SEARCH),
            alias_cache: root.join(obs_engine::aliases::ALIAS_DIR),
            logs: root.join(Self::LOGS),
            downloads: root.join(Self::DOWNLOADS),
            study_set: None,
        }
    }

    /// With the debug Study set override.
    pub fn with_study_set(mut self, dir: Option<PathBuf>) -> Self {
        self.study_set = dir;
        self
    }

    /// The folders the app makes at boot, the User database's folder among them.
    pub fn folders(&self) -> Vec<PathBuf> {
        let mut out = vec![
            self.library.clone(),
            self.search_cache.clone(),
            self.alias_cache.clone(),
            self.logs.clone(),
            self.downloads.clone(),
        ];
        if let Some(dir) = self.user_db.parent() {
            out.push(dir.to_path_buf());
        }
        out
    }

    /// The folders a device backup leaves out (iOS `isExcludedFromBackup`): what the app can
    /// copy or download again. The User database is backed up.
    pub fn backup_excluded(&self) -> Vec<PathBuf> {
        vec![
            self.library.clone(),
            self.search_cache.clone(),
            self.alias_cache.clone(),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_root_names_the_six_paths() {
        let root = Path::new("/apps/OpenBible");
        let p = Paths::under(root);
        assert_eq!(p.library, root.join("library"));
        assert_eq!(p.user_db, root.join("user.db"));
        assert_eq!(p.search_cache, root.join("search"));
        assert_eq!(p.alias_cache, root.join("aliases"));
        assert_eq!(p.logs, root.join("logs"));
        assert_eq!(p.downloads, root.join("downloads"));
        assert_eq!(p.study_set, None);
        assert!(
            p.folders().contains(&root.to_path_buf()),
            "user.db's folder"
        );
        assert_eq!(
            p.backup_excluded(),
            [
                root.join("library"),
                root.join("search"),
                root.join("aliases")
            ]
        );
        let debug = p.with_study_set(Some("/dist/starter".into()));
        assert_eq!(debug.study_set.as_deref(), Some(Path::new("/dist/starter")));
    }
}
