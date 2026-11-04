//! End-to-end checks of the `obs-cli` binary.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::path::PathBuf;
use std::process::Command;

fn cli(args: &[&str]) -> (bool, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_obs-cli"))
        .args(args)
        .output()
        .expect("run obs-cli");
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

fn temp_obr(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("obs-cli-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir.join(name)
}

#[test]
fn inspect_shows_manifest_units_spans_and_verses() {
    let path = temp_obr("sample.obr");
    let p = path.to_str().expect("utf-8 path");
    let (ok, _, err) = cli(&["write-sample", p]);
    assert!(ok, "{err}");

    let (ok, out, err) = cli(&["inspect", p]);
    assert!(ok, "{err}");
    assert!(out.contains("resource_id     kjv-sample"), "{out}");
    assert!(
        out.contains("3 units, 15 spans, 4 verses, 1 keys, 1 unit aliases"),
        "{out}"
    );
    assert!(out.contains("dropped markup  \\fig ×2, \\rq ×1"), "{out}");
    assert!(
        out.contains("verse map       0 unmapped, 1 absent"),
        "{out}"
    );
    assert!(out.contains("JHN"), "{out}");

    let (ok, out, _) = cli(&["inspect", p, "--unit", "JHN"]);
    assert!(ok);
    assert!(out.contains("tagged_word"), "{out}");
    assert!(out.contains("\"God\""), "{out}");

    let (ok, out, _) = cli(&["inspect", p, "--verse", "Ps 3:0-1; Jn 3:16-17"]);
    assert!(ok);
    assert!(
        out.contains("Ps 3:title [PSA 0..52]: A Psalm of David"),
        "{out}"
    );
    assert!(out.contains("John 3:16 [JHN"), "{out}");

    let (ok, _, err) = cli(&["inspect", p, "--unit", "JOH"]);
    assert!(!ok);
    assert!(err.contains("renamed to `JHN`"), "{err}");

    let (ok, _, err) = cli(&["inspect", "/no/such/file.obr"]);
    assert!(!ok);
    assert!(err.contains("does not exist"), "{err}");
    let _ = std::fs::remove_file(&path);
}

#[test]
fn run_dispatches_engine_commands() {
    let (ok, out, err) = cli(&["run", "reference.parse", "text=jn 3:16-18; rom 5:8"]);
    assert!(ok, "{err}");
    assert!(out.contains("\"text\": \"John 3:16-18; Rom 5:8\""), "{out}");
    let (ok, _, err) = cli(&["run", "reference.parse", "text=Jn 3:99"]);
    assert!(!ok);
    assert!(err.contains("does not exist"), "{err}");
}

#[test]
fn import_usfm_then_inspect() {
    let fixtures = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../crates/formats/tests/assets/usfm/kjv2006"
    );
    let out = temp_obr("kjv-excerpt.obr");
    let o = out.to_str().expect("utf-8 path");
    let (ok, stdout, err) = cli(&[
        "import",
        fixtures,
        "-o",
        o,
        "--id",
        "kjv-excerpt",
        "--abbrev",
        "KJV",
        "--lang",
        "en",
    ]);
    assert!(ok, "{err}");
    assert!(stdout.contains("3 Bible book(s)"), "{stdout}");
    assert!(stdout.contains("licence: none found"), "{stdout}");
    let (ok, stdout, _) = cli(&["inspect", o, "--verse", "Gen 1:1"]);
    assert!(ok);
    assert!(
        stdout.contains("In the beginning God created the heaven and the earth."),
        "{stdout}"
    );
    let (ok, _, err) = cli(&["import", fixtures]);
    assert!(!ok);
    assert!(err.contains("usage"), "{err}");
    let _ = std::fs::remove_file(&out);
}

/// SWORD Import end to end: a public-domain Bible and a commentary, with the conf's licence
/// fields shown by `inspect` (the manifest fields the About Tab renders).
#[test]
fn import_sword_modules_then_inspect_licence_fields() {
    let library = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../crates/formats/tests/assets/sword"
    );
    let bible = temp_obr("sword-bible.obr");
    let b = bible.to_str().expect("utf-8 path");
    let (ok, stdout, err) = cli(&["import", library, "--module", "ObsKjvOsis", "-o", b]);
    assert!(ok, "{err}");
    assert!(stdout.contains("format sword (OSIS)"), "{stdout}");
    assert!(stdout.contains("5 Bible book(s), 12 verse(s)"), "{stdout}");
    assert!(stdout.contains("conf versification: KJV"), "{stdout}");
    assert!(stdout.contains("dropped markup (3):"), "{stdout}");
    assert!(!stdout.contains("licence: none found"), "{stdout}");
    let (ok, stdout, _) = cli(&["inspect", b]);
    assert!(ok);
    assert!(stdout.contains("license         LicenseRef-PD"), "{stdout}");
    assert!(
        stdout.contains("attribution     Licence: Public Domain\n                Text source: King James Version (1769)"),
        "{stdout}"
    );
    let (ok, stdout, _) = cli(&["inspect", b, "--verse", "Ps 3:0"]);
    assert!(ok);
    assert!(
        stdout.contains("A Psalm of David, when he fled"),
        "{stdout}"
    );

    let notes = temp_obr("sword-notes.obr");
    let n = notes.to_str().expect("utf-8 path");
    let conf = format!("{library}/mods.d/obsnotes.conf");
    let (ok, stdout, err) = cli(&["import", &conf, "-o", n]);
    assert!(ok, "{err}");
    assert!(
        stdout.contains("6 entr(y/ies), 2 Scripture reference(s)"),
        "{stdout}"
    );
    let (ok, stdout, _) = cli(&["inspect", n]);
    assert!(ok);
    assert!(stdout.contains("type            Commentary"), "{stdout}");
    assert!(stdout.contains("license         CC0-1.0"), "{stdout}");
    assert!(
        stdout.contains("source url      https://github.com/inhesrom/openbiblesoftware"),
        "{stdout}"
    );
    assert!(
        stdout.contains("Copyright holder: Open Bible Software contributors"),
        "{stdout}"
    );

    // Several modules and none chosen: the error lists them.
    let (ok, _, err) = cli(&["import", library, "-o", n]);
    assert!(!ok);
    assert!(
        err.contains("--module") && err.contains("ObsNotes (Commentary)"),
        "{err}"
    );
    let _ = std::fs::remove_file(&bible);
    let _ = std::fs::remove_file(&notes);
}

#[test]
fn shape_reports_glyphs_and_writes_svg() {
    let svg = temp_obr("shape.svg");
    let s = svg.to_str().expect("utf-8 path");
    let (ok, out, err) = cli(&["shape", "--svg", s, "In the beginning", "בְּרֵאשִׁ֖ית"]);
    assert!(ok, "{err}");
    assert!(
        out.contains("face Regular") && out.contains("face Hebrew"),
        "{out}"
    );
    assert!(out.contains("0 missing"), "{out}");
    assert!(
        std::fs::read_to_string(&svg)
            .expect("svg")
            .starts_with("<svg")
    );
    let _ = std::fs::remove_file(&svg);
}

#[test]
fn layout_reports_blocks_and_lines() {
    let path = temp_obr("layout-sample.obr");
    let p = path.to_str().expect("utf-8 path");
    assert!(cli(&["write-sample", p]).0);
    let (ok, out, err) = cli(&[
        "layout", p, "--unit", "JHN", "--width", "200", "--runs", "1",
    ]);
    assert!(ok, "{err}");
    assert!(
        out.contains("run 1: 1 blocks,") && out.contains("lines"),
        "{out}"
    );
    let (ok, _, err) = cli(&["layout", p, "--unit", "NOPE"]);
    assert!(!ok);
    assert!(err.contains("no unit"), "{err}");
    let _ = std::fs::remove_file(&path);
}
