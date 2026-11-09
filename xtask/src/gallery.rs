//! `cargo xtask gallery`: the Screenshot gallery (#222, `docs/dev/screenshot-gallery.md`).
//!
//! The gallery is the `obs-ui-egui` tests in `crates/ui-egui/src/gallery_tests.rs`: every Tab
//! kind, toolbar pane, menu, window, dialog and popover drawn headlessly in Light, Dark and
//! Sepia at 1× and 2× and compared with its golden in `docs/previews/gallery/`. The `test` gate
//! runs them too; this task runs only them, and with `--bless` accepts what they draw.

use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};

const USAGE: &str = "usage: cargo xtask gallery [--bless]

  (no option)  draw every surface and compare it with its golden in docs/previews/gallery/;
               a difference fails, with golden | new | diff images in target/gallery/
  --bless      write the goldens that differ or are new, remove those of surfaces no longer
               drawn, and write docs/previews/gallery.md";

/// The environment variable the tests bless under.
const BLESS: &str = "OBS_GALLERY_BLESS";
/// The tests' module path, as a test-name filter.
const TESTS: &str = "shell::tests::gallery::";
/// Where a failing run leaves its new and diff images.
const OUT: &str = "target/gallery";

pub fn run(root: &Path, args: &[String]) -> Result<()> {
    let bless = match args {
        [] => false,
        [a] if a == "--bless" => true,
        [a] if a == "--help" || a == "-h" => {
            println!("{USAGE}");
            return Ok(());
        }
        _ => bail!("unexpected arguments {args:?}\n\n{USAGE}"),
    };
    // Images from an earlier run would read as this run's.
    for dir in ["new", "diff"] {
        let dir = root.join(OUT).join(dir);
        if dir.exists() {
            std::fs::remove_dir_all(&dir).with_context(|| format!("removing {}", dir.display()))?;
        }
    }
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let mut cmd = Command::new(&cargo);
    cmd.args([
        "test",
        "--locked",
        "-p",
        "obs-ui-egui",
        "--lib",
        "--",
        TESTS,
    ])
    .current_dir(root);
    if bless {
        cmd.env(BLESS, "1");
    } else {
        cmd.env_remove(BLESS);
    }
    let status = cmd
        .status()
        .with_context(|| format!("could not run `{cargo}`"))?;
    if !status.success() {
        bail!(
            "the Screenshot gallery differs from its goldens: see the list above and \
             {OUT}/diff/ (golden | new | diff). If the change is intended, run \
             `cargo xtask gallery --bless` and commit docs/previews/gallery/."
        );
    }
    if bless {
        eprintln!("xtask gallery: goldens and docs/previews/gallery.md are up to date");
    }
    Ok(())
}
