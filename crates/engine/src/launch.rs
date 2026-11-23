//! The Session as the desktop app opens it, whichever UI draws it (ADR 0025): the library, the
//! User database, the Starter library, the caches and the Log folder. The UI then restores its
//! Workspace on top. Both `obs-ui-egui` and `obs-ui-gpui` start from [`open_session`].

use std::path::Path;

use crate::{JobMode, Session};

/// What [`open_session`] made: the Session and what the UI shows about it.
pub struct Launch {
    pub session: Session,
    /// Files and databases that could not be read, in the order met; the UI shows the first.
    pub problems: Vec<String>,
    /// The Starter library has no bundle here and was never added: offer its download (#131).
    pub offer_starter: bool,
    /// The Resource of the file given on the command line, to show on top of the restored
    /// Workspace.
    pub startup_resource: Option<String>,
}

/// The Session with the library (the default library folder) and, if given, a Resource file
/// added to it. Like any Resource, that file takes its place in the Preferred order: at the
/// bottom of its type unless the order already ranks it.
pub fn open_session(file: Option<&Path>) -> Launch {
    #[cfg_attr(target_arch = "wasm32", allow(unused_mut))]
    let mut session = Session::new(JobMode::default_for_host());
    let mut problems = Vec::new();
    #[cfg(not(target_arch = "wasm32"))]
    {
        if let Some(p) = file
            && let Err(e) = session.library.add_file(p)
        {
            problems.push(format!("{}: {e}", p.display()));
        }
        if let Some(dir) = crate::default_library_dir() {
            for (f, e) in session.library.scan(&dir) {
                problems.push(format!("{}: {e}", f.display()));
            }
        }
        // Get Bibles opens on the last fetched catalog, even offline (#91).
        if let Some(cache) = crate::get_bibles::default_catalog_cache() {
            session.get_bibles.set_cache_path(cache);
        }
        // Search indexes (#58): Imports build theirs into the cache folder (this installs the
        // Import indexer); missing or stale ones rebuild in the background.
        if let Some(dir) = crate::default_search_cache_dir() {
            session.set_search_cache_dir(dir);
        }
        session.refresh_search_indexes();
        // People & places ↔ Dictionary aliases (#54): a derived cache per pair, rebuilt in the
        // background when either Resource changed (or used prebuilt for the Starter).
        if let Some(dir) = crate::aliases::default_alias_cache_dir() {
            session.set_alias_cache_dir(dir);
        }
        session.refresh_alias_caches();
        // Help → Open log folder and Report a problem (#219).
        if let Some(dir) = crate::diagnostics::default_log_dir() {
            session.set_log_dir(dir);
        }
    }
    #[cfg(target_arch = "wasm32")]
    if let Some(p) = file {
        problems.push(format!(
            "{}: opening files is not supported here",
            p.display()
        ));
    }
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(db) = crate::default_user_db() {
        match obs_store::UserStore::open(&db) {
            Ok((store, _)) => session.attach_user(Box::new(store)),
            Err(e) => problems.push(format!("User database {}: {e}", db.display())),
        }
    }
    // The Starter library (#131): added from the bundle on the first run (newer revisions and
    // new Starter Resources later); without a bundle, offered for download once.
    #[cfg(not(target_arch = "wasm32"))]
    let offer_starter = {
        session
            .starter
            .set_download_base(&crate::starter::default_download_base());
        let sync = session.sync_starter_library(crate::starter::find_starter_bundle().as_deref());
        problems.extend(sync.problems);
        sync.offer_download
    };
    #[cfg(target_arch = "wasm32")]
    let offer_starter = false;
    #[cfg(not(target_arch = "wasm32"))]
    let startup_resource = file.and_then(|p| {
        session
            .library
            .entries()
            .iter()
            .find(|e| e.path.as_path() == p)
            .map(|e| e.manifest.resource_id.clone())
    });
    #[cfg(target_arch = "wasm32")]
    let startup_resource = None;
    Launch {
        session,
        problems,
        offer_starter,
        startup_resource,
    }
}

/// Update checks (#276, ADR 0017): this executable's install, whose update folder is cleaned
/// off the UI thread once the old version has closed, and the release source
/// (`OBS_UPDATE_URL` names a mock one). Releases come through the Session's Fetch transport;
/// the UI arms the launch check.
#[cfg(not(target_arch = "wasm32"))]
pub fn start_update_checks(session: &mut Session) {
    use crate::updates::{
        CLEAN_UP_PATIENCE, Install, UpdateSource, clean_up_update_folder_at_launch,
    };
    session.set_update_source(UpdateSource::from_env());
    if let Some(install) = Install::current() {
        let dir = install.dir.clone();
        let _ = std::thread::Builder::new()
            .name("update clean-up".into())
            .spawn(move || clean_up_update_folder_at_launch(&dir, CLEAN_UP_PATIENCE));
        session.set_install(install);
    }
}
