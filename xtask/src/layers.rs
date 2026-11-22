//! Crate layering rules (ADR 0005, #12).
//!
//! - A workspace crate may depend only on workspace crates in the same or a lower layer.
//! - Below L6 no crate may depend, even transitively, on a UI crate (egui, eframe, winit, rfd, …).
//! - Only `obs-store` may depend directly on SQLite crates.
//! - No library crate may depend, even transitively, on a networking crate; only apps may
//!   (offline-first engine; Get Bibles, #91, injects its transport from the app).
//! - No library crate may depend, even transitively, on an FFI crate; only apps may (the
//!   Mobile app's one UniFFI crate is `apps/obs-ffi`, `docs/dev/mobile-core.md`).
//! - Every workspace crate outside `apps/` and `xtask` must be listed in [`LAYERS`].
//! - Every crate forbids `unsafe_code` but `apps/obs-ffi`, which denies it and allows it on
//!   one `unsafe impl` (its tiles' bulk byte path, ADR 0021, plan S13); no other `unsafe` may
//!   appear in that crate.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use anyhow::{Context, Result, bail};
use cargo_metadata::{DependencyKind, Metadata, MetadataCommand, PackageId};

/// Layer of every library crate. Apps sit above all layers.
pub const LAYERS: &[(&str, u8)] = &[
    ("obs-scripture", 0),
    ("obs-model", 1),
    ("obs-fonts", 1),
    ("obs-formats", 2),
    ("obs-layout", 2),
    ("obs-search", 2),
    ("obs-store", 2),
    ("obs-render", 3),
    ("obs-engine", 4),
    ("obs-reading", 5),
    ("obs-mobile", 5),
    ("obs-ui-egui", 6),
];

/// The first layer allowed to use UI crates.
const UI_LAYER: u8 = 6;

/// Crates that must not appear below [`UI_LAYER`], directly or transitively.
const UI_CRATES: &[&str] = &[
    "egui",
    "eframe",
    "egui_tiles",
    "egui-winit",
    "egui_glow",
    "egui-wgpu",
    "egui_kittest",
    "winit",
    "rfd",
    "accesskit_winit",
];

/// Crates that make network connections. Only apps may depend on them, directly or
/// transitively; libraries take a transport from the app instead.
const NET_CRATES: &[&str] = &[
    "ureq",
    "reqwest",
    "hyper",
    "h2",
    "isahc",
    "curl",
    "attohttpc",
    "minreq",
    "rustls",
    "native-tls",
    "openssl",
    "tungstenite",
    "suppaftp",
];

/// Foreign-function binding crates. Only apps may depend on them, directly or transitively, so
/// every library stays plain Rust and compiles for wasm32.
const FFI_CRATES: &[&str] = &["uniffi", "uniffi_core", "uniffi_macros"];

/// Crates only `obs-store` may depend on directly.
const SQLITE_CRATES: &[&str] = &["rusqlite", "libsqlite3-sys", "sqlite-wasm-rs"];
const SQLITE_OWNER: &str = "obs-store";

/// What a package is, for the rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Library(u8),
    App,
    Tool,
}

/// The facts the rules need about one workspace package.
#[derive(Debug, Clone)]
pub struct Package {
    pub name: String,
    pub kind: Option<Kind>,
    /// Direct normal dependencies on other workspace packages.
    pub workspace_deps: BTreeSet<String>,
    /// Direct normal dependencies on external crates.
    pub external_deps: BTreeSet<String>,
    /// All crates reachable through normal dependencies.
    pub transitive_deps: BTreeSet<String>,
}

pub fn metadata(root: &Path) -> Result<Metadata> {
    MetadataCommand::new()
        .current_dir(root)
        .exec()
        .context("cargo metadata failed")
}

/// Library crates checked for wasm32 (everything except apps and tools).
pub fn wasm_crates(metadata: &Metadata) -> Vec<String> {
    packages(metadata)
        .into_iter()
        .filter(|p| matches!(p.kind, Some(Kind::Library(_))))
        .map(|p| p.name)
        .collect()
}

/// The crate allowed one `unsafe impl` (ADR 0021, plan S13), and that impl's first line.
const UNSAFE_CRATE: &str = "apps/obs-ffi";
const UNSAFE_IMPL: &str = "unsafe impl<UT> uniffi::FfiConverter<UT> for TilePixels {";

/// Every other `unsafe` in [`UNSAFE_CRATE`]'s Rust files, and any `allow(unsafe_code)` but the
/// one just above [`UNSAFE_IMPL`]: the exception cannot grow silently.
fn unsafe_exception(root: &Path) -> Result<Vec<String>> {
    let mut files = Vec::new();
    let mut dirs = vec![root.join(UNSAFE_CRATE)];
    while let Some(dir) = dirs.pop() {
        for entry in
            std::fs::read_dir(&dir).with_context(|| format!("reading {}", dir.display()))?
        {
            let path = entry?.path();
            if path.is_dir() {
                dirs.push(path);
            } else if path.extension().is_some_and(|e| e == "rs") {
                files.push(path);
            }
        }
    }
    files.sort();
    let mut out = Vec::new();
    let mut impls = 0;
    for file in files {
        let text = std::fs::read_to_string(&file)?;
        let name = file
            .strip_prefix(root)
            .unwrap_or(&file)
            .display()
            .to_string();
        let (found, bad) = unsafe_violations(&name, &text);
        impls += found;
        out.extend(bad);
    }
    if impls > 1 {
        out.push(format!(
            "{UNSAFE_CRATE}: `{UNSAFE_IMPL}` appears {impls} times"
        ));
    }
    Ok(out)
}

/// In one file: how many times [`UNSAFE_IMPL`] appears (with `#[allow(unsafe_code)]` on the
/// line before), and every other line with the keyword `unsafe` or an `allow(unsafe_code)`.
/// Comments are not code.
fn unsafe_violations(file: &str, text: &str) -> (usize, Vec<String>) {
    let code: Vec<&str> = (text.lines())
        .map(|l| l.split("//").next().unwrap_or_default().trim())
        .collect();
    let keyword = |line: &str| {
        line.match_indices("unsafe").any(|(i, _)| {
            let before = line[..i].chars().next_back();
            let after = line[i + "unsafe".len()..].chars().next();
            let word = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || c == '_');
            !word(before) && !word(after)
        })
    };
    let mut found = 0;
    let mut bad = Vec::new();
    for (i, line) in code.iter().enumerate() {
        let allowed = *line == UNSAFE_IMPL && i > 0 && code[i - 1] == "#[allow(unsafe_code)]";
        let allow_for_it =
            *line == "#[allow(unsafe_code)]" && code.get(i + 1) == Some(&UNSAFE_IMPL);
        if allowed {
            found += 1;
        } else if allow_for_it {
        } else if keyword(line) || line.contains("allow(unsafe_code)") {
            bad.push(format!(
                "{file}:{}: `unsafe` outside the one allowed impl (ADR 0021): {line}",
                i + 1
            ));
        }
    }
    (found, bad)
}

pub fn run(root: &Path) -> Result<()> {
    let metadata = metadata(root)?;
    let pkgs = packages(&metadata);
    let mut violations = check(&pkgs);
    violations.extend(unsafe_exception(root)?);
    if violations.is_empty() {
        eprintln!("layers: {} crates OK", pkgs.len());
        return Ok(());
    }
    for v in &violations {
        eprintln!("layers: {v}");
    }
    bail!("{} layering violation(s)", violations.len())
}

fn kind_of(name: &str, manifest: &Path, root: &Path) -> Option<Kind> {
    if let Some((_, layer)) = LAYERS.iter().find(|(n, _)| *n == name) {
        return Some(Kind::Library(*layer));
    }
    let rel = manifest.strip_prefix(root).unwrap_or(manifest);
    if rel.starts_with("apps") {
        Some(Kind::App)
    } else if rel.starts_with("xtask") {
        Some(Kind::Tool)
    } else {
        None
    }
}

/// Collect the rule inputs from cargo metadata.
pub fn packages(metadata: &Metadata) -> Vec<Package> {
    let root = metadata.workspace_root.as_std_path();
    let members: BTreeSet<&PackageId> = metadata.workspace_members.iter().collect();
    let name_of: BTreeMap<&PackageId, &str> = metadata
        .packages
        .iter()
        .map(|p| (&p.id, p.name.as_str()))
        .collect();
    let Some(resolve) = metadata.resolve.as_ref() else {
        return Vec::new();
    };
    let normal_deps: BTreeMap<&PackageId, Vec<&PackageId>> = resolve
        .nodes
        .iter()
        .map(|node| {
            let deps = node
                .deps
                .iter()
                .filter(|d| d.dep_kinds.iter().any(|k| k.kind == DependencyKind::Normal))
                .map(|d| &d.pkg)
                .collect();
            (&node.id, deps)
        })
        .collect();

    let mut out = Vec::new();
    for pkg in metadata.packages.iter().filter(|p| members.contains(&p.id)) {
        let mut workspace_deps = BTreeSet::new();
        let mut external_deps = BTreeSet::new();
        for dep in normal_deps.get(&pkg.id).into_iter().flatten() {
            let name = name_of.get(dep).copied().unwrap_or_default().to_string();
            if members.contains(dep) {
                workspace_deps.insert(name);
            } else {
                external_deps.insert(name);
            }
        }
        let mut transitive_deps = BTreeSet::new();
        let mut stack: Vec<&PackageId> = normal_deps.get(&pkg.id).cloned().unwrap_or_default();
        let mut seen = BTreeSet::new();
        while let Some(id) = stack.pop() {
            if !seen.insert(id) {
                continue;
            }
            transitive_deps.insert(name_of.get(id).copied().unwrap_or_default().to_string());
            stack.extend(normal_deps.get(id).into_iter().flatten());
        }
        out.push(Package {
            name: pkg.name.to_string(),
            kind: kind_of(&pkg.name, pkg.manifest_path.as_std_path(), root),
            workspace_deps,
            external_deps,
            transitive_deps,
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// Apply the rules. Returns one human-readable message per violation.
pub fn check(pkgs: &[Package]) -> Vec<String> {
    let kinds: BTreeMap<&str, Option<Kind>> =
        pkgs.iter().map(|p| (p.name.as_str(), p.kind)).collect();
    let mut errors = Vec::new();
    for pkg in pkgs {
        let layer = match pkg.kind {
            None => {
                errors.push(format!(
                    "`{}` has no layer: add it to LAYERS in xtask/src/layers.rs",
                    pkg.name
                ));
                continue;
            }
            Some(Kind::Library(layer)) => layer,
            Some(Kind::App | Kind::Tool) => continue,
        };
        for dep in &pkg.workspace_deps {
            match kinds.get(dep.as_str()).copied().flatten() {
                Some(Kind::Library(dep_layer)) if dep_layer > layer => errors.push(format!(
                    "`{}` (L{layer}) depends on `{dep}` (L{dep_layer}), a higher layer",
                    pkg.name
                )),
                Some(Kind::App | Kind::Tool) => errors.push(format!(
                    "`{}` depends on `{dep}`, which is an app or tool",
                    pkg.name
                )),
                _ => {}
            }
        }
        if layer < UI_LAYER {
            for ui in UI_CRATES {
                if pkg.transitive_deps.contains(*ui) {
                    let how = if pkg.external_deps.contains(*ui) {
                        "directly"
                    } else {
                        "transitively"
                    };
                    errors.push(format!(
                        "`{}` (L{layer}) depends {how} on UI crate `{ui}`; only L{UI_LAYER} may",
                        pkg.name
                    ));
                }
            }
        }
        for net in NET_CRATES {
            if pkg.transitive_deps.contains(*net) {
                let how = if pkg.external_deps.contains(*net) {
                    "directly"
                } else {
                    "transitively"
                };
                errors.push(format!(
                    "`{}` (L{layer}) depends {how} on networking crate `{net}`; only apps may",
                    pkg.name
                ));
            }
        }
        for ffi in FFI_CRATES {
            if pkg.transitive_deps.contains(*ffi) {
                let how = if pkg.external_deps.contains(*ffi) {
                    "directly"
                } else {
                    "transitively"
                };
                errors.push(format!(
                    "`{}` (L{layer}) depends {how} on FFI crate `{ffi}`; only apps may",
                    pkg.name
                ));
            }
        }
        if pkg.name != SQLITE_OWNER {
            for db in SQLITE_CRATES {
                if pkg.external_deps.contains(*db) {
                    errors.push(format!(
                        "`{}` depends on `{db}`; only `{SQLITE_OWNER}` may use SQLite",
                        pkg.name
                    ));
                }
            }
        }
    }
    errors
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pkg(name: &str, kind: Option<Kind>, ws: &[&str], ext: &[&str], trans: &[&str]) -> Package {
        let set = |xs: &[&str]| xs.iter().map(|s| s.to_string()).collect();
        let mut transitive: BTreeSet<String> = set(trans);
        transitive.extend(set(ext));
        transitive.extend(set(ws));
        Package {
            name: name.into(),
            kind,
            workspace_deps: set(ws),
            external_deps: set(ext),
            transitive_deps: transitive,
        }
    }

    fn good() -> Vec<Package> {
        vec![
            pkg("obs-scripture", Some(Kind::Library(0)), &[], &[], &[]),
            pkg(
                "obs-model",
                Some(Kind::Library(1)),
                &["obs-scripture"],
                &[],
                &[],
            ),
            pkg(
                "obs-store",
                Some(Kind::Library(2)),
                &["obs-model"],
                &["rusqlite"],
                &["libsqlite3-sys"],
            ),
            pkg(
                "obs-engine",
                Some(Kind::Library(4)),
                &["obs-store"],
                &[],
                &["rusqlite"],
            ),
            pkg(
                "obs-ui-egui",
                Some(Kind::Library(6)),
                &["obs-engine"],
                &["egui", "eframe"],
                &["winit"],
            ),
            pkg(
                "openbible",
                Some(Kind::App),
                &["obs-ui-egui"],
                &["eframe"],
                &["winit"],
            ),
            pkg("xtask", Some(Kind::Tool), &[], &["anyhow"], &[]),
        ]
    }

    #[test]
    fn real_layout_passes() {
        assert_eq!(check(&good()), Vec::<String>::new());
    }

    #[test]
    fn the_real_ffi_crate_has_only_the_one_unsafe_impl() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        assert_eq!(unsafe_exception(&root).unwrap(), Vec::<String>::new());
    }

    #[test]
    fn unsafe_outside_the_one_impl_fails() {
        let ok = "// unsafe in a comment\n#[allow(unsafe_code)]\nunsafe impl<UT> uniffi::FfiConverter<UT> for TilePixels {\n}\nlet unsafe_code = 1;\n";
        assert_eq!(unsafe_violations("a.rs", ok), (1, Vec::new()));
        let block = "fn f() { unsafe { g() } }\n";
        assert_eq!(unsafe_violations("a.rs", block).1.len(), 1);
        let no_allow = "unsafe impl<UT> uniffi::FfiConverter<UT> for TilePixels {\n";
        assert_eq!(unsafe_violations("a.rs", no_allow), (0, vec![
            "a.rs:1: `unsafe` outside the one allowed impl (ADR 0021): unsafe impl<UT> uniffi::FfiConverter<UT> for TilePixels {".to_string()
        ]));
        let crate_wide = "#![allow(unsafe_code)]\n";
        assert_eq!(unsafe_violations("a.rs", crate_wide).1.len(), 1);
        let other_impl = "#[allow(unsafe_code)]\nunsafe impl Send for X {}\n";
        assert_eq!(unsafe_violations("a.rs", other_impl).1.len(), 2);
    }

    #[test]
    fn networking_crate_in_a_library_fails_but_an_app_may_use_it() {
        let mut pkgs = good();
        pkgs[3].external_deps.insert("ureq".into());
        pkgs[3].transitive_deps.insert("ureq".into());
        let errors = check(&pkgs);
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(errors[0].contains("`obs-engine`"), "{errors:?}");
        assert!(errors[0].contains("directly on networking crate `ureq`"));

        let mut pkgs = good();
        pkgs[4].transitive_deps.insert("rustls".into());
        let errors = check(&pkgs);
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(errors[0].contains("`obs-ui-egui` (L6) depends transitively"));

        let mut pkgs = good();
        pkgs[5].external_deps.insert("ureq".into());
        pkgs[5].transitive_deps.insert("rustls".into());
        assert_eq!(check(&pkgs), Vec::<String>::new());
    }

    #[test]
    fn uniffi_in_a_library_fails_but_an_app_may_use_it() {
        let mut pkgs = good();
        pkgs.push(pkg(
            "obs-mobile",
            Some(Kind::Library(5)),
            &["obs-engine"],
            &["uniffi"],
            &["uniffi_core"],
        ));
        let errors = check(&pkgs);
        assert_eq!(errors.len(), 2, "{errors:?}");
        assert!(
            errors[0].contains("`obs-mobile` (L5) depends directly on FFI crate `uniffi`"),
            "{errors:?}"
        );
        assert!(
            errors[1].contains("`obs-mobile` (L5) depends transitively on FFI crate `uniffi_core`"),
            "{errors:?}"
        );

        // UniFFI's runtime traits are usable without the `uniffi` facade.
        let mut pkgs = good();
        pkgs.push(pkg(
            "obs-mobile",
            Some(Kind::Library(5)),
            &["obs-engine"],
            &["uniffi_core"],
            &[],
        ));
        let errors = check(&pkgs);
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(
            errors[0].contains("`obs-mobile` (L5) depends directly on FFI crate `uniffi_core`"),
            "{errors:?}"
        );

        // Through another crate, and in a UI-layer crate too.
        let mut pkgs = good();
        pkgs[4].transitive_deps.insert("uniffi".into());
        let errors = check(&pkgs);
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(errors[0].contains("`obs-ui-egui` (L6) depends transitively"));

        let mut pkgs = good();
        pkgs.push(pkg(
            "obs-ffi",
            Some(Kind::App),
            &["obs-engine"],
            &["uniffi"],
            &["uniffi_core"],
        ));
        assert_eq!(check(&pkgs), Vec::<String>::new());
    }

    #[test]
    fn egui_below_ui_layer_fails_naming_the_crate() {
        let mut pkgs = good();
        pkgs.push(pkg(
            "obs-layout",
            Some(Kind::Library(2)),
            &[],
            &["egui"],
            &[],
        ));
        let errors = check(&pkgs);
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(errors[0].contains("`obs-layout`") && errors[0].contains("directly"));
    }

    #[test]
    fn transitive_winit_below_ui_layer_fails() {
        let mut pkgs = good();
        pkgs.push(pkg(
            "obs-render",
            Some(Kind::Library(3)),
            &[],
            &["some-crate"],
            &["winit"],
        ));
        let errors = check(&pkgs);
        assert!(
            errors
                .iter()
                .any(|e| e.contains("`obs-render`") && e.contains("transitively"))
        );
    }

    #[test]
    fn sqlite_outside_store_fails() {
        let mut pkgs = good();
        pkgs.push(pkg(
            "obs-search",
            Some(Kind::Library(2)),
            &[],
            &["rusqlite"],
            &[],
        ));
        let errors = check(&pkgs);
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(errors[0].contains("`obs-search`") && errors[0].contains("rusqlite"));
    }

    #[test]
    fn upward_dependency_fails() {
        let mut pkgs = good();
        pkgs.push(pkg(
            "obs-fonts",
            Some(Kind::Library(1)),
            &["obs-engine"],
            &[],
            &[],
        ));
        let errors = check(&pkgs);
        assert!(
            errors
                .iter()
                .any(|e| e.contains("`obs-fonts` (L1) depends on `obs-engine` (L4)"))
        );
    }

    #[test]
    fn library_depending_on_app_fails() {
        let mut pkgs = good();
        pkgs.push(pkg(
            "obs-search",
            Some(Kind::Library(2)),
            &["openbible"],
            &[],
            &[],
        ));
        assert!(check(&pkgs).iter().any(|e| e.contains("app or tool")));
    }

    #[test]
    fn unlisted_crate_fails() {
        let mut pkgs = good();
        pkgs.push(pkg("obs-new", None, &[], &[], &[]));
        assert!(
            check(&pkgs)
                .iter()
                .any(|e| e.contains("`obs-new` has no layer"))
        );
    }
}
