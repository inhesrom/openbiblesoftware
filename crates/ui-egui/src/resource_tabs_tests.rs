//! Reading-Tab acceptance tests using the shell's real-event headless harness.

use obs_engine::{HeaderBox, TabId};
use obs_model::{
    Key, KeyScheme, Manifest, Resource, ResourceBuilder, ResourceType, UnitId, UnitKind,
};
use obs_scripture::{Book, Passage, Verse};

use super::*;

fn ge(chapter: u16, verse: u16) -> Verse {
    Verse::new(Book::Genesis, chapter, verse)
}

fn study_resource(ty: ResourceType) -> Resource {
    let (id, title, abbreviation, schemes) = match ty {
        ResourceType::Commentary => (
            "test-commentary",
            "Test Commentary",
            "TC",
            vec![KeyScheme::Passage],
        ),
        ResourceType::Lexicon => (
            "test-lexicon",
            "Test Lexicon",
            "TL",
            vec![KeyScheme::StrongsNumber, KeyScheme::Lemma],
        ),
        ResourceType::Dictionary => (
            "test-dictionary",
            "Test Dictionary",
            "TD",
            vec![KeyScheme::Headword],
        ),
        _ => unreachable!(),
    };
    let mut manifest = Manifest::new(id, title, abbreviation, ty, "en");
    manifest.key_schemes = schemes;
    manifest.license = "CC0-1.0".into();
    let mut b = ResourceBuilder::new(manifest);
    match ty {
        ResourceType::Commentary => {
            for (unit, start, end, text) in [
                (
                    "GEN.31.50",
                    ge(31, 50),
                    ge(31, 54),
                    "Genesis 31:50–54\nA witness between Jacob and Laban.",
                ),
                (
                    "GEN.32.5",
                    ge(32, 5),
                    ge(32, 9),
                    "Genesis 32:5–9\nJacob prepares to meet Esau.",
                ),
            ] {
                let id = UnitId::new(unit).unwrap();
                add_prose_unit(&mut b, &id, UnitKind::Section, text);
                b.add_key(
                    Key::Passage(Passage::new(start, end).unwrap()),
                    &id,
                    None,
                    false,
                );
            }
        }
        ResourceType::Lexicon => {
            for (number, text) in [
                ("G25", "G25 — ἀγαπάω\nTo love."),
                ("G26", "G26 — ἀγάπη\nLove; affection."),
                ("G27", "G27 — ἀγαπητός\nBeloved."),
            ] {
                let id = UnitId::new(number).unwrap();
                add_prose_unit(&mut b, &id, UnitKind::Entry, text);
                b.add_key(
                    Key::StrongsNumber(number.parse().unwrap()),
                    &id,
                    None,
                    false,
                );
                if number == "G26" {
                    b.add_key(Key::Lemma("ἀγάπη".into()), &id, None, false);
                }
            }
        }
        ResourceType::Dictionary => {
            for (headword, text) in [
                ("Abraham", "Abraham\nFather of a multitude."),
                ("Adam", "Adam\nThe first man."),
            ] {
                let id = UnitId::new(headword).unwrap();
                add_prose_unit(&mut b, &id, UnitKind::Entry, text);
                b.add_key(Key::Headword(headword.into()), &id, None, false);
                if headword == "Abraham" {
                    b.add_key(Key::Headword("Abram".into()), &id, None, true);
                }
            }
        }
        _ => unreachable!(),
    }
    b.build().unwrap()
}

fn add_prose_unit(b: &mut ResourceBuilder, id: &UnitId, kind: UnitKind, text: &str) {
    let split = text.find('\n').unwrap();
    b.add_unit(id.clone(), kind, None, text.into());
    b.add_span(id, 0..split, obs_model::SpanKind::Heading { level: 1 });
    b.add_span(
        id,
        split + 1..text.len(),
        obs_model::SpanKind::Para {
            style: obs_model::ParaStyle::Prose,
        },
    );
}

fn open_study(h: &mut Harness, ty: ResourceType) -> TabId {
    let resource = study_resource(ty);
    let id = resource.manifest.resource_id.clone();
    add_doc(&mut h.shell, Arc::new(ReadingDoc::from_resource(resource)));
    let tab = h
        .shell
        .run("resource.open", &json!({ "id": id, "split": true }))
        .unwrap()["tab"]
        .as_u64()
        .unwrap();
    h.frames(3);
    TabId(tab)
}

fn type_key(h: &mut Harness, shown: &str, text: &str) {
    h.click_text(shown);
    h.events.push(Event::Key {
        key: egui::Key::A,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::COMMAND,
    });
    h.events.push(Event::Text(text.into()));
    h.frame();
    h.events.push(Event::Key {
        key: egui::Key::Enter,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::NONE,
    });
    h.frames(3);
}

fn shown_unit(h: &mut Harness, tab: TabId) -> String {
    let view = h
        .shell
        .st
        .tabs
        .get_mut(&tab)
        .unwrap()
        .view
        .as_mut()
        .unwrap();
    let visible = view.visible(400.0);
    view.doc.units[view.doc.blocks[visible[0].index].unit]
        .id
        .to_string()
}

#[test]
fn commentary_tab_follows_bible_to_nearest_earlier_section_and_flashes_without_feedback() {
    let mut h = Harness::new();
    let bible = h.tab_id("TB");
    h.shell
        .run("tab.go", &json!({ "tab": bible.0, "text": "Gen 31:55" }));
    let commentary = open_study(&mut h, ResourceType::Commentary);
    assert_eq!(
        h.shell.session().workspace.tab(commentary).unwrap().link,
        Some('A')
    );
    assert_eq!(shown_unit(&mut h, commentary), "GEN.31.50");
    assert_eq!(h.shell.st.location_text(commentary), "Genesis 31:50");
    h.shell
        .run("tab.go", &json!({ "tab": bible.0, "text": "Gen 32:6" }));
    settle_all(&mut h);
    assert_eq!(shown_unit(&mut h, commentary), "GEN.32.5");
    assert_eq!(
        h.shell
            .session()
            .workspace
            .tab(bible)
            .unwrap()
            .history
            .current(),
        Some(ge(32, 6))
    );
    assert!(h.shell.st.tabs[&commentary].flash.is_some());
    let doc = &h.shell.st.tabs[&commentary].view.as_ref().unwrap().doc;
    let (unit, byte) = doc.verse_position(ge(32, 5)).unwrap();
    assert!(doc.verse_range_at(unit, byte).is_some());
    // Commentary box accepts a Standard Verse with no exact section and relative input.
    type_key(&mut h, "Genesis 32:5", "Gen 31:55");
    assert_eq!(shown_unit(&mut h, commentary), "GEN.31.50");
    assert_eq!(
        h.shell
            .session()
            .workspace
            .tab(bible)
            .unwrap()
            .history
            .current(),
        Some(ge(31, 55))
    );
    h.shell
        .run("tab.go", &json!({"tab": commentary.0, "text": "52"}));
    assert_eq!(h.shell.st.location_text(commentary), "Genesis 31:50");
}

#[test]
fn commentary_follows_translated_hebrew_numbering_instead_of_treating_it_as_standard() {
    let mut h = Harness::new();
    let commentary = open_study(&mut h, ResourceType::Commentary);
    let hebrew = TabId(
        h.shell
            .run("resource.open", &json!({"id": "heb-num", "split": true}))
            .unwrap()["tab"]
            .as_u64()
            .unwrap(),
    );
    h.shell
        .run("tab.go", &json!({"tab": hebrew.0, "text": "Gen 32:1"}));
    settle_all(&mut h);
    assert_eq!(h.shell.st.location_text(hebrew), "Genesis 32:1");
    assert_eq!(h.shell.st.location_text(commentary), "Genesis 31:50");
    assert_eq!(shown_unit(&mut h, commentary), "GEN.31.50");
}

#[test]
fn g26_typed_in_lexicon_tab_renders_entry_with_shared_pipeline_and_entry_navigation() {
    let mut h = Harness::new();
    let tab = open_study(&mut h, ResourceType::Lexicon);
    assert_eq!(
        h.shell.session().workspace.tab(tab).unwrap().header_box(),
        HeaderBox::Lexicon
    );
    type_key(&mut h, "G25", "G26");
    assert_eq!(shown_unit(&mut h, tab), "G26");
    assert_eq!(h.shell.st.location_text(tab), "G26");
    // Resource prose isn't an egui label: glyph atlas + AccessKit expose it instead.
    assert!(!h.texts().iter().any(|t| t.contains("Love; affection")));
    h.ctx.enable_accesskit();
    settle_all(&mut h);
    assert!(
        h.tree
            .as_ref()
            .unwrap()
            .nodes
            .iter()
            .any(|(_, node)| node.value().is_some_and(|s| s.contains("Love; affection.")))
    );
    assert!(!h.meshes.is_empty());
    h.click_text("Next entry");
    assert_eq!(shown_unit(&mut h, tab), "G27");
    h.click_text("Previous entry");
    assert_eq!(shown_unit(&mut h, tab), "G26");
    h.shell.run("tab.back", &json!({"tab": tab.0}));
    assert_eq!(shown_unit(&mut h, tab), "G27");
    h.shell.run("tab.forward", &json!({"tab": tab.0}));
    assert_eq!(shown_unit(&mut h, tab), "G26");
    for text in ["agape", "G26A"] {
        h.shell.run("tab.go", &json!({"tab": tab.0, "text": text}));
        assert_eq!(shown_unit(&mut h, tab), "G26");
    }
    assert!(
        h.shell
            .run("tab.go", &json!({"tab": tab.0, "text": "G99999"}))
            .is_none()
    );
    assert_eq!(shown_unit(&mut h, tab), "G26");
}

#[test]
fn dictionary_headword_alias_and_saved_lexicon_location_reopen_correct_entries() {
    let mut h = Harness::new();
    let dictionary = open_study(&mut h, ResourceType::Dictionary);
    assert_eq!(
        h.shell
            .session()
            .workspace
            .tab(dictionary)
            .unwrap()
            .header_box(),
        HeaderBox::Headword
    );
    type_key(&mut h, "Abraham", "ABRAM");
    assert_eq!(shown_unit(&mut h, dictionary), "Abraham");
    h.click_text("Next entry");
    assert_eq!(shown_unit(&mut h, dictionary), "Adam");
    let lexicon = open_study(&mut h, ResourceType::Lexicon);
    h.shell
        .run("tab.go", &json!({"tab": lexicon.0, "text": "G26"}));
    let snapshot = h.shell.session().workspace.snapshot();
    h.shell.session_mut().workspace.restore(snapshot);
    h.shell.apply_effects();
    h.frames(3);
    assert_eq!(shown_unit(&mut h, dictionary), "Adam");
    assert_eq!(shown_unit(&mut h, lexicon), "G26");
}

#[test]
fn obr_reading_doc_loads_non_bible_keys_and_aliases() {
    let dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/resource-tabs-test");
    std::fs::create_dir_all(&dir).unwrap();
    for ty in [
        ResourceType::Commentary,
        ResourceType::Lexicon,
        ResourceType::Dictionary,
    ] {
        let resource = study_resource(ty);
        let path = dir.join(format!("{}.obr", resource.manifest.resource_id));
        obs_store::write_obr(&resource, &path).unwrap();
        let doc = ReadingDoc::open(&path).unwrap();
        assert_eq!(
            doc.navigation.keys,
            obs_engine::resource_navigation::ResourceNavigation::new(resource.keys).keys
        );
        match ty {
            ResourceType::Commentary => assert_eq!(
                doc.units[doc.verse_position(ge(31, 55)).unwrap().0]
                    .id
                    .as_str(),
                "GEN.31.50"
            ),
            ResourceType::Lexicon => assert_eq!(
                doc.navigation.parse_entry("G26").unwrap().unit.as_str(),
                "G26"
            ),
            ResourceType::Dictionary => assert_eq!(
                doc.navigation.parse_entry("Abram").unwrap().unit.as_str(),
                "Abraham"
            ),
            _ => unreachable!(),
        }
    }
}

#[test]
fn entry_jump_honors_key_byte_range_in_shared_reading_view() {
    let mut m = Manifest::new(
        "range-lexicon",
        "Range Lexicon",
        "RL",
        ResourceType::Lexicon,
        "en",
    );
    m.key_schemes = vec![KeyScheme::StrongsNumber];
    m.license = "CC0-1.0".into();
    let mut b = ResourceBuilder::new(m);
    let id = UnitId::new("G26").unwrap();
    let text = "Preamble\nLove; affection.";
    add_prose_unit(&mut b, &id, UnitKind::Entry, text);
    let key = Key::StrongsNumber("G26".parse().unwrap());
    b.add_key(key.clone(), &id, Some(9..text.len()), false);
    let doc = Arc::new(ReadingDoc::from_resource(b.build().unwrap()));
    let mut view = crate::view::ReadingView::new(doc, obs_layout::TextSettings::default());
    assert!(view.jump_to_entry(&key));
    let first = view.visible(400.0).remove(0);
    assert_eq!(first.laid.text, "Love; affection.");
    assert_eq!(view.current_entry(), Some(key));
}

/// Reproducible screenshots and a sequence of real rendered frames for the PR recording.
#[test]
fn resource_tabs_previews() {
    if std::env::var_os("UPDATE_PREVIEWS").is_none() {
        return;
    }
    let mut h = Harness::new();
    let bible = h.tab_id("TB");
    h.shell
        .run("tab.go", &json!({"tab": bible.0, "text": "Gen 31:55"}));
    let commentary = open_study(&mut h, ResourceType::Commentary);
    settle_all(&mut h);
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/previews/resource-tabs");
    std::fs::create_dir_all(&dir).unwrap();
    h.screenshot(&dir.join("commentary.ppm"));
    for (i, text) in ["Gen 31:55", "Gen 32:6", "Gen 31:55"].iter().enumerate() {
        h.shell
            .run("tab.go", &json!({"tab": bible.0, "text": text}));
        settle_all(&mut h);
        h.screenshot(&dir.join(format!("frame-{i:02}.ppm")));
    }
    h.shell.run("tab.close", &json!({"tab": commentary.0}));
    let lexicon = open_study(&mut h, ResourceType::Lexicon);
    for (i, text) in ["G25", "G26", "G27", "G26"].iter().enumerate() {
        h.shell
            .run("tab.go", &json!({"tab": lexicon.0, "text": text}));
        settle_all(&mut h);
        h.screenshot(&dir.join(format!("frame-{:02}.ppm", i + 3)));
    }
    h.screenshot(&dir.join("lexicon-g26.ppm"));
}
