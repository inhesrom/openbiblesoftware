//! The running Mobile app: boot over [`Paths`] and the Bundled set, the library it reads, and
//! what went wrong on the way.

use std::collections::HashMap;
use std::sync::Arc;

use obs_engine::Session;
use obs_model::{Direction, ResourceType};
use obs_reading::ReadingDoc;
use obs_render::GlyphAtlas;

use crate::Paths;
use crate::events::EventState;
use crate::readers::{Reader, ReaderId};

/// Why a [`MobileApp`] call failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MobileError {
    /// A folder of the [`Paths`] could not be made.
    Folder {
        path: std::path::PathBuf,
        error: String,
    },
    /// No Resource of that id is in the library.
    NoResource(String),
    /// The Resource's file could not be read.
    Open { resource: String, error: String },
    /// A typed reference or Key could not be read, or the Resource does not have it.
    Reference { reference: String, error: String },
    /// No reader has that id.
    NoReader(ReaderId),
    /// A tile must have a size and a scale above zero, and at most [`crate::MAX_TILE_PX`]
    /// device pixels on a side.
    TileSize,
    /// What the engine said when it could not do it.
    Failed(String),
}

impl std::fmt::Display for MobileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MobileError::Folder { path, error } => {
                write!(f, "cannot make {}: {error}", path.display())
            }
            MobileError::NoResource(id) => write!(f, "no Resource {id} in the library"),
            MobileError::Open { resource, error } => write!(f, "cannot open {resource}: {error}"),
            MobileError::Reference { reference, error } => write!(f, "“{reference}”: {error}"),
            MobileError::NoReader(r) => write!(f, "no reader {}", r.0),
            MobileError::TileSize => write!(
                f,
                "a tile needs a size and a scale above zero, and at most {} px a side",
                crate::MAX_TILE_PX
            ),
            MobileError::Failed(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for MobileError {}

/// A command's failure, in the engine's words (without the command id).
pub(crate) fn failed(e: obs_engine::CommandError) -> MobileError {
    MobileError::Failed(match e {
        obs_engine::CommandError::Failed { message, .. }
        | obs_engine::CommandError::InvalidParams { message, .. } => message,
        other => other.to_string(),
    })
}

/// One Resource of the library, as the library list shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryResource {
    pub resource_id: String,
    pub title: String,
    pub abbreviation: String,
    pub resource_type: ResourceType,
    /// BCP 47 tag (`en`, `hbo`, `grc`).
    pub language: String,
    pub right_to_left: bool,
}

/// The running Mobile app: a Session over the app's library and User database, its readers
/// (at most [`crate::MAX_READERS`], each one reading Tab of the engine's Workspace), the
/// documents they read and the glyph atlas their tiles are drawn from. One per process: every
/// scene of the shell shares it.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub struct MobileApp {
    pub(crate) paths: Paths,
    pub(crate) session: Session,
    pub(crate) readers: Vec<Reader>,
    /// Each Resource opened this run, by id: a reader that goes back to one opens it once.
    pub(crate) docs: HashMap<String, Arc<ReadingDoc>>,
    /// Bumped when every tile must be drawn again.
    pub(crate) epoch: u32,
    /// Whether the system appearance is dark, as the last frame said (the Auto Page tone and
    /// the Follow system Theme follow it).
    pub(crate) dark: bool,
    pub(crate) atlas: GlyphAtlas,
    pub(crate) problems: Vec<String>,
    /// Threads that measure a document's height estimates when it opens (cold start).
    pub(crate) estimate_threads: usize,
    pub(crate) events: EventState,
    /// The Mobile Last session as last written, to skip writes that change nothing.
    pub(crate) saved_session: Option<String>,
    /// The downloads planned this run (`library`).
    pub(crate) downloads: Vec<crate::library::Download>,
}

impl MobileApp {
    /// Start over `paths` and the Bundled set folder `bundle`, as the desktop starts
    /// (`obs_ui_egui::App::with_file`), with jobs on worker threads: make the folders, scan
    /// the library, set the Search index, alias and log folders, open (or create) the User
    /// database, add the Starter library from the bundle on the first run, then open the
    /// readers of the Mobile Last session, or the first Bible in Preferred order at its start.
    /// What went wrong without stopping it is in [`Self::problems`].
    #[cfg(not(target_arch = "wasm32"))]
    pub fn boot(paths: Paths, bundle: Option<&std::path::Path>) -> Result<Self, MobileError> {
        Self::boot_with(paths, bundle, obs_engine::JobMode::default_for_host())
    }

    /// [`Self::boot`] with the job queue's mode (tests run jobs inline, [`Self::pump`] runs
    /// them).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn boot_with(
        paths: Paths,
        bundle: Option<&std::path::Path>,
        jobs: obs_engine::JobMode,
    ) -> Result<Self, MobileError> {
        for dir in paths.folders() {
            std::fs::create_dir_all(&dir).map_err(|e| MobileError::Folder {
                path: dir.clone(),
                error: e.to_string(),
            })?;
        }
        let mut session = Session::new(jobs);
        let mut problems: Vec<String> = (session.library.scan(&paths.library).into_iter())
            .map(|(f, e)| format!("{}: {e}", f.display()))
            .collect();
        session.set_search_cache_dir(paths.search_cache.clone());
        session.refresh_search_indexes();
        session.set_alias_cache_dir(paths.alias_cache.clone());
        session.refresh_alias_caches();
        session.set_log_dir(paths.logs.clone());
        match obs_store::UserStore::open(&paths.user_db) {
            Ok((store, _)) => session.attach_user(Box::new(store)),
            Err(e) => problems.push(format!("User database {}: {e}", paths.user_db.display())),
        }
        // The Mobile app's Starter library rows come from the released manifest (#333); the
        // Bundled set installs and revises only what it holds.
        session.starter.set_released_leads(true);
        problems.extend(session.sync_starter_library(bundle).problems);
        if let Some(dir) = &paths.study_set {
            problems.extend(add_study_set(&mut session, dir));
        }
        let estimate_threads = std::thread::available_parallelism()
            .map_or(1, |n| n.get())
            .clamp(1, 4);
        let mut app = Self {
            paths,
            session,
            readers: Vec::new(),
            docs: HashMap::new(),
            epoch: 0,
            dark: false,
            atlas: GlyphAtlas::new(),
            problems,
            estimate_threads,
            events: EventState::default(),
            saved_session: None,
            downloads: Vec::new(),
        };
        app.restore_readers();
        // What boot itself queued (a message from the User database, the readers' Visits)
        // is the pump's to report.
        app.events.seen_library = app.session.library.preferred_order();
        app.events.seen_history = app.session.history().first().map(|v| v.id);
        app.events.seen_starter = app.starter_seen();
        Ok(app)
    }

    pub fn paths(&self) -> &Paths {
        &self.paths
    }

    /// The engine's version, as the desktop's About shows it.
    pub fn engine_version() -> &'static str {
        obs_engine::updates::APP_VERSION
    }

    /// What went wrong at [`Self::boot`] without stopping it.
    pub fn problems(&self) -> &[String] {
        &self.problems
    }

    /// The library's Resources, in Preferred order.
    pub fn library(&self) -> Vec<LibraryResource> {
        let library = &self.session.library;
        (library.preferred_order().iter())
            .filter_map(|id| library.get(id))
            .map(|e| LibraryResource {
                resource_id: e.manifest.resource_id.clone(),
                title: e.manifest.title.clone(),
                abbreviation: e.manifest.abbreviation.clone(),
                resource_type: e.manifest.resource_type,
                language: e.manifest.language.clone(),
                right_to_left: e.manifest.direction == Direction::Rtl,
            })
            .collect()
    }

    /// The document of Resource `id`, opened once a run.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn doc(&mut self, id: &str) -> Result<Arc<ReadingDoc>, MobileError> {
        if let Some(doc) = self.docs.get(id) {
            return Ok(Arc::clone(doc));
        }
        let entry = (self.session.library.get(id))
            .ok_or_else(|| MobileError::NoResource(id.to_string()))?;
        let doc = ReadingDoc::open(&entry.path).map_err(|e| MobileError::Open {
            resource: id.to_string(),
            error: e.to_string(),
        })?;
        let doc = Arc::new(doc);
        self.docs.insert(id.to_string(), Arc::clone(&doc));
        Ok(doc)
    }

    /// The document of Resource `id`: without file access (wasm), only one opened already.
    #[cfg(target_arch = "wasm32")]
    pub(crate) fn doc(&mut self, id: &str) -> Result<Arc<ReadingDoc>, MobileError> {
        (self.docs.get(id).cloned()).ok_or_else(|| MobileError::NoResource(id.to_string()))
    }
}

/// Add the Study set Resources that `dir`'s `starter-manifest.json` lists to the library, read
/// in place (the debug Study set override). Returns what went wrong.
#[cfg(not(target_arch = "wasm32"))]
fn add_study_set(session: &mut Session, dir: &std::path::Path) -> Vec<String> {
    let manifest = match std::fs::read_to_string(dir.join("starter-manifest.json")) {
        Ok(json) => obs_engine::starter::StarterManifest::parse(&json),
        Err(e) => Err(e.to_string()),
    };
    let manifest = match manifest {
        Ok(m) => m,
        Err(e) => return vec![format!("Study set {}: {e}", dir.display())],
    };
    let mut problems = Vec::new();
    for item in &manifest.resources {
        if item.mobile_set.as_deref() != Some("study")
            || session.library.get(&item.resource_id).is_some()
        {
            continue;
        }
        let file = dir.join(format!("{}.obr", item.resource_id));
        if let Err(e) = session.library.add_file(&file) {
            problems.push(format!("{}: {e}", file.display()));
        }
    }
    problems
}
