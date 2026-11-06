//! Repository automation. Run `cargo xtask help`.

mod assets;
mod bench;
mod layers;
mod licenses;
mod release;
mod starter;
mod tvtms;

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

const USAGE: &str = "usage: cargo xtask <task>

tasks:
  ci       run every gate below, in order
  fmt      cargo fmt --check
  clippy   cargo clippy -D warnings on all targets
  test     cargo nextest (falls back to cargo test locally) plus doctests
  layers   enforce the crate layering rules (ADR 0005, #12)
  wasm     cargo check every non-app crate for wasm32-unknown-unknown
  assets   every file under an assets/ directory has an ATTRIBUTION.toml entry
  licenses the committed third-party licence listing matches cargo-about's (`--check`);
           without `--check`, regenerate it (`cargo xtask licenses --help`)

performance:
  bench    headless reading-pane benchmark; fails past the thresholds in xtask/bench.toml

starter library:
  starter  build dist/starter/*.obr from starter/sources.toml (`cargo xtask starter --help`)

releases (docs/dev/releasing.md):
  release-archive  pack the app and dist/starter/ into a Release archive (`--help`)
  release-smoke    launch an unpacked Release archive, check its Starter library got in (`--help`)

maintenance:
  tvtms <TVTMS.txt>   regenerate the embedded STEPBible TVTMS rules";

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
        "bench" => bench::run(&root),
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
        "licenses" => {
            let rest: Vec<String> = std::env::args().skip(2).collect();
            licenses::run(&root, &rest)
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
