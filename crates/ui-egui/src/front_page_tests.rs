//! The README's front page (#296): its hero image and one image per feature, drawn by the
//! Screenshot gallery like every other surface (section "Front page"), so a UI change updates
//! them through the goldens (ADR 0019) rather than by hand.
//!
//! They are drawn with a library of their own: excerpts of the real sources the Starter
//! library is built from (the KJV with Strong's numbers, the World English Bible, Matthew
//! Henry on Genesis 1, the STEPBible lexicons and Hebrew text, Brown-Driver-Briggs,
//! OpenBible.info's cross-references, TIPNR and Theographic), under their real titles, so the
//! README shows the app as a reader sees it and not the test fixtures of the other sections.

use std::path::Path;

use egui::Key;
use obs_engine::TabId;
use obs_model::Resource;

use super::a11y_audit::{Overlay, key, type_text};
use super::missing_resource::settle;
use super::*;

fn asset(path: &str) -> String {
    let assets = Path::new(env!("CARGO_MANIFEST_DIR")).join("../formats/tests/assets");
    std::fs::read_to_string(assets.join(path)).unwrap()
}

/// A Bible of the USFM test excerpts in `usfm/<dir>/`.
fn usfm_bible(id: &str, title: &str, abbreviation: &str, dir: &str, books: &[&str]) -> Resource {
    let mut import = obs_formats::UsfmImport::new(obs_formats::UsfmOptions {
        resource_id: id.into(),
        title: title.into(),
        abbreviation: abbreviation.into(),
        language: "en".into(),
        source_file: dir.into(),
        tool_version: "test".into(),
        ..Default::default()
    });
    for book in books {
        import
            .add_book(book, asset(&format!("usfm/{dir}/{book}")))
            .unwrap();
    }
    let mut bible = import.finish().unwrap().0;
    bible.manifest.license = "LicenseRef-PublicDomain".into();
    bible
}

/// The STEPBible Hebrew text (TAHOT) of the test excerpt: Genesis 1:1-5 and a few verses of
/// Genesis to Deuteronomy, with Strong's numbers and morphology.
fn tahot() -> Resource {
    let mut import = obs_formats::StepbibleImport::new(obs_formats::StepbibleOptions {
        text: obs_formats::StepbibleText::Tahot,
        resource_id: "tahot".into(),
        title: "Hebrew Old Testament (STEPBible TAHOT)".into(),
        abbreviation: "TAHOT".into(),
        source_file: "tahot-gen-deu.txt".into(),
        tool_version: "test".into(),
        ..Default::default()
    });
    import.add_file("tahot-gen-deu.txt", asset("stepbible/tahot-gen-deu.txt"));
    let mut tahot = import.finish().unwrap().0;
    tahot.manifest.license = "CC-BY-4.0".into();
    tahot
}

/// Matthew Henry on Genesis 1:1-2 (public domain), as its Wikisource wikitext has it; the
/// text is the Starter build's own (`starter/goldens/mhc/gen-1-1.json`).
fn mhc_genesis_1() -> Resource {
    let mut manifest = obs_model::Manifest::new(
        "mhc",
        "Matthew Henry's Commentary on the Whole Bible",
        "MHC",
        obs_model::ResourceType::Commentary,
        "en",
    );
    manifest.license = "LicenseRef-PublicDomain".into();
    let mut import = obs_formats::StudyImport::new(manifest);
    obs_formats::import_mhc(
        &mut import,
        "<mediawiki><page><title>MHC/Volume 1/Genesis/Chapter 1</title><revision><text>\
         ==Verses 1-2==\n\
         The Creation. (b. c. 4004.)\n\n\
         '''{{sup|1}} In the beginning God created the heaven and the earth. {{sup|2}} And the \
         earth was without form, and void; and darkness was upon the face of the deep. And \
         the Spirit of God moved upon the face of the waters.'''\n\n\
         In these verses we have the work of creation in its epitome and in its embryo.\n\n\
         '''I.''' In its epitome, v. 1, where we find, to our comfort, the first article of \
         our creed, that God the Father Almighty is the Maker of heaven and earth, and as \
         such we believe in him.\n\n\
         '''1.''' Observe, in this verse, four things:—\n\n\
         (1.) The effect produced—the heaven and the earth, that is, the world, including the \
         whole frame and furniture of the universe, the world and all things therein, Acts \
         xvii. 24. The world is a great house, consisting of upper and lower stories, the \
         structure stately and magnificent, uniform and convenient, and every room well and \
         wisely furnished.\n\n\
         (2.) The author and cause of this great work—GOD. The Hebrew word is Elohim, which \
         bespeaks, [1.] The power of God the Creator. El signifies the strong God; and what \
         less than almighty strength could bring all things out of nothing? [2.] The \
         plurality of persons in the Godhead, Father, Son, and Holy Ghost.\n\n\
         (3.) The manner in which this work was effected: God created it, that is, made it \
         out of nothing. There was not any pre-existent matter out of which the world was \
         produced. The fish and fowl were indeed produced out of the waters and the beasts \
         and man out of the earth; but that earth and those waters were made out of \
         nothing.\n\n\
         (4.) When this work was produced: In the beginning, that is, in the beginning of \
         time, when that clock was first set a going: time began with the production of \
         those beings that are measured by time. Before the beginning of time there was \
         none but that Infinite Being that inhabits eternity.\n\
         </text></revision></page></mediawiki>",
    )
    .unwrap();
    import.finish().unwrap().0
}

/// The OpenBible.info cross-reference excerpt.
fn cross_references() -> Resource {
    let options = obs_formats::DatasetOptions {
        resource_id: "openbible-xrefs".into(),
        title: "OpenBible.info Cross References".into(),
        abbreviation: "OpenBible".into(),
        ..Default::default()
    };
    let mut xrefs = obs_formats::import_openbible_cross_references(
        &asset("openbible/cross-references-excerpt.txt"),
        &options,
    )
    .unwrap()
    .0;
    xrefs.manifest.license = "CC-BY-4.0".into();
    xrefs
}

/// The Front page's library, in the Library's order: the KJV first, so it is the Bible a
/// launch opens.
pub(super) fn library() -> Vec<Resource> {
    let mut set = vec![
        usfm_bible(
            "kjv-strongs",
            "King James Version with Strong's Numbers",
            "KJV",
            "kjv2006",
            &["GEN.usfm", "PSA.usfm", "JHN.usfm"],
        ),
        usfm_bible(
            "web",
            "World English Bible",
            "WEB",
            "web",
            &["GEN.usfm", "2KI.usfm", "PSA.usfm", "MAT.usfm", "3JN.usfm"],
        ),
        tahot(),
        mhc_genesis_1(),
        cross_references(),
    ];
    // Licences as `starter/sources.toml` gives them.
    for mut lexicon in obs_engine::testing::starter_lexicon_excerpts() {
        let id = lexicon.manifest.resource_id.as_str();
        lexicon.manifest.license = if id.starts_with("tbes") || id == "bdb" {
            "CC-BY-4.0".into()
        } else {
            "LicenseRef-PublicDomain".into()
        };
        set.push(lexicon);
    }
    let tipnr = obs_engine::testing::tipnr();
    set.push(obs_engine::testing::theographic(Some(&tipnr)));
    set.push(tipnr);
    for r in &mut set {
        r.manifest.origin = obs_model::Origin::Starter;
    }
    set
}

/// Add `resources` to the launch's library, replacing any of the same id.
fn add(h: &mut Harness, resources: &[Resource]) {
    let library = &mut h.shell.st.session.library;
    let dir = library.directory().unwrap().to_path_buf();
    for r in resources {
        let path = dir.join(format!("{}.obr", r.manifest.resource_id));
        obs_store::write_obr(r, &path).unwrap();
        library.add_file(&path).unwrap();
    }
}

fn focused_kind(h: &Harness) -> Option<&TabKind> {
    let ws = &h.shell.session().workspace;
    ws.focused().and_then(|t| ws.tab(t)).map(|t| &t.kind)
}

fn has_tab(h: &Harness, kind: &TabKind) -> bool {
    h.shell.session().workspace.tabs().any(|t| &t.kind == kind)
}

/// Verse `verse` of Genesis 1 selected in the Bible Tab `tab`.
fn select_verse(h: &mut Harness, tab: TabId, verse: u16) {
    let view = (h.shell.st.tabs.get_mut(&tab).unwrap().view.as_mut()).unwrap();
    let verse = obs_scripture::Verse::new(obs_scripture::Book::Genesis, 1, verse);
    let (unit, byte) = view.doc.verse_position(verse).unwrap();
    let range = view.doc.verse_range_at(unit, byte).unwrap();
    view.selection = Some(crate::Selection {
        anchor: crate::DocPos {
            unit,
            byte: range.start,
        },
        head: crate::DocPos {
            unit,
            byte: range.end,
        },
        granularity: Default::default(),
    });
    h.frames(2);
}

/// The Bible beside a Commentary, both following Genesis 1:1, and a Word Study of the Hebrew
/// word for "created" (בָּרָא, H1254) at the right, in reading order.
fn bible_commentary_and_word_study(h: &mut Harness) {
    h.shell
        .run("layout.builtin", &json!({ "name": "Bible + Commentary" }));
    settle(h);
    settle_all(h);
    // A split opens beside the focused Tab.
    let mhc = h.tab_id("MHC");
    h.shell.st.session.workspace.focus(mhc);
    h.shell.apply_effects();
    super::gallery::run_split(h, "guide.word_study", json!({ "strongs": "H1254" }));
}

/// Word Info on "created" in Genesis 1:1, the pointer resting on it.
fn word_info(h: &mut Harness) {
    let info = h.tab_id("Word Info");
    h.shell.st.session.workspace.focus(info);
    h.shell.apply_effects();
    h.frames(2);
    let kjv = h.tab_id("KJV");
    let created = super::word_info::word_spot(h, kjv, "created");
    h.events.push(Event::PointerMoved(created));
    h.frames(14);
    settle(h);
}

/// The README's images, in its order.
pub(super) fn front_page() -> Vec<Overlay> {
    vec![
        (
            "Front page: Bible, Commentary and Word Study",
            bible_commentary_and_word_study,
            |h| {
                focused_kind(h) == Some(&TabKind::Guide(obs_engine::Guide::WordStudy))
                    && has_tab(h, &TabKind::Resource("mhc".into()))
            },
        ),
        ("Front page: Word Info", word_info, |h| {
            h.shell.st.session.word_info.shown.is_some()
        }),
        (
            "Front page: Text Comparison",
            |h| {
                super::gallery::run_split(
                    h,
                    "tool.text_comparison",
                    json!({ "passage": "Gen 1:1-3" }),
                );
            },
            |h| focused_kind(h) == Some(&TabKind::Tool(obs_engine::Tool::TextComparison)),
        ),
        (
            "Front page: Interlinear",
            |h| {
                let tab = h
                    .shell
                    .run("resource.open", &json!({ "id": "tahot", "split": true }))
                    .unwrap()["tab"]
                    .as_u64()
                    .unwrap();
                h.shell
                    .run("tab.go", &json!({ "tab": tab, "text": "Gen 1:1" }));
                h.shell
                    .run("tab.interlinear", &json!({ "tab": tab, "on": true }));
                settle(h);
                settle_all(h);
            },
            |h| has_tab(h, &TabKind::Resource("tahot".into())),
        ),
        (
            "Front page: Exegetical Guide",
            |h| {
                super::gallery::run_split(
                    h,
                    "guide.exegetical_guide",
                    json!({ "passage": "Gen 1:1" }),
                );
            },
            |h| focused_kind(h) == Some(&TabKind::Guide(obs_engine::Guide::ExegeticalGuide)),
        ),
        (
            "Front page: Search",
            |h| {
                h.shell.st.session.refresh_search_indexes();
                super::gallery::settle_search(h);
                super::gallery::run_split(h, "tool.search", json!({}));
                let tab = h.shell.session().workspace.focused().unwrap();
                h.shell
                    .run("search.run", &json!({ "tab": tab.0, "text": "light" }));
                super::gallery::settle_search(h);
                settle_all(h);
            },
            |h| focused_kind(h) == Some(&TabKind::Tool(obs_engine::Tool::Search)),
        ),
        (
            "Front page: Notes and Highlights",
            |h| {
                let kjv = h.tab_id("KJV");
                select_verse(h, kjv, 1);
                key(h, Key::Num1, Modifiers::ALT);
                select_verse(h, kjv, 3);
                key(h, Key::N, Modifiers::COMMAND | Modifiers::SHIFT);
                type_text(
                    h,
                    "Light before the sun (v. 16): God himself is its source.",
                );
                key(h, Key::Escape, Modifiers::NONE);
                settle_all(h);
                super::gallery::run_split(h, "tool.notes", json!({}));
            },
            |h| focused_kind(h) == Some(&TabKind::Tool(obs_engine::Tool::Notes)),
        ),
        (
            "Front page: Atlas",
            |h| {
                let tipnr = obs_engine::testing::tipnr_acts_13();
                let geo = obs_engine::testing::openbible_geo(Some(&tipnr));
                add(h, &[tipnr, geo]);
                super::gallery::run_split(h, "tool.atlas", json!({ "passage": "Acts 13" }));
            },
            |h| focused_kind(h) == Some(&TabKind::Tool(obs_engine::Tool::Atlas)),
        ),
        (
            "Front page: Timeline",
            |h| {
                super::gallery::run_split(h, "tool.timeline", json!({ "passage": "Gen 1:1" }));
                // The whole Timeline, not only Creation.
                let fit = h.at("Fit");
                h.click(fit);
                h.frames(4);
            },
            |h| focused_kind(h) == Some(&TabKind::Tool(obs_engine::Tool::Timeline)),
        ),
        (
            "Front page: Home and a Reading plan",
            |h| {
                h.shell
                    .run("reading_plan.create", &json!({ "template": "nt-90-days" }));
                super::gallery::run_split(h, "home.open", json!({}));
            },
            |h| focused_kind(h) == Some(&TabKind::Home),
        ),
        (
            "Front page: Layouts",
            |h| key(h, Key::Y, Modifiers::ALT),
            |h| h.shell.st.toolbar.open == Some(obs_engine::Launcher::Layouts),
        ),
    ]
}
