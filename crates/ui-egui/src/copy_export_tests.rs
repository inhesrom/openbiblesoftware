//! Copy verses, Export Notes and Print (#154) through the headless harness: the Copy verses
//! window opened from the Tools menu, a Copy preset saved and chosen again, the preview, the
//! three Copy buttons and Print; the Notes Tool's Export menu writing a Markdown file and its
//! Print page.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use obs_engine::testing::bibles::{bible, paragraphed_bible, v, write_library};
use obs_model::Direction;
use obs_scripture::{Book, Verse};

use super::*;

const KJV: [&str; 3] = [
    "¶ For God so loved the world, that he gave his only begotten Son, that whosoever believeth in him should not perish, but have everlasting life.",
    "For God sent not his Son into the world to condemn the world; but that the world through him might be saved.",
    "¶ He that believeth on him is not condemned: but he that believeth not is condemned already, because he hath not believed in the name of the only begotten Son of God.",
];

const BSB: [&str; 3] = [
    "For God so loved the world that He gave His one and only Son, that everyone who believes in Him shall not perish but have eternal life.",
    "For God did not send His Son into the world to condemn the world, but to save the world through Him.",
    "Whoever believes in Him is not condemned, but whoever does not believe has already been condemned, because he has not believed in the name of God’s one and only Son.",
];

fn folder(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("obs-copy-ui-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn john(text: [&str; 3]) -> Vec<(Verse, &str)> {
    let mut verses = vec![(v(Book::John, 3, 15), "Verse fifteen.")];
    verses.extend((16..=18).zip(text).map(|(n, t)| (v(Book::John, 3, n), t)));
    verses.push((v(Book::John, 3, 19), "Verse nineteen."));
    verses
}

/// The KJV with each verse its own paragraph, the BSB as one paragraph, as `.obr` files.
fn launch(dir: &Path) -> Harness {
    write_library(
        &dir.join("library"),
        &[
            bible("kjv", "KJV", &john(KJV), None, &[]),
            paragraphed_bible("bsb", "BSB", &john(BSB), Direction::Ltr),
        ],
    );
    let mut h = Harness::launch_full(Some(&dir.join("user.db")), JobMode::Inline, Some(dir));
    for id in ["kjv", "bsb"] {
        h.shell
            .st
            .session
            .library
            .add_file(&dir.join("library").join(format!("{id}.obr")))
            .unwrap();
    }
    h.frames(3);
    h
}

/// Rich text the app put on the clipboard: (HTML, plain text).
#[derive(Default)]
struct Clipboard(Mutex<Vec<(String, String)>>);

impl crate::RichClipboard for Clipboard {
    fn copy_html(&self, html: &str, plain: &str) -> Result<(), String> {
        self.0.lock().unwrap().push((html.into(), plain.into()));
        Ok(())
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

/// Replace a text field's contents: click it, select all, type.
fn type_into(h: &mut Harness, at: Pos2, text: &str) {
    h.click(at);
    h.events.push(Event::Key {
        key: egui::Key::A,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::COMMAND,
    });
    h.events.push(Event::Text(text.into()));
    h.frames(2);
}

/// Where `text` is painted in the Copy verses window.
fn in_window(h: &Harness, text: &str) -> Rect {
    let window = h
        .ctx
        .memory(|m| m.area_rect(egui::Id::unique(super::super::copy_window::WINDOW)))
        .expect("the Copy verses window is open");
    // The topmost: the window is drawn over the Workspace.
    h.shapes
        .iter()
        .rev()
        .find(|(r, t)| t == text && window.contains(r.center()))
        .map(|(r, _)| *r)
        .unwrap_or_else(|| panic!("no {text:?} in the window: {:?}", h.texts()))
}

/// Click `text` in the window, long enough after the last click not to be a double click.
fn click_in_window(h: &mut Harness, text: &str) {
    h.frames(25);
    settle_preview(h);
    let at = in_window(h, text).center();
    h.click(at);
}

/// The window's text field on the row labelled `label`.
fn field(h: &Harness, label: &str) -> Pos2 {
    let r = in_window(h, label);
    pos2(r.right() + 120.0, r.center().y)
}

/// Frames until the preview's lines are laid out (the window, centred, stops moving).
fn settle_preview(h: &mut Harness) {
    for _ in 0..400 {
        let ui = &h.shell.st.copy_ui;
        let laid = (ui.shown.iter()).all(|(line, laid)| line.text.is_empty() || laid.is_some());
        if !ui.has_pending() && laid {
            h.frames(2);
            return;
        }
        h.paced_frame();
    }
}

/// The preview's text once laid out, a line per preview line.
fn preview_text(h: &mut Harness) -> String {
    settle_preview(h);
    let shown = &h.shell.st.copy_ui.shown;
    assert!(
        (shown.iter()).all(|(line, laid)| line.text.is_empty() || laid.is_some()),
        "the preview settles"
    );
    let lines: Vec<&str> = shown.iter().map(|(line, _)| line.text.as_str()).collect();
    lines.join("\n")
}

/// The print page the app last opened in the browser (a `file://` URL: `file:///tmp/…`, or
/// `file:///C:/…` on Windows).
fn print_page(h: &Harness) -> String {
    let url = h.opened.last().expect("a page was opened");
    let encoded = url.strip_prefix("file://").unwrap().as_bytes();
    let mut bytes = Vec::new();
    let mut i = 0;
    while i < encoded.len() {
        if encoded[i] == b'%' {
            let hex = std::str::from_utf8(&encoded[i + 1..i + 3]).unwrap();
            bytes.push(u8::from_str_radix(hex, 16).unwrap());
            i += 3;
        } else {
            bytes.push(encoded[i]);
            i += 1;
        }
    }
    let path = String::from_utf8(bytes).unwrap();
    let path = match path.as_bytes() {
        [b'/', _, b':', ..] => &path[1..],
        _ => &path,
    };
    assert!(!url.contains(' '), "{url}");
    std::fs::read_to_string(path).unwrap()
}

fn expected_markdown() -> String {
    format!(
        "John 3:16-18 (KJV)\n\n“**16** {}\n\n**17** {}\n\n**18** {}”\n\n\
         John 3:16-18 (BSB)\n\n“**16** {} **17** {} **18** {}”",
        KJV[0], KJV[1], KJV[2], BSB[0], BSB[1], BSB[2]
    )
}

fn expected_html() -> String {
    let n = |n: u16| format!("<sup>{n}</sup>\u{a0}");
    format!(
        "<p>John 3:16-18 (KJV)</p>\n<p>“{}{}</p>\n<p>{}{}</p>\n<p>{}{}”</p>\n\
         <p>John 3:16-18 (BSB)</p>\n<p>“{}{} {}{} {}{}”</p>",
        n(16),
        KJV[0],
        n(17),
        KJV[1],
        n(18),
        KJV[2],
        n(16),
        BSB[0],
        n(17),
        BSB[1],
        n(18),
        BSB[2]
    )
}

#[test]
fn copy_verses_copies_jn_3_16_18_in_kjv_and_bsb_with_a_preset_as_markdown_and_rich_text() {
    let dir = folder("window");
    let mut h = launch(&dir);
    let clipboard = Arc::new(Clipboard::default());
    h.shell.set_rich_clipboard(clipboard.clone());
    h.open_menu("Tools");
    h.click_text("Copy verses…");
    assert!(
        h.shell.st.copy_ui.open,
        "Tools → Copy verses… opens the window"
    );
    // It opens where the Bible Tab is, with that Bible chosen.
    assert_eq!(h.shell.st.copy_ui.passages, "Gen 30:1");
    assert_eq!(h.shell.st.copy_ui.bibles, ["test-bible"]);
    // Passages and Bibles, as a user picks them.
    settle_preview(&mut h);
    let passages = field(&h, "Passages");
    type_into(&mut h, passages, "Jn 3:16-18");
    for id in h.shell.st.copy_ui.bibles.clone() {
        let abbrev = h
            .shell
            .st
            .session
            .library
            .get(&id)
            .unwrap()
            .manifest
            .abbreviation
            .clone();
        click_in_window(&mut h, &abbrev);
    }
    click_in_window(&mut h, "KJV");
    click_in_window(&mut h, "BSB");
    assert_eq!(h.shell.st.copy_ui.bibles, ["kjv", "bsb"]);
    // The Handout options, saved as a preset.
    click_in_window(&mut h, "Before the text");
    settle_preview(&mut h);
    let quotes = in_window(&h, "Quotation marks");
    let on = (h.shapes.iter())
        .filter(|(r, t)| t == "On" && (r.center().y - quotes.center().y).abs() < 6.0)
        .map(|(r, _)| r.center())
        .next()
        .unwrap();
    h.click(on);
    settle_preview(&mut h);
    let name = in_window(&h, "Preset name").center();
    type_into(&mut h, name, "Handout");
    click_in_window(&mut h, "Save preset");
    assert_eq!(h.shell.st.session.copy_verses.presets.len(), 1);
    // Something else, then the preset chosen again from the list.
    click_in_window(&mut h, "One verse per line");
    assert!(h.shell.st.session.copy_verses.current_preset().is_none());
    click_in_window(&mut h, "Custom");
    h.click_text("Handout");
    assert_eq!(
        h.shell
            .st
            .session
            .copy_verses
            .current_preset()
            .map(|p| p.name.as_str()),
        Some("Handout")
    );

    // The Markdown preview, then Copy Markdown.
    click_in_window(&mut h, "Markdown");
    let markdown = expected_markdown();
    assert_eq!(preview_text(&mut h), markdown);
    // Resource text goes through the text pipeline (ADR 0004), not egui labels.
    assert!(!h.texts().iter().any(|t| t.contains("loved the world")));
    preview(&h, "copy-verses-markdown");
    click_in_window(&mut h, "Copy Markdown");
    assert_eq!(h.copied.last(), Some(&markdown));
    // Rich text: HTML with its plain-text form.
    click_in_window(&mut h, "Rich text");
    let rich = preview_text(&mut h);
    assert!(
        rich.starts_with(&format!("John 3:16-18 (KJV)\n\n“16\u{a0}{}", KJV[0])),
        "{rich}"
    );
    let numbers = &h.shell.st.copy_ui.shown[2].0;
    assert_eq!(&numbers.text[numbers.numbers[0].clone()], "16");
    preview(&h, "copy-verses-rich-text");
    click_in_window(&mut h, "Copy rich text");
    let (html, plain) = clipboard.0.lock().unwrap().last().cloned().unwrap();
    assert_eq!(html, expected_html());
    assert!(plain.starts_with(&format!("John 3:16-18 (KJV)\n“16 {}", KJV[0])));
    click_in_window(&mut h, "Copy plain text");
    assert_eq!(h.copied.last(), Some(&plain));

    // Print / Save as PDF: a print page opened in the browser.
    click_in_window(&mut h, "Print…");
    let page = print_page(&h);
    assert!(page.contains(&expected_html()) && page.contains("window.print()"));
    drop(h);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn without_a_rich_clipboard_rich_text_is_copied_as_plain_text() {
    let dir = folder("plain");
    let mut h = launch(&dir);
    h.shell.run(
        "tool.copy_verses",
        &json!({ "passages": "Jn 3:16", "bibles": "bsb" }),
    );
    h.frames(2);
    click_in_window(&mut h, "Copy rich text");
    assert_eq!(
        h.copied.last().map(String::as_str),
        Some(format!("16 {}\nJohn 3:16 (BSB)", BSB[0]).as_str())
    );
    // A reference that does not parse says so and copies nothing.
    h.shell.st.copy_ui.passages = "Hezekiah 3:16".into();
    h.frames(2);
    let copies = h.copied.len();
    click_in_window(&mut h, "Copy Markdown");
    assert_eq!(h.copied.len(), copies);
    assert!(
        h.texts().iter().any(|t| t.contains("Hezekiah")),
        "the error names what did not parse: {:?}",
        h.texts()
    );
    drop(h);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_notes_tool_exports_its_notebook_to_markdown_and_prints_it() {
    let dir = folder("notes");
    let mut h = launch(&dir);
    let out = h
        .shell
        .run("notebook.create", &json!({ "name": "Sermon prep" }))
        .unwrap();
    let notebook = out["id"].as_str().unwrap().to_string();
    h.shell.run(
        "annotation.note",
        &json!({ "passage": "Jn 3:16", "body": "God's love\n# for the world", "notebook": notebook }),
    );
    h.shell.run("tool.notes", &json!({}));
    h.shell.run("notes.notebook", &json!({ "value": notebook }));
    h.frames(3);
    let saved = dir.join("export.md");
    let to = saved.clone();
    h.shell.st.export_ui.save_dialog = Arc::new(move |name, format| {
        assert_eq!(
            (name, format),
            ("Sermon prep.md", obs_engine::NotesFormat::Markdown)
        );
        Some(to.clone())
    });
    h.click_text("Export…");
    h.frames(30);
    preview(&h, "notes-export-menu");
    h.click_text("Export as Markdown…");
    for _ in 0..200 {
        if saved.exists() {
            break;
        }
        h.paced_frame();
    }
    let markdown = std::fs::read_to_string(&saved).unwrap();
    assert_eq!(
        markdown,
        "# Sermon prep\n\n## John 3:16\n\nGod's love\\\n\\# for the world\n"
    );
    h.click_text("Export…");
    h.click_text("Print / Save as PDF…");
    let page = print_page(&h);
    assert!(page.contains("<h1>Sermon prep</h1>") && page.contains("<h2>John 3:16</h2>"));
    drop(h);
    let _ = std::fs::remove_dir_all(&dir);
}

const GEN_1_1_2: [&str; 2] = ["בְּרֵאשִׁ֖ית בָּרָ֣א אֱלֹהִ֑ים אֵ֥ת הַשָּׁמַ֖יִם וְאֵ֥ת הָאָֽרֶץ׃", "וְהָאָ֗רֶץ הָיְתָ֥ה תֹ֙הוּ֙ וָבֹ֔הוּ"];

#[test]
fn a_hebrew_bibles_preview_reads_right_to_left_through_the_text_pipeline() {
    let dir = folder("hebrew");
    let verses: Vec<(Verse, &str)> = (1..=2)
        .zip(GEN_1_1_2)
        .map(|(n, t)| (v(Book::Genesis, 1, n), t))
        .collect();
    write_library(
        &dir.join("library"),
        &[paragraphed_bible(
            "heb-gen",
            "HGEN",
            &verses,
            Direction::Rtl,
        )],
    );
    let mut h = Harness::launch_full(Some(&dir.join("user.db")), JobMode::Inline, Some(&dir));
    h.shell
        .st
        .session
        .library
        .add_file(&dir.join("library").join("heb-gen.obr"))
        .unwrap();
    h.shell.run(
        "tool.copy_verses",
        &json!({ "passages": "Gen 1:1-2", "bibles": "heb-gen" }),
    );
    h.frames(2);
    let first_word = "בְּרֵאשִׁ֖ית";
    let last_word = "הָאָֽרֶץ׃";
    for (format, label) in [
        (obs_engine::CopyFormat::PlainText, "Plain text"),
        (obs_engine::CopyFormat::RichText, "Rich text"),
        (obs_engine::CopyFormat::Markdown, "Markdown"),
    ] {
        click_in_window(&mut h, label);
        assert_eq!(h.shell.st.copy_ui.preview, format);
        let text = preview_text(&mut h);
        assert!(text.contains(first_word), "{text}");
        assert!(
            !h.texts().iter().any(|t| t.contains(first_word)),
            "{label}: Hebrew is not drawn by egui"
        );
        let settings = h.shell.st.session.text;
        let hebrew_size = settings.size * settings.hebrew_scale;
        let mut hebrew_lines = 0;
        for (line, laid) in &h.shell.st.copy_ui.shown {
            let Some(laid) = laid else { continue };
            if !line.text.contains(first_word) {
                assert!(!line.rtl, "{label}: {:?} reads left to right", line.text);
                continue;
            }
            assert!(line.rtl, "{label}: {:?} reads right to left", line.text);
            // In the Hebrew Reading font's size, the first word to the right of the last.
            let items: Vec<_> = laid.lines.iter().flat_map(|l| &l.items).collect();
            let at = |word: &str| {
                let start = line.text.find(word).unwrap();
                let item = (items.iter())
                    .find(|i| i.bytes.contains(&start))
                    .unwrap_or_else(|| panic!("{label}: no item for {word}"));
                assert!(item.rtl && (item.style.size - hebrew_size).abs() < 0.01);
                item.x
            };
            assert!(
                at(first_word) > at(last_word),
                "{label}: Hebrew runs right to left"
            );
            hebrew_lines += 1;
        }
        assert!(hebrew_lines > 0, "{label}: a Hebrew line was laid out");
    }
    drop(h);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn updating_or_removing_a_chosen_bible_with_the_window_open_copies_the_library_as_it_is() {
    let dir = folder("library-change");
    let mut h = launch(&dir);
    h.shell.run(
        "tool.copy_verses",
        &json!({ "passages": "Jn 3:16", "bibles": "kjv,bsb" }),
    );
    h.frames(2);
    click_in_window(&mut h, "Copy plain text");
    let before = h.copied.last().cloned().unwrap();
    assert!(
        before.contains(KJV[0]) && before.contains(BSB[0]),
        "{before}"
    );
    // An update of the KJV comes in while the window is open.
    let updated = [
        "UPDATED For God so loved the world.",
        "UPDATED For God sent not his Son.",
        "UPDATED He that believeth on him.",
    ];
    let mut kjv = bible("kjv", "KJV", &john(updated), None, &[]);
    kjv.manifest.revision = 2;
    kjv.manifest.license = "CC-BY-4.0".into();
    kjv.manifest.attribution = "KJV update, CC BY 4.0.".into();
    write_library(&dir.join("update"), &[kjv]);
    h.shell
        .st
        .session
        .library
        .add_file(&dir.join("update").join("kjv.obr"))
        .unwrap();
    h.shell
        .st
        .session
        .workspace
        .push_effect(obs_engine::Effect::Replaced("kjv".into()));
    h.frames(3);
    click_in_window(&mut h, "Copy plain text");
    let after = h.copied.last().cloned().unwrap();
    assert_eq!(
        after,
        format!(
            "16 {}\nJohn 3:16 (KJV)\n\n16 {}\nJohn 3:16 (BSB)\n\nKJV update, CC BY 4.0.",
            updated[0], BSB[0]
        )
    );
    // The BSB is removed: it is no longer chosen, and the copy is the KJV's alone.
    h.shell
        .run("library.remove", &json!({ "id": "bsb", "confirm": true }));
    h.frames(3);
    assert_eq!(h.shell.st.copy_ui.bibles, ["kjv"]);
    click_in_window(&mut h, "Copy plain text");
    assert_eq!(
        h.copied.last().cloned().unwrap(),
        format!(
            "16 {}\nJohn 3:16 (KJV)\n\nKJV update, CC BY 4.0.",
            updated[0]
        )
    );
    // With no Bible left, nothing is copied.
    h.shell
        .run("library.remove", &json!({ "id": "kjv", "confirm": true }));
    h.frames(3);
    let copies = h.copied.len();
    click_in_window(&mut h, "Copy plain text");
    assert_eq!(
        h.copied.len(),
        copies,
        "a removed Bible's cached copy is gone"
    );
    drop(h);
    let _ = std::fs::remove_dir_all(&dir);
}
