//! Empty states that say how to get the missing Resource (#135), through the shell's
//! real-event headless harness: with an empty Library, every Guide section, the
//! Cross-references Tool and the Word Study chooser show buttons — Get Resources at the right
//! filter, the Starter library, Import a file… — and an open Guide fills in once the Resource
//! arrives, without being reopened.

use std::path::PathBuf;
use std::sync::Arc;

use obs_engine::get_bibles::ResourceFilter;
use obs_engine::testing::{MockRepos, starter_set, write_starter_bundle};
use obs_engine::{GuideSection, Library, TabId};
use obs_model::ResourceType;
use obs_scripture::{Passage, Verse};

use super::*;

const ADD_STARTER: &str = "Add the Starter library…";
const IMPORT: &str = "Import a file…";
const TBESG: &str = "Brief lexicon of Extended Strongs for Greek";
/// The mock CrossWire Lexicon with G26 (`StrongsGreek`).
const STRONGS_GREEK: &str = "Strong's Greek Bible Dictionary";

pub(super) struct Lab {
    pub(super) root: PathBuf,
    _repos: Arc<MockRepos>,
}

impl Drop for Lab {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// A launch with an empty Library folder, mock SWORD repositories for Get Resources, and a
/// released Starter library (no bundle) at a `file://` URL. `resources` are in the Library.
pub(super) fn setup(name: &str, resources: &[obs_model::Resource]) -> (Harness, Lab) {
    let root =
        std::env::temp_dir().join(format!("obs-shell-missing-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("library")).unwrap();
    let repos = Arc::new(MockRepos::new(&root.join("repos")));
    let release = root.join("release");
    write_starter_bundle(&release, &release_set());
    let mut h = Harness::launch(Some(&root.join("user.db")));
    let s = &mut h.shell.st.session;
    s.set_fetch(repos.clone());
    s.get_bibles.set_cache_path(root.join("sword-catalog.json"));
    s.get_bibles.set_sword_dir(root.join("sword"));
    s.set_search_cache_dir(root.join("cache"));
    s.starter
        .set_download_base(&obs_engine::sword_repo::file_url(&release));
    s.library = Library::new();
    s.library.set_directory(root.join("library"));
    for r in resources {
        let path = root
            .join("library")
            .join(format!("{}.obr", r.manifest.resource_id));
        obs_store::write_obr(r, &path).unwrap();
        s.library.add_file(&path).unwrap();
    }
    h.shell.reset_layout();
    h.frames(2);
    (
        h,
        Lab {
            root,
            _repos: repos,
        },
    )
}

/// The OpenBible.info Cross-reference excerpt.
fn cross_references() -> obs_model::Resource {
    let options = obs_formats::DatasetOptions {
        resource_id: "openbible-xrefs".into(),
        title: "OpenBible.info Cross-references".into(),
        ..Default::default()
    };
    obs_formats::import_openbible_cross_references(
        include_str!("../../formats/tests/assets/openbible/cross-references-excerpt.txt"),
        &options,
    )
    .unwrap()
    .0
}

/// A released Starter library with a Resource for every Guide section: the KJV with Strong's
/// and the Lexicons of [`starter_set`], a Commentary, the Cross-reference set, TIPNR and
/// Greek Bibles with Morphology (G26 in one of them).
pub(super) fn release_set() -> Vec<obs_model::Resource> {
    let john = Passage::verse(Verse::new(obs_scripture::Book::John, 3, 16));
    let mut set = starter_set();
    set.extend([
        obs_engine::testing::commentary(
            "mhc",
            "Matthew Henry (test)",
            "MHC",
            &[(john, None, "God so loved the world.")],
        ),
        cross_references(),
        obs_engine::testing::bsb_parallels(),
        obs_engine::testing::tipnr(),
        obs_engine::testing::tagnt_grace(),
        obs_engine::testing::tagnt_love(),
    ]);
    for r in &mut set {
        r.manifest.origin = obs_model::Origin::Starter;
        if r.manifest.license.is_empty() {
            r.manifest.license = "LicenseRef-PublicDomain".into();
        }
    }
    set
}

/// Run queued jobs (inline mode), apply their results and draw.
pub(super) fn settle(h: &mut Harness) {
    for _ in 0..30 {
        h.shell.st.session.jobs().pump(64);
        h.frames(2);
    }
}

fn pane(h: &Harness, tab: TabId) -> Option<Rect> {
    let tiles = &h.shell.tree.tiles;
    tiles.rect(tiles.find_pane(&tab)?)
}

fn find_in(h: &Harness, tab: TabId, text: &str) -> Option<Rect> {
    let r = pane(h, tab)?;
    h.shapes
        .iter()
        .find(|(s, t)| t == text && r.contains(s.center()))
        .map(|(s, _)| *s)
}

/// Frames (running jobs) until `done` holds.
fn until(h: &mut Harness, what: &str, done: impl Fn(&Harness) -> bool) {
    for _ in 0..300 {
        if done(h) {
            return;
        }
        h.shell.st.session.jobs().pump(16);
        h.frames(1);
    }
    panic!("never saw {what}: {:?}", h.texts());
}

/// `label` inside `section` of a Guide: below its header, above the next one.
fn in_section(h: &Harness, tab: TabId, section: GuideSection, label: &str) -> Option<Rect> {
    let r = pane(h, tab)?;
    let header = find_in(h, tab, section.title())?;
    let next = h
        .shapes
        .iter()
        .filter(|(s, t)| {
            GuideSection::ALL.iter().any(|g| g.title() == t)
                && r.contains(s.center())
                && s.top() > header.bottom()
        })
        .map(|(s, _)| s.top())
        .fold(r.bottom(), f32::min);
    h.shapes
        .iter()
        .find(|(s, t)| {
            t == label
                && r.contains(s.center())
                && s.center().y > header.bottom()
                && s.center().y < next
        })
        .map(|(s, _)| *s)
}

fn open_tab_of(h: &Harness, kind: &TabKind) -> Option<TabId> {
    h.shell
        .session()
        .workspace
        .tabs()
        .find(|t| t.kind == *kind)
        .map(|t| t.id)
}

/// Close every Tab of `kind` so the Guide is in view again.
fn close(h: &mut Harness, kind: &TabKind) {
    while let Some(tab) = open_tab_of(h, kind) {
        h.shell.run("tab.close", &json!({ "tab": tab.0 }));
        h.frames(2);
    }
}

fn open_word_study(h: &mut Harness, strongs: &str) -> TabId {
    let tab = h
        .shell
        .run(
            "guide.word_study",
            &json!({ "strongs": strongs, "split": true }),
        )
        .unwrap()["tab"]
        .as_u64()
        .unwrap();
    h.frames(2);
    TabId(tab)
}

const LEXICON_HINT: &str = "None in your library. Add a Lexicon to read its entry for this word.";

/// Acceptance: an empty Library's Word Study on G26 offers Get Lexicons…; one more click
/// downloads a Lexicon, and the open Word Study shows its entry without being reopened.
#[test]
fn word_study_g26_with_an_empty_library_gets_a_lexicon_in_one_more_click_and_refreshes() {
    let (mut h, _lab) = setup("get-lexicons", &[]);
    let study = open_word_study(&mut h, "G26");
    until(&mut h, "the Lexicon entries hint", |h| {
        find_in(h, study, LEXICON_HINT).is_some()
    });
    let lexicons = GuideSection::LexiconEntries;
    for label in ["Get Lexicons…", ADD_STARTER, IMPORT] {
        assert!(
            in_section(&h, study, lexicons, label).is_some(),
            "no {label:?} under Lexicon entries: {:?}",
            h.texts()
        );
    }
    if std::env::var_os("UPDATE_PREVIEWS").is_some() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/previews");
        h.frames(10);
        h.screenshot(&dir.join("word-study-empty-library.ppm"));
    }
    let at = in_section(&h, study, lexicons, "Get Lexicons…")
        .unwrap()
        .center();
    h.click(at);
    settle(&mut h);
    // Get Resources, at Lexicons.
    assert!(open_tab_of(&h, &TabKind::GetBibles).is_some());
    assert_eq!(
        h.shell.st.session.get_bibles.filter(),
        ResourceFilter::Type(ResourceType::Lexicon)
    );
    assert!(h.find("ALL LEXICONS · 1").is_some(), "{:?}", h.texts());
    // One more click: Download.
    let row = h
        .find(STRONGS_GREEK)
        .unwrap_or_else(|| panic!("{:?}", h.texts()));
    let download = h
        .shapes
        .iter()
        .find(|(r, t)| t == "Download" && (r.center().y - row.center().y).abs() < 40.0)
        .map(|(r, _)| r.center())
        .unwrap_or_else(|| panic!("no Download: {:?}", h.texts()));
    h.click(download);
    settle(&mut h);
    assert_eq!(
        h.shell
            .st
            .session
            .library
            .of_type(ResourceType::Lexicon)
            .count(),
        1
    );
    // The same Word Study Tab, never reopened, now shows the entry.
    close(&mut h, &TabKind::GetBibles);
    close(&mut h, &TabKind::ImportReport("strongsgreek-1".into()));
    until(&mut h, "the downloaded Lexicon's G26 entry", |h| {
        find_in(h, study, LEXICON_HINT).is_none()
            && in_section(h, study, lexicons, "Get Lexicons…").is_none()
            && find_in(h, study, STRONGS_GREEK).is_some()
    });
    assert!(h.shell.session().workspace.tab(study).is_some());
}

/// Acceptance: the same Word Study offers the Starter library; one more click downloads it,
/// and the Guide shows TBESG's G26 entry.
#[test]
fn word_study_g26_adds_the_starter_library_in_one_more_click_and_refreshes() {
    let (mut h, _lab) = setup("add-starter", &[]);
    let study = open_word_study(&mut h, "G26");
    until(&mut h, "the Lexicon entries hint", |h| {
        find_in(h, study, LEXICON_HINT).is_some()
    });
    let at = in_section(&h, study, GuideSection::LexiconEntries, ADD_STARTER)
        .unwrap()
        .center();
    h.click(at);
    settle(&mut h);
    assert!(open_tab_of(&h, &TabKind::StarterLibrary).is_some());
    // No bundle: the Tab fetched the released list and offers it in one click.
    let wanted = h.shell.st.session.starter_wanted_bytes();
    assert!(wanted > 0);
    let label = format!(
        "Download Starter library (≈{} MB)",
        wanted.div_ceil(1_000_000)
    );
    h.click_text(&label);
    settle(&mut h);
    assert!(h.shell.st.session.library.get("tbesg").is_some());
    close(&mut h, &TabKind::StarterLibrary);
    until(&mut h, "TBESG's G26 entry", |h| {
        find_in(h, study, TBESG).is_some()
    });
    assert!(find_in(&h, study, LEXICON_HINT).is_none());
    // The Starter library is in: only Get Lexicons is left for numbers no Lexicon has.
    h.shell
        .run("tab.go", &json!({ "tab": study.0, "text": "G9999" }));
    until(&mut h, "the no-entry state", |h| {
        find_in(
            h,
            study,
            "None in your library: no Lexicon has an entry for this Strong's number.",
        )
        .is_some()
    });
    let lexicons = GuideSection::LexiconEntries;
    assert!(in_section(&h, study, lexicons, "Get Lexicons…").is_some());
    assert!(in_section(&h, study, lexicons, ADD_STARTER).is_none());
}

/// Click `label` in `section` of `guide`, check what it opened, and close that again.
fn click_action(h: &mut Harness, guide: TabId, section: GuideSection, label: &str) {
    let at = in_section(h, guide, section, label)
        .unwrap_or_else(|| panic!("no {label:?} under {section:?}: {:?}", h.texts()))
        .right_center();
    // A wrapped row's text shapes may start at the row's left edge: aim at the label's end.
    h.click(at - vec2(6.0, 0.0));
    settle(h);
}

fn expect_get_resources(h: &mut Harness, filter: ResourceFilter) {
    assert!(
        open_tab_of(h, &TabKind::GetBibles).is_some(),
        "no Get Resources Tab"
    );
    assert_eq!(h.shell.st.session.get_bibles.filter(), filter);
    close(h, &TabKind::GetBibles);
}

fn expect_starter_library(h: &mut Harness) {
    assert!(
        open_tab_of(h, &TabKind::StarterLibrary).is_some(),
        "no Starter library Tab"
    );
    close(h, &TabKind::StarterLibrary);
}

fn expect_import(h: &mut Harness) {
    assert!(
        h.shell.st.pick_import,
        "Import a file… did not ask for a file"
    );
    h.shell.st.pick_import = false;
}

#[test]
fn every_passage_guide_section_empty_state_has_a_working_action() {
    let (mut h, _lab) = setup("passage-guide", &[]);
    let guide = h
        .shell
        .run(
            "guide.passage_guide",
            &json!({ "passage": "Jn 3:16", "split": true }),
        )
        .unwrap()["tab"]
        .as_u64()
        .map(TabId)
        .unwrap();
    h.frames(2);
    use GuideSection::*;
    let hints = [
        (Commentaries, "None in your library. Add a Commentary"),
        (
            CrossReferences,
            "None in your library. Add a Cross-reference set",
        ),
        (
            ParallelPassages,
            "None in your library. Add a set of parallel passages",
        ),
        (
            KeyWords,
            "None in your library. Add a Bible with Strong's numbers",
        ),
        (
            PeopleAndPlaces,
            "None in your library. Add a People & places index",
        ),
    ];
    until(&mut h, "every section's hint", |h| {
        hints.iter().all(|(_, hint)| {
            let r = pane(h, guide);
            h.shapes
                .iter()
                .any(|(s, t)| t.starts_with(hint) && r.is_some_and(|r| r.contains(s.center())))
        })
    });
    // Get Resources lists Commentaries and Bibles; Cross-reference sets and People & places
    // indexes come with the Starter library (SWORD repositories offer none).
    assert!(in_section(&h, guide, CrossReferences, "Get Cross-reference sets…").is_none());
    if std::env::var_os("UPDATE_PREVIEWS").is_some() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/previews");
        h.frames(10);
        h.screenshot(&dir.join("passage-guide-empty-library.ppm"));
    }
    click_action(&mut h, guide, Commentaries, "Get Commentaries…");
    expect_get_resources(&mut h, ResourceFilter::Type(ResourceType::Commentary));
    click_action(&mut h, guide, KeyWords, "Get Bibles…");
    expect_get_resources(&mut h, ResourceFilter::Type(ResourceType::Bible));
    for section in [
        Commentaries,
        CrossReferences,
        ParallelPassages,
        KeyWords,
        PeopleAndPlaces,
    ] {
        click_action(&mut h, guide, section, ADD_STARTER);
        expect_starter_library(&mut h);
        click_action(&mut h, guide, section, IMPORT);
        expect_import(&mut h);
    }
    // The Starter library arrives: the open Guide fills in without being reopened.
    h.shell.st.pick_import = false;
    h.shell.run("starter.add", &json!({}));
    settle(&mut h);
    until(&mut h, "the Commentary row and the Cross-references", |h| {
        find_in(h, guide, "Matthew Henry (test)").is_some()
            && find_in(h, guide, "John 11:25-26").is_some()
    });
    let r = pane(&h, guide).unwrap();
    assert!(
        !h.shapes
            .iter()
            .any(|(s, t)| t.starts_with("None in your library. Add a") && r.contains(s.center())),
        "{:?}",
        h.texts()
    );
}

#[test]
fn every_word_study_section_empty_state_has_a_working_action() {
    let (mut h, _lab) = setup("word-study", &[]);
    let study = open_word_study(&mut h, "G26");
    // Forms starts collapsed: expand it to see its empty state.
    h.shell.run(
        "guide.collapse_section",
        &json!({ "guide": "word_study", "section": "forms", "collapsed": false }),
    );
    use GuideSection::*;
    until(&mut h, "every section's buttons", |h| {
        [LexiconEntries, TranslatedAs, Occurrences, Forms]
            .iter()
            .all(|s| in_section(h, study, *s, ADD_STARTER).is_some())
    });
    click_action(&mut h, study, LexiconEntries, "Get Lexicons…");
    expect_get_resources(&mut h, ResourceFilter::Type(ResourceType::Lexicon));
    click_action(&mut h, study, TranslatedAs, "Get Bibles…");
    expect_get_resources(&mut h, ResourceFilter::Type(ResourceType::Bible));
    click_action(&mut h, study, Occurrences, "Get Bibles…");
    expect_get_resources(&mut h, ResourceFilter::Type(ResourceType::Bible));
    click_action(&mut h, study, Forms, "Get Hebrew & Greek texts…");
    expect_get_resources(&mut h, ResourceFilter::HebrewAndGreek);
    for section in [LexiconEntries, TranslatedAs, Occurrences, Forms] {
        click_action(&mut h, study, section, ADD_STARTER);
        expect_starter_library(&mut h);
        click_action(&mut h, study, section, IMPORT);
        expect_import(&mut h);
    }
}

/// The Library has a Lexicon, but none has G26: Get Lexicons only, not the Starter library.
#[test]
fn no_entry_for_the_number_offers_get_lexicons_only() {
    let only_hebrew = obs_engine::testing::lexicon("only-h", "OH", &[("H1", "אָב", "father")]);
    let (mut h, _lab) = setup("no-entry", &[only_hebrew]);
    let study = open_word_study(&mut h, "G26");
    let lexicons = GuideSection::LexiconEntries;
    until(&mut h, "the no-entry state", |h| {
        find_in(
            h,
            study,
            "None in your library: no Lexicon has an entry for this Strong's number.",
        )
        .is_some()
    });
    assert!(in_section(&h, study, lexicons, "Get Lexicons…").is_some());
    assert!(in_section(&h, study, lexicons, ADD_STARTER).is_none());
    assert!(in_section(&h, study, lexicons, IMPORT).is_some());
}

/// The Cross-references Tool: the Starter library's set, then the Tool fills in on its own.
#[test]
fn the_cross_references_tool_offers_the_starter_library_and_refreshes() {
    let (mut h, lab) = setup("xrefs-tool", &[]);
    let tool = h
        .shell
        .run(
            "tool.cross_references",
            &json!({ "passage": "Jn 3:16", "split": true }),
        )
        .unwrap()["tab"]
        .as_u64()
        .map(TabId)
        .unwrap();
    let hint = "None in your library. Add a Cross-reference set to see related Passages.";
    until(&mut h, "the Tool's hint", |h| {
        find_in(h, tool, hint).is_some()
    });
    assert!(find_in(&h, tool, IMPORT).is_some());
    let at = find_in(&h, tool, ADD_STARTER).unwrap().center();
    h.click(at);
    settle(&mut h);
    expect_starter_library(&mut h);
    // A Cross-reference set joins the Library (as the Starter library would add it).
    let references = cross_references();
    let path = lab.root.join("library").join("openbible.obr");
    obs_store::write_obr(&references, &path).unwrap();
    h.shell.st.session.library.add_file(&path).unwrap();
    until(&mut h, "the Cross-references", |h| {
        find_in(h, tool, hint).is_none() && find_in(h, tool, "John 11:25-26").is_some()
    });
}

/// The Word Study chooser with no tagged English Bible offers one.
#[test]
fn the_word_study_chooser_without_a_tagged_english_bible_offers_one() {
    let (mut h, _lab) = setup("chooser", &[]);
    let study = open_word_study(&mut h, "G26");
    h.shell
        .run("tab.go", &json!({ "tab": study.0, "text": "love" }));
    until(&mut h, "the chooser's hint", |h| {
        h.texts()
            .iter()
            .any(|t| t.starts_with("No English Bible in your library has Strong's numbers"))
    });
    let at = find_in(&h, study, "Get Bibles…").unwrap().center();
    h.click(at);
    settle(&mut h);
    expect_get_resources(&mut h, ResourceFilter::Type(ResourceType::Bible));
    let at = find_in(&h, study, ADD_STARTER).unwrap().center();
    h.click(at);
    settle(&mut h);
    expect_starter_library(&mut h);
}

/// A Library with only untagged Bibles, English and Greek: Key words and Forms stay empty, so
/// they still offer the Starter library (its KJV with Strong's and TAGNT would fill them).
#[test]
fn untagged_bibles_still_offer_the_starter_library_for_key_words_and_forms() {
    let john_3_16 = Verse::new(obs_scripture::Book::John, 3, 16);
    let english = obs_engine::testing::tagged_bible(
        "plain",
        "Plain",
        "PLN",
        "en",
        &[(john_3_16, "For God so loved the world")],
    );
    let greek = obs_engine::testing::tagged_bible(
        "plain-grc",
        "Plain Greek",
        "PLG",
        "grc",
        &[(john_3_16, "Οὕτως γὰρ ἠγάπησεν ὁ θεὸς τὸν κόσμον")],
    );
    let (mut h, _lab) = setup("untagged", &[english, greek]);
    // Tall enough for the Word Study's last section, opened beside the Passage Guide that
    // keeps the focus once the Starter library Tab closes (#256).
    h.screen = vec2(1400.0, 1300.0);
    let guide = h
        .shell
        .run(
            "guide.passage_guide",
            &json!({ "passage": "Jn 3:16", "split": true }),
        )
        .unwrap()["tab"]
        .as_u64()
        .map(TabId)
        .unwrap();
    let key_words = GuideSection::KeyWords;
    until(&mut h, "Key words' buttons", |h| {
        find_in(
            h,
            guide,
            "None in your library: no Bible with Strong's numbers has this Passage.",
        )
        .is_some()
            && in_section(h, guide, key_words, ADD_STARTER).is_some()
    });
    assert!(in_section(&h, guide, key_words, "Get Bibles…").is_some());
    click_action(&mut h, guide, key_words, ADD_STARTER);
    expect_starter_library(&mut h);

    let study = open_word_study(&mut h, "G26");
    h.shell.run(
        "guide.collapse_section",
        &json!({ "guide": "word_study", "section": "forms", "collapsed": false }),
    );
    let forms = GuideSection::Forms;
    until(&mut h, "Forms' buttons", |h| {
        in_section(h, study, forms, "Get Hebrew & Greek texts…").is_some()
            && in_section(h, study, forms, ADD_STARTER).is_some()
    });
    click_action(&mut h, study, forms, ADD_STARTER);
    expect_starter_library(&mut h);
}

/// A Greek Bible with Strong's numbers but no Morphology has G26, yet its Forms stay empty:
/// Forms still offers the Starter library (its TAGNT has Morphology), and once it is added
/// the same Word Study shows G26's forms from TAGNT.
#[test]
fn a_strongs_only_greek_bible_still_offers_the_starter_library_for_forms() {
    let greek = obs_engine::testing::tagged_bible(
        "strongs-grc",
        "Greek with Strong's",
        "GRS",
        "grc",
        &[(
            Verse::new(obs_scripture::Book::John, 15, 13),
            "μείζονα{G3187} ταύτης{G3778} ἀγάπης{G26} οὐδεὶς{G3762} ἔχει{G2192}",
        )],
    );
    let (mut h, _lab) = setup("strongs-only", &[greek]);
    let study = open_word_study(&mut h, "G26");
    h.shell.run(
        "guide.collapse_section",
        &json!({ "guide": "word_study", "section": "forms", "collapsed": false }),
    );
    let forms = GuideSection::Forms;
    until(&mut h, "Forms' buttons", |h| {
        find_in(
            h,
            study,
            "None in your library: no Hebrew or Greek Bible with Morphology has this word.",
        )
        .is_some()
            && in_section(h, study, forms, "Get Hebrew & Greek texts…").is_some()
            && in_section(h, study, forms, ADD_STARTER).is_some()
    });
    click_action(&mut h, study, forms, ADD_STARTER);
    expect_starter_library(&mut h);
    h.shell.run("starter.add", &json!({}));
    settle(&mut h);
    // Collapse the sections above Forms so its rows are in view.
    for section in ["lexicon_entries", "translated_as", "occurrences"] {
        h.shell.run(
            "guide.collapse_section",
            &json!({ "guide": "word_study", "section": section, "collapsed": true }),
        );
    }
    let genitive = format!("{} ×1", obs_engine::word_sections::form_of("N-GSF").0);
    until(&mut h, "G26's forms from TAGNT", |h| {
        in_section(
            h,
            study,
            forms,
            "In the TAGNT. Click a form to list only its occurrences.",
        )
        .is_some()
            && in_section(h, study, forms, &genitive).is_some()
            && in_section(h, study, forms, ADD_STARTER).is_none()
    });
    assert!(h.shell.session().workspace.tab(study).is_some());
}

/// The Factbook (#149) with an empty Library: its chooser and every section offer the way to
/// get TIPNR (the Starter library; SWORD repositories offer no People & places index), and
/// Dictionary articles Get Dictionaries….
#[test]
fn every_factbook_section_and_its_chooser_have_a_working_action() {
    let (mut h, _lab) = setup("factbook", &[]);
    let chooser = h
        .shell
        .run("guide.factbook", &json!({ "name": "John", "split": true }))
        .unwrap()["tab"]
        .as_u64()
        .map(TabId)
        .unwrap();
    until(&mut h, "the chooser's buttons", |h| {
        find_in(
            h,
            chooser,
            "No People & places index in your library. Add one (TIPNR) to look up people and \
             places.",
        )
        .is_some()
            && find_in(h, chooser, ADD_STARTER).is_some()
    });
    assert!(find_in(&h, chooser, "Get People & places indexes…").is_none());
    let at = find_in(&h, chooser, ADD_STARTER).unwrap().center();
    h.click(at);
    settle(&mut h);
    expect_starter_library(&mut h);
    h.shell.run("tab.close", &json!({ "tab": chooser.0 }));
    h.frames(2);

    let factbook = h
        .shell
        .run(
            "guide.factbook",
            &json!({ "entity": "Abraham@Gen.11.26-1Pe", "split": true }),
        )
        .unwrap()["tab"]
        .as_u64()
        .map(TabId)
        .unwrap();
    use GuideSection::*;
    until(&mut h, "every section's buttons", |h| {
        [Summary, KeyPassages, Related]
            .iter()
            .all(|s| in_section(h, factbook, *s, ADD_STARTER).is_some())
            && in_section(h, factbook, DictionaryArticles, "Get Dictionaries…").is_some()
    });
    click_action(&mut h, factbook, DictionaryArticles, "Get Dictionaries…");
    expect_get_resources(&mut h, ResourceFilter::Type(ResourceType::Dictionary));
    for section in [Summary, KeyPassages, Related] {
        click_action(&mut h, factbook, section, ADD_STARTER);
        expect_starter_library(&mut h);
    }
    for section in [Summary, KeyPassages, DictionaryArticles, Related] {
        click_action(&mut h, factbook, section, IMPORT);
        expect_import(&mut h);
    }
}

/// A Factbook with Easton's but no People & places index: Dictionary articles ask for TIPNR
/// (the Starter library), not for more Dictionaries, and the same Tab fills once it is added.
#[test]
fn factbook_dictionary_articles_without_tipnr_offer_tipnr_and_refresh() {
    let easton = obs_engine::testing::dictionary("easton");
    let (mut h, _lab) = setup("factbook-no-index", &[easton]);
    let factbook = h
        .shell
        .run(
            "guide.factbook",
            &json!({ "entity": "Abraham@Gen.11.26-1Pe", "split": true }),
        )
        .unwrap()["tab"]
        .as_u64()
        .map(TabId)
        .unwrap();
    let articles = GuideSection::DictionaryArticles;
    until(&mut h, "the Dictionary articles hint", |h| {
        find_in(
            h,
            factbook,
            "None in your library. Add a People & places index (TIPNR) to find the Dictionary \
             articles on them.",
        )
        .is_some()
            && in_section(h, factbook, articles, ADD_STARTER).is_some()
    });
    assert!(in_section(&h, factbook, articles, "Get Dictionaries…").is_none());
    click_action(&mut h, factbook, articles, ADD_STARTER);
    expect_starter_library(&mut h);
    h.shell.run("starter.add", &json!({}));
    settle(&mut h);
    assert!(h.shell.st.session.library.get("tipnr").is_some());
    until(&mut h, "Easton's article on Abraham", |h| {
        in_section(h, factbook, articles, "Easton: Abraham").is_some()
            && in_section(h, factbook, articles, ADD_STARTER).is_none()
    });
    assert!(h.shell.session().workspace.tab(factbook).is_some());
}

/// #261 GUIDES-14: an Atlas with no People & places index says where places come from and
/// offers the Starter library, like the Timeline, instead of claiming the Passage names no
/// place; it shows the map once an index arrives.
#[test]
fn the_atlas_without_a_people_and_places_index_offers_the_starter_library() {
    let (mut h, lab) = setup("atlas", &[]);
    let atlas = h
        .shell
        .run(
            "tool.atlas",
            &json!({ "passage": "Acts 13", "split": true }),
        )
        .unwrap()["tab"]
        .as_u64()
        .map(TabId)
        .unwrap();
    let hint = obs_engine::atlas::ATLAS_NO_INDEX;
    until(&mut h, "the Atlas's hint", |h| {
        find_in(h, atlas, hint).is_some() && find_in(h, atlas, ADD_STARTER).is_some()
    });
    assert!(find_in(&h, atlas, IMPORT).is_some());
    assert!(
        !h.texts().iter().any(|t| t.contains("No place is listed")),
        "{:?}",
        h.texts()
    );
    let at = find_in(&h, atlas, ADD_STARTER).unwrap().center();
    h.click(at);
    settle(&mut h);
    expect_starter_library(&mut h);
    // TIPNR joins the Library (as the Starter library would add it): the map comes back.
    let tipnr = obs_engine::testing::tipnr_acts_13();
    let path = lab.root.join("library").join("tipnr.obr");
    obs_store::write_obr(&tipnr, &path).unwrap();
    h.shell.st.session.library.add_file(&path).unwrap();
    until(&mut h, "the Atlas's places", |h| {
        find_in(h, atlas, hint).is_none() && h.texts().iter().any(|t| t.contains("Perga"))
    });
}
