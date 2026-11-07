//! Visual filters (#211) through the shell's real-event headless harness: a filter for the
//! imperatives of Romans 12 switched on from each Tab header's Filters menu colours the
//! Greek imperatives in TAGNT and, through Strong's alignment, the English words that render
//! them in the KJV; a Page tone switch recolours them without laying the text out again; and
//! View → Visual filters… makes a filter, refusing Passages it cannot read.

use egui::Color32;
use obs_engine::{Library, TabId};
use obs_model::SpanKind;

use super::*;
use crate::selection::{DocPos, range_rects};
use crate::view::ReadingView;

struct Files(std::path::PathBuf);
impl Drop for Files {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A User database and a library of real `.obr` files: the KJV and TAGNT at Romans 12:2,
/// 12:13, 12:14 and 13:14.
fn setup(name: &str) -> (Harness, Files) {
    let dir = std::env::temp_dir().join(format!("obs-shell-filters-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut h = Harness::launch(Some(&dir.join("user.db")));
    h.shell.st.session.library = Library::new();
    for r in [
        obs_engine::testing::kjv_romans_12(),
        obs_engine::testing::tagnt_romans_12(),
    ] {
        let path = dir.join(format!("{}.obr", r.manifest.resource_id));
        obs_store::write_obr(&r, &path).unwrap();
        h.shell.st.session.library.add_file(&path).unwrap();
    }
    h.shell.reset_layout();
    h.frames(2);
    (h, Files(dir))
}

fn open_at(h: &mut Harness, id: &str, reference: &str) -> TabId {
    let tab = h
        .shell
        .run("resource.open", &json!({ "id": id, "split": true }))
        .unwrap()["tab"]
        .as_u64()
        .map(TabId)
        .unwrap();
    h.frames(2);
    h.shell
        .run("tab.go", &json!({ "tab": tab.0, "text": reference }));
    settle_all(h);
    tab
}

fn view(h: &Harness, tab: TabId) -> &ReadingView {
    h.shell.st.tabs[&tab].view.as_ref().unwrap()
}

/// The Panel `tab`'s text is in.
fn panel_of(h: &Harness, tab: TabId) -> Rect {
    let [ox, oy] = view(h, tab).text_origin;
    h.panels()
        .into_iter()
        .map(|(r, _)| r)
        .find(|r| r.contains(pos2(ox + 1.0, oy + 1.0)))
        .expect("the Tab's Panel")
}

/// Click `text` in `tab`'s header strip.
fn click_header(h: &mut Harness, tab: TabId, text: &str) {
    let [_, oy] = view(h, tab).text_origin;
    let panel = panel_of(h, tab);
    // A narrow Panel's header says "Filt.".
    let short = text.replacen("Filters", "Filt.", 1);
    let p = h
        .shapes
        .iter()
        .rev()
        .find(|(r, t)| {
            (t == text || *t == short) && panel.contains(r.center()) && r.center().y < oy
        })
        .map(|(r, _)| r.center())
        .unwrap_or_else(|| panic!("no {text:?} in the header: {:?}", h.texts()));
    h.click(p);
}

/// Frames until the filters' matches are found and drawn.
fn settle_filters(h: &mut Harness) {
    for _ in 0..200 {
        h.paced_frame();
        let busy = (h.shell.st.tabs.values()).any(|t| t.filters.is_pending());
        if !busy {
            h.frames(2);
            return;
        }
    }
    panic!("the filters never finished");
}

/// The Tagged words in view in `tab` that a rect painted in `color` covers, in text order.
fn painted_words(h: &Harness, tab: TabId, color: Color32) -> Vec<String> {
    let v = view(h, tab);
    let [ox, oy] = v.text_origin;
    let panel = panel_of(h, tab);
    let painted: Vec<Rect> = (h.rects.iter())
        .filter(|(r, c)| *c == color && panel.contains(r.center()))
        .map(|(r, _)| *r)
        .collect();
    let mut out = Vec::new();
    for (index, _, _) in v.laid_in_view(900.0) {
        let block = &v.doc.blocks[index];
        let unit = &v.doc.units[block.unit];
        for s in &unit.spans {
            let (a, b) = (s.start as usize, s.end as usize);
            if !matches!(s.kind, SpanKind::TaggedWord { .. })
                || a < block.source.range.start
                || b > block.source.range.end
            {
                continue;
            }
            let range = (
                DocPos {
                    unit: block.unit,
                    byte: a,
                },
                DocPos {
                    unit: block.unit,
                    byte: b,
                },
            );
            let covered = range_rects(v, &[range], 900.0)[0]
                .iter()
                .any(|[x0, y0, x1, y1]| {
                    let c = pos2(ox + (x0 + x1) / 2.0, oy + (y0 + y1) / 2.0);
                    painted.iter().any(|p| p.contains(c))
                });
            if covered {
                out.push(unit.text[a..b].to_string());
            }
        }
    }
    out
}

/// Acceptance (#211): "imperatives in Romans 12" colours the right words in TAGNT and, through
/// Strong's alignment, in the KJV, switched on from each Tab's header.
#[test]
fn imperatives_in_romans_12_are_painted_in_tagnt_and_through_strongs_alignment_in_the_kjv() {
    let (mut h, _files) = setup("romans");
    let made = h
        .shell
        .run(
            "filter.save",
            &json!({
                "name": "Imperatives in Romans 12",
                "query": "@imperative",
                "scope": "Rom 12",
                "look": "background",
                "color": "#ffd6aa",
            }),
        )
        .unwrap();
    let id = made["id"].as_str().unwrap().to_string();
    let kjv = open_at(&mut h, "kjv", "Rom 12:2");
    let tagnt = open_at(&mut h, "tagnt", "Rom 12:2");
    let appearance = h
        .shell
        .st
        .session
        .visual_filters()
        .last()
        .unwrap()
        .appearance();
    let colour = |h: &Harness| h.shell.st.palette.paint(&appearance).color;
    settle_filters(&mut h);
    assert!(
        painted_words(&h, kjv, colour(&h)).is_empty(),
        "off until switched on"
    );

    for tab in [tagnt, kjv] {
        assert!(h.shell.st.session.tab_visual_filters(tab).is_empty());
        click_header(&mut h, tab, "Filters");
        h.frames(2);
        h.click_text("Imperatives in Romans 12");
        h.frames(2);
        let shown: Vec<String> = (h.shell.st.session.tab_visual_filters(tab).into_iter())
            .map(|f| f.id)
            .collect();
        assert_eq!(shown, [id.clone()], "switched on in that Tab");
        h.events.push(Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        });
        h.frame();
    }
    settle_filters(&mut h);
    let greek = [
        "συσχηματίζεσθε",
        "μεταμορφοῦσθε",
        "εὐλογεῖτε",
        "εὐλογεῖτε",
        "καταρᾶσθε",
    ];
    let english = ["be", "conformed", "transformed", "Bless", "bless", "curse"];
    assert_eq!(painted_words(&h, tagnt, colour(&h)), greek);
    assert_eq!(
        painted_words(&h, kjv, colour(&h)),
        english,
        "the KJV words carrying the Greek imperatives' Strong's numbers, in Romans 12 only"
    );
    assert!(
        h.find("Filt. 1").is_some(),
        "the header counts them: {:?}",
        h.texts()
    );

    // A Page tone switch recolours them at once, without laying the text out again.
    let laid: Vec<usize> = view(&h, kjv)
        .laid_in_view(900.0)
        .iter()
        .map(|(_, _, l)| Arc::as_ptr(l) as usize)
        .collect();
    let paper = colour(&h);
    h.shell.run("view.page_tone", &json!({ "value": "dark" }));
    h.frames(2);
    let dark = colour(&h);
    assert_ne!(dark, paper);
    assert_eq!(painted_words(&h, kjv, dark), english);
    assert_eq!(painted_words(&h, tagnt, dark), greek);
    let after: Vec<usize> = view(&h, kjv)
        .laid_in_view(900.0)
        .iter()
        .map(|(_, _, l)| Arc::as_ptr(l) as usize)
        .collect();
    assert_eq!(after, laid, "no block was laid out again");

    // Switched off in the KJV's header, it goes from there only.
    click_header(&mut h, kjv, "Filters 1");
    h.frames(2);
    h.click_text("Imperatives in Romans 12");
    settle_filters(&mut h);
    assert!(painted_words(&h, kjv, dark).is_empty());
    assert_eq!(painted_words(&h, tagnt, dark), greek);
}

fn key(h: &mut Harness, key: egui::Key, modifiers: Modifiers) {
    h.events.push(Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    });
    h.frame();
}

/// Click the field showing `shown` (its hint, or what it holds) and type `text`, over what
/// it holds when `replace`.
fn type_into(h: &mut Harness, shown: &str, text: &str, replace: bool) {
    let at = h.at(shown);
    h.click(at);
    h.frame();
    if replace {
        key(h, egui::Key::A, Modifiers::COMMAND);
    }
    h.events.push(Event::Text(text.into()));
    h.frame();
}

/// View → Visual filters… makes a filter: a Passage list it cannot read is refused under the
/// form, and the filter saved is on in all Tabs, painting at once.
#[test]
fn the_visual_filters_window_makes_a_filter_and_says_why_it_refuses_one() {
    let (mut h, _files) = setup("window");
    let kjv = open_at(&mut h, "kjv", "Rom 12:2");
    h.open_menu("View");
    h.click_text("Visual filters…");
    h.frames(2);
    for name in ["Words of Christ", "Divine names", "Imperatives"] {
        assert!(h.find(name).is_some(), "{name}: {:?}", h.texts());
    }
    h.click_text("New filter…");
    h.frames(2);
    type_into(&mut h, "Imperatives in Romans 12", "Blessing", false);
    let query = "@imperative · strong:G26 · \"love one another\"";
    type_into(&mut h, query, "strong:G2127", false);
    type_into(&mut h, "Whole Bible", "Hezekiah 3", false);
    h.click_text("On in all Tabs");
    h.frame();
    h.click_text("Save");
    h.frames(2);
    assert!(
        h.texts().iter().any(|t| t.starts_with("Passages:")),
        "the refusal shows under the form: {:?}",
        h.texts()
    );
    assert_eq!(
        h.shell.st.session.visual_filters().len(),
        3,
        "nothing saved"
    );
    type_into(&mut h, "Hezekiah 3", "Rom 12", true);
    h.click_text("Save");
    h.frames(2);
    let saved = h.shell.st.session.visual_filters().last().unwrap().clone();
    assert_eq!(
        (
            saved.name.as_str(),
            saved.query.as_str(),
            saved.scope.as_str()
        ),
        ("Blessing", "strong:G2127", "Rom 12")
    );
    assert!(saved.all_tabs);
    assert!(h.find("Save").is_none(), "the form closed");
    // Esc closes the window; the KJV Tab shows the new filter at once.
    key(&mut h, egui::Key::Escape, Modifiers::NONE);
    h.frame();
    assert!(h.find("New filter…").is_none());
    settle_filters(&mut h);
    let colour = h.shell.st.palette.paint(&saved.appearance()).color;
    assert_eq!(painted_words(&h, kjv, colour), ["Bless", "bless"]);
}
