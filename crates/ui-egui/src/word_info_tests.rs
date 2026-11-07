//! Word Info acceptance tests (#50): hover dwell, the Lemma card, pinning, and the KJV+Strong's
//! case, through the shell's real-event headless harness.

use obs_engine::TabId;
use obs_model::{Key, KeyScheme, Manifest, MorphemePart, ParaStyle, ResourceBuilder, SpanKind};

use super::*;

/// A KJV-style English Bible with Strong's numbers: Tagged words carry only the number (and
/// sometimes an unreadable TH-style code), never a Lemma or Morphology we can read.
fn kjv_strongs_doc() -> Arc<ReadingDoc> {
    use obs_model::{ResourceType, TextAccumulator, UnitId, UnitKind};
    use obs_scripture::{Book, Verse};
    let mut m = Manifest::new("kjv-test", "KJV test", "KJVT", ResourceType::Bible, "en");
    m.key_schemes = vec![KeyScheme::Passage];
    m.license = "LicenseRef-PD".into();
    let mut b = ResourceBuilder::new(m);
    let genesis = UnitId::book(Book::Genesis);
    let mut t = TextAccumulator::new();
    let v1 = t.push("In the beginning God created the heaven and the earth.");
    t.push(" ");
    let v2 = t.push("And the earth was without form, and void.");
    let end = t.pos();
    let text = t.into_string();
    let tagged: [(&str, &str, Option<&str>); 2] = [
        ("God", "H430", None),
        ("created", "H1254A", Some("strongMorph:TH8804")),
    ];
    let words: Vec<(std::ops::Range<usize>, MorphemePart)> = tagged
        .into_iter()
        .map(|(w, strongs, morph)| {
            let start = text.find(w).unwrap();
            (
                start..start + w.len(),
                MorphemePart {
                    strongs: strongs.parse().unwrap(),
                    lemma: None,
                    morph: morph.map(String::from),
                    gloss: None,
                },
            )
        })
        .collect();
    b.add_unit(genesis.clone(), UnitKind::BibleBook, None, text);
    b.add_span(
        &genesis,
        0..end,
        SpanKind::Para {
            style: ParaStyle::Prose,
        },
    );
    for (range, part) in words {
        b.add_span(&genesis, range, SpanKind::TaggedWord { parts: vec![part] });
    }
    b.add_verse(&genesis, v1, Verse::new(Book::Genesis, 1, 1), 1);
    b.add_verse(&genesis, v2, Verse::new(Book::Genesis, 1, 2), 2);
    Arc::new(ReadingDoc::from_resource(b.build().unwrap()))
}

/// A Hebrew Lexicon whose entries open with the headword line, then the gloss paragraph; the
/// unit title is the headword, which a KJV word borrows as its Lemma.
fn hebrew_lexicon_doc() -> Arc<ReadingDoc> {
    use obs_model::{ResourceType, UnitId, UnitKind};
    let mut m = Manifest::new(
        "heb-lexicon",
        "Test Hebrew Lexicon",
        "THL",
        ResourceType::Lexicon,
        "en",
    );
    m.key_schemes = vec![KeyScheme::StrongsNumber];
    m.license = "CC0-1.0".into();
    let mut b = ResourceBuilder::new(m);
    for (number, headword, text) in [
        ("H430", "אֱלֹהִים", "אֱלֹהִים elohim\nGod, gods."),
        ("H1254A", "בָּרָא", "בָּרָא bara\nto create."),
        ("H7225", "רֵאשִׁית", "רֵאשִׁית reshit\nbeginning, first."),
    ] {
        let id = UnitId::new(number).unwrap();
        let split = text.find('\n').unwrap();
        b.add_unit(
            id.clone(),
            UnitKind::Entry,
            Some(headword.into()),
            text.into(),
        );
        b.add_span(&id, 0..split, SpanKind::Heading { level: 1 });
        b.add_span(
            &id,
            split + 1..text.len(),
            SpanKind::Para {
                style: ParaStyle::Prose,
            },
        );
        b.add_key(
            Key::StrongsNumber(number.parse().unwrap()),
            &id,
            None,
            false,
        );
    }
    Arc::new(ReadingDoc::from_resource(b.build().unwrap()))
}

/// Where `word`'s glyphs sit on screen in `tab`'s reading pane.
pub(super) fn word_spot(h: &Harness, tab: TabId, word: &str) -> Pos2 {
    let view = h.shell.st.tabs[&tab].view.as_ref().unwrap();
    let [ox, oy] = view.text_origin;
    for (index, top, laid) in view.laid_in_view(900.0) {
        let unit = view.doc.blocks[index].unit;
        let text = &view.doc.units[unit].text;
        let Some(start) = text.find(word) else {
            continue;
        };
        let range = start..start + word.len();
        for line in &laid.lines {
            for item in &line.items {
                for (bytes, x0, x1) in item.clusters() {
                    if let Some(obs_layout::UnitPos::Text(b)) = laid.to_unit(bytes.start)
                        && range.contains(&b)
                    {
                        return pos2(
                            ox + (x0 + x1) / 2.0,
                            oy + top + (line.top + line.bottom) / 2.0,
                        );
                    }
                }
            }
        }
    }
    panic!("{word:?} not laid out on screen");
}

/// Open the Hebrew Bible, make Word Info the Tools Panel's active Tab, and let layout settle.
fn hebrew_harness() -> (Harness, TabId) {
    let mut h = Harness::new();
    h.shell.run("resource.open", &json!({ "id": "heb-test" }));
    h.frames(4);
    let p = h.tab_at("Word Info");
    h.click(p);
    let heb = h.tab_id("HEB");
    (h, heb)
}

/// The dwell: moving onto a Tagged word underlines it at once, but Word Info follows only
/// after the pointer has rested ~150 ms; moving off keeps what it last showed (sticky).
#[test]
fn word_info_follows_hover_only_after_the_dwell_and_is_sticky() {
    let (mut h, heb) = hebrew_harness();
    let spot = word_spot(&h, heb, "בָּרָ֣א");
    h.events.push(Event::PointerMoved(spot));
    h.frames(2);
    assert!(
        h.shell.st.session.word_info.shown.is_none(),
        "two frames (~33 ms) is less than the dwell"
    );
    h.frames(12);
    let shown = h
        .shell
        .st
        .session
        .word_info
        .shown
        .clone()
        .expect("shown after the dwell");
    assert_eq!(shown.main.strongs.to_string(), "H1254A");
    // Off every word: the panel keeps the word (sticky).
    h.events
        .push(Event::PointerMoved(pos2(spot.x, spot.y + 200.0)));
    h.frames(3);
    let kept = h.shell.st.session.word_info.shown.clone().unwrap();
    assert_eq!(kept.main.strongs.to_string(), "H1254A");
}

/// Acceptance: hovering TAHOT Gen 1:1 בָּרָא shows H1254A, Qal perfect 3ms and a gloss.
#[test]
fn hovering_tahot_bara_shows_strongs_morphology_and_a_gloss() {
    let (mut h, heb) = hebrew_harness();
    add_doc(&mut h.shell, hebrew_lexicon_doc());
    let spot = word_spot(&h, heb, "בָּרָ֣א");
    h.events.push(Event::PointerMoved(spot));
    h.frames(14);
    let shown = h
        .shell
        .st
        .session
        .word_info
        .shown
        .clone()
        .expect("shown after the dwell");
    assert_eq!(shown.main.strongs.to_string(), "H1254A");
    assert_eq!(
        shown.main.morphology.as_deref(),
        Some("Qal perfect, 3rd masc. sing.")
    );
    assert_eq!(shown.main.morph_code.as_deref(), Some("HVqp3ms"));
    assert_eq!(shown.gloss.as_deref(), Some("he created"));
    assert_eq!(shown.reference, "Genesis 1:1 (HEB)");
    // The first Lexicon in Preferred order supplies the card's gloss.
    assert_eq!(shown.main.lexicon.as_ref().unwrap().abbreviation, "THL");
    assert_eq!(shown.main.lexicon.as_ref().unwrap().text, "to create.");
    // The panel paints the number, the Morphology, the glosses and the reference.
    for t in [
        "H1254A",
        "Qal perfect, 3rd masc. sing.",
        "Genesis 1:1 (HEB)",
    ] {
        assert!(h.find(t).is_some(), "{t:?} missing: {:?}", h.texts());
    }
    assert!(
        h.texts().iter().any(|t| t.contains("he created")),
        "{:?}",
        h.texts()
    );
    assert!(
        h.texts().iter().any(|t| t.contains("to create.")),
        "{:?}",
        h.texts()
    );
}

/// A word of several morphemes: the prefix is listed under Parts, the stem is the main card,
/// and the contextual gloss joins both.
#[test]
fn a_prefixed_hebrew_word_lists_its_prefix_under_parts() {
    let (mut h, heb) = hebrew_harness();
    let spot = word_spot(&h, heb, "בְּרֵאשִׁ֖ית");
    h.events.push(Event::PointerMoved(spot));
    h.frames(14);
    let shown = h
        .shell
        .st
        .session
        .word_info
        .shown
        .clone()
        .expect("shown after the dwell");
    assert_eq!(shown.main.strongs.to_string(), "H7225");
    assert_eq!(shown.gloss.as_deref(), Some("in beginning"));
    assert_eq!(shown.parts.len(), 1);
    assert_eq!(shown.parts[0].card.strongs.to_string(), "H9003");
    assert!(h.find("PARTS").is_some(), "{:?}", h.texts());
    assert!(h.find("prefix").is_some());
    assert!(h.find("H9003").is_some());
}

/// Acceptance: a KJV+Strong's word shows the English word plus the Lemma card (the Lemma
/// borrowed from the Lexicon's headword; the unreadable TH code only in a tooltip).
#[test]
fn a_kjv_strongs_word_shows_the_english_word_and_the_lemma_card() {
    let mut h = Harness::new();
    add_doc(&mut h.shell, kjv_strongs_doc());
    add_doc(&mut h.shell, hebrew_lexicon_doc());
    h.shell.run("resource.open", &json!({ "id": "kjv-test" }));
    h.frames(4);
    h.click(h.tab_at("Word Info"));
    let kjv = h.tab_id("KJVT");
    let spot = word_spot(&h, kjv, "created");
    h.events.push(Event::PointerMoved(spot));
    h.frames(14);
    let shown = h
        .shell
        .st
        .session
        .word_info
        .shown
        .clone()
        .expect("shown after the dwell");
    assert_eq!(shown.written, "created");
    assert_eq!(
        shown.transliteration, None,
        "an English word is not transliterated"
    );
    assert_eq!(shown.main.strongs.to_string(), "H1254A");
    assert_eq!(
        shown.main.lemma.as_deref(),
        Some("בָּרָא"),
        "the Lexicon's headword"
    );
    assert_eq!(shown.main.morphology, None, "a TH code is not read");
    assert_eq!(shown.main.morph_code.as_deref(), Some("strongMorph:TH8804"));
    assert_eq!(shown.main.lexicon.as_ref().unwrap().text, "to create.");
    assert_eq!(shown.reference, "Genesis 1:1 (KJVT)");
    for t in ["H1254A", "Genesis 1:1 (KJVT)"] {
        assert!(h.find(t).is_some(), "{t:?} missing: {:?}", h.texts());
    }
    assert!(
        h.texts().iter().any(|t| t.contains("to create.")),
        "{:?}",
        h.texts()
    );
}

/// A KJV-style Bible at Gen 12:1 whose `Abram` carries Abram's own Strong's number, `H87`
/// (Abraham is `H85`).
fn kjv_abram_doc() -> Arc<ReadingDoc> {
    use obs_model::{ResourceType, TextAccumulator, UnitId, UnitKind};
    use obs_scripture::{Book, Verse};
    let mut m = Manifest::new("kjv-abram", "KJV Abram", "KJVA", ResourceType::Bible, "en");
    m.key_schemes = vec![KeyScheme::Passage];
    m.license = "LicenseRef-PD".into();
    let mut b = ResourceBuilder::new(m);
    let genesis = UnitId::book(Book::Genesis);
    let mut t = TextAccumulator::new();
    let v1 = t.push("Now the LORD had said unto Abram, Get thee out of thy country.");
    let end = t.pos();
    let text = t.into_string();
    b.add_unit(genesis.clone(), UnitKind::BibleBook, None, text.clone());
    b.add_span(
        &genesis,
        0..end,
        SpanKind::Para {
            style: ParaStyle::Prose,
        },
    );
    let start = text.find("Abram").unwrap();
    b.add_span(
        &genesis,
        start..start + "Abram".len(),
        SpanKind::TaggedWord {
            parts: vec![MorphemePart {
                strongs: "H87".parse().unwrap(),
                lemma: None,
                morph: None,
                gloss: None,
            }],
        },
    );
    b.add_verse(&genesis, v1, Verse::new(Book::Genesis, 12, 1), 1);
    Arc::new(ReadingDoc::from_resource(b.build().unwrap()))
}

/// #149: Word Info on Abram (`H87`, not Abraham's unified `H85`) offers Abraham's Factbook,
/// with nothing else running: the People & places directory loads by itself on a cold
/// start, and the link opens his Factbook.
#[test]
fn word_info_on_abram_offers_abrahams_factbook_from_a_cold_start() {
    let dir = std::env::temp_dir().join(format!("obs-word-info-abram-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut h = Harness::new();
    let path = dir.join("tipnr.obr");
    obs_store::write_obr(&obs_engine::testing::tipnr(), &path).unwrap();
    h.shell.st.session.library.add_file(&path).unwrap();
    add_doc(&mut h.shell, kjv_abram_doc());
    h.shell.run("resource.open", &json!({ "id": "kjv-abram" }));
    h.frames(4);
    h.click(h.tab_at("Word Info"));
    let kjv = h.tab_id("KJVA");
    let spot = word_spot(&h, kjv, "Abram");
    h.events.push(Event::PointerMoved(spot));
    h.frames(14);
    let shown = h.shell.st.session.word_info.shown.clone().unwrap();
    assert_eq!(shown.main.strongs.to_string(), "H87");
    assert!(!h.shell.st.session.guides.is_busy());
    assert!(!h.shell.st.session.word_study.is_busy());
    for _ in 0..20 {
        if h.find("Factbook: Abraham").is_some() {
            break;
        }
        h.frame();
    }
    let link = h
        .find("Factbook: Abraham")
        .unwrap_or_else(|| panic!("no Factbook link: {:?}", h.texts()));
    h.click(link.center());
    h.frames(3);
    let ws = &h.shell.st.session.workspace;
    let t = ws.tab(ws.focused().unwrap()).unwrap();
    assert_eq!(t.kind, TabKind::Guide(obs_engine::Guide::Factbook));
    assert_eq!(
        t.entry_history.current(),
        Some(Key::Headword("Abraham@Gen.11.26-1Pe".into()))
    );
    drop(h);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Every horizontal line segment painted last frame, as (x0, x1, y).
fn horizontal_lines(h: &Harness) -> Vec<(f32, f32, f32)> {
    fn walk(shape: &Shape, out: &mut Vec<(f32, f32, f32)>) {
        match shape {
            Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
            Shape::LineSegment { points: [a, b], .. } if (a.y - b.y).abs() < 0.1 => {
                out.push((a.x.min(b.x), a.x.max(b.x), a.y));
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    for c in &h.last_shapes {
        walk(&c.shape, &mut out);
    }
    out
}

/// Acceptance: pinning holds the panel while hovering elsewhere. Clicking a word pins it (📌
/// in the header); hover still underlines other words but no longer changes the panel; Esc
/// lets it follow hover again.
#[test]
fn pinning_holds_the_panel_while_hovering_elsewhere() {
    let (mut h, heb) = hebrew_harness();
    let bara = word_spot(&h, heb, "בָּרָ֣א");
    h.click(bara);
    assert!(h.shell.st.session.word_info.pinned, "a click pins at once");
    assert_eq!(
        h.shell
            .st
            .session
            .word_info
            .shown
            .as_ref()
            .unwrap()
            .main
            .strongs
            .to_string(),
        "H1254A"
    );
    assert!(h.find("Pinned").is_some(), "{:?}", h.texts());
    // Hovering another Tagged word underlines it but the panel holds.
    let elohim = word_spot(&h, heb, "אֱלֹהִ֑ים");
    h.events.push(Event::PointerMoved(elohim));
    h.frames(14);
    assert_eq!(
        h.shell
            .st
            .session
            .word_info
            .shown
            .as_ref()
            .unwrap()
            .main
            .strongs
            .to_string(),
        "H1254A",
        "pinned: the panel holds while hovering elsewhere"
    );
    assert!(
        horizontal_lines(&h)
            .iter()
            .any(|(x0, x1, y)| *x0 <= elohim.x && elohim.x <= *x1 && (*y - elohim.y).abs() < 30.0),
        "the hovered word is underlined while pinned"
    );
    // Esc unpins: the panel follows hover again.
    h.events.push(Event::Key {
        key: egui::Key::Escape,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::NONE,
    });
    h.frames(2);
    assert!(!h.shell.st.session.word_info.pinned);
    assert!(h.find("Follows hover").is_some(), "{:?}", h.texts());
    h.events.push(Event::PointerMoved(bara));
    h.frames(2);
    h.events.push(Event::PointerMoved(elohim));
    h.frames(14);
    assert_eq!(
        h.shell
            .st
            .session
            .word_info
            .shown
            .as_ref()
            .unwrap()
            .main
            .strongs
            .to_string(),
        "H430",
        "unpinned: the panel follows hover again"
    );
}

/// The 📌 chip in the header unpins too, and Copy puts the word's cards on the clipboard.
#[test]
fn the_header_chip_unpins_and_copy_writes_the_cards() {
    let (mut h, heb) = hebrew_harness();
    let bara = word_spot(&h, heb, "בָּרָ֣א");
    h.click(bara);
    assert!(h.shell.st.session.word_info.pinned);
    h.click_text("Pinned");
    assert!(!h.shell.st.session.word_info.pinned);
    assert!(h.find("Follows hover").is_some());
    // Copy: the command returns the card text (the clipboard itself is the OS's).
    let out = h
        .shell
        .st
        .session
        .run("word_info.copy", &json!({}))
        .unwrap();
    let text = out["text"].as_str().unwrap();
    assert!(text.contains("H1254A"), "{text}");
    assert!(text.contains("Genesis 1:1 (HEB)"), "{text}");
    // Word Study of this word opens the shown word (#55); Search this word works (#59).
    assert!(h.shell.st.session.is_enabled("word_info.word_study"));
    assert!(h.shell.st.session.is_enabled("word_info.search"));
    assert!(h.find("Word Study of this word").is_some());
    assert!(h.find("Search this word").is_some());
}

/// Reproducible screenshots for the PR evidence (run with UPDATE_PREVIEWS=1).
#[test]
fn word_info_previews() {
    if std::env::var_os("UPDATE_PREVIEWS").is_none() {
        return;
    }
    let (mut h, heb) = hebrew_harness();
    add_doc(&mut h.shell, hebrew_lexicon_doc());
    let dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/previews/word-info");
    std::fs::create_dir_all(&dir).unwrap();
    let spot = word_spot(&h, heb, "בָּרָ֣א");
    h.events.push(Event::PointerMoved(spot));
    h.frames(20);
    while h.shell.st.snippets.has_pending() {
        h.frame();
    }
    h.frames(2);
    h.screenshot(&dir.join("word-info-hover.ppm"));
    h.click(spot);
    let elohim = word_spot(&h, heb, "אֱלֹהִ֑ים");
    h.events.push(Event::PointerMoved(elohim));
    h.frames(20);
    while h.shell.st.snippets.has_pending() {
        h.frame();
    }
    h.frames(2);
    h.screenshot(&dir.join("word-info-pinned.ppm"));
}

/// A Resource imported from the SWORD test modules (`crates/formats/tests/assets/sword`).
fn sword_doc(module: &str) -> Arc<ReadingDoc> {
    let source = obs_formats::SwordSource::open(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../formats/tests/assets/sword"),
    )
    .unwrap();
    let (resource, _) =
        obs_formats::import_sword(&source, module, &obs_formats::SwordOptions::default()).unwrap();
    Arc::new(ReadingDoc::from_resource(resource))
}

/// Acceptance (#133): hovering a word in an imported SWORD Greek NT (tagged as CrossWire's
/// Byz and MorphGNT are) shows its Lemma, plain-English Morphology and the Lexicon's Gloss.
#[test]
fn hovering_a_word_in_an_imported_sword_greek_nt_shows_lemma_morphology_and_gloss() {
    let mut h = Harness::new();
    let gnt = sword_doc("ObsGreekNt");
    let gnt_id = gnt.manifest.resource_id.clone();
    add_doc(&mut h.shell, gnt);
    add_doc(&mut h.shell, sword_doc("ObsGreek"));
    h.shell.run("resource.open", &json!({ "id": gnt_id }));
    h.frames(4);
    h.click(h.tab_at("Word Info"));
    let tab = h.tab_id("OBS-GNT");

    // John 3:16 ἠγάπησεν: `strong:G25 strong:G5656`, `robinson:V-AAI-3S`; no Lemma in the
    // text, so the Lexicon's headword is the Lemma.
    let spot = word_spot(&h, tab, "ἠγάπησεν");
    h.events.push(Event::PointerMoved(spot));
    h.frames(14);
    let shown = h
        .shell
        .st
        .session
        .word_info
        .shown
        .clone()
        .expect("shown after the dwell");
    assert_eq!(shown.main.strongs.to_string(), "G25");
    assert!(shown.parts.is_empty(), "G5656 is not a word: {shown:?}");
    assert_eq!(
        shown.main.morphology.as_deref(),
        Some("aorist active indicative, 3rd sing.")
    );
    assert_eq!(shown.main.morph_code.as_deref(), Some("V-AAI-3S"));
    assert!(shown.main.lemma.is_some(), "{shown:?}");
    let gloss = shown.main.lexicon.as_ref().expect("a Lexicon gloss");
    assert!(gloss.text.contains("to love"), "{gloss:?}");
    assert_eq!(shown.reference, "John 3:16 (OBS-GNT)");
    assert!(shown.transliteration.is_some());
    for t in [
        "G25",
        "aorist active indicative, 3rd sing.",
        "John 3:16 (OBS-GNT)",
    ] {
        assert!(h.find(t).is_some(), "{t:?} missing: {:?}", h.texts());
    }
    assert!(
        h.texts().iter().any(|t| t.contains("to love")),
        "{:?}",
        h.texts()
    );
}
