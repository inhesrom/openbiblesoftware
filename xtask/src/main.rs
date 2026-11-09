//! Repository automation. Run `cargo xtask help`.

mod assets;
mod atlas_basemap;
mod bench;
mod language_names;
mod layers;
mod licenses;
mod release;
mod starter;
mod strongs_index;
mod tvtms;
mod update_e2e;

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

const USAGE: &str = "usage: cargo xtask <task>

tasks:
  ci       run every gate below, in order
  fmt      cargo fmt --check
  clippy   cargo clippy -D warnings on all targets
  a11y     the accessibility audit (#182): names, keyboard and contrast of every surface
  test     cargo nextest (falls back to cargo test locally) plus doctests
  layers   enforce the crate layering rules (ADR 0005, #12)
  wasm     cargo check every non-app crate for wasm32-unknown-unknown
  assets   every file under an assets/ directory has an ATTRIBUTION.toml entry
  licenses the committed third-party licence listing matches cargo-about's (`--check`);
           without `--check`, regenerate it (`cargo xtask licenses --help`)

performance:
  bench    headless reading-pane and feature benchmark; fails past the thresholds in xtask/bench.toml
           (`--scale`: the app over a generated 2,000-Resource library, a manual gate; docs/dev/bench.md)

starter library:
  starter  build dist/starter/*.obr from starter/sources.toml (`cargo xtask starter --help`)

releases (docs/dev/releasing.md):
  release-archive  pack the app and dist/starter/ into a Release archive (`--help`)
  release-smoke    launch an unpacked Release archive, check its Starter library got in (`--help`)
  update-e2e       install this version from an archive, update it to the next from a mock
                   release source and restart into it (`--help`, #276; needs a display)

maintenance:
  tvtms <TVTMS.txt>   regenerate the embedded STEPBible TVTMS rules
  atlas-basemap       regenerate the Atlas's built-in base map from Natural Earth (`--help`)
  strongs-index       regenerate the built-in Strong's index from STEPBible TBESH/TBESG (`--help`)
  language-names      regenerate the built-in language names from CLDR and ISO 639-3 (`--help`)";

fn main() {
    if let Err(err) = run() {
        eprintln!("xtask: {err:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let task = std::env::args().nth(1).unwrap_or_else(|| "help".into());
    let root = workspace_root()?;
    match task.as_str() {
        "ci" => {
            for (name, gate) in GATES {
                eprintln!("\n==> xtask {name}");
                gate(&root).with_context(|| format!("gate `{name}` failed"))?;
            }
            eprintln!("\nxtask ci: all gates passed");
            Ok(())
        }
        "bench" => {
            let rest: Vec<String> = std::env::args().skip(2).collect();
            if rest.iter().any(|a| a == "--scale") {
                bench::run_scale(&root, &rest)
            } else {
                bench::run(&root)
            }
        }
        "starter" => {
            let rest: Vec<String> = std::env::args().skip(2).collect();
            starter::run(&root, &rest)
        }
        "release-archive" => {
            let rest: Vec<String> = std::env::args().skip(2).collect();
            release::run_archive(&root, &rest)
        }
        "release-smoke" => {
            let rest: Vec<String> = std::env::args().skip(2).collect();
            release::run_smoke(&root, &rest)
        }
        "update-e2e" => {
            let rest: Vec<String> = std::env::args().skip(2).collect();
            update_e2e::run(&root, &rest)
        }
        "licenses" => {
            let rest: Vec<String> = std::env::args().skip(2).collect();
            licenses::run(&root, &rest)
        }
        "atlas-basemap" => {
            let rest: Vec<String> = std::env::args().skip(2).collect();
            atlas_basemap::run(&root, &rest)
        }
        "strongs-index" => {
            let rest: Vec<String> = std::env::args().skip(2).collect();
            strongs_index::run(&root, &rest)
        }
        "language-names" => {
            let rest: Vec<String> = std::env::args().skip(2).collect();
            language_names::run(&root, &rest)
        }
        "tvtms" => {
            let rest: Vec<String> = std::env::args().skip(2).collect();
            tvtms::run(&root, &rest)
        }
        "help" | "-h" | "--help" => {
            println!("{USAGE}");
            Ok(())
        }
        other => match GATES.iter().find(|(name, _)| *name == other) {
            Some((_, gate)) => gate(&root),
            None => bail!("unknown task `{other}`\n\n{USAGE}"),
        },
    }
}

type Gate = fn(&Path) -> Result<()>;

const GATES: &[(&str, Gate)] = &[
    ("fmt", fmt),
    ("layers", layers::run),
    ("assets", assets::run),
    ("licenses", licenses::gate),
    ("clippy", clippy),
    ("a11y", a11y),
    ("test", test),
    ("wasm", wasm),
];

fn workspace_root() -> Result<PathBuf> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .map(Path::to_path_buf)
        .context("xtask must live one level below the workspace root")
}

fn cargo(root: &Path, args: &[&str]) -> Result<()> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let status = Command::new(&cargo)
        .args(args)
        .current_dir(root)
        .status()
        .with_context(|| format!("could not run `{cargo}`"))?;
    if !status.success() {
        bail!("`cargo {}` exited with {status}", args.join(" "));
    }
    Ok(())
}

fn fmt(root: &Path) -> Result<()> {
    cargo(root, &["fmt", "--all", "--check"])
}

fn clippy(root: &Path) -> Result<()> {
    cargo(
        root,
        &[
            "clippy",
            "--workspace",
            "--all-targets",
            "--locked",
            "--",
            "-D",
            "warnings",
        ],
    )
}

/// The accessibility audit (#182, `docs/dev/accessibility.md`): every Tab kind, toolbar pane,
/// menu and window has named controls, works from the keyboard alone, and meets WCAG AA
/// contrast in every theme × Page tone. Also part of `test`; run first, on its own, so a
/// failure names the gate.
fn a11y(root: &Path) -> Result<()> {
    cargo(
        root,
        &[
            "test",
            "--locked",
            "-p",
            "obs-ui-egui",
            "--lib",
            "--",
            "a11y_audit",
        ],
    )
}

fn test(root: &Path) -> Result<()> {
    let has_nextest = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
        .args(["nextest", "--version"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if has_nextest {
        cargo(
            root,
            &[
                "nextest",
                "run",
                "--workspace",
                "--locked",
                "--no-tests=pass",
            ],
        )?;
    } else if std::env::var_os("CI").is_some() {
        bail!("cargo-nextest is required in CI");
    } else {
        eprintln!("xtask: cargo-nextest not found, falling back to `cargo test`");
        cargo(
            root,
            &[
                "test",
                "--workspace",
                "--locked",
                "--lib",
                "--bins",
                "--tests",
            ],
        )?;
    }
    cargo(root, &["test", "--workspace", "--locked", "--doc"])
}

fn wasm(root: &Path) -> Result<()> {
    let metadata = layers::metadata(root)?;
    let crates = layers::wasm_crates(&metadata);
    let mut args = vec!["check", "--locked", "--target", "wasm32-unknown-unknown"];
    for name in &crates {
        args.push("-p");
        args.push(name);
    }
    cargo(root, &args).context(
        "wasm check failed (is the target installed? `rustup target add wasm32-unknown-unknown`)",
    )
}
