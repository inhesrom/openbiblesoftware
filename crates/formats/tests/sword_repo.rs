//! SWORD repositories through the app-supplied transport (#91): catalogs and installs from a
//! local `file://` mock repository built from the open-text test modules. No network.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use obs_formats::sword_repo::{
    Fetch, FetchError, LocalFetch, Progress, Repository, file_url, installer, is_cancelled,
};
use obs_formats::{SwordSource, import_sword};

fn assets() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/assets/sword")
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let dest = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &dest);
        } else {
            std::fs::copy(entry.path(), dest).unwrap();
        }
    }
}

/// A raw SWORD repository (`mods.d/` + `modules/`, no package zips) in a temp dir.
fn mock_repo(name: &str) -> (PathBuf, Repository) {
    let root = std::env::temp_dir().join(format!("obs-sword-repo-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let repo = root.join("repo");
    copy_dir(&assets().join("mods.d"), &repo.join("mods.d"));
    copy_dir(&assets().join("modules"), &repo.join("modules"));
    let url = file_url(&repo);
    (root, Repository::from_url("Mock", &url).unwrap())
}

/// Records every URL and passes through to [`LocalFetch`].
#[derive(Debug, Default)]
struct Recording(Mutex<Vec<String>>);
impl Fetch for Recording {
    fn get(&self, url: &str, progress: Progress<'_>) -> Result<Vec<u8>, FetchError> {
        self.0.lock().unwrap().push(url.to_string());
        LocalFetch.get(url, progress)
    }
}

#[test]
fn catalog_and_install_go_through_the_given_fetch_and_the_install_imports() {
    let (root, repo) = mock_repo("install");
    let fetch = Arc::new(Recording::default());
    let bytes = Arc::new(AtomicU64::new(0));
    let seen = Arc::clone(&bytes);
    let inst = installer(
        fetch.clone(),
        Arc::new(move |done, _| {
            seen.fetch_add(done, Ordering::Relaxed);
            true
        }),
    );
    let catalog = inst.catalog(&repo).unwrap();
    let bibles: Vec<&str> = catalog
        .of_kind(sword_modules::ModuleKind::Bible)
        .map(|m| m.name())
        .collect();
    assert!(bibles.contains(&"ObsKjvOsis"), "{bibles:?}");
    let licence = catalog
        .get("ObsKjvOsis")
        .unwrap()
        .conf
        .get("DistributionLicense");
    assert_eq!(licence, Some("Public Domain"));

    let sword = root.join("sword");
    let installed = inst.install(&catalog, "ObsKjvOsis", &sword).unwrap();
    assert_eq!(installed, ["ObsKjvOsis"]);
    assert!(bytes.load(Ordering::Relaxed) > 0, "progress reported");
    assert!(
        fetch
            .0
            .lock()
            .unwrap()
            .iter()
            .all(|u| u.starts_with("file://")),
        "every byte came through the given Fetch"
    );

    let source = SwordSource::open(&sword).unwrap();
    let (resource, report) = import_sword(&source, "ObsKjvOsis", &Default::default()).unwrap();
    assert!(report.verses > 0);
    assert_eq!(resource.manifest.abbreviation, "KJV-O");
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_progress_callback_returning_false_cancels_and_installs_nothing() {
    let (root, repo) = mock_repo("cancel");
    let inst = installer(Arc::new(LocalFetch), Arc::new(|_, _| true));
    let catalog = inst.catalog(&repo).unwrap();
    let cancelling = installer(Arc::new(LocalFetch), Arc::new(|_, _| false));
    let sword = root.join("sword");
    let err = cancelling
        .install(&catalog, "ObsKjvOsis", &sword)
        .unwrap_err();
    assert!(is_cancelled(&err), "{err}");
    let left = SwordSource::open(&sword)
        .map(|s| s.modules().len())
        .unwrap_or(0);
    assert_eq!(left, 0, "no module half-installed");
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn local_fetch_never_reaches_the_network() {
    let err = LocalFetch
        .get(
            "https://crosswire.org/ftpmirror/pub/sword/masterRepoList.conf",
            &mut |_, _| true,
        )
        .unwrap_err();
    assert!(matches!(err, FetchError::Network(m) if m.contains("no network transport")));
    let missing = file_url(&std::env::temp_dir().join("nope.conf"));
    assert_eq!(
        LocalFetch.get(&missing, &mut |_, _| true).unwrap_err(),
        FetchError::NotFound
    );
}
