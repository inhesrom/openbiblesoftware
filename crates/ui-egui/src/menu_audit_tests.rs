//! The menu and command audit (#226): the menu bar is File · Edit · View · Go · Tools · Window ·
//! Help; every registered command is in a menu, listed by the Command box (Help → All
//! commands), answers scripts only, or is run with its parameters from its own place in the
//! UI; no command is in the menus twice and no two listed commands share a label; labels
//! follow the naming rules of `docs/dev/menus.md`; the menu map there is the menus as built;
//! and every menu item and listed command shows its Keyboard shortcut.

use std::collections::BTreeSet;

use egui::Key;
use obs_engine::{
    COMMANDS_PREFIX, CommandBoxAction, CommandSpec, IN_PLACE_COMMANDS, MENUS, MenuItem,
    SCRIPTING_COMMANDS, SETTINGS, SettingKind, is_listed,
};

use super::*;
use crate::shell::command_box::BOX_ID;

const CTRL_SHIFT: Modifiers = Modifiers {
    ctrl: true,
    command: true,
    shift: true,
    ..Modifiers::NONE
};

fn key(h: &mut Harness, key: Key, modifiers: Modifiers) {
    h.modifiers = modifiers;
    for pressed in [true, false] {
        h.events.push(Event::Key {
            key,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers,
        });
        h.frame();
    }
    h.modifiers = Modifiers::NONE;
    h.frames(2);
}

fn type_text(h: &mut Harness, text: &str) {
    h.events.push(Event::Text(text.into()));
    h.frames(2);
}

fn escape(h: &mut Harness) {
    key(h, Key::Escape, Modifiers::NONE);
}

fn specs(h: &Harness) -> Vec<CommandSpec> {
    h.shell.st.session.commands().iter().copied().collect()
}

/// Every command the menu bar runs, with the menu it is in, submenus and choices included.
fn menu_commands() -> Vec<(&'static str, &'static str)> {
    MENUS
        .iter()
        .flat_map(|m| m.commands().into_iter().map(move |id| (m.title, id)))
        .collect()
}

fn workspace_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The UI's own sources (tests left out), where a command run with parameters is named.
fn ui_sources() -> String {
    fn walk(dir: &std::path::Path, out: &mut String) {
        for e in std::fs::read_dir(dir).unwrap().flatten() {
            let p = e.path();
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or_default();
            if p.is_dir() {
                walk(&p, out);
            } else if name.ends_with(".rs") && !name.ends_with("tests.rs") {
                out.push_str(&std::fs::read_to_string(&p).unwrap());
            }
        }
    }
    let mut out = String::new();
    walk(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
        &mut out,
    );
    out
}

#[test]
fn the_menu_bar_is_file_edit_view_go_tools_window_help() {
    let titles: Vec<&str> = MENUS.iter().map(|m| m.title).collect();
    assert_eq!(
        titles,
        ["File", "Edit", "View", "Go", "Tools", "Window", "Help"]
    );
    // Drawn in that order, left of the Launchers (the toolbar is at the Top by default).
    let h = Harness::new();
    let lefts: Vec<f32> = titles
        .iter()
        .map(|t| {
            (h.shapes.iter())
                .filter(|(_, s)| s == t)
                .map(|(r, _)| *r)
                .min_by(|a, b| a.top().total_cmp(&b.top()))
                .unwrap_or_else(|| panic!("no menu {t}: {:?}", h.texts()))
                .left()
        })
        .collect();
    assert!(lefts.windows(2).all(|w| w[0] < w[1]), "{lefts:?}");
    let library_launcher = (h.shapes.iter())
        .filter(|(_, s)| s == "Library")
        .map(|(r, _)| *r)
        .min_by(|a, b| a.top().total_cmp(&b.top()))
        .unwrap();
    assert!(library_launcher.left() > lefts[6], "Launchers after Help");
}

/// No orphans: every registered command is in a menu, listed by the Command box, a scripting
/// command, a Setting's choices, or run from the UI with the parameters only its own place
/// has.
#[test]
fn every_command_is_in_a_menu_the_command_box_or_its_own_place() {
    let h = Harness::new();
    let in_menus: BTreeSet<&str> = menu_commands().into_iter().map(|(_, id)| id).collect();
    let sources = ui_sources();
    let run_by_the_ui = |id: &str| sources.contains(&format!("\"{id}\""));
    // A choice or a switch only Settings offers (Log level, Include pre-releases): its row
    // runs it with the value picked.
    let a_settings_choice = |id: &str| {
        SETTINGS.iter().any(|s| match s.kind {
            SettingKind::Choice { command, .. } => command == id,
            SettingKind::Toggle {
                command,
                param: Some(_),
                ..
            } => command == id,
            _ => false,
        })
    };
    let mut orphans = Vec::new();
    for spec in specs(&h) {
        let id = spec.id;
        let scripting = SCRIPTING_COMMANDS.contains(&id);
        let reached = in_menus.contains(id)
            || is_listed(&spec)
            || scripting
            || a_settings_choice(id)
            || (IN_PLACE_COMMANDS.contains(&id) || !spec.params.is_empty()) && run_by_the_ui(id);
        if !reached {
            orphans.push(id);
        }
        // A scripting command changes nothing on screen: no menu offers it.
        assert!(!(scripting && in_menus.contains(id)), "{id} is in a menu");
    }
    assert!(orphans.is_empty(), "orphan commands: {orphans:?}");
    for id in SCRIPTING_COMMANDS.iter().chain(IN_PLACE_COMMANDS) {
        assert!(h.shell.st.session.commands().get(id).is_some(), "{id}");
    }
    // Every menu command is registered; one with parameters is a choice or a list item
    // (Theme, Recently closed …), else it needs nothing but the focused Tab.
    for m in MENUS {
        for item in m.items {
            for id in item.commands() {
                let spec = h.shell.st.session.commands().get(id);
                let spec = spec.unwrap_or_else(|| panic!("{id} in {} is not registered", m.title));
                if matches!(item, MenuItem::Command(_) | MenuItem::Submenu { .. }) {
                    assert!(is_listed(spec), "{id} in {} needs parameters", m.title);
                }
            }
        }
    }
}

/// The Command box lists every listed command: `>` alone gives all the enabled ones, each
/// with its Keyboard shortcut, and `>` with a label finds that command first.
#[test]
fn the_command_box_lists_every_command_with_its_shortcut() {
    let mut h = Harness::new();
    let s = &mut h.shell.st.session;
    let listed = s.listed_commands();
    let all = s.command_box_suggestions(COMMANDS_PREFIX);
    let enabled: Vec<&str> = (listed.iter())
        .filter(|c| s.is_enabled(c.id))
        .map(|c| c.id)
        .collect();
    let shown: Vec<&str> = (all.iter())
        .map(|x| match &x.action {
            CommandBoxAction::Command(id) => *id,
            other => panic!("{other:?} in the command list"),
        })
        .collect();
    assert_eq!(shown, enabled);
    assert!(shown.len() > 60, "{}", shown.len());
    for spec in &listed {
        if !s.is_enabled(spec.id) {
            continue;
        }
        let found = s.command_box_suggestions(&format!("{COMMANDS_PREFIX}{}", spec.label));
        assert_eq!(
            found.first().map(|x| &x.action),
            Some(&CommandBoxAction::Command(spec.id)),
            "> {} finds {found:?}",
            spec.label
        );
        let detail = &found[0].detail;
        match s.shortcut(spec.id) {
            Some(key) => assert_eq!(detail, &format!("Command · {key}")),
            None => assert_eq!(detail, "Command"),
        }
    }
    // Typed without `>`, a command's name still finds it among the other suggestions.
    let close = s.command_box_suggestions("close tab");
    assert!(
        (close.iter()).any(|x| x.action == CommandBoxAction::Command("tab.close")),
        "{close:?}"
    );
}

/// No duplicates: a command is in the menus once, and no two commands the Command box lists
/// (or the menus show) have the same label.
#[test]
fn no_command_is_in_the_menus_twice_and_no_two_listed_commands_share_a_label() {
    let h = Harness::new();
    let mut seen = BTreeSet::new();
    for (menu, id) in menu_commands() {
        // Recently closed reopens the entry chosen; Reopen closed Tab the newest.
        if id == "tab.reopen_closed" && seen.contains(id) {
            continue;
        }
        assert!(
            seen.insert(id),
            "{id} is in the menus twice (again in {menu})"
        );
    }
    let mut labels: std::collections::BTreeMap<String, &str> = Default::default();
    for spec in h.shell.st.session.listed_commands() {
        let key = spec.label.trim_end_matches('…').to_lowercase();
        if let Some(other) = labels.insert(key, spec.id) {
            panic!("{} and {other} are both labelled {:?}", spec.id, spec.label);
        }
    }
}

/// CONTEXT.md terms the app writes with their capitals mid-sentence (a Tab, the Word Study);
/// a label naming one uses its casing (#226).
const PROPER_TERMS: &[&str] = &[
    "Annotation",
    "Atlas",
    "Bible",
    "Built-in Layout",
    "Collection",
    "Command box",
    "Copy preset",
    "Exegetical Guide",
    "Factbook",
    "Favorite",
    "Get Resources",
    "Guide",
    "Guide section",
    "Highlight",
    "Highlight palette",
    "Highlight style",
    "History",
    "Home",
    "Import report",
    "Interface size",
    "Keyboard shortcut",
    "Layout",
    "Lemma",
    "Link set",
    "Note",
    "Notebook",
    "Panel",
    "Passage",
    "Passage Guide",
    "Preferred order",
    "Reading plan",
    "Resource",
    "Resource type",
    "Saved search",
    "Search history",
    "Search index",
    "Search scope",
    "Starter library",
    "Strong's number",
    "Tab",
    "Text Comparison",
    "Text size",
    "Timeline",
    "Today's reading",
    "Tool",
    "Topic Guide",
    "Verse of the day",
    "Word Info",
    "Word Study",
    "Workspace",
];

/// Names that keep their capitals but are not CONTEXT.md terms.
const NAMES: &[&str] = &[
    "Open Bible",
    "Library",
    "Search",
    "Notes",
    "CSV",
    "Greek",
    "Hebrew",
    "English",
];

/// Verbs a label may start with when it ends in "…": the command asks for something (a name,
/// a file, a choice, a confirmation) before it acts.
const ASKING_VERBS: &[&str] = &[
    "Back",
    "Copy",
    "Customize",
    "Delete",
    "Export",
    "Get",
    "Import",
    "New",
    "Remove",
    "Rename",
    "Report",
    "Save",
];

/// The terms of CONTEXT.md: its bold headings.
fn context_terms() -> Vec<String> {
    let text = std::fs::read_to_string(workspace_root().join("CONTEXT.md")).unwrap();
    text.lines()
        .filter_map(|l| l.strip_prefix("**")?.split("**").next())
        .map(str::to_string)
        .collect()
}

/// Where `term` (or its plural) is in `label` as whole words, ignoring case.
fn occurrences(label: &str, term: &str) -> Vec<std::ops::Range<usize>> {
    let lower = label.to_lowercase();
    let t = term.to_lowercase();
    let word = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || c == '\'' || c == '-');
    let mut out = Vec::new();
    for (i, _) in lower.match_indices(&t) {
        if word(lower[..i].chars().next_back()) {
            continue;
        }
        let mut end = i + t.len();
        for plural in ["'s", "es", "s"] {
            if lower[end..].starts_with(plural) && !word(lower[end + plural.len()..].chars().next())
            {
                end += plural.len();
                break;
            }
        }
        if !word(lower[end..].chars().next()) {
            out.push(i..end);
        }
    }
    out
}

/// What is wrong with a label under the naming rules of `docs/dev/menus.md`, if anything.
fn label_problem(label: &str, terms: &[String]) -> Option<String> {
    if label.trim() != label || label.contains("  ") || label.is_empty() {
        return Some("spacing".into());
    }
    if label.contains("...") {
        return Some("three dots: write …".into());
    }
    if let Some(i) = label.find('…')
        && (i + '…'.len_utf8() != label.len() || label[..i].ends_with(' '))
    {
        return Some("… only at the end, after a word".into());
    }
    let first = label.split_whitespace().next().unwrap_or_default();
    let first = first.trim_end_matches('…');
    if !first.chars().next().is_some_and(char::is_uppercase) {
        return Some("starts with a capital".into());
    }
    if label.ends_with('…') && !ASKING_VERBS.contains(&first) {
        return Some(format!(
            "… is for a command that asks before it acts, not {first:?}"
        ));
    }
    // Where terms and names are written as they should be, longest first.
    let mut covered = vec![false; label.len()];
    let mut all: Vec<&str> = (terms.iter().map(String::as_str))
        .chain(NAMES.iter().copied())
        .collect();
    all.sort_by_key(|t| std::cmp::Reverse(t.len()));
    for t in &all {
        // A sentence-case term that is not a proper one is written in lower case mid-label
        // ("Compare a parallel passage"); a plural term also names one ("parallel resource").
        let sentence = !PROPER_TERMS.contains(t) && t[1..].chars().all(|c| !c.is_uppercase());
        let lower = t.to_lowercase();
        let forms = [Some(*t), t.strip_suffix('s')];
        for form in forms.into_iter().flatten() {
            for r in occurrences(label, form) {
                let written = &label[r.clone()];
                let matches = written.starts_with(form)
                    || (r.start == 0 && written[1..].starts_with(&form[1..]))
                    || (sentence && written.starts_with(&lower[..form.len()]));
                if matches {
                    covered[r].iter_mut().for_each(|c| *c = true);
                }
            }
        }
    }
    // A proper term not covered is written in the wrong case.
    for t in PROPER_TERMS {
        for r in occurrences(label, t) {
            if !covered[r.start] {
                return Some(format!("write {t:?} as CONTEXT.md does"));
            }
        }
    }
    // Sentence case: a capital after the first word is a term's or a name's.
    let mut at = 0;
    for (n, w) in label.split(' ').enumerate() {
        if n > 0 && w.chars().next().is_some_and(char::is_uppercase) && !covered[at] {
            return Some(format!("sentence case: {w:?} is not a CONTEXT.md term"));
        }
        at += w.len() + 1;
    }
    None
}

#[test]
fn labels_follow_the_naming_rules() {
    let h = Harness::new();
    let terms = context_terms();
    for t in PROPER_TERMS {
        assert!(terms.iter().any(|c| c == t), "{t} is not in CONTEXT.md");
    }
    // The rules catch what they are for.
    for (bad, why) in [
        ("Reset layout", "Layout"),
        ("Fonts…", "asks"),
        ("Import...", "three dots"),
        ("Close tab", "Tab"),
        ("Open In Notes", "sentence case"),
        ("Rename …", "only at the end"),
    ] {
        let p = label_problem(bad, &terms).unwrap_or_else(|| panic!("{bad} passes"));
        assert!(p.contains(why), "{bad}: {p}");
    }
    let mut problems = Vec::new();
    for spec in specs(&h) {
        if let Some(p) = label_problem(spec.label, &terms) {
            problems.push(format!("{} {:?}: {p}", spec.id, spec.label));
        }
    }
    fn titles(items: &[MenuItem], out: &mut Vec<&'static str>) {
        for i in items {
            match i {
                MenuItem::Choice { title, .. } => out.push(title),
                MenuItem::Submenu { title, items } => {
                    out.push(title);
                    titles(items, out);
                }
                _ => {}
            }
        }
    }
    let mut menu_titles = Vec::new();
    for m in MENUS {
        menu_titles.push(m.title);
        titles(m.items, &mut menu_titles);
    }
    for t in
        menu_titles
            .into_iter()
            .chain(["Notebooks in text", "Recently closed", "Reading layout"])
    {
        if let Some(p) = label_problem(t, &terms) {
            problems.push(format!("menu {t:?}: {p}"));
        }
    }
    assert!(problems.is_empty(), "{problems:#?}");
}

/// The menu map in `docs/dev/menus.md`, from [`MENUS`]: each menu's items in order with their
/// command ids and default Keyboard shortcuts.
fn menu_map(h: &Harness) -> String {
    let commands = h.shell.st.session.commands();
    let mut out = String::new();
    fn rows(
        items: &[MenuItem],
        depth: usize,
        commands: &obs_engine::CommandRegistry,
        out: &mut String,
    ) {
        let indent = "↳ ".repeat(depth);
        for item in items {
            let row = match item {
                MenuItem::Command(id) => {
                    let spec = commands.get(id).unwrap();
                    format!(
                        "| {indent}{} | `{id}` | {} |",
                        spec.label,
                        spec.shortcut.unwrap_or("")
                    )
                }
                MenuItem::Separator => "| — | | |".to_string(),
                MenuItem::Choice { title, command } => {
                    format!("| {indent}{title} ▸ | `{command}` (one per choice) | |")
                }
                MenuItem::Notebooks => format!(
                    "| {indent}Notebooks in text ▸ | `notebook.show_in_text` (one per Notebook) | |"
                ),
                MenuItem::RecentlyClosed => {
                    format!("| {indent}Recently closed ▸ | `tab.reopen_closed` (one per entry) | |")
                }
                MenuItem::ReadingLayout => {
                    format!(
                        "| {indent}Reading layout ▸ | `tab.reading_layout` (one per choice) | |"
                    )
                }
                MenuItem::Submenu { title, items } => {
                    out.push_str(&format!("| {indent}{title} ▸ | | |\n"));
                    rows(items, depth + 1, commands, out);
                    continue;
                }
                MenuItem::Favorites | MenuItem::Layouts | MenuItem::Resources => {
                    panic!("{item:?} is drawn by a Launcher only")
                }
            };
            out.push_str(&row);
            out.push('\n');
        }
    }
    for m in MENUS {
        out.push_str(&format!(
            "\n### {}\n\n| Item | Command | Keyboard shortcut |\n|---|---|---|\n",
            m.title
        ));
        rows(m.items, 0, commands, &mut out);
    }
    out
}

const MAP_START: &str =
    "<!-- menu-map:start (written by menu_audit_tests; OBS_BLESS=1 rewrites it) -->\n";
const MAP_END: &str = "<!-- menu-map:end -->";

/// The menu map is documented, and is the menus as built. `OBS_BLESS=1` rewrites it.
#[test]
fn the_menu_map_in_docs_dev_menus_md_is_the_menu_bar() {
    let h = Harness::new();
    let path = workspace_root().join("docs/dev/menus.md");
    let doc = std::fs::read_to_string(&path).unwrap();
    let start = doc.find(MAP_START).expect("menu map start marker") + MAP_START.len();
    let end = doc.find(MAP_END).expect("menu map end marker");
    let map = menu_map(&h);
    if std::env::var_os("OBS_BLESS").is_some() {
        let new = format!("{}{map}\n{}", &doc[..start], &doc[end..]);
        std::fs::write(&path, new).unwrap();
        return;
    }
    assert_eq!(
        doc[start..end].trim(),
        map.trim(),
        "docs/dev/menus.md is out of date: run with OBS_BLESS=1"
    );
}

/// Every menu item shows its label and its Keyboard shortcut; a submenu's items too.
#[test]
fn every_menu_item_shows_its_label_and_shortcut() {
    let mut h = Harness::new();
    for m in MENUS {
        h.open_menu(m.title);
        let mut submenus = Vec::new();
        for item in m.items {
            match item {
                MenuItem::Command(id) => shows(&h, m.title, id),
                MenuItem::Submenu { title, items } => submenus.push((*title, *items)),
                _ => {}
            }
        }
        for (title, items) in submenus {
            let p = h.at(title);
            h.events.push(Event::PointerMoved(p));
            h.frames(3);
            for item in items {
                if let MenuItem::Command(id) = item {
                    shows(&h, title, id);
                }
            }
        }
        escape(&mut h);
        escape(&mut h);
    }

    fn shows(h: &Harness, menu: &str, id: &str) {
        let s = &h.shell.st.session;
        let spec = s.commands().get(id).unwrap();
        assert!(
            h.texts().iter().any(|t| t.ends_with(spec.label)),
            "{id} ({}) not in {menu}: {:?}",
            spec.label,
            h.texts()
        );
        if let Some(key) = s.shortcut(id) {
            let shown = crate::key_labels::label(key);
            assert!(
                h.find(&shown).is_some(),
                "{id}'s {shown} not shown in {menu}"
            );
        }
    }
}

/// A menu command that needs a Tab runs on the focused one: Window → Close Tab.
#[test]
fn a_tab_command_in_a_menu_runs_on_the_focused_tab() {
    let mut h = Harness::new();
    h.shell
        .run("resource.open", &serde_json::json!({ "id": "heb-test" }));
    h.frames(3);
    let heb = h.tab_id("HEB");
    assert_eq!(h.shell.st.session.workspace.focused(), Some(heb));
    h.open_menu("Window");
    h.click_text("Close Tab");
    assert!(h.shell.st.session.workspace.tab(heb).is_none());
    // The Go menu shows the focused Tab's movement keys.
    h.open_menu("Go");
    assert!(h.find("Alt+←").is_some(), "{:?}", h.texts());
    escape(&mut h);
}

/// Help → All commands and Ctrl+Shift+P put the keyboard in the Command box listing the
/// commands; typing filters them and Enter runs the first on the focused Tab.
#[test]
fn all_commands_lists_them_in_the_command_box_and_runs_one() {
    let mut h = Harness::new();
    h.open_menu("Help");
    h.click_text("All commands");
    assert!(h.ctx.memory(|m| m.has_focus(egui::Id::unique(BOX_ID))));
    assert_eq!(h.shell.st.command_box.text, COMMANDS_PREFIX);
    // The list scrolls: more commands than rows, each with its key.
    assert!(h.shell.st.command_box.suggestions.len() > 60);
    assert!(h.find("File").is_some());
    assert!(
        h.texts().iter().any(|t| t.starts_with("Command · ")),
        "{:?}",
        h.texts()
    );
    escape(&mut h);

    h.shell
        .run("resource.open", &serde_json::json!({ "id": "heb-test" }));
    h.frames(3);
    let heb = h.tab_id("HEB");
    key(&mut h, Key::P, CTRL_SHIFT);
    assert_eq!(h.shell.st.command_box.text, COMMANDS_PREFIX);
    // The caret is after `>`: typing adds to it.
    type_text(&mut h, "close tab");
    assert_eq!(h.shell.st.command_box.text, ">close tab");
    let first = &h.shell.st.command_box.suggestions[0];
    assert_eq!(first.action, CommandBoxAction::Command("tab.close"));
    assert_eq!(first.detail, "Command · Ctrl+W");
    key(&mut h, Key::Enter, Modifiers::NONE);
    assert!(h.shell.st.session.workspace.tab(heb).is_none());
    // A command list is not kept as a recent entry.
    assert!(
        (h.shell.st.session.command_box_recent().iter()).all(|t| !t.starts_with(COMMANDS_PREFIX))
    );
}

/// All commands opened again after its list was scrolled down shows its first command, the
/// one highlighted, not the rows it was scrolled to.
#[test]
fn all_commands_reopened_after_a_scroll_shows_its_first_command() {
    let mut h = Harness::new();
    key(&mut h, Key::P, CTRL_SHIFT);
    let first = h.shell.st.command_box.suggestions[0].detail.clone();
    // The first row's detail (its key) is painted only in the list.
    let top = h.at(&first).y;
    for _ in 0..40 {
        key(&mut h, Key::ArrowDown, Modifiers::NONE);
    }
    assert_eq!(h.shell.st.command_box.selected, 40);
    assert!(h.at(&first).y < top - 20.0 * 26.0, "scrolled down");
    escape(&mut h);
    assert!(h.find(&first).is_none(), "the list closed");
    key(&mut h, Key::P, CTRL_SHIFT);
    assert_eq!(h.shell.st.command_box.selected, 0);
    assert_eq!(h.at(&first).y, top, "{:?}", h.texts());
}

/// All commands run from the Command box keeps the list it asked for: the box is not emptied
/// after it, and keeps the keyboard.
#[test]
fn all_commands_run_from_the_command_box_keeps_its_list() {
    let mut h = Harness::new();
    key(&mut h, Key::P, CTRL_SHIFT);
    type_text(&mut h, "all commands");
    let first = &h.shell.st.command_box.suggestions[0];
    assert_eq!(first.label, "All commands");
    key(&mut h, Key::Enter, Modifiers::NONE);
    assert_eq!(h.shell.st.command_box.text, COMMANDS_PREFIX);
    assert!(h.ctx.memory(|m| m.has_focus(egui::Id::unique(BOX_ID))));
    assert!(h.shell.st.command_box.suggestions.len() > 60);
    assert!(h.find("Close Tab").is_some(), "{:?}", h.texts());
    // Typing filters the list again.
    type_text(&mut h, "close tab");
    assert_eq!(h.shell.st.command_box.text, ">close tab");
    assert_eq!(
        h.shell.st.command_box.suggestions[0].action,
        CommandBoxAction::Command("tab.close")
    );
}

/// Word Info's Copy run from the Command box puts the shown word's cards on the clipboard, as
/// its button does.
#[test]
fn copy_from_the_command_box_writes_the_clipboard() {
    let (mut h, heb) = super::word_info::hebrew_harness();
    let bara = super::word_info::word_spot(&h, heb, "בָּרָ֣א");
    h.click(bara);
    assert!(h.shell.st.session.word_info.pinned);
    let cards = (h.shell.st.session.word_info.shown.as_ref())
        .unwrap()
        .copy_text();
    h.click_text("Copy");
    assert_eq!(h.copied.last(), Some(&cards), "the button");
    h.copied.clear();

    key(&mut h, Key::P, CTRL_SHIFT);
    type_text(&mut h, "copy");
    let first = &h.shell.st.command_box.suggestions[0];
    assert_eq!(first.action, CommandBoxAction::Command("word_info.copy"));
    key(&mut h, Key::Enter, Modifiers::NONE);
    assert_eq!(h.copied, vec![cards], "the Command box");
    assert!(h.find("Copied").is_some(), "{:?}", h.texts());
}

/// Visual evidence for docs/previews/ when `UPDATE_PREVIEWS` is set.
fn preview(h: &Harness, name: &str) {
    if std::env::var_os("UPDATE_PREVIEWS").is_some() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/previews");
        std::fs::create_dir_all(&dir).unwrap();
        h.screenshot(&dir.join(format!("{name}.ppm")));
    }
}

/// The menu bar, the File menu, Window → Go to Panel, and All commands, for the PR.
#[test]
fn menu_previews() {
    let mut h = Harness::new();
    preview(&h, "menus-bar-kittest");
    h.open_menu("File");
    // Past the menus' fade-in.
    h.frames(30);
    preview(&h, "menus-file-kittest");
    escape(&mut h);
    h.open_menu("Window");
    let p = h.at("Go to Panel");
    h.events.push(Event::PointerMoved(p));
    h.frames(30);
    assert!(h.find("Go to Panel 9").is_some());
    preview(&h, "menus-window-go-to-panel-kittest");
    escape(&mut h);
    escape(&mut h);
    key(&mut h, Key::P, CTRL_SHIFT);
    h.frames(30);
    preview(&h, "menus-all-commands-kittest");
    type_text(&mut h, "close");
    h.frames(30);
    preview(&h, "menus-all-commands-close-kittest");
}
