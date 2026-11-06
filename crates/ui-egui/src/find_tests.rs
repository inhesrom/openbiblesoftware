//! Find in Resource (#175) through the shell's real-event headless harness: Ctrl+F, typing,
//! the count, Enter / Shift+Enter / ▲ ▼, Esc, the options, Greek and Hebrew without marks,
//! highlights across a Text size change, Text Comparison rows and the hand-off to Search.

use egui::Key;
use obs_engine::TabId;
use obs_model::{
    KeyScheme, Manifest, ParaStyle, ResourceBuilder, ResourceType, SpanKind, TextAccumulator,
    UnitId, UnitKind,
};
use obs_scripture::{Book, Standard, Verse, Versification};

use super::*;
use crate::shell::find_bar::{FIND_HINT, field_id};

/// Ctrl as Linux and Windows report it.
const CTRL: Modifiers = Modifiers {
    ctrl: true,
    command: true,
    ..Modifiers::NONE
};

/// Verses of John (KJV, public domain) where "love" is; the rest of the fixture is filler.
const JOHN_LOVE: &[(u16, u16, &str)] = &[
    (
        3,
        16,
        "For God so loved the world, that he gave his only begotten Son, that whosoever \
         believeth in him should not perish, but have everlasting life.",
    ),
    (
        3,
        19,
        "And this is the condemnation, that light is come into the world, and men loved \
         darkness rather than light, because their deeds were evil.",
    ),
    (
        3,
        35,
        "The Father loveth the Son, and hath given all things into his hand.",
    ),
    (
        13,
        34,
        "A new commandment I give unto you, That ye love one another; as I have loved you, \
         that ye also love one another.",
    ),
    (
        13,
        35,
        "By this shall all men know that ye are my disciples, if ye have love one to another.",
    ),
    (14, 15, "If ye love me, keep my commandments."),
    (
        15,
        9,
        "As the Father hath loved me, so have I loved you: continue ye in my love.",
    ),
    (
        15,
        13,
        "Greater love hath no man than this, that a man lay down his life for his friends.",
    ),
    (
        21,
        15,
        "So when they had dined, Jesus saith to Simon Peter, Simon, son of Jonas, lovest thou \
         me more than these? He saith unto him, Yea, Lord; thou knowest that I love thee. He \
         saith unto him, Feed my lambs.",
    ),
    (
        21,
        17,
        "He saith unto him the third time, Simon, son of Jonas, Lovest thou me? Peter was \
         grieved because he said unto him the third time, Lovest thou me? And he said unto \
         him, Lord, thou knowest all things; thou knowest that I LOVE thee. Jesus saith unto \
         him, Feed my sheep.",
    ),
];

const FILLER: &[&str] = &[
    "and", "Jesus", "answered", "said", "unto", "them", "verily", "I", "say", "you", "the",
    "disciples", "went", "into", "Galilee", "with", "him", "came", "to", "Jerusalem",
];

/// The Gospel of John, every chapter at its KJV length, one paragraph per five verses: the
/// verses of [`JOHN_LOVE`] in their KJV words, the others filler without "love".
fn kjv_john() -> Arc<ReadingDoc> {
    let mut verses = Vec::new();
    for chapter in 1..=21u16 {
        for v in 1..=Standard.last_verse(Book::John, chapter).unwrap() {
            let text = JOHN_LOVE
                .iter()
                .find(|(c, n, _)| (*c, *n) == (chapter, v))
                .map_or_else(
                    || {
                        let n = 14 + (usize::from(v) * 5 + usize::from(chapter)) % 17;
                        let words: Vec<&str> = (0..n)
                            .map(|i| FILLER[(i * 7 + usize::from(v)) % FILLER.len()])
                            .collect();
                        format!("{}.", words.join(" "))
                    },
                    |(_, _, text)| (*text).to_string(),
                );
            verses.push((chapter, v, text));
        }
    }
    english_bible("kjv-john", "KJV John", "KJVJ", Book::John, &verses)
}

/// An English Bible of one book's `verses` (chapter, verse, text), one paragraph per five
/// verses and at each chapter's end.
fn english_bible(
    id: &str,
    title: &str,
    abbrev: &str,
    book: Book,
    verses: &[(u16, u16, String)],
) -> Arc<ReadingDoc> {
    let mut m = Manifest::new(id, title, abbrev, ResourceType::Bible, "en");
    m.key_schemes = vec![KeyScheme::Passage];
    m.license = "LicenseRef-PD".into();
    let mut b = ResourceBuilder::new(m);
    let unit = UnitId::book(book);
    let mut t = TextAccumulator::new();
    let mut spans = Vec::new();
    let mut paras = Vec::new();
    let mut para_start = t.pos();
    for (k, (chapter, v, text)) in verses.iter().enumerate() {
        if t.pos() > para_start {
            t.push(" ");
        }
        let r = t.push(text);
        spans.push((r.clone(), Verse::new(book, *chapter, *v)));
        let chapter_ends = verses.get(k + 1).is_none_or(|(c, _, _)| c != chapter);
        if v % 5 == 0 || chapter_ends {
            paras.push(para_start..r.end);
            t.push("\n");
            para_start = t.pos();
        }
    }
    b.add_unit(
        unit.clone(),
        UnitKind::BibleBook,
        Some(book.name().into()),
        t.into_string(),
    );
    for p in paras {
        b.add_span(
            &unit,
            p,
            SpanKind::Para {
                style: ParaStyle::Prose,
            },
        );
    }
    for (r, v) in spans {
        b.add_verse(&unit, r, v, v.verse);
    }
    Arc::new(ReadingDoc::from_resource(b.build().unwrap()))
}

/// 1 John 4:7–8 in Greek (public domain), as a one-chapter Greek Bible.
fn greek_1_john_4() -> Arc<ReadingDoc> {
    let mut m = Manifest::new("grk-test", "Greek test", "GRK", ResourceType::Bible, "grc");
    m.key_schemes = vec![KeyScheme::Passage];
    m.license = "LicenseRef-PD".into();
    let mut b = ResourceBuilder::new(m);
    let unit = UnitId::book(Book::FirstJohn);
    let mut t = TextAccumulator::new();
    let v7 = t.push(
        "Ἀγαπητοί, ἀγαπῶμεν ἀλλήλους, ὅτι ἡ ἀγάπη ἐκ τοῦ θεοῦ ἐστιν, καὶ πᾶς ὁ ἀγαπῶν ἐκ τοῦ \
         θεοῦ γεγέννηται καὶ γινώσκει τὸν θεόν.",
    );
    t.push(" ");
    let v8 = t.push("ὁ μὴ ἀγαπῶν οὐκ ἔγνω τὸν θεόν, ὅτι ὁ θεὸς ἀγάπη ἐστίν.");
    let end = t.pos();
    b.add_unit(
        unit.clone(),
        UnitKind::BibleBook,
        Some("1 John".into()),
        t.into_string(),
    );
    b.add_span(
        &unit,
        0..end,
        SpanKind::Para {
            style: ParaStyle::Prose,
        },
    );
    b.add_verse(&unit, v7, Verse::new(Book::FirstJohn, 4, 7), 7);
    b.add_verse(&unit, v8, Verse::new(Book::FirstJohn, 4, 8), 8);
    Arc::new(ReadingDoc::from_resource(b.build().unwrap()))
}

/// Visual evidence for `docs/previews/find-<name>-kittest.png` when `UPDATE_PREVIEWS` is set.
fn preview(h: &Harness, name: &str) {
    if std::env::var_os("UPDATE_PREVIEWS").is_some() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/previews");
        std::fs::create_dir_all(&dir).unwrap();
        h.screenshot(&dir.join(format!("find-{name}-kittest.ppm")));
    }
}

fn key(h: &mut Harness, key: Key, modifiers: Modifiers) {
    h.modifiers = modifiers;
    h.events.push(Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    });
    h.frame();
    h.events.push(Event::Key {
        key,
        physical_key: None,
        pressed: false,
        repeat: false,
        modifiers,
    });
    h.modifiers = Modifiers::NONE;
    h.frames(2);
}

fn type_text(h: &mut Harness, text: &str) {
    h.events.push(Event::Text(text.into()));
    h.frames(2);
}

/// Open `doc` in a new Panel (it is the focused Tab).
fn open(h: &mut Harness, doc: Arc<ReadingDoc>) -> TabId {
    let id = doc.manifest.resource_id.clone();
    add_doc(&mut h.shell, doc);
    let tab = h
        .shell
        .run("resource.open", &json!({ "id": id, "split": true }))
        .unwrap()["tab"]
        .as_u64()
        .unwrap();
    h.frames(3);
    TabId(tab)
}

fn bar(h: &Harness, tab: TabId) -> Option<&crate::find::FindBar> {
    h.shell.st.tabs.get(&tab).and_then(|t| t.find.as_ref())
}

/// Frames until the Find bar's worker has answered.
fn settle(h: &mut Harness, tab: TabId) {
    for _ in 0..500 {
        h.paced_frame();
        if bar(h, tab).is_some_and(|b| b.status() != crate::find::FindStatus::Finding) {
            h.frames(2);
            return;
        }
    }
    panic!("finding never finished");
}

/// The matches' texts, in order.
fn found_texts(h: &Harness, tab: TabId) -> Vec<String> {
    let t = &h.shell.st.tabs[&tab];
    let doc = &t.view.as_ref().unwrap().doc;
    t.find
        .as_ref()
        .unwrap()
        .found()
        .matches
        .iter()
        .map(|m| doc.units[m.unit].text[m.range.clone()].to_string())
        .collect()
}

fn current(h: &Harness, tab: TabId) -> Option<usize> {
    bar(h, tab).and_then(|b| b.current)
}

/// Rectangles filled with `color` last frame.
fn filled(h: &Harness, color: egui::Color32) -> Vec<Rect> {
    h.rects
        .iter()
        .filter(|(_, c)| *c == color)
        .map(|(r, _)| *r)
        .collect()
}

/// How many glyphs were drawn with their centre inside `r`.
fn glyphs_in(h: &Harness, r: Rect) -> usize {
    h.glyphs.iter().filter(|g| r.contains(g.center())).count()
}

#[test]
fn ctrl_f_love_in_kjv_john_counts_every_hit_enter_steps_through_all_in_order_and_esc_clears() {
    let mut h = Harness::new();
    let doc = kjv_john();
    // Every "love" in the text, however written, in document order.
    let expected: Vec<(usize, usize)> = doc
        .units
        .iter()
        .enumerate()
        .flat_map(|(u, unit)| {
            unit.text
                .to_lowercase()
                .match_indices("love")
                .map(move |(b, _)| (u, b))
                .collect::<Vec<_>>()
        })
        .collect();
    let n = expected.len();
    assert_eq!(n, 17, "the fixture's count");
    let tab = open(&mut h, doc);
    assert!(h.find(FIND_HINT).is_none(), "no Find bar before Ctrl+F");

    key(&mut h, Key::F, CTRL);
    assert!(h.find(FIND_HINT).is_some(), "{:?}", h.texts());
    assert!(h.ctx.memory(|m| m.has_focus(field_id(tab))), "the keyboard is in the field");
    type_text(&mut h, "love");
    settle(&mut h, tab);
    let matches: Vec<(usize, usize)> = bar(&h, tab)
        .unwrap()
        .found()
        .matches
        .iter()
        .map(|m| (m.unit, m.range.start))
        .collect();
    assert_eq!(matches, expected);
    let texts = found_texts(&h, tab);
    assert!(texts.contains(&"Love".to_string()) && texts.contains(&"LOVE".to_string()));
    // From the top of John, the first hit (Jn 3:16) is current: "1 of 17".
    assert_eq!(current(&h, tab), Some(0));
    assert!(h.find(&format!("1 of {n}")).is_some(), "{:?}", h.texts());
    let palette = h.shell.st.palette;
    assert!(!filled(&h, palette.find_match).is_empty(), "other hits are drawn");
    preview(&h, "kjv-john-love");

    // Enter goes through every hit in order, each scrolled into view and drawn as current
    // (over the four letters of "love"), then wraps to the first.
    let mut moved = 0;
    for k in 1..=n {
        let top = h.shell.st.tabs.get_mut(&tab).unwrap().view.as_mut().unwrap().text_anchor();
        key(&mut h, Key::Enter, Modifiers::NONE);
        let want = k % n;
        assert_eq!(current(&h, tab), Some(want), "Enter #{k}");
        assert!(
            h.find(&format!("{} of {n}", want + 1)).is_some(),
            "Enter #{k}: {:?}",
            h.texts()
        );
        let now = filled(&h, palette.search_hit);
        assert_eq!(now.len(), 1, "Enter #{k}: one current hit drawn");
        assert_eq!(glyphs_in(&h, now[0]), 4, "Enter #{k}: over “love”");
        let after = h.shell.st.tabs.get_mut(&tab).unwrap().view.as_mut().unwrap().text_anchor();
        if after != top {
            moved += 1;
        }
        assert!(
            h.ctx.memory(|m| m.has_focus(field_id(tab))),
            "the keyboard stays in the field"
        );
    }
    assert!(moved >= 5, "hits out of view were scrolled to ({moved})");
    // Shift+Enter and ▲ go back; ▼ forward.
    key(&mut h, Key::Enter, Modifiers::SHIFT);
    assert_eq!(current(&h, tab), Some(n - 1));
    let [up, down] = bar(&h, tab).unwrap().arrows.unwrap();
    h.click(up.center());
    assert_eq!(current(&h, tab), Some(n - 2));
    h.click(down.center());
    assert_eq!(current(&h, tab), Some(n - 1));

    // Esc closes the bar and clears every highlight.
    h.ctx.memory_mut(|m| m.request_focus(field_id(tab)));
    h.frames(2);
    key(&mut h, Key::Escape, Modifiers::NONE);
    assert!(bar(&h, tab).is_none());
    assert!(h.find(FIND_HINT).is_none());
    assert!(filled(&h, palette.find_match).is_empty());
    assert!(filled(&h, palette.search_hit).is_empty());
    // Ctrl+F opens a new, empty one.
    key(&mut h, Key::F, CTRL);
    assert!(h.find(FIND_HINT).is_some());
    assert_eq!(bar(&h, tab).unwrap().status(), crate::find::FindStatus::Empty);
}

#[test]
fn whole_word_and_match_case_narrow_the_hits() {
    let mut h = Harness::new();
    let tab = open(&mut h, kjv_john());
    key(&mut h, Key::F, CTRL);
    type_text(&mut h, "love");
    settle(&mut h, tab);
    assert_eq!(found_texts(&h, tab).len(), 17);
    h.click_text("Whole word");
    settle(&mut h, tab);
    // love ×7, LOVE: not loved, loveth, lovest.
    assert_eq!(found_texts(&h, tab).len(), 8);
    assert!(h.find("1 of 8").is_some(), "{:?}", h.texts());
    h.click_text("Aa");
    settle(&mut h, tab);
    assert_eq!(found_texts(&h, tab), vec!["love"; 7]);
    h.click_text("Whole word");
    h.click_text("Aa");
    key(&mut h, Key::F, CTRL);
    type_text(&mut h, "unicorn");
    settle(&mut h, tab);
    assert!(h.find("No matches").is_some(), "{:?}", h.texts());
}

#[test]
fn ignore_diacritics_finds_greek_and_hebrew_typed_without_accents_or_points() {
    let mut h = Harness::new();
    // Greek typed without accents and breathings, then in Latin letters.
    let tab = open(&mut h, greek_1_john_4());
    key(&mut h, Key::F, CTRL);
    type_text(&mut h, "αγαπη");
    settle(&mut h, tab);
    assert_eq!(found_texts(&h, tab), ["Ἀγαπη", "ἀγάπη", "ἀγάπη"]);
    assert!(h.find("1 of 3").is_some(), "{:?}", h.texts());
    h.click_text("Whole word");
    settle(&mut h, tab);
    assert_eq!(found_texts(&h, tab), ["ἀγάπη", "ἀγάπη"]);
    key(&mut h, Key::F, CTRL);
    type_text(&mut h, "agape");
    settle(&mut h, tab);
    assert_eq!(found_texts(&h, tab), ["ἀγάπη", "ἀγάπη"]);
    preview(&h, "greek-agape");
    key(&mut h, Key::F, CTRL);
    type_text(&mut h, "θεος");
    settle(&mut h, tab);
    assert_eq!(found_texts(&h, tab), ["θεὸς"]);
    // With Ignore diacritics off, only the marks as written match.
    h.click_text("Ignore diacritics");
    settle(&mut h, tab);
    assert!(found_texts(&h, tab).is_empty());
    assert!(h.find("No matches").is_some());
    key(&mut h, Key::F, CTRL);
    type_text(&mut h, "θεὸς");
    settle(&mut h, tab);
    assert_eq!(found_texts(&h, tab), ["θεὸς"]);

    // Hebrew typed without points (and a final mem typed as written).
    let heb = TabId(
        h.shell
            .run("resource.open", &json!({ "id": "heb-test", "split": true }))
            .unwrap()["tab"]
            .as_u64()
            .unwrap(),
    );
    h.frames(3);
    key(&mut h, Key::F, CTRL);
    type_text(&mut h, "בראשית");
    settle(&mut h, heb);
    assert_eq!(found_texts(&h, heb), ["בְּרֵאשִׁ֖ית"]);
    key(&mut h, Key::F, CTRL);
    type_text(&mut h, "אלהים");
    settle(&mut h, heb);
    assert_eq!(found_texts(&h, heb), ["אֱלֹהִ֑ים"]);
    // The highlight is drawn over the right-to-left word (the Greek Tab still shows its own).
    let tiles = &h.shell.tree.tiles;
    let pane = tiles.rect(tiles.find_pane(&heb).unwrap()).unwrap();
    let rects: Vec<Rect> = filled(&h, h.shell.st.palette.search_hit)
        .into_iter()
        .filter(|r| pane.contains(r.center()))
        .collect();
    assert_eq!(rects.len(), 1);
    assert!(glyphs_in(&h, rects[0]) >= 4, "over אֱלֹהִ֑ים");
    preview(&h, "hebrew-elohim");
}

/// Every Find rectangle drawn last frame, with how many glyphs are inside each.
fn find_rects(h: &Harness) -> Vec<(Rect, usize)> {
    let p = h.shell.st.palette;
    filled(h, p.find_match)
        .into_iter()
        .chain(filled(h, p.search_hit))
        .map(|r| (r, glyphs_in(h, r)))
        .collect()
}

#[test]
fn highlights_stay_on_their_words_after_a_text_size_change() {
    let mut h = Harness::new();
    let tab = open(&mut h, kjv_john());
    key(&mut h, Key::F, CTRL);
    type_text(&mut h, "love");
    settle(&mut h, tab);
    // Go to a crowded place: Jn 13:34 has three hits.
    for _ in 0..3 {
        key(&mut h, Key::Enter, Modifiers::NONE);
    }
    preview(&h, "default-text");
    let before = find_rects(&h);
    assert!(before.len() >= 3, "{before:?}");
    // Each covers exactly the four letters of "love".
    assert!(before.iter().all(|(_, g)| *g == 4), "{before:?}");
    let was = current(&h, tab);
    let size = h.shell.st.session.text.size;
    key(&mut h, Key::Equals, CTRL);
    key(&mut h, Key::Equals, CTRL);
    h.frames(3);
    assert_eq!(h.shell.st.session.text.size, size + 2.0);
    let after = find_rects(&h);
    assert!(after.len() >= 3, "{after:?}");
    assert!(after.iter().all(|(_, g)| *g == 4), "{after:?}");
    // Larger text: taller highlights, the same current hit, still in view.
    let height = |v: &[(Rect, usize)]| v.iter().map(|(r, _)| r.height()).fold(0.0, f32::max);
    assert!(height(&after) > height(&before));
    assert_eq!(current(&h, tab), was);
    preview(&h, "larger-text");
    assert_eq!(filled(&h, h.shell.st.palette.search_hit).len(), 1);
}

#[test]
fn text_comparison_rows_are_found_and_stepped_through() {
    let mut h = Harness::new();
    let tb = h.tab_id("TB");
    h.shell.st.session.workspace.user_focused(tb);
    h.shell
        .run("tool.text_comparison", &json!({ "split": true }));
    h.frames(2);
    let cmp = h.tab_id("Text Comparison");
    h.shell
        .run("tab.go", &json!({ "tab": cmp.0, "text": "Gen 31:1" }));
    h.frames(4);
    for _ in 0..200 {
        h.paced_frame();
    }
    key(&mut h, Key::F, CTRL);
    assert!(h.find(FIND_HINT).is_some(), "{:?}", h.texts());
    type_text(&mut h, "Laban");
    settle(&mut h, cmp);
    let view = h.shell.st.tabs[&cmp].comparison.as_ref().unwrap();
    let rows = view.find_rows();
    let expected: usize = rows.iter().map(|(_, _, t)| t.matches("Laban").count()).sum();
    let found = bar(&h, cmp).unwrap().found();
    assert_eq!(found.matches.len(), expected);
    assert!(expected > 10);
    // Two Bibles share this text: the same verse's hits come row by row.
    assert!(found.matches.windows(2).all(|w| {
        (w[0].unit, w[0].row, w[0].range.start) < (w[1].unit, w[1].row, w[1].range.start)
    }));
    preview(&h, "text-comparison");
    for k in 1..=expected {
        key(&mut h, Key::Enter, Modifiers::NONE);
        assert_eq!(current(&h, cmp), Some(k % expected));
        assert_eq!(
            filled(&h, h.shell.st.palette.search_hit).len(),
            1,
            "Enter #{k}: the current hit is in view"
        );
    }
}

#[test]
fn alt_enter_hands_the_find_text_to_the_search_tool() {
    let mut h = Harness::new();
    let tab = open(&mut h, kjv_john());
    key(&mut h, Key::F, CTRL);
    type_text(&mut h, "love one another");
    key(&mut h, Key::Enter, Modifiers::ALT);
    h.frames(3);
    let search = h
        .shell
        .st
        .session
        .workspace
        .tabs()
        .find(|t| t.kind == TabKind::Tool(Tool::Search))
        .map(|t| t.id)
        .expect("a Search Tab");
    let st = h.shell.st.session.search_tab(search).unwrap();
    assert_eq!(st.query, "\"love one another\"");
    assert_eq!(st.mode, obs_engine::SearchMode::Bible);
    let mut bibles = st.bibles(&h.shell.st.session.library);
    bibles.sort();
    assert!(bibles.contains(&"kjv-john".to_string()) && bibles.len() >= 3, "{bibles:?}");
    // The Find bar stays as it was.
    assert_eq!(bar(&h, tab).unwrap().text, "love one another");
}

#[test]
fn ctrl_f_with_a_selection_finds_the_selected_words() {
    let mut h = Harness::new();
    let tab = open(&mut h, kjv_john());
    {
        let view = h.shell.st.tabs.get_mut(&tab).unwrap().view.as_mut().unwrap();
        let text = &view.doc.units[0].text;
        let at = text.find("everlasting life").unwrap();
        view.selection = Some(crate::Selection {
            anchor: crate::DocPos { unit: 0, byte: at },
            head: crate::DocPos {
                unit: 0,
                byte: at + "everlasting life".len(),
            },
            granularity: crate::Granularity::Grapheme,
        });
    }
    key(&mut h, Key::F, CTRL);
    assert_eq!(bar(&h, tab).unwrap().text, "everlasting life");
    settle(&mut h, tab);
    assert_eq!(found_texts(&h, tab), ["everlasting life"]);
}

#[test]
fn the_tab_menu_offers_find_with_its_shortcut() {
    let mut h = Harness::new();
    let tab = open(&mut h, kjv_john());
    let pane = {
        let tiles = &h.shell.tree.tiles;
        tiles.rect(tiles.find_pane(&tab).unwrap()).unwrap()
    };
    let dots = h
        .shapes
        .iter()
        .rev()
        .find(|(r, t)| t == "…" && pane.contains(r.center()))
        .map(|(r, _)| r.center())
        .expect("the Tab's ⋯ menu");
    h.click(dots);
    assert!(h.find("Ctrl+F").is_some(), "{:?}", h.texts());
    h.click_text("Find…");
    assert!(bar(&h, tab).is_some());
    assert!(h.ctx.memory(|m| m.has_focus(field_id(tab))));
}

/// The acceptance run on the real KJV (with Strong's) of the Starter library, which CI does
/// not have: `OBS_KJV_OBR=path/to/kjv-strongs.obr cargo test -p obs-ui-egui real_kjv --
/// --ignored --nocapture`.
#[test]
#[ignore = "needs the real KJV: OBS_KJV_OBR=path/to/kjv-strongs.obr"]
fn real_kjv_john_love_counts_and_enter_visits_every_hit_in_order() {
    let path = std::env::var("OBS_KJV_OBR").expect("OBS_KJV_OBR");
    let doc = Arc::new(ReadingDoc::open(std::path::Path::new(&path)).unwrap());
    let john = doc
        .units
        .iter()
        .position(|u| u.id == UnitId::book(Book::John))
        .expect("John");
    let expected: usize = doc
        .units
        .iter()
        .map(|u| u.text.to_lowercase().matches("love").count())
        .sum();
    let in_john = doc.units[john].text.to_lowercase().matches("love").count();
    let mut h = Harness::new();
    let tab = open(&mut h, doc);
    h.shell
        .run("tab.go", &json!({ "tab": tab.0, "text": "John 1:1" }));
    h.frames(3);
    key(&mut h, Key::F, CTRL);
    type_text(&mut h, "love");
    settle(&mut h, tab);
    let matches = bar(&h, tab).unwrap().found().matches.to_vec();
    assert_eq!(matches.len(), expected);
    let first = current(&h, tab).unwrap();
    assert_eq!(matches[first].unit, john, "the first hit from John 1:1 is in John");
    assert!(first == 0 || matches[first - 1].unit < john);
    let n = matches.len();
    assert!(h.find(&format!("{} of {n}", first + 1)).is_some(), "{:?}", h.texts());
    eprintln!("KJV: {n} hits of “love”, {in_john} in John; John's first is {} of {n}", first + 1);
    let started = std::time::Instant::now();
    for k in 1..=n {
        key(&mut h, Key::Enter, Modifiers::NONE);
        assert_eq!(current(&h, tab), Some((first + k) % n), "Enter #{k}");
        // One rectangle per run of the line it is in: a hit can straddle two (a word of
        // Jesus in red, a Tagged word's edge).
        let now = filled(&h, h.shell.st.palette.search_hit);
        let glyphs: usize = now.iter().map(|r| glyphs_in(&h, *r)).sum();
        let m = &matches[(first + k) % n];
        let unit = &h.shell.st.tabs[&tab].view.as_ref().unwrap().doc.units[m.unit];
        let context = (m.unit, m.range.clone(), &unit.text[m.range.clone()]);
        assert!(!now.is_empty(), "Enter #{k}: the current hit is drawn ({context:?})");
        assert_eq!(glyphs, 4, "Enter #{k}: over “love” ({context:?}, {now:?})");
    }
    eprintln!("Enter visited all {n} hits in order in {:?}", started.elapsed());
    key(&mut h, Key::Escape, Modifiers::NONE);
    assert!(bar(&h, tab).is_none());
    assert!(filled(&h, h.shell.st.palette.find_match).is_empty());
}

#[test]
fn a_hit_at_a_verse_start_covers_its_letters_not_the_verse_number() {
    let mut h = Harness::new();
    let verses = [
        (
            13,
            9,
            "For this, Thou shalt not commit adultery, Thou shalt not kill, Thou shalt not \
             steal, Thou shalt not bear false witness, Thou shalt not covet; and if there be \
             any other commandment, it is briefly comprehended in this saying, namely, Thou \
             shalt love thy neighbour as thyself.",
        ),
        (
            13,
            10,
            "Love worketh no ill to his neighbour: therefore love is the fulfilling of the law.",
        ),
    ]
    .map(|(c, v, t)| (c, v, t.to_string()));
    let tab = open(
        &mut h,
        english_bible("kjv-rom", "KJV Romans", "KJVR", Book::Romans, &verses),
    );
    key(&mut h, Key::F, CTRL);
    type_text(&mut h, "love");
    settle(&mut h, tab);
    assert_eq!(found_texts(&h, tab), ["love", "Love", "love"]);
    key(&mut h, Key::Enter, Modifiers::NONE);
    assert_eq!(current(&h, tab), Some(1));
    // "10" is drawn just before "Love", but only the four letters are marked.
    let rects = filled(&h, h.shell.st.palette.search_hit);
    let glyphs: usize = rects.iter().map(|r| glyphs_in(&h, *r)).sum();
    assert_eq!(glyphs, 4, "{rects:?}");
    assert!(find_rects(&h).iter().all(|(_, g)| *g == 4));
}
