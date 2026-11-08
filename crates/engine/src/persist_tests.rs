//! What a Workspace snapshot keeps of each Tab (#259 AUD-ENGINE-DATA-3): a Search Tab's
//! search and a Text Comparison's choices come back with the Last session, a Layout and
//! Recently closed, through the commands the menus run and a real User database.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use obs_model::{Manifest, ResourceType};
use serde_json::json;

use crate::bible_search::SearchMode;
use crate::*;

struct Temp(PathBuf);
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn launch(root: &std::path::Path) -> Session {
    let mut s = Session::new(JobMode::Inline);
    for (id, ab) in [("kjv", "KJV"), ("web", "WEB"), ("asv", "ASV")] {
        s.library.add(LibraryEntry {
            manifest: Manifest::new(id, ab, ab, ResourceType::Bible, "en"),
            path: format!("/library/{id}.obr").into(),
        });
    }
    let (store, _) = obs_store::UserStore::open(&root.join("user.db")).unwrap();
    s.attach_user(Box::new(store));
    s
}

/// KJV, WEB and ASV, with a User database; a Note mentioning grace; a Notes search for
/// `grace`; and a Text Comparison of WEB alone, WEB the base, Differences off.
fn studied(name: &str) -> (Session, Temp, TabId, TabId) {
    let root = std::env::temp_dir().join(format!("obs-persist-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let mut s = launch(&root);
    s.run(
        "annotation.note",
        &json!({ "body": "Saved by grace", "passage": "Eph 2:8" }),
    )
    .unwrap();
    let search = TabId(
        s.run("search.run", &json!({ "text": "grace", "mode": "notes" }))
            .unwrap()["tab"]
            .as_u64()
            .unwrap(),
    );
    let compare = TabId(
        s.run("tool.text_comparison", &json!({ "split": true }))
            .unwrap()["tab"]
            .as_u64()
            .unwrap(),
    );
    for id in ["kjv", "asv"] {
        s.run("compare.bible", &json!({ "tab": compare.0, "id": id }))
            .unwrap();
    }
    s.run("compare.base", &json!({ "tab": compare.0, "id": "web" }))
        .unwrap();
    s.run("compare.differences", &json!({ "tab": compare.0 }))
        .unwrap();
    (s, Temp(root), search, compare)
}

fn search_of(s: &Session) -> &crate::bible_search::SearchTab {
    let t = s
        .workspace
        .tabs()
        .find(|t| t.kind == TabKind::Tool(Tool::Search))
        .expect("a Search Tab");
    &t.search
}

fn comparison_of(s: &Session) -> &Comparison {
    let t = s
        .workspace
        .tabs()
        .find(|t| t.kind == TabKind::Tool(Tool::TextComparison))
        .expect("a Text Comparison Tab");
    &t.comparison
}

/// The search came back as it was, run again (its Notes found), without a Search history
/// entry of its own.
fn assert_search_back(s: &Session, history: usize) {
    let st = search_of(s);
    assert_eq!(st.query, "grace");
    assert_eq!(st.mode, SearchMode::Notes);
    assert!(st.restored > 0, "the UI shows the restored fields");
    let found = st.notes.as_ref().expect("the search ran again");
    assert_eq!(found.hits.len(), 1);
    assert_eq!(
        s.search_history().len(),
        history,
        "restoring is not a search"
    );
}

fn assert_comparison_back(s: &Session) {
    let c = comparison_of(s);
    assert_eq!(c.bibles.as_deref(), Some(&["web".to_string()][..]));
    assert_eq!(c.base.as_deref(), Some("web"));
    assert!(!c.differences);
}

#[test]
fn the_last_session_keeps_a_search_and_a_comparison() {
    let (mut s, t, _, compare) = studied("last");
    assert_comparison_back(&s);
    let history = s.search_history().len();
    s.save_last_session().unwrap();
    drop(s);
    let mut s = launch(&t.0);
    assert!(s.restore_last_session());
    assert_search_back(&s, history);
    assert_comparison_back(&s);
    assert!(s.workspace.tab(compare).is_some());
}

#[test]
fn a_layout_keeps_a_search_and_a_comparison() {
    let (mut s, _t, search, compare) = studied("layout");
    let history = s.search_history().len();
    s.run("layout.save", &json!({ "name": "Study" })).unwrap();
    s.run("tab.close", &json!({ "tab": search.0 })).unwrap();
    s.run("tab.close", &json!({ "tab": compare.0 })).unwrap();
    s.run("layout.restore", &json!({ "name": "Study" }))
        .unwrap();
    assert_search_back(&s, history);
    assert_comparison_back(&s);
}

#[test]
fn reopening_a_closed_search_or_comparison_keeps_what_it_held() {
    let (mut s, _t, search, compare) = studied("reopen");
    let history = s.search_history().len();
    s.run("tab.close", &json!({ "tab": search.0 })).unwrap();
    s.run("tab.reopen_closed", &json!({})).unwrap();
    assert_search_back(&s, history);
    s.run("tab.close", &json!({ "tab": compare.0 })).unwrap();
    s.run("tab.reopen_closed", &json!({})).unwrap();
    assert_comparison_back(&s);
}

#[test]
fn snapshots_without_search_or_comparison_fields_still_read() {
    // A Last session written before #259: no `search` or `comparison` on its Tabs.
    let kind = serde_json::to_string(&TabKind::Tool(Tool::Search)).unwrap();
    let json = format!(
        r#"{{"version":1,"tabs":[{{"id":3,"kind":{kind},"title":"Search","location":null,"link":null}}],"layout":null,"focused":3,"remembered":[],"last_reading_panel":null,"last_other_panel":null}}"#
    );
    let snap = WorkspaceSnapshot::from_json(&json).expect("still readable");
    assert!(snap.tabs[0].search.is_none());
    assert_eq!(snap.tabs[0].comparison, Comparison::default());
}
