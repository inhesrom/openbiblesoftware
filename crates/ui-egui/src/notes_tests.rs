//! The Notes Tool and Notes search (#64), through the headless harness with real pointer and
//! key events, a real `.obr` and a real User database in a temp folder.

use std::path::{Path, PathBuf};

use obs_engine::{HighlightStyle, TabId};
use obs_scripture::{Book, Verse};
use unicode_segmentation::UnicodeSegmentation;

use super::annotation::{close_window, double_click};
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

/// The first long word of the `nth` block in view in TB, as a selection's command params.
fn word_in_view(h: &Harness, tb: TabId, nth: usize) -> serde_json::Value {
    let v = h.shell.st.tabs[&tb].view.as_ref().unwrap();
    let (index, _, _) = v.laid_in_view(900.0)[nth].clone();
    let block = &v.doc.blocks[index];
    let unit = &v.doc.units[block.unit];
    let range = block.source.range.clone();
    let (i, w) = unit.text[range.clone()]
        .split_word_bound_indices()
        .find(|(_, w)| w.len() >= 5 && w.chars().all(char::is_alphabetic))
        .unwrap();
    let start = range.start + i;
    json!({
        "resource": v.doc.manifest.resource_id,
        "start_unit": unit.id.as_str(),
        "start": start,
        "end": start + w.len(),
    })
}

/// A Note on the first long word of the third block in view in TB, made by the command the
/// Note editor runs. Returns its id.
fn note_in_view(h: &mut Harness, tb: TabId, body: &str) -> String {
    let mut params = word_in_view(h, tb, 2);
    params["body"] = json!(body);
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

/// Where a card was drawn.
fn card_rect(h: &Harness, id: &str) -> Rect {
    h.shell
        .st
        .notes_ui
        .drawn
        .iter()
        .find(|c| c.id == id)
        .map(|c| c.rect)
        .unwrap_or_else(|| panic!("no card for {id}"))
}

/// The card being edited: its text and why it last failed to save.
fn card_editor(h: &Harness) -> Option<(String, Option<String>)> {
    let ed = h.shell.st.notes_ui.editing.as_ref()?;
    Some((ed.text.clone(), ed.error.clone()))
}

/// When the database refuses a card's save (#129), the editor stays open with the typed text
/// and the error, the save is tried again a moment later, and Discard changes puts back the
/// text last saved. Closing the window then keeps it open, showing the card.
#[test]
fn a_card_whose_save_fails_keeps_its_text_and_shows_the_error() {
    use std::sync::atomic::Ordering;
    let dir = folder("write-fails");
    let mut h = launch(&dir);
    let (store, _) = obs_store::UserStore::open(&dir.join("failing.db")).unwrap();
    let (store, failures) = obs_engine::testing::WritesFail::changes(store);
    h.shell.st.session.attach_user(Box::new(store));
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
    let saved = |h: &Harness| {
        h.shell
            .st
            .session
            .annotation(&id)
            .unwrap()
            .body()
            .to_string()
    };
    let not_saved = |h: &Harness| {
        h.texts()
            .iter()
            .any(|t| t.starts_with("Not saved:") && t.contains("disk is full"))
    };

    // The disk fills while the user types: clicking away cannot save.
    failures.store(u32::MAX, Ordering::SeqCst);
    retype(&mut h, &id, "Galeed, the heap of witness");
    let p = pane(&h);
    h.click(pos2(p.left() + 30.0, p.bottom() - 30.0));
    let (text, error) = card_editor(&h).expect("the editor stays open");
    assert_eq!(text, "Galeed, the heap of witness", "with the typed text");
    let error = error.expect("and the error");
    assert!(error.contains("disk is full"), "{error}");
    assert!(not_saved(&h), "the card shows why: {:?}", h.texts());
    assert!(h.find("Discard changes").is_some());
    assert_eq!(saved(&h), "The heap");
    // Not retried every frame.
    let left = failures.load(Ordering::SeqCst);
    h.frames(5);
    assert_eq!(failures.load(Ordering::SeqCst), left);

    // A moment later it is tried again; the disk has room now, so it saves.
    failures.store(0, Ordering::SeqCst);
    h.time += crate::shell::notes_tab::AUTOSAVE_SECS + 0.2;
    h.frames(3);
    assert_eq!(saved(&h), "Galeed, the heap of witness");
    assert_eq!(card_editor(&h).and_then(|(_, e)| e), None);
    assert!(!not_saved(&h), "{:?}", h.texts());

    // Refused again: Discard changes puts back the text last saved.
    failures.store(u32::MAX, Ordering::SeqCst);
    retype(&mut h, &id, "Words to drop");
    h.click(pos2(p.left() + 30.0, p.bottom() - 30.0));
    assert!(not_saved(&h), "{:?}", h.texts());
    h.click_text("Discard changes");
    assert!(card_editor(&h).is_none());
    assert_eq!(saved(&h), "Galeed, the heap of witness");
    h.frames(2);
    assert!(!not_saved(&h), "{:?}", h.texts());

    // Another Tab covers the Notes Tab before the words can be saved: they are kept.
    retype(&mut h, &id, "Unsaved at closing");
    let search = TabId(
        h.shell.run("tool.search", &json!({})).unwrap()["tab"]
            .as_u64()
            .unwrap(),
    );
    h.frames(2);
    assert_eq!(h.shell.st.session.workspace.focused(), Some(search));
    assert_eq!(
        card_editor(&h).map(|(t, _)| t).as_deref(),
        Some("Unsaved at closing")
    );
    // Closing the window keeps it open and shows the card that did not save.
    assert!(close_window(&mut h), "the close is cancelled");
    h.frames(2);
    assert_eq!(h.shell.st.session.workspace.focused(), Some(notes));
    assert!(not_saved(&h), "{:?}", h.texts());
    // Once the disk has room, closing saves it and the window closes.
    failures.store(0, Ordering::SeqCst);
    assert!(!close_window(&mut h));
    assert_eq!(saved(&h), "Unsaved at closing");
    drop(h);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// A refused save is retried on its own clock (#129): with the Notes Tab covered, its toast
/// gone and nothing else moving, each failed try still asks for the frame of the next one, so
/// the words save once the disk has room without the user touching the window.
#[test]
fn a_covered_card_whose_save_fails_keeps_retrying_while_the_window_is_idle() {
    use std::sync::atomic::Ordering;
    use std::time::Duration;
    let dir = folder("idle-retry");
    let mut h = launch(&dir);
    let (store, _) = obs_store::UserStore::open(&dir.join("failing.db")).unwrap();
    let (store, failures) = obs_engine::testing::WritesFail::changes(store);
    h.shell.st.session.attach_user(Box::new(store));
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
    failures.store(u32::MAX, Ordering::SeqCst);
    retype(&mut h, &id, "Mizpah, the watchpost");
    h.shell.run("tool.search", &json!({})).unwrap();
    h.frames(2);
    assert!(
        card_editor(&h).and_then(|(_, e)| e).is_some(),
        "the first save, as the Tab was covered, failed"
    );
    let autosave = Duration::from_secs_f64(crate::shell::notes_tab::AUTOSAVE_SECS);
    // Each frame comes only when the last one asked for it, long after the toast ran out.
    for _ in 0..3 {
        assert!(h.repaint_delay <= autosave, "{:?}", h.repaint_delay);
        let left = failures.load(Ordering::SeqCst);
        h.time += 5.0;
        h.idle_frame();
        assert!(failures.load(Ordering::SeqCst) < left, "tried again");
        assert!(h.shell.st.toast.is_none());
    }
    assert!(h.repaint_delay <= autosave, "{:?}", h.repaint_delay);
    failures.store(0, Ordering::SeqCst);
    h.time += 5.0;
    h.idle_frame();
    let body = h.shell.st.session.annotation(&id).unwrap();
    assert_eq!(body.body(), "Mizpah, the watchpost");
    assert!(card_editor(&h).is_none(), "saved, and let go");
    assert_eq!(h.repaint_delay, Duration::MAX, "nothing left to wake for");
    drop(h);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// A Note deleted while its card is edited (an undo removed it) is let go: there is nothing
/// left to save into.
#[test]
fn a_card_whose_note_is_gone_stops_editing() {
    let dir = folder("gone");
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
    retype(&mut h, &id, "Galeed");
    h.shell.run("edit.undo", &json!({})).unwrap();
    assert!(h.shell.st.session.annotation(&id).is_err());
    h.time += crate::shell::notes_tab::AUTOSAVE_SECS + 0.2;
    h.frames(3);
    assert!(card_editor(&h).is_none());
    drop(h);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// A Highlight card shows its style's swatch: the colour filled, or the underline drawn.
#[test]
fn a_highlight_card_shows_its_style_swatch() {
    let dir = folder("swatch");
    let mut h = launch(&dir);
    let tb = h.tab_id("TB");
    let mut ids = Vec::new();
    for (nth, style) in [(1, "green"), (2, "red_underline")] {
        let mut params = word_in_view(&h, tb, nth);
        params["style"] = json!(style);
        let out = h.shell.run("annotation.highlight", &params).unwrap();
        ids.push(out["ids"][0].as_str().unwrap().to_string());
    }
    open_notes(&mut h);
    let palette = h.shell.st.palette;
    let painted = |h: &Harness, id: &str, color: egui::Color32| -> Vec<Rect> {
        let card = card_rect(h, id);
        h.rects
            .iter()
            .filter(|(r, c)| *c == color && card.contains_rect(*r))
            .map(|(r, _)| *r)
            .collect()
    };
    let green = palette.highlight(HighlightStyle::Green).unwrap();
    let swatch = painted(&h, &ids[0], green);
    assert_eq!(swatch.len(), 1, "{swatch:?}");
    assert!(
        (swatch[0].size() - vec2(16.0, 16.0)).length() < 0.5,
        "{swatch:?}"
    );
    assert!(painted(&h, &ids[0], palette.underline).is_empty());
    // The underline style: an underline, no fill colour.
    assert_eq!(painted(&h, &ids[1], palette.underline).len(), 1);
    assert!(painted(&h, &ids[1], green).is_empty());

    // Changing the style repaints the swatch.
    h.shell
        .run(
            "annotation.style",
            &json!({ "id": ids[0], "style": "pink" }),
        )
        .unwrap();
    settle_notes(&mut h);
    let pink = palette.highlight(HighlightStyle::Pink).unwrap();
    assert_eq!(painted(&h, &ids[0], pink).len(), 1);
    assert!(painted(&h, &ids[0], green).is_empty());
    drop(h);
    std::fs::remove_dir_all(&dir).unwrap();
}

fn notebook_names(h: &Harness) -> Vec<String> {
    (h.shell.st.session.notebooks().unwrap().into_iter())
        .map(|n| n.name)
        .collect()
}

fn type_text(h: &mut Harness, text: &str) {
    h.events.push(Event::Text(text.into()));
    h.frame();
}

/// A Notebook is renamed in its row: double-click (or Rename… on right-click), type, Enter.
/// Esc leaves the name alone; undo puts the old name back.
#[test]
fn a_notebook_is_renamed_in_place() {
    let dir = folder("rename");
    let mut h = launch(&dir);
    h.shell
        .run("notebook.create", &json!({ "name": "Study" }))
        .unwrap();
    open_notes(&mut h);

    let p = leftmost(&h, "Study");
    double_click(&mut h, p);
    assert!(
        h.ctx.text_edit_focused(),
        "the name is a field with the keys"
    );
    press_key(&mut h, egui::Key::A, Modifiers::COMMAND);
    type_text(&mut h, "Genesis study");
    press_key(&mut h, egui::Key::Enter, Modifiers::NONE);
    h.frames(2);
    assert!(!h.ctx.text_edit_focused());
    assert_eq!(notebook_names(&h), ["My Notebook", "Genesis study"]);
    assert!(h.find("Genesis study").is_some(), "{:?}", h.texts());
    assert!(h.find("Study").is_none());

    // Rename… from the row's menu; Esc cancels.
    let p = leftmost(&h, "Genesis study");
    right_click(&mut h, p);
    h.click_text("Rename…");
    assert!(h.ctx.text_edit_focused());
    press_key(&mut h, egui::Key::A, Modifiers::COMMAND);
    type_text(&mut h, "Ignored");
    press_key(&mut h, egui::Key::Escape, Modifiers::NONE);
    h.frames(2);
    assert_eq!(notebook_names(&h), ["My Notebook", "Genesis study"]);
    assert!(h.find("Ignored").is_none(), "{:?}", h.texts());
    assert!(h.find("Genesis study").is_some());

    // One undo step brings the old name back.
    press_key(&mut h, egui::Key::Z, Modifiers::COMMAND);
    h.frames(2);
    assert_eq!(notebook_names(&h), ["My Notebook", "Study"]);
    assert!(h.find("Study").is_some(), "{:?}", h.texts());
    drop(h);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// The “+ New notebook” button turns into a name field; Enter creates the Notebook. An empty
/// name, or clicking away, creates nothing.
#[test]
fn new_notebook_creates_one_from_the_name_typed() {
    let dir = folder("new-notebook");
    let mut h = launch(&dir);
    open_notes(&mut h);
    assert_eq!(notebook_names(&h), ["My Notebook"]);

    h.click_text("+ New notebook");
    assert!(h.ctx.text_edit_focused(), "a name field with the keys");
    assert!(h.find("+ New notebook").is_none());
    type_text(&mut h, "Prophets");
    press_key(&mut h, egui::Key::Enter, Modifiers::NONE);
    h.frames(2);
    assert_eq!(notebook_names(&h), ["My Notebook", "Prophets"]);
    assert!(h.find("Prophets").is_some(), "{:?}", h.texts());
    assert!(h.find("+ New notebook").is_some(), "the button is back");

    // Enter on an empty name creates nothing.
    h.click_text("+ New notebook");
    press_key(&mut h, egui::Key::Enter, Modifiers::NONE);
    h.frames(2);
    assert_eq!(notebook_names(&h), ["My Notebook", "Prophets"]);
    assert!(h.find("+ New notebook").is_some());

    // Clicking away leaves the name unused.
    h.click_text("+ New notebook");
    type_text(&mut h, "Stray");
    let p = pane(&h);
    h.click(pos2(p.left() + 30.0, p.bottom() - 30.0));
    assert_eq!(notebook_names(&h), ["My Notebook", "Prophets"]);
    assert!(h.find("Stray").is_none());
    drop(h);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// A Note with Note formatting (#186): a heading, a nested list, bold and italic text and a
/// Bible reference.
const STUDY: &str = "## Justified by faith\n- peace with God\n- access by **grace**\n  - *rejoice* in hope\nGod commendeth his love: Rom 5:8.";

/// A KJV with Romans 5:8, opened in a Tab of its own (the most recent Bible Tab).
fn open_romans(h: &mut Harness, dir: &Path) -> TabId {
    use obs_engine::testing::bibles::{bible, v};
    let r = bible(
        "kjv-rom",
        "KJV",
        &[
            (
                v(Book::Romans, 5, 1),
                "Therefore being justified by faith, we have peace with God through our Lord Jesus Christ:",
            ),
            (
                v(Book::Romans, 5, 8),
                "But God commendeth his love toward us, in that, while we were yet sinners, Christ died for us.",
            ),
        ],
        None,
        &[],
    );
    let path = dir.join("kjv-rom.obr");
    obs_store::write_obr(&r, &path).unwrap();
    h.shell.st.session.library.add_file(&path).unwrap();
    // The ★ Bible, which previews references.
    h.shell
        .run("library.move", &json!({ "id": "kjv-rom", "to": 0 }))
        .unwrap();
    let out = h
        .shell
        .run("resource.open", &json!({ "id": "kjv-rom", "split": true }))
        .unwrap();
    settle_all(h);
    TabId(out["tab"].as_u64().unwrap())
}

/// The faces and colour roles a drawn line's `part` was laid out in.
fn styles_of(line: &crate::shell::notes_tab::DrawnLine, part: &str) -> Vec<obs_layout::RunStyle> {
    let at = line.laid.text.find(part).unwrap();
    let range = at..at + part.len();
    line.laid
        .lines
        .iter()
        .flat_map(|l| &l.items)
        .filter(|i| i.bytes.start < range.end && range.start < i.bytes.end)
        .map(|i| i.style)
        .collect()
}

fn drawn_line<'a>(h: &'a Harness, text: &str) -> &'a crate::shell::notes_tab::DrawnLine {
    h.shell
        .st
        .notes_ui
        .drawn_lines
        .iter()
        .find(|l| l.laid.text == text)
        .unwrap_or_else(|| {
            let drawn: Vec<&str> = (h.shell.st.notes_ui.drawn_lines.iter())
                .map(|l| l.laid.text.as_str())
                .collect();
            panic!("{text:?} not drawn: {drawn:?}")
        })
}

/// Acceptance (#186): a Note with a heading, a list and "Rom 5:8" is drawn formatted;
/// hovering the reference previews the verse and clicking it goes there.
#[test]
fn a_formatted_note_renders_its_heading_list_and_a_live_reference() {
    use obs_fonts::FaceId;
    use obs_model::NoteLineKind;
    let dir = folder("formatted");
    let mut h = launch(&dir);
    let kjv = open_romans(&mut h, &dir);
    // Room for the Notes Tool beside the KJV.
    let tb = h.tab_id("TB");
    h.shell.run("tab.close", &json!({ "tab": tb.0 }));
    settle_all(&mut h);
    let id = h
        .shell
        .run(
            "annotation.note",
            &json!({ "passage": "Rom 5:8", "body": STUDY }),
        )
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    open_notes(&mut h);
    let size = h.shell.st.session.text.size;
    let body = card_body(&h, &id);

    // The heading: no markup shown, bold, larger, in the heading colour.
    let heading = drawn_line(&h, "Justified by faith");
    assert_eq!(heading.kind, NoteLineKind::Heading(2));
    for s in styles_of(heading, "Justified by faith") {
        assert_eq!(s.face, FaceId::Bold);
        assert_eq!(s.role, obs_layout::ColorRole::Accent);
        assert!(s.size > size * 1.1, "{s:?}");
    }
    // The list: each item after its bullet, the nested one further in.
    let first = drawn_line(&h, "peace with God");
    let second = drawn_line(&h, "access by grace");
    let nested = drawn_line(&h, "rejoice in hope");
    assert_eq!(first.marker.as_deref(), Some("•"));
    assert_eq!(nested.marker.as_deref(), Some("○"));
    assert!(first.text_rect.left() > body.left() + 10.0);
    assert_eq!(first.text_rect.left(), second.text_rect.left());
    assert!(nested.text_rect.left() > first.text_rect.left() + 10.0);
    assert!(first.text_rect.top() > heading.text_rect.bottom() - 1.0);
    // Bold and italic text.
    for s in styles_of(second, "grace") {
        assert_eq!(s.face, FaceId::Bold);
    }
    for s in styles_of(second, "access by") {
        assert_eq!(s.face, FaceId::Regular);
    }
    for s in styles_of(nested, "rejoice") {
        assert_eq!(s.face, FaceId::Italic);
    }
    // The reference, as a link.
    let text = drawn_line(&h, "God commendeth his love: Rom 5:8.");
    assert_eq!(text.text_rect.left(), body.left());
    for s in styles_of(text, "Rom 5:8") {
        assert_eq!(s.role, obs_layout::ColorRole::Link);
    }
    preview(&h, "notes-formatted-card-kittest");

    // Hovering it previews the verse from the KJV.
    let at = text.laid.text.find("Rom 5:8").unwrap();
    let rects = crate::compare_view::range_rects(&text.laid, &(at..at + "Rom 5:8".len()));
    let [x0, y0, x1, y1] = rects[0];
    let on = text.text_rect.min + vec2((x0 + x1) / 2.0, (y0 + y1) / 2.0);
    hover(&mut h, on);
    settle_notes(&mut h);
    hover(&mut h, on + vec2(0.5, 0.0));
    assert!(h.find("Rom 5:8").is_some(), "{:?}", h.texts());
    assert!(h.find("★ KJV").is_some(), "{:?}", h.texts());
    preview(&h, "notes-formatted-reference-popup-kittest");
    // Clicking it goes there in the most recent Bible Tab, and is not an edit.
    let at_now = |h: &Harness| {
        h.shell
            .st
            .session
            .workspace
            .tab(kjv)
            .unwrap()
            .history
            .current()
    };
    assert_eq!(at_now(&h), Some(Verse::new(Book::Romans, 5, 1)));
    h.click(on);
    h.frames(2);
    assert_eq!(at_now(&h), Some(Verse::new(Book::Romans, 5, 8)));
    assert!(h.shell.st.notes_ui.editing.is_none());
    drop(h);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Acceptance (#186): a Note written as plain text before Note formatting looks the same
/// after the v10 migration: every line is drawn as plain text at the left, in the body face,
/// with the markup characters it holds shown as typed, and blank lines as gaps.
#[test]
fn an_old_plain_text_note_looks_the_same_after_the_migration() {
    use obs_fonts::FaceId;
    let dir = folder("plain");
    let mut h = launch(&dir);
    let plain = "# not a heading\n- nor a list\n\n1. nor this, see Gen 32:24\n*stars* and a_b > c";
    let id = h
        .shell
        .run(
            "annotation.note",
            &json!({ "passage": "Gen 32:24", "body": obs_model::escape_note_text(plain) }),
        )
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    open_notes(&mut h);
    let body = card_body(&h, &id);
    let lines = &h.shell.st.notes_ui.drawn_lines;
    let drawn: Vec<&str> = lines.iter().map(|l| l.laid.text.as_str()).collect();
    assert_eq!(
        drawn,
        plain
            .split('\n')
            .filter(|l| !l.is_empty())
            .collect::<Vec<_>>()
    );
    let size = h.shell.st.session.text.size;
    for l in lines {
        assert_eq!(l.kind, obs_model::NoteLineKind::Text);
        assert!(l.marker.is_none());
        assert_eq!(l.text_rect.left(), body.left());
        for item in l.laid.lines.iter().flat_map(|x| &x.items) {
            assert_eq!(item.style.face, FaceId::Regular, "{:?}", l.laid.text);
            assert_eq!(item.style.size, size);
        }
    }
    // The blank line is the same gap as before: 0.8 of the text size between two lines.
    let gap = lines[2].text_rect.top() - lines[1].text_rect.bottom();
    let spacing = h.ctx.global_style().spacing.item_spacing.y;
    assert!((gap - (size * 0.8 + spacing)).abs() < 0.5, "{gap}");
    // And lines next to each other the usual spacing apart.
    let next = lines[1].text_rect.top() - lines[0].text_rect.bottom();
    assert!((next - spacing).abs() < 0.5, "{next}");
    // Each line is laid out exactly as plain Note text always was: references as links.
    let reference = drawn_line(&h, "1. nor this, see Gen 32:24");
    for s in styles_of(reference, "Gen 32:24") {
        assert_eq!(s.role, obs_layout::ColorRole::Link);
    }
    preview(&h, "notes-plain-after-migration-kittest");
    drop(h);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// The Note editor (#186): Ctrl+B and the toolbar format the text, Enter continues a list,
/// the field's own undo takes each change back, the keys stay in the field, and the saves of
/// the editing pass are one Annotation edit that Edit → Undo takes back.
#[test]
fn the_note_editor_formats_with_shortcuts_and_its_toolbar_and_undoes() {
    let dir = folder("editor");
    let mut h = launch(&dir);
    let tb = h.tab_id("TB");
    h.shell.run("tab.close", &json!({ "tab": tb.0 }));
    settle_all(&mut h);
    let id = h
        .shell
        .run(
            "annotation.note",
            &json!({ "passage": "Gen 32:24", "body": "wrestled until dawn" }),
        )
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    open_notes(&mut h);
    let body = card_body(&h, &id);
    h.click(body.center());
    assert!(h.ctx.text_edit_focused());
    let text = |h: &Harness| {
        h.shell
            .st
            .notes_ui
            .editing
            .as_ref()
            .map(|e| e.text.clone())
            .unwrap_or_default()
    };
    // Ctrl+B on the whole text.
    press_key(&mut h, egui::Key::A, Modifiers::COMMAND);
    press_key(&mut h, egui::Key::B, Modifiers::COMMAND);
    h.frame();
    assert_eq!(text(&h), "**wrestled until dawn**");
    // The field's undo takes it back, and redo puts it again.
    press_key(&mut h, egui::Key::Z, Modifiers::COMMAND);
    h.frame();
    assert_eq!(text(&h), "wrestled until dawn");
    press_key(&mut h, egui::Key::Z, Modifiers::COMMAND | Modifiers::SHIFT);
    h.frame();
    assert_eq!(text(&h), "**wrestled until dawn**");
    // The toolbar's bulleted list: the editor keeps the keys and stays open.
    h.click_text("•");
    assert_eq!(text(&h), "- **wrestled until dawn**");
    assert!(h.shell.st.notes_ui.editing.is_some(), "still editing");
    assert!(h.ctx.text_edit_focused(), "the field kept the keys");
    // Enter at the end continues the list.
    press_key(&mut h, egui::Key::End, Modifiers::COMMAND);
    press_key(&mut h, egui::Key::Enter, Modifiers::NONE);
    h.events.push(Event::Text("blessed at Peniel".into()));
    h.frame();
    assert_eq!(text(&h), "- **wrestled until dawn**\n- blessed at Peniel");
    h.events.push(Event::Text(", cf. Gen 32:30".into()));
    h.frame();
    assert_eq!(
        text(&h),
        "- **wrestled until dawn**\n- blessed at Peniel, cf. Gen 32:30"
    );
    preview(&h, "notes-editor-kittest");
    // Ctrl+Enter ends editing: saved.
    press_key(&mut h, egui::Key::Enter, Modifiers::COMMAND);
    h.frames(2);
    assert!(h.shell.st.notes_ui.editing.is_none());
    let saved = |h: &Harness| {
        h.shell
            .st
            .session
            .annotation(&id)
            .unwrap()
            .body()
            .to_string()
    };
    assert_eq!(
        saved(&h),
        "- **wrestled until dawn**\n- blessed at Peniel, cf. Gen 32:30"
    );
    // Edit → Undo takes the whole pass back as one Annotation edit; Redo puts it again.
    h.shell.run("edit.undo", &json!({})).unwrap();
    assert_eq!(saved(&h), "wrestled until dawn");
    h.shell.run("edit.redo", &json!({})).unwrap();
    assert_eq!(
        saved(&h),
        "- **wrestled until dawn**\n- blessed at Peniel, cf. Gen 32:30"
    );
    drop(h);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// A card's Note is saved as typed (#186 review): white space is CommonMark, so none of it
/// is trimmed or normalized, and undo and redo give the same bytes back.
#[test]
fn a_card_edit_saves_its_text_byte_for_byte() {
    let dir = folder("editor-verbatim");
    let mut h = launch(&dir);
    let tb = h.tab_id("TB");
    h.shell.run("tab.close", &json!({ "tab": tb.0 }));
    settle_all(&mut h);
    let id = h
        .shell
        .run(
            "annotation.note",
            &json!({ "passage": "Gen 32:24", "body": "wrestled" }),
        )
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    open_notes(&mut h);
    let body = card_body(&h, &id);
    h.click(body.center());
    assert!(h.ctx.text_edit_focused());
    press_key(&mut h, egui::Key::A, Modifiers::COMMAND);
    let typed = "\n    **code, not bold**  \n\n\t``faith\n hope``  \n";
    h.events.push(Event::Text(typed.into()));
    h.frame();
    press_key(&mut h, egui::Key::Enter, Modifiers::COMMAND);
    h.frames(2);
    assert!(h.shell.st.notes_ui.editing.is_none());
    let saved = |h: &Harness| {
        h.shell
            .st
            .session
            .annotation(&id)
            .unwrap()
            .body()
            .to_string()
    };
    assert_eq!(saved(&h), typed);
    h.shell.run("edit.undo", &json!({})).unwrap();
    assert_eq!(saved(&h), "wrestled");
    h.shell.run("edit.redo", &json!({})).unwrap();
    assert_eq!(saved(&h), typed);
    drop(h);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// The Note editor's link button (#186) makes no link across lines: `[faith\nhope](…)`
/// would show as typed, since a Note's link ends with its line. On one line it links.
#[test]
fn the_note_editor_link_button_does_not_link_across_lines() {
    let dir = folder("editor-link");
    let mut h = launch(&dir);
    let tb = h.tab_id("TB");
    h.shell.run("tab.close", &json!({ "tab": tb.0 }));
    settle_all(&mut h);
    let id = h
        .shell
        .run(
            "annotation.note",
            &json!({ "passage": "Rom 5:8", "body": "faith\nhope" }),
        )
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    open_notes(&mut h);
    let body = card_body(&h, &id);
    h.click(body.center());
    assert!(h.ctx.text_edit_focused());
    let text = |h: &Harness| {
        h.shell
            .st
            .notes_ui
            .editing
            .as_ref()
            .map(|e| e.text.clone())
            .unwrap_or_default()
    };
    // Both lines selected: the link button does nothing, and the editor stays open.
    press_key(&mut h, egui::Key::A, Modifiers::COMMAND);
    h.frame();
    h.click_text("🔗");
    assert_eq!(text(&h), "faith\nhope");
    assert!(h.shell.st.notes_ui.editing.is_some(), "still editing");
    // The cursor on one line: it links there.
    press_key(&mut h, egui::Key::End, Modifiers::COMMAND);
    h.frame();
    h.click_text("🔗");
    assert_eq!(text(&h), "faith\nhope[](<>)");
    drop(h);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// The Note editor's list, quote and heading buttons (#186 review) are off while they
/// would put a block boundary inside a code span or link over lines, saying why to the
/// pointer and to a screen reader, and the Note keeps its bytes; bold over lines is cut so
/// each line keeps it.
#[test]
fn the_note_editor_buttons_are_off_over_a_code_span_or_link_across_lines_or_partly_selected() {
    let dir = folder("editor-span-lines");
    let mut h = launch(&dir);
    h.ctx.enable_accesskit();
    let tb = h.tab_id("TB");
    h.shell.run("tab.close", &json!({ "tab": tb.0 }));
    settle_all(&mut h);
    let body = "` a\nb ` and [x\ny](<https://example.org/?q=\\&amp;>)";
    let id = h
        .shell
        .run(
            "annotation.note",
            &json!({ "passage": "Rom 5:8", "body": body }),
        )
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    open_notes(&mut h);
    let card = card_body(&h, &id);
    h.click(card.center());
    assert!(h.ctx.text_edit_focused());
    let text = |h: &Harness| {
        h.shell
            .st
            .notes_ui
            .editing
            .as_ref()
            .map(|e| e.text.clone())
            .unwrap_or_default()
    };
    press_key(&mut h, egui::Key::A, Modifiers::COMMAND);
    h.frames(2);
    // (label, disabled, AccessKit description) of the toolbar's buttons named.
    let buttons = |h: &Harness, named: &[&str]| -> Vec<(String, bool, Option<String>)> {
        h.tree
            .as_ref()
            .unwrap()
            .nodes
            .iter()
            .filter(|(_, n)| n.role() == egui::accesskit::Role::Button)
            .filter_map(|(_, n)| {
                let label = n.label()?.to_string();
                named
                    .contains(&label.as_str())
                    .then(|| (label, n.is_disabled(), n.description().map(str::to_string)))
            })
            .collect()
    };
    // The whole Note: a mark on each line, or a block of its own, would cut the code span
    // and the link at their line breaks.
    let cutting = ["B", "I", "H", "•", "1.", "“”"];
    let off = buttons(&h, &cutting);
    assert_eq!(off.len(), 6, "{off:?}");
    const SPAN_ACROSS_LINES: &str = "Selection has a code span or link that crosses lines";
    for (label, disabled, description) in &off {
        assert!(disabled, "{label}");
        assert_eq!(description.as_deref(), Some(SPAN_ACROSS_LINES), "{label}");
    }
    for b in ["•", "1.", "“”", "H", "B", "I"] {
        press_key(&mut h, egui::Key::A, Modifiers::COMMAND);
        h.click_text(b);
        assert_eq!(text(&h), body, "{b}");
    }
    for key in [egui::Key::B, egui::Key::I] {
        press_key(&mut h, egui::Key::A, Modifiers::COMMAND);
        press_key(&mut h, key, Modifiers::COMMAND);
        assert_eq!(text(&h), body, "{key:?}");
    }
    // The selection ending inside the link (before its `)`): every button is off.
    press_key(&mut h, egui::Key::A, Modifiers::COMMAND);
    press_key(&mut h, egui::Key::ArrowLeft, Modifiers::SHIFT);
    h.frames(2);
    let every = ["B", "I", "H", "•", "1.", "“”", "🔗"];
    let off = buttons(&h, &every);
    assert_eq!(off.len(), 7, "{off:?}");
    const PARTLY_COVERS_SPAN: &str = "Selection partly covers a code span or link";
    for (label, disabled, description) in &off {
        assert!(disabled, "{label}");
        assert_eq!(description.as_deref(), Some(PARTLY_COVERS_SPAN), "{label}");
    }
    for key in [egui::Key::B, egui::Key::I] {
        press_key(&mut h, egui::Key::A, Modifiers::COMMAND);
        press_key(&mut h, egui::Key::ArrowLeft, Modifiers::SHIFT);
        press_key(&mut h, key, Modifiers::COMMAND);
        assert_eq!(text(&h), body, "{key:?}");
    }
    assert!(h.shell.st.notes_ui.editing.is_some(), "still editing");
    // Bold over lines: the buttons are on, and each line keeps it.
    press_key(&mut h, egui::Key::A, Modifiers::COMMAND);
    h.events.push(Event::Text("**faith\nhope**".into()));
    h.frame();
    press_key(&mut h, egui::Key::A, Modifiers::COMMAND);
    h.frames(2);
    let on = buttons(&h, &cutting);
    assert_eq!(on.len(), 6, "{on:?}");
    for (label, disabled, description) in on {
        assert!(!disabled, "{label}");
        assert!(
            ![SPAN_ACROSS_LINES, PARTLY_COVERS_SPAN]
                .contains(&description.as_deref().unwrap_or_default()),
            "{label}"
        );
    }
    h.click_text("•");
    assert_eq!(text(&h), "- **faith**\n- **hope**");
    drop(h);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn the_note_editor_symbols_are_in_the_chrome_fonts_and_bullets_in_the_reading_font() {
    let h = Harness::new();
    h.ctx.fonts_mut(|f| {
        let missing: Vec<&str> = ["•", "“”", "🔗"]
            .into_iter()
            .filter(|s| !f.has_glyphs(&egui::FontId::proportional(14.0), s))
            .collect();
        assert!(missing.is_empty(), "{missing:?}");
    });
    let face = obs_fonts::face(obs_fonts::FaceId::Regular).unwrap();
    for c in ['•', '○', '–'] {
        assert!(face.covers(c), "{c}");
    }
}

/// Visual evidence for `docs/previews/<name>.png` when `UPDATE_PREVIEWS` is set.
fn preview(h: &Harness, name: &str) {
    if std::env::var_os("UPDATE_PREVIEWS").is_some() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/previews");
        std::fs::create_dir_all(&dir).unwrap();
        h.screenshot(&dir.join(format!("{name}.ppm")));
    }
}
