//! The headless test driver for GPUI surfaces (#379): fixtures, a Session over them, and the
//! golden comparison of ADR 0019 (amended by #374). Shared by `tests/interaction.rs` and
//! `tests/rendering.rs`.
//!
//! The fixtures are the egui tests': the accessibility audit's Starter subset (the released
//! Starter library's Resources) plus the BSB's Genesis 1, a User database in a scratch folder,
//! and the clock fixed at [`NOW_MS`] (`obs_store::clock`, per thread).

#![allow(dead_code)] // Each test binary uses part of it.

use std::path::{Path, PathBuf};

use obs_engine::{JobMode, Library, Session};
use obs_scripture::{Book, Passage, Verse};

/// The gallery's fixed time (the egui gallery's `NOW_MS`).
pub const NOW_MS: i64 = 1_794_220_200_000;

/// ADR 0019: a pixel matches when no channel is more than 2 levels off, and an image matches
/// when no more than 0.01 % of its pixels differ beyond that.
pub const CHANNEL_TOLERANCE: u8 = 2;
pub const PIXEL_SHARE: f64 = 0.0001;

/// A scratch folder for one test's Library and User database, removed when dropped.
pub struct Lab {
    pub root: PathBuf,
}

impl Drop for Lab {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// The BSB's Genesis 1 (the egui gallery's `bsb_genesis_1`).
pub fn bsb_genesis_1() -> obs_model::Resource {
    let usfm = "\\id GEN\n\\c 1\n\\s1 The Creation\n\\p\n\\v 1 In the beginning God created \
                the heavens and the earth.\\x - \\xo 1:1 \\xt Genesis 1:3\\x*\n\\v 2 Now the \
                earth was formless and void, and darkness was over the surface of the deep. \
                And the Spirit of God was hovering over the surface of the waters.\n\\v 3 And \
                God said, “Let there be light,”\\f + \\fr 1:3 \\ft Cited in 2 Corinthians \
                4:6\\f* and there was light.\n";
    let mut import = obs_formats::UsfmImport::new(obs_formats::UsfmOptions {
        resource_id: "bsb".into(),
        title: "Berean Standard Bible (test)".into(),
        abbreviation: "BSB".into(),
        language: "en".into(),
        source_file: "GEN.usfm".into(),
        tool_version: "test".into(),
        ..Default::default()
    });
    import.add_book("GEN.usfm", usfm.into()).unwrap();
    let mut bsb = import.finish().unwrap().0;
    bsb.manifest.license = "LicenseRef-PublicDomain".into();
    bsb
}

fn cross_references() -> obs_model::Resource {
    let options = obs_formats::DatasetOptions {
        resource_id: "openbible-xrefs".into(),
        title: "OpenBible.info Cross-references".into(),
        ..Default::default()
    };
    obs_formats::import_openbible_cross_references(
        include_str!("../../../formats/tests/assets/openbible/cross-references-excerpt.txt"),
        &options,
    )
    .unwrap()
    .0
}

/// The accessibility audit's Starter subset (the egui tests' `release_set`): the KJV with
/// Strong's and the Lexicons, a Commentary, the Cross-reference set, TIPNR and Greek Bibles.
pub fn starter_subset() -> Vec<obs_model::Resource> {
    let john = Passage::verse(Verse::new(Book::John, 3, 16));
    let mut set = obs_engine::testing::starter_set();
    set.extend([
        obs_engine::testing::commentary(
            "mhc",
            "Matthew Henry (test)",
            "MHC",
            &[(john, None, "God so loved the world.")],
        ),
        cross_references(),
        obs_engine::testing::bsb_parallels(),
        obs_engine::testing::tipnr(),
        obs_engine::testing::tagnt_grace(),
        obs_engine::testing::tagnt_love(),
    ]);
    for r in &mut set {
        r.manifest.origin = obs_model::Origin::Starter;
        if r.manifest.license.is_empty() {
            r.manifest.license = "LicenseRef-PublicDomain".into();
        }
    }
    set
}

/// A Session over the fixtures, as the egui tests launch one: the Starter subset and the BSB's
/// Genesis 1 in a Library in a scratch folder, a User database there, jobs run inline, and
/// this thread's clock fixed at [`NOW_MS`].
pub fn session(name: &str) -> (Session, Lab) {
    obs_store::clock::fix(Some(NOW_MS));
    let root = std::env::temp_dir().join(format!(
        "obs-ui-gpui-{name}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("library")).unwrap();
    let mut session = Session::new(JobMode::Inline);
    let (store, _) = obs_store::UserStore::open(&root.join("user.db")).unwrap();
    session.attach_user(Box::new(store));
    session.set_search_cache_dir(root.join("cache"));
    session.library = Library::new();
    session.library.set_directory(root.join("library"));
    let mut set = starter_subset();
    set.push(bsb_genesis_1());
    for r in &set {
        let path = root
            .join("library")
            .join(format!("{}.obr", r.manifest.resource_id));
        obs_store::write_obr(r, &path).unwrap();
        session.library.add_file(&path).unwrap();
    }
    (session, Lab { root })
}

/// Add `r` to `session`'s Library, written into the Lab's library folder (#380's reading
/// fixtures).
pub fn add_resource(session: &mut Session, lab: &Lab, r: &obs_model::Resource) {
    let path = lab
        .root
        .join("library")
        .join(format!("{}.obr", r.manifest.resource_id));
    obs_store::write_obr(r, &path).unwrap();
    session.library.add_file(&path).unwrap();
}

/// Genesis 1:1-5 in Hebrew with points and cantillation, as TAHOT prints it: one right-to-left
/// paragraph, so its lines wrap right to left (#380).
pub fn hebrew_genesis_1() -> obs_model::Resource {
    use obs_engine::testing::bibles::{paragraphed_bible, v};
    let verses = [
        (
            v(Book::Genesis, 1, 1),
            "בְּרֵאשִׁ֖ית בָּרָ֣א אֱלֹהִ֑ים אֵ֥ת הַשָּׁמַ֖יִם וְאֵ֥ת הָאָֽרֶץ׃",
        ),
        (
            v(Book::Genesis, 1, 2),
            "וְהָאָ֗רֶץ הָיְתָ֥ה תֹ֙הוּ֙ וָבֹ֔הוּ וְחֹ֖שֶׁךְ עַל־פְּנֵ֣י תְה֑וֹם וְר֣וּחַ אֱלֹהִ֔ים מְרַחֶ֖פֶת עַל־פְּנֵ֥י הַמָּֽיִם׃",
        ),
        (v(Book::Genesis, 1, 3), "וַיֹּ֥אמֶר אֱלֹהִ֖ים יְהִ֣י א֑וֹר וַֽיְהִי־אֽוֹר׃"),
        (
            v(Book::Genesis, 1, 4),
            "וַיַּ֧רְא אֱלֹהִ֛ים אֶת־הָא֖וֹר כִּי־ט֑וֹב וַיַּבְדֵּ֣ל אֱלֹהִ֔ים בֵּ֥ין הָא֖וֹר וּבֵ֥ין הַחֹֽשֶׁךְ׃",
        ),
        (
            v(Book::Genesis, 1, 5),
            "וַיִּקְרָ֨א אֱלֹהִ֤ים ׀ לָאוֹר֙ י֔וֹם וְלַחֹ֖שֶׁךְ קָ֣רָא לָ֑יְלָה וַֽיְהִי־עֶ֥רֶב וַֽיְהִי־בֹ֖קֶר י֥וֹם אֶחָֽד׃",
        ),
    ];
    let mut r = paragraphed_bible("tahot-test", "TAHOT", &verses, obs_model::Direction::Rtl);
    r.manifest.language = "hbo".into();
    r
}

/// John 1:1-5 in Greek: one left-to-right paragraph whose lines break at spaces (#380).
pub fn greek_john_1() -> obs_model::Resource {
    use obs_engine::testing::bibles::{paragraphed_bible, v};
    let verses = [
        (
            v(Book::John, 1, 1),
            "Ἐν ἀρχῇ ἦν ὁ λόγος, καὶ ὁ λόγος ἦν πρὸς τὸν θεόν, καὶ θεὸς ἦν ὁ λόγος.",
        ),
        (v(Book::John, 1, 2), "οὗτος ἦν ἐν ἀρχῇ πρὸς τὸν θεόν."),
        (
            v(Book::John, 1, 3),
            "πάντα δι’ αὐτοῦ ἐγένετο, καὶ χωρὶς αὐτοῦ ἐγένετο οὐδὲ ἕν ὃ γέγονεν.",
        ),
        (
            v(Book::John, 1, 4),
            "ἐν αὐτῷ ζωὴ ἦν, καὶ ἡ ζωὴ ἦν τὸ φῶς τῶν ἀνθρώπων·",
        ),
        (
            v(Book::John, 1, 5),
            "καὶ τὸ φῶς ἐν τῇ σκοτίᾳ φαίνει, καὶ ἡ σκοτία αὐτὸ οὐ κατέλαβεν.",
        ),
    ];
    let mut r = paragraphed_bible("tagnt-test", "TAGNT", &verses, obs_model::Direction::Ltr);
    r.manifest.language = "grc".into();
    r
}

/// Psalm 119 as one long paragraph (176 verses, about 30 KB): a jump deep into it lands in a
/// block far too long to lay out within one frame's budget (#380).
pub fn long_psalm() -> obs_model::Resource {
    use obs_engine::testing::bibles::{paragraphed_bible, v};
    const TEXT: &str = "Blessed are those whose ways are blameless, who walk according to                         the law of the LORD; blessed are those who keep his statutes and seek                         him with all their heart.";
    let verses: Vec<(Verse, &str)> = (1..=176).map(|n| (v(Book::Psalms, 119, n), TEXT)).collect();
    paragraphed_bible("long-psalm", "LP", &verses, obs_model::Direction::Ltr)
}

/// How a frame compares with another under ADR 0019's tolerance.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Comparison {
    /// Pixels with a channel more than [`CHANNEL_TOLERANCE`] levels off.
    pub off: usize,
    pub total: usize,
    /// The largest channel difference.
    pub max: u8,
    /// Whether the sizes are the same and `off` is within [`PIXEL_SHARE`] of `total`.
    pub matches: bool,
}

pub fn compare(a: &image::RgbaImage, b: &image::RgbaImage) -> Comparison {
    let total = (a.width() as usize) * (a.height() as usize);
    if a.dimensions() != b.dimensions() {
        return Comparison {
            off: total,
            total,
            max: u8::MAX,
            matches: false,
        };
    }
    let mut off = 0;
    let mut max = 0u8;
    for (p, q) in a.pixels().zip(b.pixels()) {
        let d = (0..4).map(|i| p[i].abs_diff(q[i])).max().unwrap_or(0);
        max = max.max(d);
        if d > CHANNEL_TOLERANCE {
            off += 1;
        }
    }
    Comparison {
        off,
        total,
        max,
        matches: (off as f64) <= PIXEL_SHARE * total as f64,
    }
}

/// `goldens/<os>/` in this crate.
pub fn golden_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("goldens")
        .join(std::env::consts::OS)
}

/// Where frames are written for review: `ui-gpui-frames/` in `CARGO_TARGET_DIR` when it is set
/// (a relative one from the workspace), else in the workspace's `target/`.
pub fn frame_dir() -> PathBuf {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let target = match std::env::var_os("CARGO_TARGET_DIR") {
        Some(dir) if !dir.is_empty() => workspace.join(dir),
        _ => workspace.join("target"),
    };
    let dir = target.join("ui-gpui-frames");
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// `UPDATE_GOLDEN=1` rewrites goldens that are missing or off (as the egui goldens' switch).
pub fn bless() -> bool {
    std::env::var_os("UPDATE_GOLDEN").is_some()
}

/// Whether a frame is drawn at all: more than one colour in it.
pub fn not_blank(image: &image::RgbaImage) -> bool {
    let mut pixels = image.pixels();
    match pixels.next() {
        Some(first) => pixels.any(|p| p != first),
        None => false,
    }
}

// ---------------------------------------------------------------------------------------
// Driving a window: what the egui harness and `OBS_STEPS` do, on GPUI. Elements are found by
// the accessibility snapshots `gpui-base` records under its `test-support` feature (role,
// label, value, focus, bounds: what GPUI writes into each AccessKit node). GPUI Kit wraps the
// same snapshots in its own test API, but its `test-support` feature names its optional
// styled and GPUI Fast layers. GPUI Fast is in Cargo.lock already (an optional dependency of
// `gpui-base`) though in no build; the feature would add the styled layer's and GPUI Fast's
// platform crates and an HTTP client, and fail the layering check. So the driver uses
// `gpui-base` and GPUI directly.

use gpui_base::test_support::{self as observation, ElementSnapshot};
use gpui_kit::{
    App, InputEvent as _, KeyDownEvent, KeyUpEvent, Keystroke, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, Pixels, Point, Role, Window,
};

/// What a test does with a window: draw a frame, find elements by accessible label or role,
/// and send pointer and keyboard input. Each input step completes a frame after it.
pub trait Driver {
    /// Lay out and paint a frame, so the snapshots are current.
    fn frame(&mut self, cx: &mut App);
    /// Every element the last frame exposed, top to bottom, then left to right.
    fn elements(&self) -> Vec<ElementSnapshot>;
    /// The one element whose accessible name is `label`; panics if there is none or more.
    fn find_by_label(&self, label: &str) -> ElementSnapshot;
    fn find_all_by_role(&self, role: Role) -> Vec<ElementSnapshot>;
    fn pointer_move(&mut self, at: Point<Pixels>, cx: &mut App);
    fn pointer_down(&mut self, at: Point<Pixels>, cx: &mut App);
    fn pointer_up(&mut self, at: Point<Pixels>, cx: &mut App);
    /// Move to the centre of the element labelled `label` and click it there.
    fn click_label(&mut self, label: &str, cx: &mut App);
    /// A key down and up, as `"enter"`, `"tab"` or `"ctrl-f"`.
    fn press(&mut self, key: &str, cx: &mut App);
    /// Type `text`, one character at a time, into whatever has the keyboard.
    fn type_text(&mut self, text: &str, cx: &mut App);
}

impl Driver for Window {
    fn frame(&mut self, cx: &mut App) {
        self.refresh();
        self.draw(cx).clear(cx);
    }

    fn elements(&self) -> Vec<ElementSnapshot> {
        let mut elements = observation::snapshots(self);
        elements.sort_by_cached_key(|e| {
            (
                e.bounds().origin.y,
                e.bounds().origin.x,
                format!("{:?}", e.path()),
            )
        });
        elements
    }

    fn find_by_label(&self, label: &str) -> ElementSnapshot {
        let mut found: Vec<ElementSnapshot> = self
            .elements()
            .into_iter()
            .filter(|e| e.label() == Some(label))
            .collect();
        assert!(
            found.len() == 1,
            "{} elements labelled {label:?}; the frame exposes: {:?}",
            found.len(),
            self.elements()
        );
        found.remove(0)
    }

    fn find_all_by_role(&self, role: Role) -> Vec<ElementSnapshot> {
        self.elements()
            .into_iter()
            .filter(|e| e.role() == Some(role))
            .collect()
    }

    fn pointer_move(&mut self, at: Point<Pixels>, cx: &mut App) {
        let modifiers = self.modifiers();
        self.dispatch_event(
            MouseMoveEvent {
                position: at,
                pressed_button: None,
                modifiers,
            }
            .to_platform_input(),
            cx,
        );
        self.frame(cx);
    }

    fn pointer_down(&mut self, at: Point<Pixels>, cx: &mut App) {
        let modifiers = self.modifiers();
        self.dispatch_event(
            MouseDownEvent {
                button: MouseButton::Left,
                position: at,
                modifiers,
                click_count: 1,
                first_mouse: false,
            }
            .to_platform_input(),
            cx,
        );
        self.frame(cx);
    }

    fn pointer_up(&mut self, at: Point<Pixels>, cx: &mut App) {
        let modifiers = self.modifiers();
        self.dispatch_event(
            MouseUpEvent {
                button: MouseButton::Left,
                position: at,
                modifiers,
                click_count: 1,
            }
            .to_platform_input(),
            cx,
        );
        self.frame(cx);
    }

    fn click_label(&mut self, label: &str, cx: &mut App) {
        self.frame(cx);
        let target = self.find_by_label(label);
        assert!(target.visible(), "{label:?} is not visible");
        let at = target.bounds().center();
        self.pointer_move(at, cx);
        self.pointer_down(at, cx);
        self.pointer_up(at, cx);
    }

    fn press(&mut self, key: &str, cx: &mut App) {
        let keystroke = Keystroke::parse_with_os(key, cx.platform_info().os)
            .unwrap_or_else(|e| panic!("{key:?}: {e}"));
        self.frame(cx);
        self.dispatch_event(
            KeyDownEvent {
                keystroke: keystroke.clone(),
                is_held: false,
                prefer_character_input: false,
            }
            .to_platform_input(),
            cx,
        );
        // Buttons act on key up, so the pair is always completed.
        self.dispatch_event(KeyUpEvent { keystroke }.to_platform_input(), cx);
        self.frame(cx);
    }

    fn type_text(&mut self, text: &str, cx: &mut App) {
        self.frame(cx);
        for c in text.chars() {
            let s = c.to_string();
            let mut keystroke = Keystroke::parse_with_os(&s, cx.platform_info().os)
                .unwrap_or_else(|e| panic!("{s:?}: {e}"));
            keystroke.key_char = Some(s);
            self.dispatch_keystroke(keystroke, cx);
            self.frame(cx);
        }
    }
}
