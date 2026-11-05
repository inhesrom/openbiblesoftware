//! The Notes Tool and Notes search (#64), through the headless harness with real pointer and
//! key events, a real `.obr` and a real User database in a temp folder.

use std::path::{Path, PathBuf};

use obs_engine::TabId;
use obs_scripture::{Book, Verse};
use unicode_segmentation::UnicodeSegmentation;

use super::*;

fn folder(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("obs-notes-ui-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn launch(dir: &Path) -> Harness {
    let mut h = Harness::launch_full(Some(&dir.join("user.db")), JobMode::Inline, Some(dir));
    settle_all(&mut h);
    h
}

/// Frames until the Notes Tool's and the Search Tool's text is laid out.
fn settle_notes(h: &mut Harness) {
    let mut idle = 0;
    for _ in 0..300 {
        h.paced_frame();
        let busy = h.shell.st.notes_ui.has_pending() || h.shell.st.snippets.has_pending();
        idle = if busy { 0 } else { idle + 1 };
        if idle >= 3 {
            return;
        }
    }
}

/// TB's reading pane on screen: its Panel below the header strip.
fn pane(h: &Harness) -> Rect {
    let panel = h.panels()[0].0;
    Rect::from_min_max(pos2(panel.left(), panel.top() + 70.0), panel.max)
}

/// A Note on the first long word of the third block in view in TB, made by the command the
/// Note editor runs. Returns its id.
fn note_in_view(h: &mut Harness, tb: TabId, body: &str) -> String {
    let v = h.shell.st.tabs[&tb].view.as_ref().unwrap();
    let (index, _, _) = v.laid_in_view(900.0)[2].clone();
    let block = &v.doc.blocks[index];
    let unit = &v.doc.units[block.unit];
    let range = block.source.range.clone();
    let (i, w) = unit.text[range.clone()]
        .split_word_bound_indices()
        .find(|(_, w)| w.len() >= 5 && w.chars().all(char::is_alphabetic))
        .unwrap();
    let start = range.start + i;
    let params = json!({
        "resource": v.doc.manifest.resource_id,
        "start_unit": unit.id.as_str(),
        "start": start,
        "end": start + w.len(),
        "body": body,
    });
    let out = h.shell.run("annotation.note", &params).unwrap();
    h.frames(2);
    out["id"].as_str().unwrap().to_string()
}

/// The Note markers painted in TB's pane.
fn markers(h: &Harness) -> Vec<Rect> {
    let color = h.shell.st.palette.note_marker;
    let pane = pane(h);
    h.rects
        .iter()
        .filter(|(r, c)| *c == color && pane.contains(r.center()))
        .map(|(r, _)| *r)
        .collect()
}

fn hover(h: &mut Harness, p: Pos2) {
    h.events.push(Event::PointerMoved(p));
    h.frames(3);
}

fn press_key(h: &mut Harness, key: egui::Key, modifiers: Modifiers) {
    h.events.push(Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    });
    h.frame();
}

fn right_click(h: &mut Harness, p: Pos2) {
    h.events.push(Event::PointerMoved(p));
    h.frame();
    for pressed in [true, false] {
        h.events.push(Event::PointerButton {
            pos: p,
            button: PointerButton::Secondary,
            pressed,
            modifiers: Modifiers::NONE,
        });
        h.frame();
    }
    h.frame();
}

/// The leftmost place `text` is painted (the Notebook column, rather than a card).
fn leftmost(h: &Harness, text: &str) -> Pos2 {
    h.shapes
        .iter()
        .filter(|(_, t)| t == text)
        .map(|(r, _)| *r)
        .min_by(|a, b| a.left().total_cmp(&b.left()))
        .unwrap_or_else(|| panic!("{text:?} not on screen: {:?}", h.texts()))
        .center()
}

fn open_notes(h: &mut Harness) -> TabId {
    let out = h.shell.run("tool.notes", &json!({})).unwrap();
    settle_notes(h);
    TabId(out["tab"].as_u64().unwrap())
}

/// Where a card's body was drawn.
fn card_body(h: &Harness, id: &str) -> Rect {
    h.shell
        .st
        .notes_ui
        .drawn
        .iter()
        .find(|c| c.id == id)
        .and_then(|c| c.body)
        .unwrap_or_else(|| panic!("no card body for {id}"))
}

/// Click a Note card's body and replace its text by typing.
fn retype(h: &mut Harness, id: &str, text: &str) {
    let body = card_body(h, id);
    h.click(body.center());
    assert_eq!(
        h.shell.st.notes_ui.editing.as_ref().map(|e| e.id.as_str()),
        Some(id),
        "clicking the body edits the card in place"
    );
    assert!(h.ctx.text_edit_focused(), "the card's editor has the keys");
    press_key(h, egui::Key::A, Modifiers::COMMAND);
    h.events.push(Event::Text(text.into()));
    h.frame();
}

#[test]
fn editing_a_card_updates_the_in_text_marker_preview() {
    let dir = folder("edit");
    let mut h = launch(&dir);
    let tb = h.tab_id("TB");
    let id = note_in_view(&mut h, tb, "First thoughts on Laban");
    let m = markers(&h);
    assert_eq!(m.len(), 1, "{m:?}");
    hover(&mut h, m[0].center());
    assert!(
        h.find("First thoughts on Laban").is_some(),
        "{:?}",
        h.texts()
    );

    // The Notes Tool opens beside TB; the Note's card is there.
    open_notes(&mut h);
    assert!(
        pane(&h).contains(markers(&h)[0].center()),
        "TB stays in view"
    );
    retype(&mut h, &id, "Mizpah: the Lord watch between me and thee");
    // Click away: the card saves.
    let p = pane(&h);
    h.click(pos2(p.left() + 30.0, p.bottom() - 30.0));
    assert!(h.shell.st.notes_ui.editing.is_none());
    assert_eq!(
        h.shell.st.session.annotation(&id).unwrap().body(),
        "Mizpah: the Lord watch between me and thee"
    );

    // The marker's preview in the text shows the new words.
    let m = markers(&h);
    assert_eq!(m.len(), 1);
    hover(&mut h, m[0].center());
    assert!(
        h.find("Mizpah: the Lord watch between me and thee")
            .is_some(),
        "{:?}",
        h.texts()
    );
    assert!(h.find("First thoughts on Laban").is_none());
    // One undo step brings the old text back, in the preview too.
    press_key(&mut h, egui::Key::Z, Modifiers::COMMAND);
    hover(&mut h, m[0].center() + vec2(0.5, 0.0));
    assert!(
        h.find("First thoughts on Laban").is_some(),
        "{:?}",
        h.texts()
    );
    drop(h);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn deleting_a_notebook_offers_move_to_default_preselected_or_delete_and_is_undoable() {
    let dir = folder("delete");
    let mut h = launch(&dir);
    let study = h
        .shell
        .run("notebook.create", &json!({ "name": "Study" }))
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let mut notes = Vec::new();
    for (passage, body) in [("Gen 31:3", "Return"), ("Gen 32:1", "Mahanaim")] {
        let out = h.shell.run(
            "annotation.note",
            &json!({ "passage": passage, "body": body, "notebook": study }),
        );
        notes.push(out.unwrap()["id"].as_str().unwrap().to_string());
    }
    open_notes(&mut h);
    let default = h.shell.st.session.default_notebook().unwrap();
    let in_notebook = |h: &Harness, nb: &str| {
        notes
            .iter()
            .all(|id| h.shell.st.session.annotation(id).unwrap().notebook_id == nb)
    };

    // Right-click the Notebook: Delete… asks what happens to its 2 Annotations.
    let p = leftmost(&h, "Study");
    right_click(&mut h, p);
    h.click_text("Delete…");
    let dialog = h.shell.st.notes_ui.deleting.as_ref().expect("a choice");
    assert!(!dialog.delete, "moving them is pre-selected");
    assert!(
        h.find("Delete the Notebook “Study”? It holds 2 Annotations.")
            .is_some(),
        "{:?}",
        h.texts()
    );
    assert!(h.find("Move them to “My Notebook”").is_some());
    assert!(h.find("Delete them").is_some());
    h.click_text("Delete");
    assert!(h.shell.st.notes_ui.deleting.is_none());
    assert_eq!(h.shell.st.session.notebooks().unwrap().len(), 1);
    assert!(in_notebook(&h, &default.id), "moved to the default");
    h.frames(2);
    assert!(h.find("Study").is_none(), "{:?}", h.texts());

    // Ctrl+Z: the Notebook and its Annotations are back.
    assert!(!h.ctx.text_edit_focused());
    press_key(&mut h, egui::Key::Z, Modifiers::COMMAND);
    h.frames(2);
    assert_eq!(h.shell.st.session.notebooks().unwrap().len(), 2);
    assert!(in_notebook(&h, &study));
    assert!(h.find("Study").is_some());

    // Choosing Delete them deletes the Annotations; undo brings them back too.
    let p = leftmost(&h, "Study");
    right_click(&mut h, p);
    h.click_text("Delete…");
    h.click_text("Delete them");
    assert!(h.shell.st.notes_ui.deleting.as_ref().unwrap().delete);
    h.click_text("Delete");
    assert!(h.shell.st.session.annotations(None).unwrap().is_empty());
    press_key(&mut h, egui::Key::Z, Modifiers::COMMAND);
    assert!(in_notebook(&h, &study));
    // Cancel leaves everything alone; the default Notebook cannot be deleted at all.
    let p = leftmost(&h, "Study");
    right_click(&mut h, p);
    h.click_text("Delete…");
    h.click_text("Cancel");
    assert_eq!(h.shell.st.session.notebooks().unwrap().len(), 2);
    let p = leftmost(&h, "My Notebook");
    right_click(&mut h, p);
    h.click_text("Delete…");
    assert!(h.shell.st.notes_ui.deleting.is_none());
    drop(h);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn notes_search_finds_a_word_typed_in_a_note_seconds_earlier() {
    let dir = folder("search");
    let mut h = launch(&dir);
    let id = h
        .shell
        .run(
            "annotation.note",
            &json!({ "passage": "Gen 31:49", "body": "The heap" }),
        )
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    open_notes(&mut h);
    retype(&mut h, &id, "The heap of witness, Zerubbabel");
    // Still typing (the editor keeps focus): a moment later the card has saved itself.
    h.time += crate::shell::notes_tab::AUTOSAVE_SECS + 0.2;
    h.frames(2);
    assert!(h.ctx.text_edit_focused());
    assert_eq!(
        h.shell.st.session.annotation(&id).unwrap().body(),
        "The heap of witness, Zerubbabel"
    );

    // Search, Notes mode: found, with its reference and Notebook.
    h.shell.run("tool.search", &json!({}));
    h.frames(2);
    // The mode switch's "Notes" is the lowest one (below the tab bars).
    let mode = h
        .shapes
        .iter()
        .filter(|(_, t)| t == "Notes")
        .map(|(r, _)| *r)
        .max_by(|a, b| a.top().total_cmp(&b.top()))
        .unwrap();
    h.click(mode.center());
    let search = h.tab_id("Search");
    assert_eq!(
        h.shell.st.session.search_tab(search).unwrap().mode,
        obs_engine::SearchMode::Notes
    );
    let field = h.at(crate::shell::search_tab::query_hint(
        obs_engine::SearchMode::Notes,
    ));
    h.click(field);
    h.events.push(Event::Text("zerubbabel".into()));
    h.frame();
    press_key(&mut h, egui::Key::Enter, Modifiers::NONE);
    settle_notes(&mut h);
    let st = h.shell.st.session.search_tab(search).unwrap();
    let hits = &st.notes.as_ref().unwrap().hits;
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].id, id);
    assert!(h.find("1 note").is_some(), "{:?}", h.texts());
    assert!(
        h.find("Genesis 31:49 · My Notebook").is_some(),
        "{:?}",
        h.texts()
    );
    // The hit is marked under its glyphs.
    let tint = h.shell.st.palette.search_hit;
    assert!(h.rects.iter().any(|(_, c)| *c == tint));
    // Opening it shows the Note in the Notes Tool, ready to edit.
    let row = h.find("Genesis 31:49 · My Notebook").unwrap();
    h.click(row.center() + vec2(0.0, 20.0));
    h.frames(2);
    assert_eq!(
        h.shell.st.notes_ui.editing.as_ref().map(|e| e.id.as_str()),
        Some(id.as_str())
    );
    drop(h);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_marker_click_opens_the_note_in_notes_and_show_in_text_hides_markers() {
    let dir = folder("marker");
    let mut h = launch(&dir);
    let tb = h.tab_id("TB");
    let id = note_in_view(&mut h, tb, "Opened from the text");
    let m = markers(&h)[0];
    h.click(m.center());
    settle_notes(&mut h);
    let notes = h.tab_id("Notes");
    assert_eq!(h.shell.st.session.workspace.focused(), Some(notes));
    assert_eq!(
        h.shell.st.notes_ui.editing.as_ref().map(|e| e.id.as_str()),
        Some(id.as_str()),
        "the clicked Note is ready to edit"
    );
    // Esc ends editing (and saves nothing: nothing changed).
    press_key(&mut h, egui::Key::Escape, Modifiers::NONE);
    h.frame();
    assert!(h.shell.st.notes_ui.editing.is_none());

    // 👁 hides the Notebook's Annotations in the text, and shows them again.
    let eye = leftmost(&h, "👁");
    h.click(eye);
    assert!(markers(&h).is_empty(), "hidden");
    let default = h.shell.st.session.default_notebook().unwrap().id;
    assert!(h.shell.st.session.hidden_in_text().contains(&default));
    let eye = leftmost(&h, "👁");
    h.click(eye);
    assert_eq!(markers(&h).len(), 1);
    drop(h);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn references_in_a_note_body_are_links_with_popups_and_cards_follow_their_anchor() {
    let dir = folder("links");
    let mut h = launch(&dir);
    let tb = h.tab_id("TB");
    h.shell
        .run(
            "annotation.note",
            &json!({ "passage": "Gen 33:4", "body": "Esau ran to meet him; compare Gen 32:24 here." }),
        )
        .unwrap();
    open_notes(&mut h);
    // The body's references were found once, not every frame.
    let parses = h.shell.st.notes_ui.ref_parses;
    assert!(parses >= 1);
    h.frames(5);
    assert_eq!(h.shell.st.notes_ui.ref_parses, parses);
    // The reference is underlined in the link colour; hovering it shows the popup.
    let link = h.shell.st.palette.link.gamma_multiply(0.6);
    let cards = h.panels()[1].0;
    let under = h
        .rects
        .iter()
        .find(|(r, c)| *c == link && cards.contains(r.center()))
        .map(|(r, _)| *r)
        .unwrap_or_else(|| panic!("no link underline"));
    let on = under.center() - vec2(0.0, 5.0);
    hover(&mut h, on);
    assert!(h.find("Gen 32:24").is_some(), "{:?}", h.texts());
    // Clicking it shows the Passage in the most recent Bible: TB.
    h.click(on);
    h.frames(2);
    let at = |h: &Harness| {
        h.shell
            .st
            .session
            .workspace
            .tab(tb)
            .unwrap()
            .history
            .current()
    };
    assert_eq!(at(&h), Some(Verse::new(Book::Genesis, 32, 24)));
    assert!(
        h.shell.st.notes_ui.editing.is_none(),
        "a link is not an edit"
    );
    assert_eq!(
        h.shell.st.notes_ui.ref_parses, parses,
        "hovering and clicking reuse the references"
    );

    // Following the card's reference goes to its anchor.
    h.click_text("Genesis 33:4");
    h.frames(2);
    assert_eq!(at(&h), Some(Verse::new(Book::Genesis, 33, 4)));
    drop(h);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Make `n` Notes on Passages in Genesis (six a chapter), oldest first. Returns their ids.
fn many_notes(h: &mut Harness, n: usize) -> Vec<String> {
    (0..n)
        .map(|i| {
            let passage = format!("Gen {}:{}", i / 6 + 1, i % 6 + 1);
            let body = format!("Note number {i}");
            let out = h.shell.run(
                "annotation.note",
                &json!({ "passage": passage, "body": body }),
            );
            out.unwrap()["id"].as_str().unwrap().to_string()
        })
        .collect()
}

#[test]
fn the_card_list_lays_out_only_the_cards_in_view() {
    let dir = folder("virtual");
    let mut h = launch(&dir);
    many_notes(&mut h, 300);
    open_notes(&mut h);
    assert_eq!(h.shell.st.notes_ui.cards_len(), 300);
    let drawn = h.shell.st.notes_ui.drawn.len();
    assert!(
        (1..20).contains(&drawn),
        "{drawn} of 300 cards were laid out"
    );
    assert!(h.find("300 Annotations").is_some(), "{:?}", h.texts());
    drop(h);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// In Last modified order the card being edited would jump to the top on each save; it stays
/// where it is, drawn and with the keys, until editing ends.
#[test]
fn a_card_being_edited_keeps_its_place_and_the_keys_when_it_saves() {
    let dir = folder("pinned");
    let mut h = launch(&dir);
    let ids = many_notes(&mut h, 40);
    h.shell
        .run("notes.order", &json!({ "value": "modified" }))
        .unwrap();
    open_notes(&mut h);
    // The oldest Note is the last card; opening it scrolls down to it and edits it.
    let oldest = ids[0].clone();
    h.shell.run("notes.open", &json!({ "id": oldest })).unwrap();
    settle_notes(&mut h);
    let index = |h: &Harness| h.shell.st.notes_ui.card_index(&oldest);
    assert_eq!(index(&h), Some(39));
    card_body(&h, &oldest);
    assert!(h.ctx.text_edit_focused());
    press_key(&mut h, egui::Key::End, Modifiers::COMMAND);
    h.events.push(Event::Text(", revised".into()));
    h.frame();
    // A moment later it saves itself, which makes it the most recently changed Note.
    h.time += crate::shell::notes_tab::AUTOSAVE_SECS + 0.2;
    h.frames(3);
    assert_eq!(
        h.shell.st.session.annotation(&oldest).unwrap().body(),
        "Note number 0, revised"
    );
    assert_eq!(
        index(&h),
        Some(39),
        "the list did not re-sort under the editor"
    );
    card_body(&h, &oldest);
    assert!(h.ctx.text_edit_focused(), "the editor kept the keys");
    // Typing goes on into the same Note.
    h.events.push(Event::Text(" again".into()));
    h.frame();
    assert_eq!(
        h.shell
            .st
            .notes_ui
            .editing
            .as_ref()
            .map(|e| e.text.as_str()),
        Some("Note number 0, revised again")
    );
    // Editing ends: it saves and the list re-sorts, newest first.
    press_key(&mut h, egui::Key::Escape, Modifiers::NONE);
    h.frames(2);
    assert!(h.shell.st.notes_ui.editing.is_none());
    assert_eq!(index(&h), Some(0));
    assert_eq!(
        h.shell.st.session.annotation(&oldest).unwrap().body(),
        "Note number 0, revised again"
    );
    drop(h);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Words typed a moment before another Tab covers the Notes Tab are saved at once, so Notes
/// search finds them.
#[test]
fn a_card_saves_as_soon_as_another_tab_covers_the_notes_tab() {
    let dir = folder("covered");
    let mut h = launch(&dir);
    let id = h
        .shell
        .run(
            "annotation.note",
            &json!({ "passage": "Gen 31:49", "body": "The heap" }),
        )
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let notes = open_notes(&mut h);
    retype(&mut h, &id, "Galeed, the heap of witness");
    assert!(h.ctx.text_edit_focused());
    // No time passes: Search opens over the Notes Tab, in the same Panel.
    let search = TabId(
        h.shell.run("tool.search", &json!({})).unwrap()["tab"]
            .as_u64()
            .unwrap(),
    );
    h.frames(2);
    let panel = |t: TabId| h.shell.st.session.workspace.panel_of(t);
    assert_eq!(panel(search), panel(notes), "Search covers the Notes Tab");
    assert!(
        h.find("Last modified").is_none(),
        "the Notes Tab is not drawn"
    );
    assert!(
        h.shell.st.notes_ui.editing.is_none(),
        "the editing pass ended"
    );
    assert_eq!(
        h.shell.st.session.annotation(&id).unwrap().body(),
        "Galeed, the heap of witness"
    );
    h.shell
        .run("search.mode", &json!({ "tab": search.0, "value": "notes" }))
        .unwrap();
    h.shell
        .run("search.run", &json!({ "tab": search.0, "text": "galeed" }))
        .unwrap();
    let st = h.shell.st.session.search_tab(search).unwrap();
    assert_eq!(st.notes.as_ref().unwrap().hits.len(), 1);
    drop(h);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// With no Bible open, following a Passage card opens the ★ Bible in a new Tab at the Verse.
#[test]
fn following_a_card_with_no_bible_open_opens_one_at_the_verse() {
    let dir = folder("follow-new");
    let mut h = launch(&dir);
    let tb = h.tab_id("TB");
    h.shell.run("tab.close", &json!({ "tab": tb.0 })).unwrap();
    h.shell
        .run(
            "annotation.note",
            &json!({ "passage": "Gen 32:24", "body": "Jacob wrestles" }),
        )
        .unwrap();
    open_notes(&mut h);
    let bibles = |h: &Harness| -> Vec<TabId> {
        h.shell
            .st
            .session
            .workspace
            .tabs()
            .filter(|t| matches!(&t.kind, obs_engine::TabKind::Resource(_)))
            .map(|t| t.id)
            .collect()
    };
    assert!(bibles(&h).is_empty());
    h.click_text("Genesis 32:24");
    settle_all(&mut h);
    settle_notes(&mut h);
    let opened = bibles(&h);
    assert_eq!(opened.len(), 1, "a Bible Tab opened");
    let tab = opened[0];
    assert_eq!(h.shell.st.session.workspace.focused(), Some(tab));
    let ws = h.shell.st.session.workspace.tab(tab).unwrap();
    assert_eq!(ws.title, "TB");
    assert_eq!(
        ws.history.current(),
        Some(Verse::new(Book::Genesis, 32, 24))
    );
    let view = h
        .shell
        .st
        .tabs
        .get_mut(&tab)
        .unwrap()
        .view
        .as_mut()
        .unwrap();
    assert_eq!(
        view.current_location(),
        Some(Verse::new(Book::Genesis, 32, 24)),
        "the new Tab shows the Verse"
    );
    drop(h);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn the_notes_symbols_are_in_the_chrome_fonts() {
    let h = Harness::new();
    h.ctx.fonts_mut(|f| {
        let missing: Vec<&str> = ["👁", "…", "✏", "“”", "⚠"]
            .into_iter()
            .filter(|s| !f.has_glyphs(&egui::FontId::proportional(14.0), s))
            .collect();
        assert!(missing.is_empty(), "{missing:?}");
    });
}
