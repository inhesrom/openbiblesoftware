//! `cargo xtask licenses`: the third-party crate licence listing shown in Help → Credits (#67,
//! #20 decision 4).
//!
//! cargo-about (pinned to [`CARGO_ABOUT_VERSION`]) reads the desktop app's dependency graph
//! for the targets in `about.toml` and finds each crate's licence texts. This task keeps what
//! the Credits view needs (crate, version, declared licence, repository; each distinct licence
//! text and the crates under it), sorts it, and writes [`OUTPUT`], which the engine embeds.
//! It writes a second listing, [`IOS_OUTPUT`], over the Mobile app's FFI crate for the iOS
//! targets, for the iOS app's Credits (`docs/dev/mobile-core.md`).
//!
//! The `licenses` gate regenerates both listings and fails when a committed file differs, so
//! neither can go stale when a dependency changes. cargo-about runs with `--frozen` after
//! `cargo fetch`: it reads only the licence files packaged in each crate and never the network,
//! so every host produces the same bytes.

use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

/// The committed listing, embedded by `obs-engine`.
pub const OUTPUT: &str = "crates/engine/data/third-party-licenses.json";
/// The committed listing of the iOS app's crates.
pub const IOS_OUTPUT: &str = "crates/engine/data/third-party-licenses-ios.json";
const CONFIG: &str = "about.toml";
/// The app whose dependency graph is listed (`obs-cli`'s is a subset of it).
const APP_MANIFEST: &str = "apps/openbible/Cargo.toml";
/// The crate the iOS app links, and the targets it is built for.
const IOS_MANIFEST: &str = "apps/obs-ffi/Cargo.toml";
const IOS_TARGETS: &[&str] = &["aarch64-apple-ios", "aarch64-apple-ios-sim"];

/// One committed listing: which graph, for which targets, into which file.
#[derive(Debug, Clone, Copy)]
struct Listed {
    output: &'static str,
    manifest: &'static str,
    /// Empty: the targets in `about.toml`.
    targets: &'static [&'static str],
}

const LISTINGS: &[Listed] = &[
    Listed {
        output: OUTPUT,
        manifest: APP_MANIFEST,
        targets: &[],
    },
    Listed {
        output: IOS_OUTPUT,
        manifest: IOS_MANIFEST,
        targets: IOS_TARGETS,
    },
];
/// Output can change between cargo-about releases, so the gate insists on this one.
pub const CARGO_ABOUT_VERSION: &str = "0.9.3";

const USAGE: &str = "usage: cargo xtask licenses [--check]

  (no option)  regenerate crates/engine/data/third-party-licenses.json (desktop) and
               third-party-licenses-ios.json (apps/obs-ffi for iOS) with cargo-about
  --check      fail if a committed listing differs from a fresh one (the CI gate)

needs cargo-about 0.9.3: cargo install cargo-about --locked --features cli --version 0.9.3";

/// cargo-about's JSON output (the parts we read).
#[derive(Debug, Deserialize)]
struct About {
    licenses: Vec<AboutLicense>,
    crates: Vec<AboutCrate>,
}

#[derive(Debug, Deserialize)]
struct AboutLicense {
    id: String,
    name: String,
    text: String,
    used_by: Vec<AboutUse>,
}

#[derive(Debug, Deserialize)]
struct AboutUse {
    #[serde(rename = "crate")]
    krate: AboutPackage,
}

#[derive(Debug, Deserialize)]
struct AboutCrate {
    package: AboutPackage,
    license: String,
}

#[derive(Debug, Deserialize)]
struct AboutPackage {
    name: String,
    version: String,
    #[serde(default)]
    repository: Option<String>,
}

/// The committed listing. `obs-engine` reads the same shape (`credits.rs`).
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Listing {
    /// How the file was made.
    pub generator: String,
    pub crates: Vec<CrateRow>,
    pub licenses: Vec<LicenseText>,
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct CrateRow {
    pub name: String,
    pub version: String,
    /// The SPDX expression the crate declares.
    pub license: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository: Option<String>,
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct LicenseText {
    /// SPDX id of the licence the text was identified as.
    pub id: String,
    pub name: String,
    /// `name version` of every crate this text covers, sorted.
    pub used_by: Vec<String>,
    pub text: String,
}

pub fn run(root: &Path, args: &[String]) -> Result<()> {
    match args.first().map(String::as_str) {
        None => {
            prepare(root)?;
            for listed in LISTINGS {
                let json = generate(root, listed)?;
                std::fs::write(root.join(listed.output), json)
                    .with_context(|| format!("cannot write {}", listed.output))?;
                eprintln!("licenses: wrote {}", listed.output);
            }
            Ok(())
        }
        Some("--check") => check(root, true),
        Some("-h" | "--help") => {
            println!("{USAGE}");
            Ok(())
        }
        Some(other) => bail!("unknown option `{other}`\n\n{USAGE}"),
    }
}

/// The CI gate. Without cargo-about it fails in CI and is skipped locally (like nextest).
pub fn gate(root: &Path) -> Result<()> {
    check(root, std::env::var_os("CI").is_some())
}

fn check(root: &Path, required: bool) -> Result<()> {
    if !required && installed_version().is_none() {
        eprintln!(
            "xtask: cargo-about not found, skipping the licence listing check \
             (install: cargo install cargo-about --locked --features cli --version \
             {CARGO_ABOUT_VERSION})"
        );
        return Ok(());
    }
    prepare(root)?;
    for listed in LISTINGS {
        check_one(root, listed)?;
    }
    Ok(())
}

fn check_one(root: &Path, listed: &Listed) -> Result<()> {
    let output = listed.output;
    let fresh = generate(root, listed)?;
    let committed = std::fs::read_to_string(root.join(output))
        .with_context(|| format!("cannot read {output}; run `cargo xtask licenses`"))?
        .replace("\r\n", "\n");
    if committed != fresh {
        let summary = match (
            serde_json::from_str::<Listing>(&committed),
            serde_json::from_str::<Listing>(&fresh),
        ) {
            (Ok(old), Ok(new)) => describe_change(&old, &new),
            _ => String::new(),
        };
        bail!(
            "{output} is stale (the dependency graph or a licence text changed){summary}\n\
             run `cargo xtask licenses` and commit the result"
        );
    }
    let listing: Listing = serde_json::from_str(&fresh)?;
    eprintln!(
        "licenses: {output}: {} crate(s), {} licence text(s), up to date",
        listing.crates.len(),
        listing.licenses.len()
    );
    Ok(())
}

fn installed_version() -> Option<String> {
    let out = Command::new(cargo_bin())
        .args(["about", "--version"])
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn cargo_bin() -> String {
    std::env::var("CARGO").unwrap_or_else(|_| "cargo".into())
}

/// Check the cargo-about version and put every target's crates on disk: cargo-about runs
/// offline.
fn prepare(root: &Path) -> Result<()> {
    let version =
        installed_version().with_context(|| format!("cargo-about is not installed\n\n{USAGE}"))?;
    if version != format!("cargo-about {CARGO_ABOUT_VERSION}") {
        bail!(
            "found {version}; the listing is made with cargo-about {CARGO_ABOUT_VERSION}\n\n{USAGE}"
        );
    }
    crate::cargo(root, &["fetch", "--locked"])
}

/// Run cargo-about and return the listing as committed (pretty JSON, LF, trailing newline).
fn generate(root: &Path, listed: &Listed) -> Result<String> {
    // To a file, not stdout: cargo-about refuses to have its output redirected under
    // PowerShell (the Windows CI shell), for fear of re-encoding.
    let json_path = root.join("target/xtask-licenses/cargo-about.json");
    if let Some(dir) = json_path.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("cannot create {}", dir.display()))?;
    }
    let mut cmd = Command::new(cargo_bin());
    cmd.args([
        "about",
        "-L",
        "error",
        "generate",
        "--frozen",
        "--format",
        "json",
        "-c",
        CONFIG,
        "-m",
        listed.manifest,
    ]);
    for target in listed.targets {
        cmd.args(["--target", target]);
    }
    let out = cmd
        .arg("-o")
        .arg(&json_path)
        .current_dir(root)
        .output()
        .context("could not run `cargo about`")?;
    if !out.status.success() {
        bail!(
            "`cargo about generate` exited with {}:\n{}",
            out.status,
            String::from_utf8_lossy(&out.stderr)
        );
    }
    let bytes = std::fs::read(&json_path)
        .with_context(|| format!("cargo-about wrote no {}", json_path.display()))?;
    let about: About =
        serde_json::from_slice(&bytes).context("cargo-about wrote unexpected JSON")?;
    let listing = listing(about, listed);
    let mut json = serde_json::to_string_pretty(&listing)?;
    json.push('\n');
    Ok(json)
}

fn listing(about: About, listed: &Listed) -> Listing {
    let mut crates: Vec<CrateRow> = about
        .crates
        .into_iter()
        .map(|c| CrateRow {
            name: c.package.name,
            version: c.package.version,
            license: c.license,
            repository: c.package.repository,
        })
        .collect();
    crates.sort();
    crates.dedup();
    // One entry per distinct (licence, text): cargo-about already groups them; merge again
    // after normalizing line endings and edge whitespace, which it does not.
    let mut texts: BTreeMap<(String, String), (String, Vec<String>)> = BTreeMap::new();
    for l in about.licenses {
        let text = normalize(&l.text);
        let entry = texts
            .entry((l.id, text))
            .or_insert_with(|| (l.name, Vec::new()));
        entry.1.extend(
            l.used_by
                .into_iter()
                .map(|u| format!("{} {}", u.krate.name, u.krate.version)),
        );
    }
    let mut licenses: Vec<LicenseText> = texts
        .into_iter()
        .map(|((id, text), (name, mut used_by))| {
            used_by.sort();
            used_by.dedup();
            LicenseText {
                id,
                name,
                used_by,
                text,
            }
        })
        .collect();
    // By licence, then by the first crate using the text: stable and easy to browse.
    licenses.sort_by(|a, b| (&a.id, &a.used_by, &a.text).cmp(&(&b.id, &b.used_by, &b.text)));
    let targets = if listed.targets.is_empty() {
        String::new()
    } else {
        format!(" for {}", listed.targets.join(", "))
    };
    Listing {
        generator: format!(
            "cargo-about {CARGO_ABOUT_VERSION} with {CONFIG} over {}{targets}; \
             regenerate with `cargo xtask licenses`",
            listed.manifest
        ),
        crates,
        licenses,
    }
}

/// LF line endings, no trailing spaces, no leading or trailing blank lines.
fn normalize(text: &str) -> String {
    let lines: Vec<&str> = text
        .lines()
        .map(|l| l.trim_end_matches(['\r', ' ', '\t']))
        .collect();
    let start = lines.iter().position(|l| !l.is_empty()).unwrap_or(0);
    let end = lines
        .iter()
        .rposition(|l| !l.is_empty())
        .map_or(0, |i| i + 1);
    lines[start..end.max(start)].join("\n")
}

/// Which crates came, went or changed licence, for the stale-file message.
fn describe_change(old: &Listing, new: &Listing) -> String {
    let key = |c: &CrateRow| (format!("{} {}", c.name, c.version), c.license.clone());
    let old: BTreeMap<String, String> = old.crates.iter().map(key).collect();
    let new: BTreeMap<String, String> = new.crates.iter().map(key).collect();
    let mut lines = Vec::new();
    for (k, l) in &new {
        match old.get(k) {
            None => lines.push(format!("  + {k} ({l})")),
            Some(o) if o != l => lines.push(format!("  ~ {k}: {o} -> {l}")),
            _ => {}
        }
    }
    for k in old.keys().filter(|k| !new.contains_key(*k)) {
        lines.push(format!("  - {k}"));
    }
    const SHOWN: usize = 30;
    let more = lines.len().saturating_sub(SHOWN);
    lines.truncate(SHOWN);
    if more > 0 {
        lines.push(format!("  … and {more} more"));
    }
    if lines.is_empty() {
        "; only licence texts differ".to_string()
    } else {
        format!(":\n{}", lines.join("\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn package(name: &str, version: &str) -> AboutPackage {
        AboutPackage {
            name: name.into(),
            version: version.into(),
            repository: None,
        }
    }

    #[test]
    fn the_listing_is_sorted_deduplicated_and_normalized() {
        let about = About {
            crates: vec![
                AboutCrate {
                    package: package("zeta", "1.0.0"),
                    license: "MIT".into(),
                },
                AboutCrate {
                    package: package("alpha", "0.2.0"),
                    license: "MIT OR Apache-2.0".into(),
                },
            ],
            licenses: vec![
                AboutLicense {
                    id: "MIT".into(),
                    name: "MIT License".into(),
                    text: "\r\nCopyright (c) Zeta  \r\n\r\nPermission is granted.\r\n\r\n".into(),
                    used_by: vec![AboutUse {
                        krate: package("zeta", "1.0.0"),
                    }],
                },
                AboutLicense {
                    id: "MIT".into(),
                    name: "MIT License".into(),
                    text: "Copyright (c) Zeta\n\nPermission is granted.".into(),
                    used_by: vec![AboutUse {
                        krate: package("alpha", "0.2.0"),
                    }],
                },
            ],
        };
        let l = listing(about, &LISTINGS[0]);
        assert_eq!(
            l.crates.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(),
            ["alpha", "zeta"]
        );
        assert_eq!(
            l.licenses.len(),
            1,
            "same text after normalizing: one entry"
        );
        assert_eq!(l.licenses[0].used_by, ["alpha 0.2.0", "zeta 1.0.0"]);
        assert_eq!(
            l.licenses[0].text,
            "Copyright (c) Zeta\n\nPermission is granted."
        );
    }

    #[test]
    fn a_stale_listing_names_what_changed() {
        let row = |n: &str, l: &str| CrateRow {
            name: n.into(),
            version: "1.0.0".into(),
            license: l.into(),
            repository: None,
        };
        let old = Listing {
            generator: String::new(),
            crates: vec![row("a", "MIT"), row("b", "MIT")],
            licenses: Vec::new(),
        };
        let new = Listing {
            generator: String::new(),
            crates: vec![row("a", "Apache-2.0"), row("c", "ISC")],
            licenses: Vec::new(),
        };
        let d = describe_change(&old, &new);
        assert!(d.contains("~ a 1.0.0: MIT -> Apache-2.0"), "{d}");
        assert!(d.contains("+ c 1.0.0 (ISC)"), "{d}");
        assert!(d.contains("- b 1.0.0"), "{d}");
    }

    fn committed(output: &str) -> Listing {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let text = std::fs::read_to_string(root.join(output)).unwrap();
        serde_json::from_str(&text).unwrap()
    }

    /// The listing names these crates, none of ours, and gives each crate a licence text.
    fn assert_complete(listing: &Listing, min_crates: usize, names: &[&str]) {
        assert!(
            listing.crates.len() > min_crates,
            "{}",
            listing.crates.len()
        );
        for name in names {
            assert!(
                listing.crates.iter().any(|c| c.name == *name),
                "{name} missing"
            );
        }
        assert!(
            !listing.crates.iter().any(|c| c.name.starts_with("obs-")),
            "our own crates are not third-party"
        );
        // Every crate is covered by at least one licence text.
        for c in &listing.crates {
            let id = format!("{} {}", c.name, c.version);
            assert!(
                listing.licenses.iter().any(|l| l.used_by.contains(&id)),
                "{id} has no licence text"
            );
        }
    }

    /// The committed listing parses, lists real crates and gives each a licence text.
    #[test]
    fn the_committed_listing_is_complete() {
        assert_complete(
            &committed(OUTPUT),
            100,
            &["egui", "eframe", "rusqlite", "serde", "harfrust"],
        );
    }

    /// The iOS listing covers the FFI crate's graph, UniFFI's runtime included, and not its
    /// bindings generator.
    #[test]
    fn the_committed_ios_listing_is_complete() {
        let listing = committed(IOS_OUTPUT);
        assert_complete(
            &listing,
            50,
            &["uniffi", "uniffi_core", "rusqlite", "harfrust"],
        );
        for name in ["uniffi_bindgen", "clap", "egui"] {
            assert!(
                !listing.crates.iter().any(|c| c.name == name),
                "{name} is not in the iOS app"
            );
        }
    }

    /// D21: MPL-2.0 is accepted for UniFFI's runtime in the iOS app only.
    #[test]
    fn only_uniffi_is_taken_under_mpl_2_0_and_only_on_ios() {
        let mpl = |l: &Listing| -> Vec<String> {
            l.licenses
                .iter()
                .filter(|t| t.id == "MPL-2.0")
                .flat_map(|t| t.used_by.clone())
                .collect()
        };
        assert_eq!(mpl(&committed(OUTPUT)), Vec::<String>::new());
        let ios = mpl(&committed(IOS_OUTPUT));
        assert!(ios.iter().any(|c| c.starts_with("uniffi ")), "{ios:?}");
        const UNIFFI_CRATES: &[&str] = &[
            "uniffi",
            "uniffi_core",
            "uniffi_internal_macros",
            "uniffi_macros",
            "uniffi_meta",
            "uniffi_pipeline",
        ];
        for crate_version in &ios {
            let name = crate_version.split(' ').next().unwrap_or_default();
            assert!(
                UNIFFI_CRATES.contains(&name),
                "{crate_version} is not UniFFI: {ios:?}"
            );
        }
    }
}
