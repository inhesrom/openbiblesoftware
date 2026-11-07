//! The Command box (#139): what typed text suggests (the reference parser, Strong's numbers,
//! the Starter Lexicons' Lemma Keys and transliterations, Resources, Layouts, Favorites and
//! menu commands), what running a suggestion does, and the recent entries kept in the User
//! database. A real `.obr` library and a real User database in a temp folder.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use obs_scripture::{Book, StrongsNumber, Verse};
use serde_json::json;

use crate::command_box::{CommandBoxAction, SUGGESTION_LIMIT, parse_passage};
use crate::testing::bibles::{bible, v, write_library};
use crate::*;

struct Temp(PathBuf);
impl Temp {
    /// BSB and WEB (Gen 1:1, Jn 3:16, Rom 8:28) and the Starter Lexicons' excerpts (TBESG
    /// has ἀγάπη, G26), scanned in the order of the Starter library's Preferred order.
    fn new(name: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("obs-command-box-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let verses = [
            (
                v(Book::Genesis, 1, 1),
                "In the beginning God created the heavens and the earth.",
            ),
            (
                v(Book::John, 3, 16),
                "For God so loved the world that He gave His one and only Son.",
            ),
            (
                v(Book::Romans, 8, 28),
                "And we know that God works all things together for the good.",
            ),
        ];
        let mut resources = vec![
            bible("bsb", "BSB", &verses, None, &[]),
            bible("web", "WEB", &verses, None, &[]),
        ];
        resources.extend(testing::starter_lexicon_excerpts());
        write_library(&root.join("library"), &resources);
        Self(root)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Launch as the app does: scan the library, then attach the User database.
fn launch(t: &Temp) -> Session {
    let mut s = Session::new(JobMode::Inline);
    assert!(s.library.scan(&t.path().join("library")).is_empty());
    let (store, _) = obs_store::UserStore::open(&t.path().join("user.db")).unwrap();
    s.attach_user(Box::new(store));
    s
}

fn tab(out: &serde_json::Value) -> TabId {
    TabId(out["tab"].as_u64().unwrap())
}

/// The best suggestion's action, label and detail.
fn top(s: &mut Session, text: &str) -> (CommandBoxAction, String, String) {
    let first = s
        .command_box_suggestions(text)
        .into_iter()
        .next()
        .unwrap_or_else(|| panic!("no suggestion for {text:?}"));
    (first.action, first.label, first.detail)
}

fn passage(text: &str) -> obs_scripture::Passage {
    parse_passage(text).unwrap()
}

fn strongs(text: &str) -> StrongsNumber {
    text.parse().unwrap()
}

fn at(s: &Session, t: TabId) -> Option<Verse> {
    s.workspace.tab(t).unwrap().history.current()
}

#[test]
fn the_acceptance_inputs_each_have_the_right_top_suggestion() {
    let t = Temp::new("acceptance");
    let mut s = launch(&t);
    s.run("layout.save", &json!({ "name": "Romans study" }))
        .unwrap();

    let (action, label, detail) = top(&mut s, "jn 3 16");
    assert_eq!(action, CommandBoxAction::GoTo(passage("John 3:16")));
    assert_eq!(label, "John 3:16");
    // No Bible is open: the first Bible in Preferred order.
    assert_eq!(detail, "Go to · BSB");

    let (action, label, detail) = top(&mut s, "G26");
    assert_eq!(action, CommandBoxAction::WordStudy(strongs("G26")));
    assert_eq!((label.as_str(), detail.as_str()), ("G26", "Word Study"));

    // A transliteration, from TBESG's transliteration index.
    let (action, label, _) = top(&mut s, "agape");
    assert_eq!(action, CommandBoxAction::WordStudy(strongs("G26")));
    assert_eq!(label, "ἀγάπη · G26");

    let (action, label, detail) = top(&mut s, "BSB");
    assert_eq!(action, CommandBoxAction::OpenResource("bsb".into()));
    assert_eq!(label, "BSB — BSB (test)");
    assert_eq!(detail, "Open Bible");

    let (action, label, detail) = top(&mut s, "pg rom 8");
    assert_eq!(action, CommandBoxAction::PassageGuide(passage("Rom 8")));
    assert_eq!(
        (label.as_str(), detail.as_str()),
        ("Romans 8", "Passage Guide")
    );

    // A Layout's name, typed in any case.
    let (action, _, detail) = top(&mut s, "romans STUDY");
    assert_eq!(
        action,
        CommandBoxAction::RestoreLayout("Romans study".into())
    );
    assert_eq!(detail, "Layout");
}

#[test]
fn the_parser_reads_references_verbs_lemmas_and_falls_back_to_search() {
    let t = Temp::new("parser");
    let mut s = launch(&t);
    // References: a space for the colon, ordinals, lists (the first Passage), full names.
    assert_eq!(passage("jn 3 16"), passage("John 3:16"));
    assert_eq!(passage("1 jn 4 8"), passage("1 John 4:8"));
    assert_eq!(passage("Rom 5:8; Jn 3:16"), passage("Romans 5:8"));
    assert!(parse_passage("agape").is_none());
    assert!(parse_passage("jn three 16").is_none());

    // Each verb, short and long.
    for (text, want) in [
        (
            "passage guide Jn 3:16",
            CommandBoxAction::PassageGuide(passage("Jn 3:16")),
        ),
        (
            "guide rom 8",
            CommandBoxAction::PassageGuide(passage("Rom 8")),
        ),
        (
            "compare jn 3:16",
            CommandBoxAction::TextComparison(passage("Jn 3:16")),
        ),
        (
            "TC gen 1",
            CommandBoxAction::TextComparison(passage("Gen 1")),
        ),
        (
            "xref rom 8:28",
            CommandBoxAction::CrossReferences(passage("Rom 8:28")),
        ),
        (
            "word study agape",
            CommandBoxAction::WordStudy(strongs("G26")),
        ),
        ("ws H1254A", CommandBoxAction::WordStudy(strongs("H1254A"))),
        ("ws love", CommandBoxAction::WordStudyWords("love".into())),
        ("open web", CommandBoxAction::OpenResource("web".into())),
        (
            "search love one another",
            CommandBoxAction::Search("love one another".into()),
        ),
        // Text whose lowercase is longer than itself (`İ` → `i̇`) keeps its own rest.
        ("search İ", CommandBoxAction::Search("İ".into())),
        (
            "SEARCH İstanbul",
            CommandBoxAction::Search("İstanbul".into()),
        ),
        ("ws ἀγάπη", CommandBoxAction::WordStudy(strongs("G26"))),
    ] {
        assert_eq!(top(&mut s, text).0, want, "{text}");
    }

    // A Lemma in Greek script, accents or not; a Hebrew Lemma.
    assert_eq!(
        top(&mut s, "ἀγάπη").0,
        CommandBoxAction::WordStudy(strongs("G26"))
    );
    assert_eq!(
        top(&mut s, "αγαπη").0,
        CommandBoxAction::WordStudy(strongs("G26"))
    );
    let hebrew = s.command_box_suggestions("בָּרָא");
    assert!(
        matches!(&hebrew[0].action, CommandBoxAction::WordStudy(n) if n.base() == strongs("H1254")),
        "{hebrew:?}"
    );

    // A Resource by part of its title; a menu command by name.
    assert_eq!(
        top(&mut s, "extended strongs").0,
        CommandBoxAction::OpenResource("tbesh".into())
    );
    assert_eq!(
        top(&mut s, "notes").0,
        CommandBoxAction::Command("tool.notes")
    );

    // Anything else searches, then offers the English-word chooser.
    let list = s.command_box_suggestions("love one another");
    let actions: Vec<_> = list.iter().map(|x| x.action.clone()).collect();
    assert_eq!(
        actions,
        [
            CommandBoxAction::Search("love one another".into()),
            CommandBoxAction::WordStudyWords("love one another".into()),
        ]
    );
    // Text something else understood gets no English-word chooser.
    assert!(
        !s.command_box_suggestions("BSB")
            .iter()
            .any(|x| matches!(x.action, CommandBoxAction::WordStudyWords(_)))
    );
    // A reference also offers Search, below it.
    let list = s.command_box_suggestions("Jn 3:16");
    assert_eq!(
        list.last().unwrap().action,
        CommandBoxAction::Search("Jn 3:16".into())
    );
    // `command_box.suggest` gives the same, as JSON.
    let out = s
        .run("command_box.suggest", &json!({ "text": "G26" }))
        .unwrap();
    assert_eq!(out[0]["kind"], "word_study");
    assert_eq!(out[0]["label"], "G26");
}

#[test]
fn search_keeps_the_last_slot_when_the_list_is_full() {
    let t = Temp::new("full");
    let mut s = launch(&t);
    for n in 1..=SUGGESTION_LIMIT {
        s.run("layout.save", &json!({ "name": format!("Study {n}") }))
            .unwrap();
    }
    let list = s.command_box_suggestions("Study");
    assert_eq!(list.len(), SUGGESTION_LIMIT, "{list:?}");
    assert_eq!(
        list.last().unwrap().action,
        CommandBoxAction::Search("Study".into()),
        "{list:?}"
    );
    assert!(
        list[..SUGGESTION_LIMIT - 1]
            .iter()
            .all(|x| matches!(x.action, CommandBoxAction::RestoreLayout(_))),
        "{list:?}"
    );
}

#[test]
fn a_reference_goes_to_the_focused_bible_or_opens_the_preferred_one() {
    let t = Temp::new("go-to");
    let mut s = launch(&t);
    // No Bible open: the first in Preferred order opens there.
    let out = s
        .run("command_box.run", &json!({ "text": "jn 3 16" }))
        .unwrap();
    assert_eq!(out["kind"], "go_to");
    let bsb = tab(&out["out"]);
    assert_eq!(
        s.workspace.tab(bsb).unwrap().kind,
        TabKind::Resource("bsb".into())
    );
    assert!(
        s.workspace
            .take_effects()
            .contains(&Effect::NavigateStandard {
                tab: bsb,
                verse: v(Book::John, 3, 16),
            })
    );

    // The focused Bible moves; no new Tab.
    let web = tab(&s.run("resource.open", &json!({ "id": "web" })).unwrap());
    assert_eq!(top(&mut s, "rom 8:28").2, "Go to · WEB");
    let out = s
        .run("command_box.run", &json!({ "text": "rom 8:28" }))
        .unwrap();
    assert_eq!(tab(&out["out"]), web);
    assert_eq!(at(&s, web), Some(v(Book::Romans, 8, 28)));
    let count = s.workspace.tabs().count();

    // Alt+Enter: a new Tab of the focused Bible.
    let out = s
        .run(
            "command_box.run",
            &json!({ "text": "gen 1:1", "new_tab": true }),
        )
        .unwrap();
    let new = tab(&out["out"]);
    assert_ne!(new, web);
    assert_eq!(s.workspace.tabs().count(), count + 1);
    assert_eq!(
        s.workspace.tab(new).unwrap().kind,
        TabKind::Resource("web".into())
    );

    // From a Tab that is not a Bible, the most recent Bible moves.
    s.run("library.open", &json!({})).unwrap();
    let out = s
        .run("command_box.run", &json!({ "text": "jn 3:16" }))
        .unwrap();
    assert_eq!(tab(&out["out"]), new);
    assert_eq!(at(&s, new), Some(v(Book::John, 3, 16)));
}

#[test]
fn suggestions_run_their_guide_tool_resource_layout_and_search() {
    let t = Temp::new("run");
    let mut s = launch(&t);
    let run = |s: &mut Session, text: &str, new_tab: bool| {
        s.run(
            "command_box.run",
            &json!({ "text": text, "new_tab": new_tab }),
        )
        .unwrap()
    };

    let guide = tab(&run(&mut s, "pg rom 8", false)["out"]);
    let g = s.workspace.tab(guide).unwrap();
    assert_eq!(g.kind, TabKind::Guide(Guide::PassageGuide));
    assert_eq!(g.history.current(), Some(v(Book::Romans, 8, 1)));
    // Again moves the same Guide; Alt+Enter opens another.
    assert_eq!(tab(&run(&mut s, "pg jn 3:16", false)["out"]), guide);
    assert_ne!(tab(&run(&mut s, "pg jn 3:16", true)["out"]), guide);

    let study = tab(&run(&mut s, "agape", false)["out"]);
    let w = s.workspace.tab(study).unwrap();
    assert_eq!(w.kind, TabKind::Guide(Guide::WordStudy));
    assert_eq!(w.entry_history.current().unwrap().to_string(), "G26");

    let compare = tab(&run(&mut s, "compare jn 3:16", false)["out"]);
    assert_eq!(
        s.workspace.tab(compare).unwrap().kind,
        TabKind::Tool(Tool::TextComparison)
    );

    // A Resource already open is focused, not opened again (unless Alt+Enter).
    let bsb = tab(&run(&mut s, "BSB", false)["out"]);
    s.run("library.open", &json!({})).unwrap();
    assert_eq!(tab(&run(&mut s, "bsb", false)["out"]), bsb);
    assert_eq!(s.workspace.focused(), Some(bsb));
    assert_ne!(tab(&run(&mut s, "bsb", true)["out"]), bsb);

    // A Layout restores.
    s.run("layout.save", &json!({ "name": "Two Bibles" }))
        .unwrap();
    let before = s.workspace.tabs().count();
    s.run("tab.close", &json!({ "tab": bsb.0 })).unwrap();
    assert_eq!(run(&mut s, "two bibles", false)["kind"], "layout");
    assert_eq!(s.workspace.tabs().count(), before);

    // Anything else searches.
    let out = run(&mut s, "love one another", false);
    assert_eq!(out["kind"], "search");
    let search = s
        .workspace
        .tabs()
        .find(|t| t.kind == TabKind::Tool(Tool::Search))
        .map(|t| t.id)
        .unwrap();
    assert_eq!(s.search_tab(search).unwrap().query, "love one another");

    // A choice past the list is an error, and nothing is remembered for it.
    assert!(
        s.run("command_box.run", &json!({ "text": "zzz", "choice": 9 }))
            .is_err()
    );
    assert!(!s.command_box_recent().contains(&"zzz".to_string()));
}

#[test]
fn recent_entries_are_kept_in_the_user_database_newest_first() {
    let t = Temp::new("recent");
    {
        let mut s = launch(&t);
        assert!(s.command_box_suggestions("").is_empty());
        for text in ["jn 3 16", "G26", "pg rom 8", "jn 3 16"] {
            s.run("command_box.run", &json!({ "text": text })).unwrap();
        }
        assert_eq!(s.command_box_recent(), ["jn 3 16", "pg rom 8", "G26"]);
    }
    // After a restart, an empty box lists them; running one runs its best suggestion again.
    let mut s = launch(&t);
    let list = s.command_box_suggestions("");
    let shown: Vec<(&str, &str)> = list
        .iter()
        .map(|x| (x.label.as_str(), x.detail.as_str()))
        .collect();
    assert_eq!(
        shown,
        [
            ("jn 3 16", "Recent"),
            ("pg rom 8", "Recent"),
            ("G26", "Recent")
        ]
    );
    let out = s
        .run("command_box.run", &json!({ "text": "", "choice": 1 }))
        .unwrap();
    assert_eq!(out["kind"], "recent");
    let guide = tab(&out["out"]);
    assert_eq!(
        s.workspace.tab(guide).unwrap().kind,
        TabKind::Guide(Guide::PassageGuide)
    );
    assert_eq!(s.command_box_recent(), ["pg rom 8", "jn 3 16", "G26"]);
}

#[test]
fn ctrl_l_asks_the_ui_to_focus_the_command_box() {
    let mut s = Session::new(JobMode::Inline);
    assert_eq!(
        s.commands().get("command_box.focus").unwrap().shortcut,
        Some("Ctrl+L")
    );
    s.run("command_box.focus", &json!({})).unwrap();
    assert!(
        s.workspace
            .take_effects()
            .contains(&Effect::FocusCommandBox)
    );
}

/// All commands (#226): `>` lists only commands, every listed one, each with its key; a
/// command that needs a Tab runs on the focused one; scripting commands are never listed;
/// a `>` text is not kept as a recent entry; `app.commands` asks the UI to list them.
#[test]
fn all_commands_lists_every_command_and_runs_a_tab_command_on_the_focused_tab() {
    let t = Temp::new("all-commands");
    let mut s = launch(&t);
    let listed: Vec<&str> = s.listed_commands().iter().map(|c| c.id).collect();
    for id in crate::SCRIPTING_COMMANDS
        .iter()
        .chain(crate::IN_PLACE_COMMANDS)
    {
        assert!(!listed.contains(id), "{id} is listed");
    }
    // Menu commands first, in menu order: File's Library leads.
    assert_eq!(listed.first(), Some(&"library.open"));
    assert!(listed.contains(&"tab.close") && listed.contains(&"toolbar.library"));
    let all = s.command_box_suggestions(">");
    assert!(
        all.len() > SUGGESTION_LIMIT,
        "no limit of eight: {}",
        all.len()
    );
    assert!(
        (all.iter()).all(|x| matches!(x.action, CommandBoxAction::Command(_))),
        "{all:?}"
    );
    let close = &s.command_box_suggestions("> close tab")[0];
    assert_eq!(close.action, CommandBoxAction::Command("tab.close"));
    assert_eq!(
        (close.label.as_str(), close.detail.as_str()),
        ("Close Tab", "Command · Ctrl+W")
    );

    let bsb = tab(&s.run("resource.open", &json!({ "id": "bsb" })).unwrap());
    assert_eq!(s.workspace.focused(), Some(bsb));
    // The UI asks which command a suggestion runs before running it (its guards, #226).
    assert_eq!(s.command_box_command("> close tab", 0), Some("tab.close"));
    assert_eq!(s.command_box_command("> close tab", 99), None);
    assert_eq!(s.command_box_command("jn 3:16", 0), None);
    s.run("command_box.run", &json!({ "text": "> close tab" }))
        .unwrap();
    assert!(s.workspace.tab(bsb).is_none());
    assert!(s.command_box_recent().is_empty(), "a > text is not kept");
    // With no Tab open, it says so instead of failing on a missing parameter.
    while let Some(t) = s.workspace.focused() {
        s.workspace.close(t);
    }
    let err = s
        .run("command_box.run", &json!({ "text": "> close tab" }))
        .unwrap_err();
    assert!(err.to_string().contains("Open a Tab first"), "{err}");

    s.workspace.take_effects();
    s.run("app.commands", &json!({})).unwrap();
    assert!(s.workspace.take_effects().contains(&Effect::ListCommands));
}
