//! The Mobile app (CONTEXT.md § Mobile) below its platform shells: what each screen shows and
//! what each action does, as plain Rust types (ADR 0021 rule 3). The shells reach it through
//! `apps/obs-ffi` only; this crate never depends on an FFI crate and compiles for wasm32
//! (`docs/dev/mobile-core.md`).
//!
//! S03 spike (`docs/plans/mobile-ios.md`): [`MobileApp::boot`], [`MobileApp::open_reader`] and
//! [`MobileApp::layout_tile`] prove the chain from a shell to tile pixels. S13 replaces them
//! with the designed surface (Readers, snapshots, events).

use std::path::PathBuf;

use obs_engine::Session;
use obs_reading::{ReadingView, TonePalette};
use obs_render::GlyphAtlas;

pub use obs_reading::{DocPos, Tile};

mod text_view;
pub use text_view::{
    AROUND, AccessibilityItem, BAND, BookChapters, Choice, ContentRect, ReaderFrame,
    StudySheetStub, TextSpan, TextUnitKind, TileKey, TilePlace, TimedTile,
    text_size_for_content_size,
};

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
    /// Prototype (plan S07), debug builds only: a Starter library folder (`dist/starter`)
    /// whose Study set Resources are read in place, so the prototype has Hebrew and Strong's
    /// tagging without the downloads S24 adds.
    pub study_set: Option<PathBuf>,
}

/// The most device pixels a tile may have on a side. A tile is `width × height × 4` bytes
/// allocated at once, and a failed allocation aborts the app, so a shell's bad size is refused
/// instead: 8192 px bounds a tile at 256 MiB, and is still several times the longest side of
/// an iPhone or iPad screen at its own scale. S03 spike.
#[doc(hidden)]
pub const MAX_TILE_PX: usize = 8192;

/// One reader of the running app: an index into its readers. S03 spike.
#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ReaderId(pub u32);

/// Why a [`MobileApp`] call failed. S03 spike.
#[doc(hidden)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MobileError {
    /// A folder of the [`Paths`] could not be made.
    Folder { path: PathBuf, error: String },
    /// No Resource of that id is in the library.
    NoResource(String),
    /// The Resource's file could not be read.
    Open { resource: String, error: String },
    /// The reference could not be read, or the Resource does not have it.
    Reference { reference: String, error: String },
    /// No reader has that id.
    NoReader(ReaderId),
    /// A tile must have a size and a scale above zero, and at most [`MAX_TILE_PX`] device
    /// pixels on a side.
    TileSize,
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
                "a tile needs a size and a scale above zero, and at most {MAX_TILE_PX} px a side"
            ),
        }
    }
}

impl std::error::Error for MobileError {}

/// The running Mobile app: a Session over the app's library and User database, its readers
/// and the glyph atlas their tiles are drawn from. S03 spike: S13 gives it the designed
/// surface.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub struct MobileApp {
    paths: Paths,
    session: Session,
    readers: Vec<ReadingView>,
    /// What the text view keeps for each reader, by the same index (S07 prototype).
    states: Vec<text_view::ReaderState>,
    /// Bumped when every tile must be drawn again (S07 prototype).
    epoch: u32,
    /// Whether the system appearance is dark, for the Auto Page tone (S07 prototype).
    dark: bool,
    atlas: GlyphAtlas,
    problems: Vec<String>,
}

impl MobileApp {
    /// Start as the desktop app does (`obs_ui_egui::App::with_file`): scan the library
    /// folder, set the Search index, alias and log folders, open (or create) the User
    /// database, add the Starter library from `bundle` (the app's Bundled set) on the first
    /// run, then open the first Bible in Preferred order as the first reader. What went wrong
    /// without stopping it is in [`Self::problems`]. S03 spike.
    #[doc(hidden)]
    #[cfg(not(target_arch = "wasm32"))]
    pub fn boot(paths: Paths, bundle: Option<&std::path::Path>) -> Result<Self, MobileError> {
        let user_db_dir = paths.user_db.parent().map(std::path::Path::to_path_buf);
        let folders = [
            Some(&paths.library),
            Some(&paths.search_cache),
            Some(&paths.alias_cache),
            Some(&paths.logs),
            Some(&paths.downloads),
            user_db_dir.as_ref(),
        ];
        for dir in folders.into_iter().flatten() {
            std::fs::create_dir_all(dir).map_err(|e| MobileError::Folder {
                path: dir.clone(),
                error: e.to_string(),
            })?;
        }
        let mut session = Session::new(obs_engine::JobMode::default_for_host());
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
        problems.extend(session.sync_starter_library(bundle).problems);
        if let Some(dir) = &paths.study_set {
            problems.extend(add_study_set(&mut session, dir));
        }
        let mut app = Self {
            paths,
            session,
            readers: Vec::new(),
            states: Vec::new(),
            epoch: 0,
            dark: false,
            atlas: GlyphAtlas::new(),
            problems,
        };
        let first_bible = (app.session.library)
            .preferred(obs_model::ResourceType::Bible)
            .map(|e| e.manifest.resource_id.clone());
        if let Some(id) = first_bible
            && let Err(e) = app.open_reader(&id, None)
        {
            app.problems.push(e.to_string());
        }
        Ok(app)
    }

    pub fn paths(&self) -> &Paths {
        &self.paths
    }

    /// The engine's version, as the desktop's About shows it.
    pub fn engine_version() -> &'static str {
        obs_engine::updates::APP_VERSION
    }

    /// What went wrong at [`Self::boot`] without stopping it. S03 spike.
    #[doc(hidden)]
    pub fn problems(&self) -> &[String] {
        &self.problems
    }

    /// The ids of the Resources in the library, in Preferred order. S03 spike.
    #[doc(hidden)]
    pub fn resource_ids(&self) -> Vec<String> {
        self.session.library.preferred_order()
    }

    /// The Resource a reader shows. S03 spike.
    #[doc(hidden)]
    pub fn reader_resource(&self, reader: ReaderId) -> Option<&str> {
        let view = self.readers.get(reader.0 as usize)?;
        Some(view.doc.manifest.resource_id.as_str())
    }

    /// A new reader of Resource `resource_id`, at `reference` (read in the Resource's own
    /// numbering; its first Verse goes to the top) or at its start. S03 spike.
    #[doc(hidden)]
    #[cfg(not(target_arch = "wasm32"))]
    pub fn open_reader(
        &mut self,
        resource_id: &str,
        reference: Option<&str>,
    ) -> Result<ReaderId, MobileError> {
        let entry = (self.session.library.get(resource_id))
            .ok_or_else(|| MobileError::NoResource(resource_id.to_string()))?;
        let doc = obs_reading::ReadingDoc::open(&entry.path).map_err(|e| MobileError::Open {
            resource: resource_id.to_string(),
            error: e.to_string(),
        })?;
        let mut view = ReadingView::new(
            std::sync::Arc::new(doc),
            self.session.text_settings_of(resource_id),
        );
        if let Some(text) = reference {
            let refused = |error: String| MobileError::Reference {
                reference: text.to_string(),
                error,
            };
            let list = obs_scripture::parse_reference(text, &view.doc.versification, None)
                .map_err(|e| refused(e.to_string()))?;
            let verse = (list.passages().first())
                .map(|p| p.start())
                .ok_or_else(|| refused("no Passage".into()))?;
            if !view.jump_to(verse) {
                return Err(refused(format!("not in {resource_id}")));
            }
        }
        let id = ReaderId(self.readers.len() as u32);
        self.readers.push(view);
        self.states.push(text_view::ReaderState::new());
        Ok(id)
    }

    /// The tile of `reader`'s page, `width` points wide, `height` points tall from `top`
    /// points below its Current location, at `scale` device px per point, in the Page tone
    /// the User database has (Auto as in a light Theme). Refused when it is more than
    /// [`MAX_TILE_PX`] device pixels on a side. S03 spike.
    #[doc(hidden)]
    pub fn layout_tile(
        &mut self,
        reader: ReaderId,
        top: f32,
        width: f32,
        height: f32,
        scale: f32,
    ) -> Result<Tile, MobileError> {
        let positive = |x: f32| x.is_finite() && x > 0.0;
        if !(positive(width) && positive(height) && positive(scale) && top.is_finite()) {
            return Err(MobileError::TileSize);
        }
        let (w, h) = obs_render::tile_size([0.0, top, width, top + height], scale);
        if w > MAX_TILE_PX || h > MAX_TILE_PX {
            return Err(MobileError::TileSize);
        }
        let appearance = self.session.appearance;
        let palette = TonePalette::for_theme(
            appearance.page_tone.resolve(false),
            appearance.theme.is_high_contrast(),
        );
        let view =
            (self.readers.get_mut(reader.0 as usize)).ok_or(MobileError::NoReader(reader))?;
        Ok(obs_reading::compose_tile(
            view,
            &mut self.atlas,
            &palette,
            width,
            top,
            height,
            scale,
        ))
    }
}

/// Add the Study set Resources that `dir`'s `starter-manifest.json` lists to the library, read
/// in place (S07 prototype: the debug Study set override). Returns what went wrong.
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

#[cfg(test)]
mod tests {
    use std::path::Path;

    use obs_engine::testing::bibles::{bsb_john_1, tahot};
    use obs_engine::testing::write_starter_bundle;

    use super::*;

    fn paths(root: &Path) -> Paths {
        Paths {
            library: root.join("library"),
            user_db: root.join("db/user.db"),
            search_cache: root.join("search"),
            alias_cache: root.join("aliases"),
            logs: root.join("logs"),
            downloads: root.join("downloads"),
            study_set: None,
        }
    }

    /// A fresh folder for one test.
    fn root(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("obs-mobile-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    /// A Bundled set of the BSB's John 1 and a TAHOT excerpt, as `cargo xtask ios
    /// bundled-set` lays one out.
    fn bundle(root: &Path) -> PathBuf {
        let dir = root.join("bundle");
        let mut set = vec![bsb_john_1(), tahot()];
        for r in &mut set {
            r.manifest.origin = obs_model::Origin::Starter;
        }
        write_starter_bundle(&dir, &set);
        dir
    }

    #[test]
    fn boot_adds_the_bundled_set_and_opens_the_first_bible() {
        let root = root("boot");
        let bundle = bundle(&root);
        let app = MobileApp::boot(paths(&root), Some(&bundle)).unwrap();
        assert!(app.problems().is_empty(), "{:?}", app.problems());
        assert_eq!(app.paths(), &paths(&root));
        let mut ids = app.resource_ids();
        ids.sort();
        assert_eq!(ids, ["bsb", "tahot"]);
        assert!(root.join("library/bsb.obr").is_file());
        assert!(root.join("db/user.db").is_file());
        let first = app.reader_resource(ReaderId(0)).unwrap();
        assert!(ids.contains(&first.to_string()), "{first}");
        assert_eq!(app.reader_resource(ReaderId(1)), None);

        // A second launch finds the library it filled.
        drop(app);
        let again = MobileApp::boot(paths(&root), Some(&bundle)).unwrap();
        assert!(again.problems().is_empty(), "{:?}", again.problems());
        assert_eq!(again.resource_ids().len(), 2);
    }

    #[test]
    fn a_reader_at_a_reference_lays_out_a_phone_tile_of_its_text() {
        let root = root("tile");
        let mut app = MobileApp::boot(paths(&root), Some(&bundle(&root))).unwrap();
        let reader = app.open_reader("bsb", Some("John 1:1")).unwrap();
        assert_eq!(app.reader_resource(reader), Some("bsb"));
        let tile = app.layout_tile(reader, 0.0, 390.0, 256.0, 3.0).unwrap();
        assert_eq!((tile.width, tile.height), (1170, 768));
        let [r, g, b] = TonePalette::PAPER.ink.0;
        let ink = (tile.rgba.as_chunks::<4>().0.iter().copied())
            .filter(|p| *p == [r, g, b, 255])
            .count();
        assert!(ink > 5_000, "{ink} pixels of ink");
        // The Hebrew Bible lays out too.
        let hebrew = app.open_reader("tahot", None).unwrap();
        let tile = app.layout_tile(hebrew, 0.0, 390.0, 256.0, 2.0).unwrap();
        assert_eq!((tile.width, tile.height), (780, 512));
        assert!(
            tile.rgba
                .as_chunks::<4>()
                .0
                .iter()
                .copied()
                .any(|p| p == [r, g, b, 255])
        );
    }

    #[test]
    fn bad_resources_references_readers_and_tiles_are_refused() {
        let root = root("refused");
        let mut app = MobileApp::boot(paths(&root), Some(&bundle(&root))).unwrap();
        assert_eq!(
            app.open_reader("nasb", None),
            Err(MobileError::NoResource("nasb".into()))
        );
        assert!(matches!(
            app.open_reader("bsb", Some("Hezekiah 4")),
            Err(MobileError::Reference { .. })
        ));
        // The excerpt has John 1 and Isaiah 40 only.
        assert!(matches!(
            app.open_reader("bsb", Some("Gen 1")),
            Err(MobileError::Reference { .. })
        ));
        assert_eq!(
            app.layout_tile(ReaderId(9), 0.0, 390.0, 256.0, 3.0),
            Err(MobileError::NoReader(ReaderId(9)))
        );
        for (w, h, s) in [
            (0.0, 256.0, 3.0),
            (390.0, -1.0, 3.0),
            (390.0, 256.0, f32::NAN),
            // Over MAX_TILE_PX on a side, up to absurd but finite sizes.
            (2731.0, 256.0, 3.0),
            (390.0, 2731.0, 3.0),
            (1e20, 256.0, 3.0),
            (390.0, f32::MAX, 3.0),
        ] {
            assert_eq!(
                app.layout_tile(ReaderId(0), 0.0, w, h, s),
                Err(MobileError::TileSize)
            );
        }
        // MAX_TILE_PX itself is allowed.
        let edge = app.layout_tile(ReaderId(0), 0.0, 4096.0, 1.0, 2.0).unwrap();
        assert_eq!((edge.width as usize, edge.height), (MAX_TILE_PX, 2));
    }

    #[test]
    fn boot_without_a_bundle_or_a_bible_has_no_reader() {
        let root = root("empty");
        let app = MobileApp::boot(paths(&root), None).unwrap();
        assert!(app.resource_ids().is_empty());
        assert_eq!(app.reader_resource(ReaderId(0)), None);
        assert_eq!(
            MobileApp::engine_version(),
            obs_engine::updates::APP_VERSION
        );
    }
}
