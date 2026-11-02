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
    assert!(out.contains("JHN"), "{out}");

    let (ok, out, _) = cli(&["inspect", p, "--unit", "JHN"]);
    assert!(ok);
    assert!(out.contains("tagged_word"), "{out}");
    assert!(out.contains("\"God\""), "{out}");

    let (ok, out, _) = cli(&["inspect", p, "--verse", "Ps 3:0-1; Jn 3:16-18"]);
    assert!(ok);
    assert!(
        out.contains("Ps 3:title [PSA 0..52]: A Psalm of David"),
        "{out}"
    );
    assert!(out.contains("John 3:16 [JHN"), "{out}");
    assert!(out.contains("John 3:18: not in this Bible"), "{out}");

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
