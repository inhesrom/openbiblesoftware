//! `cargo xtask ios`: the Mobile app's Rust side for iOS (`docs/dev/mobile-core.md`).
//!
//! - `bindgen` builds `obs-ffi` as a host cdylib and runs `apps/uniffi-bindgen` over it in
//!   library mode, writing the Swift bindings to [`BINDINGS`].
//! - `check` is the host-only check: `cargo check -p obs-ffi`, then `bindgen`.
//! - `build` builds the `aarch64-apple-ios` and `aarch64-apple-ios-sim` staticlibs with
//!   `[profile.ios]` for iOS [`IOS_DEPLOYMENT_TARGET`] and wraps them, with the bindings'
//!   header and module map, in [`XCFRAMEWORK`], then puts that and the generated Swift in the
//!   app's local Swift package [`PACKAGE`] (`docs/dev/ios-project.md`). It needs Xcode's iOS
//!   SDKs (`rusqlite` compiles SQLite with them) and says so, without building anything, when
//!   they are missing.
//! - `smoke` runs `bindgen`, compiles [`SMOKE_MAIN`] with the bindings against the host
//!   library with `swiftc`, and runs it over the Bundled set: Swift boots the app, opens the
//!   BSB at Genesis 1 and lays out a 390 × 256 pt tile at 3× (S03 of the iOS plan).
//!
//! None of these is a gate of `cargo xtask ci`, which never needs macOS or Xcode.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};

/// Where `bindgen` writes the Swift bindings.
pub const BINDINGS: &str = "target/ios/bindings";
/// Where `build` writes the XCFramework.
pub const XCFRAMEWORK: &str = "target/ios/ObsFFI.xcframework";
/// The local Swift package `apps/ios/project.yml` links: `build` copies [`XCFRAMEWORK`] to
/// its `ObsFFI.xcframework` and the generated Swift to its `Sources/Generated` (both
/// gitignored).
pub const PACKAGE: &str = "apps/ios/Packages/OpenBibleCore";
/// The app's minimum iOS, as `apps/ios/project.yml` has it. The staticlibs are built for it
/// (`IPHONEOS_DEPLOYMENT_TARGET`), so the linker sees one minimum, and a build from a shell and
/// one from Xcode's run-script phase share cargo's cache.
pub const IOS_DEPLOYMENT_TARGET: &str = "17.0";
/// The Swift smoke's source, and where it is built and run.
const SMOKE_MAIN: &str = "apps/obs-ffi/swift-smoke/main.swift";
const SMOKE_DIR: &str = "target/ios/swift-smoke";
/// The Bundled set `bundled-set` copies, which the smoke boots over by default.
const BUNDLED_SET: &str = "apps/ios/Bundled/starter";
/// The line the smoke prints for a 390 × 256 pt tile at 3×.
const SMOKE_TILE: &str = "tile 1170x768";
/// The bindings' header and module map, as the XCFramework's `Headers` wants them.
const HEADERS: &str = "target/ios/headers";
/// The UniFFI crate and its library's file stem.
const FFI_PACKAGE: &str = "obs-ffi";
const FFI_LIB: &str = "obs_ffi";
/// The Swift module `apps/obs-ffi/uniffi.toml` names, and UniFFI's name for its C module.
const SWIFT_MODULE: &str = "ObsFFI";
const C_MODULE: &str = "ObsFFIFFI";

/// One library in the XCFramework: a Rust target and the Xcode SDK its C code builds with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Slice {
    target: &'static str,
    sdk: &'static str,
}

const DEVICE: Slice = Slice {
    target: "aarch64-apple-ios",
    sdk: "iphoneos",
};
const SIMULATOR: Slice = Slice {
    target: "aarch64-apple-ios-sim",
    sdk: "iphonesimulator",
};

const USAGE: &str = "usage: cargo xtask ios <command>

  bindgen            build obs-ffi for the host and write its Swift bindings
                     (ObsFFI.swift, ObsFFIFFI.h, ObsFFIFFI.modulemap) to target/ios/bindings
  check              host only: cargo check -p obs-ffi, then bindgen
  build [--sim-only] the aarch64-apple-ios and aarch64-apple-ios-sim staticlibs ([profile.ios],
                     iOS 17) and target/ios/ObsFFI.xcframework, copied with ObsFFI.swift into
                     apps/ios/Packages/OpenBibleCore; --sim-only: the simulator's alone.
                     Needs Xcode with the iOS SDKs
  smoke [--bundle DIR]
                     host only: bindgen, then compile apps/obs-ffi/swift-smoke/main.swift
                     with swiftc against the host library and run it over the Bundled set
                     (default apps/ios/Bundled/starter): it prints the engine version and
                     `tile 1170x768`. Needs swiftc (Xcode or the Command Line Tools)
  bundled-set [--from DIR] [--out DIR] [--update-lock | --verify]
                     copy the Bundled set out of dist/starter/ into apps/ios/Bundled/starter/
                     and check it against apps/ios/bundled-set.lock (`--help`)

docs/dev/mobile-core.md, docs/dev/ios-project.md, docs/dev/starter-library.md";

pub fn run(root: &Path, args: &[String]) -> Result<()> {
    match args.first().map(String::as_str) {
        Some("bindgen") => {
            no_more(&args[1..])?;
            bindgen(root).map(|_| ())
        }
        Some("check") => {
            no_more(&args[1..])?;
            crate::cargo(root, &["check", "--locked", "-p", FFI_PACKAGE])?;
            bindgen(root).map(|_| ())
        }
        Some("build") => {
            let sim_only = build_options(&args[1..])?;
            build(root, sim_only, &xcrun_has_sdk)
        }
        Some("smoke") => {
            let bundle = smoke_options(root, &args[1..])?;
            smoke(root, &bundle)
        }
        Some("bundled-set") => crate::starter::bundled_set::run(root, &args[1..]),
        None | Some("-h" | "--help" | "help") => {
            println!("{USAGE}");
            Ok(())
        }
        Some(other) => bail!("unknown ios command `{other}`\n\n{USAGE}"),
    }
}

fn no_more(args: &[String]) -> Result<()> {
    match args.first() {
        None => Ok(()),
        Some(a) => bail!("unexpected argument `{a}`\n\n{USAGE}"),
    }
}

/// `build`'s options: whether only the simulator slice is wanted.
fn build_options(args: &[String]) -> Result<bool> {
    let mut sim_only = false;
    for a in args {
        match a.as_str() {
            "--sim-only" => sim_only = true,
            other => bail!("unknown option `{other}`\n\n{USAGE}"),
        }
    }
    Ok(sim_only)
}

/// `smoke`'s options: the Bundled set folder to boot over.
fn smoke_options(root: &Path, args: &[String]) -> Result<PathBuf> {
    match args {
        [] => Ok(root.join(BUNDLED_SET)),
        [flag, dir] if flag == "--bundle" => Ok(root.join(dir)),
        [flag] if flag == "--bundle" => bail!("`--bundle` needs a folder\n\n{USAGE}"),
        [other, ..] => bail!("unknown option `{other}`\n\n{USAGE}"),
    }
}

fn slices(sim_only: bool) -> &'static [Slice] {
    if sim_only {
        &[SIMULATOR]
    } else {
        &[DEVICE, SIMULATOR]
    }
}

/// Whether Xcode has this SDK: `xcrun --sdk <sdk> --show-sdk-path` succeeds. False where
/// there is no `xcrun` (Linux, Windows, a Mac with only the Command Line Tools).
fn xcrun_has_sdk(sdk: &str) -> bool {
    Command::new("xcrun")
        .args(["--sdk", sdk, "--show-sdk-path"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// Fails, naming the missing SDKs, unless `has_sdk` finds every one the slices need.
fn require_sdks(slices: &[Slice], has_sdk: &dyn Fn(&str) -> bool) -> Result<()> {
    let missing: Vec<&str> = slices
        .iter()
        .map(|s| s.sdk)
        .filter(|sdk| !has_sdk(sdk))
        .collect();
    if missing.is_empty() {
        return Ok(());
    }
    let probes: Vec<String> = missing
        .iter()
        .map(|sdk| format!("`xcrun --sdk {sdk} --show-sdk-path`"))
        .collect();
    bail!(
        "`cargo xtask ios build` needs Xcode: {} failed, so the iOS SDK is missing. Install \
         Xcode with the iOS platform and select it (`sudo xcode-select -s \
         <path to Xcode.app>`). `cargo xtask ios check` works without it.",
        probes.join(" and ")
    )
}

/// Build `obs-ffi` for the host and write its Swift bindings. Returns the bindings folder.
fn bindgen(root: &Path) -> Result<PathBuf> {
    crate::cargo(root, &["build", "--locked", "-p", FFI_PACKAGE, "--lib"])?;
    let lib = root.join("target/debug").join(format!(
        "{}{FFI_LIB}{}",
        std::env::consts::DLL_PREFIX,
        std::env::consts::DLL_SUFFIX
    ));
    let out = root.join(BINDINGS);
    if out.exists() {
        std::fs::remove_dir_all(&out).with_context(|| format!("cannot clear {}", out.display()))?;
    }
    std::fs::create_dir_all(&out).with_context(|| format!("cannot create {}", out.display()))?;
    let lib_arg = lib.to_string_lossy();
    let out_arg = out.to_string_lossy();
    crate::cargo(
        root,
        &[
            "run",
            "--locked",
            "--quiet",
            "-p",
            "uniffi-bindgen",
            "--",
            "generate",
            &lib_arg,
            "--language",
            "swift",
            // Unformatted, so every host writes the same bytes (no swift-format dependence).
            "--no-format",
            "--out-dir",
            &out_arg,
        ],
    )?;
    for file in binding_files() {
        if !out.join(&file).is_file() {
            bail!(
                "uniffi-bindgen wrote no {file} in {BINDINGS} (is `module_name` in \
                 apps/obs-ffi/uniffi.toml still {SWIFT_MODULE}?)"
            );
        }
    }
    eprintln!("ios: wrote {} to {BINDINGS}", binding_files().join(", "));
    Ok(out)
}

fn binding_files() -> [String; 3] {
    [
        format!("{SWIFT_MODULE}.swift"),
        format!("{C_MODULE}.h"),
        format!("{C_MODULE}.modulemap"),
    ]
}

/// The Swift smoke: bindgen, `swiftc` over the smoke and the bindings, linked to the host
/// library `bindgen` built (`target/debug`, found again at run time through an rpath), then
/// run over `bundle` with a fresh folder for the app's files.
fn smoke(root: &Path, bundle: &Path) -> Result<()> {
    if !bundle.join("starter-manifest.json").is_file() {
        bail!(
            "no Bundled set in {}: run `cargo xtask ios bundled-set` first (or pass \
             `--bundle <a Starter library folder>`)",
            bundle.display()
        );
    }
    let bindings = bindgen(root)?;
    let dir = root.join(SMOKE_DIR);
    let run_dir = dir.join("run");
    if run_dir.exists() {
        std::fs::remove_dir_all(&run_dir)
            .with_context(|| format!("cannot clear {}", run_dir.display()))?;
    }
    std::fs::create_dir_all(&run_dir)
        .with_context(|| format!("cannot create {}", run_dir.display()))?;
    let exe = dir.join("swift-smoke");
    let lib_dir = root.join("target/debug");
    // The generated module map is named for UniFFI's C module, not `module.modulemap`, so
    // clang finds it only when named (`-I` alone does not).
    let module_map = bindings.join(format!("{C_MODULE}.modulemap"));
    let status = Command::new("swiftc")
        .arg("-Xcc")
        .arg(format!("-fmodule-map-file={}", module_map.display()))
        .arg("-L")
        .arg(&lib_dir)
        .arg(format!("-l{FFI_LIB}"))
        .args(["-Xlinker", "-rpath", "-Xlinker"])
        .arg(&lib_dir)
        .arg("-o")
        .arg(&exe)
        .arg(root.join(SMOKE_MAIN))
        .arg(bindings.join(format!("{SWIFT_MODULE}.swift")))
        .current_dir(root)
        .status()
        .context("could not run `swiftc` (install Xcode or the Command Line Tools)")?;
    if !status.success() {
        bail!("`swiftc` exited with {status}");
    }
    let out = Command::new(&exe)
        .arg(bundle)
        .arg(&run_dir)
        .current_dir(root)
        .output()
        .with_context(|| format!("could not run {}", exe.display()))?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    print!("{stdout}");
    eprint!("{}", String::from_utf8_lossy(&out.stderr));
    if !out.status.success() {
        bail!("the Swift smoke exited with {}", out.status);
    }
    smoke_passed(&stdout)
}

/// Whether the smoke's output has the engine version and a 3× phone tile.
fn smoke_passed(stdout: &str) -> Result<()> {
    let version = format!("engine {}", obs_engine::updates::APP_VERSION);
    for line in [version.as_str(), SMOKE_TILE] {
        if !stdout.lines().any(|l| l == line) {
            bail!("the Swift smoke did not print `{line}`");
        }
    }
    Ok(())
}

fn build(root: &Path, sim_only: bool, has_sdk: &dyn Fn(&str) -> bool) -> Result<()> {
    let slices = slices(sim_only);
    require_sdks(slices, has_sdk)?;
    let bindings = bindgen(root)?;

    let mut libs = Vec::new();
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    for slice in slices {
        let args = [
            "rustc",
            "--locked",
            "-p",
            FFI_PACKAGE,
            "--lib",
            "--profile",
            "ios",
            "--target",
            slice.target,
            "--crate-type",
            "staticlib",
        ];
        // rustc and `cc` (SQLite) both read the minimum iOS from this variable.
        let status = Command::new(&cargo)
            .args(args)
            .env("IPHONEOS_DEPLOYMENT_TARGET", IOS_DEPLOYMENT_TARGET)
            .current_dir(root)
            .status()
            .with_context(|| format!("could not run `{cargo}`"))?;
        if !status.success() {
            bail!("`cargo {}` exited with {status}", args.join(" "));
        }
        let lib = root
            .join("target")
            .join(slice.target)
            .join("ios")
            .join(format!("lib{FFI_LIB}.a"));
        if !lib.is_file() {
            bail!("cargo built no {}", lib.display());
        }
        libs.push(lib);
    }

    // The XCFramework's Headers: the C header and the module map under the name clang
    // looks for.
    let headers = root.join(HEADERS);
    if headers.exists() {
        std::fs::remove_dir_all(&headers)
            .with_context(|| format!("cannot clear {}", headers.display()))?;
    }
    std::fs::create_dir_all(&headers)
        .with_context(|| format!("cannot create {}", headers.display()))?;
    for (from, to) in [
        (format!("{C_MODULE}.h"), format!("{C_MODULE}.h")),
        (
            format!("{C_MODULE}.modulemap"),
            "module.modulemap".to_string(),
        ),
    ] {
        std::fs::copy(bindings.join(&from), headers.join(&to))
            .with_context(|| format!("cannot copy {from} into {HEADERS}"))?;
    }

    // `-create-xcframework` refuses to write over an existing one.
    let xcframework = root.join(XCFRAMEWORK);
    if xcframework.exists() {
        std::fs::remove_dir_all(&xcframework)
            .with_context(|| format!("cannot clear {}", xcframework.display()))?;
    }
    let mut cmd = Command::new("xcodebuild");
    cmd.arg("-create-xcframework");
    for lib in &libs {
        cmd.arg("-library").arg(lib).arg("-headers").arg(&headers);
    }
    cmd.arg("-output").arg(&xcframework).current_dir(root);
    let status = cmd.status().context("could not run `xcodebuild`")?;
    if !status.success() {
        bail!("`xcodebuild -create-xcframework` exited with {status}");
    }
    for lib in &libs {
        let bytes = std::fs::metadata(lib).map(|m| m.len()).unwrap_or(0);
        eprintln!("ios: {} ({:.1} MB)", lib.display(), bytes as f64 / 1e6);
    }
    eprintln!("ios: wrote {XCFRAMEWORK}");

    let package = root.join(PACKAGE);
    let mut written = mirror(&xcframework, &package.join("ObsFFI.xcframework"))?;
    written += mirror(
        &bindings.join(format!("{SWIFT_MODULE}.swift")),
        &package
            .join("Sources/Generated")
            .join(format!("{SWIFT_MODULE}.swift")),
    )?;
    eprintln!(
        "ios: {PACKAGE} has the XCFramework and {SWIFT_MODULE}.swift ({written} file(s) changed)"
    );
    Ok(())
}

/// Make `to` (a file or a folder) hold exactly what `from` holds, writing only the files whose
/// bytes differ and removing what `from` lacks, so an unchanged library leaves Xcode nothing
/// to redo. Returns how many files it wrote.
fn mirror(from: &Path, to: &Path) -> Result<usize> {
    let read = |p: &Path| std::fs::read(p).with_context(|| format!("cannot read {}", p.display()));
    if from.is_file() {
        let bytes = read(from)?;
        if to.is_file() && read(to)? == bytes {
            return Ok(0);
        }
        if to.is_dir() {
            std::fs::remove_dir_all(to)
                .with_context(|| format!("cannot clear {}", to.display()))?;
        }
        if let Some(dir) = to.parent() {
            std::fs::create_dir_all(dir)
                .with_context(|| format!("cannot create {}", dir.display()))?;
        }
        std::fs::write(to, bytes).with_context(|| format!("cannot write {}", to.display()))?;
        return Ok(1);
    }
    if to.is_file() {
        std::fs::remove_file(to).with_context(|| format!("cannot remove {}", to.display()))?;
    }
    std::fs::create_dir_all(to).with_context(|| format!("cannot create {}", to.display()))?;
    let mut written = 0;
    let mut wanted = std::collections::BTreeSet::new();
    for entry in walkdir::WalkDir::new(from).min_depth(1) {
        let entry = entry.with_context(|| format!("cannot list {}", from.display()))?;
        let rel = entry.path().strip_prefix(from)?.to_path_buf();
        if entry.file_type().is_file() {
            written += mirror(entry.path(), &to.join(&rel))?;
        } else if entry.file_type().is_dir() {
            std::fs::create_dir_all(to.join(&rel))
                .with_context(|| format!("cannot create {}", to.join(&rel).display()))?;
        }
        wanted.insert(rel);
    }
    // Deepest first, so a removed folder's contents go before it.
    let stale: Vec<PathBuf> = walkdir::WalkDir::new(to)
        .min_depth(1)
        .contents_first(true)
        .into_iter()
        .filter_map(|e| e.ok())
        .map(|e| e.into_path())
        .filter(|p| p.strip_prefix(to).is_ok_and(|rel| !wanted.contains(rel)))
        .collect();
    for path in stale {
        let removed = if path.is_dir() {
            std::fs::remove_dir_all(&path)
        } else {
            std::fs::remove_file(&path)
        };
        removed.with_context(|| format!("cannot remove {}", path.display()))?;
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    fn args(xs: &[&str]) -> Vec<String> {
        xs.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn build_needs_xcode_when_an_sdk_is_missing() {
        let asked = RefCell::new(Vec::new());
        let no_sdk = |sdk: &str| {
            asked.borrow_mut().push(sdk.to_string());
            false
        };
        let err = require_sdks(slices(false), &no_sdk)
            .unwrap_err()
            .to_string();
        assert!(err.contains("needs Xcode"), "{err}");
        assert!(
            err.contains("`xcrun --sdk iphoneos --show-sdk-path`")
                && err.contains("`xcrun --sdk iphonesimulator --show-sdk-path`"),
            "{err}"
        );
        assert_eq!(*asked.borrow(), ["iphoneos", "iphonesimulator"]);

        // Only the missing one is named.
        let device_only = |sdk: &str| sdk == "iphoneos";
        let err = require_sdks(slices(false), &device_only)
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("--sdk iphonesimulator") && !err.contains("--sdk iphoneos "),
            "{err}"
        );
    }

    #[test]
    fn build_goes_on_when_the_sdks_are_there() {
        assert!(require_sdks(slices(false), &|_| true).is_ok());
        // The simulator slice alone does not need the device SDK.
        assert!(require_sdks(slices(true), &|sdk| sdk == "iphonesimulator").is_ok());
    }

    #[test]
    fn sim_only_builds_only_the_simulator_slice() {
        assert_eq!(slices(false), [DEVICE, SIMULATOR]);
        assert_eq!(slices(true), [SIMULATOR]);
        assert!(!build_options(&args(&[])).unwrap());
        assert!(build_options(&args(&["--sim-only"])).unwrap());
        assert!(build_options(&args(&["--device-only"])).is_err());
    }

    #[test]
    fn unknown_commands_and_arguments_fail() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        assert!(run(root, &args(&["frobnicate"])).is_err());
        assert!(run(root, &args(&["bindgen", "--sim-only"])).is_err());
        assert!(run(root, &args(&["check", "extra"])).is_err());
    }

    #[test]
    fn smoke_boots_over_the_bundled_set_unless_told_another() {
        let root = Path::new("/repo");
        assert_eq!(
            smoke_options(root, &args(&[])).unwrap(),
            root.join("apps/ios/Bundled/starter")
        );
        assert_eq!(
            smoke_options(root, &args(&["--bundle", "dist/starter"])).unwrap(),
            root.join("dist/starter")
        );
        assert!(smoke_options(root, &args(&["--bundle"])).is_err());
        assert!(smoke_options(root, &args(&["--sim-only"])).is_err());
    }

    #[test]
    fn smoke_passes_on_the_version_and_a_3x_phone_tile() {
        let version = obs_engine::updates::APP_VERSION;
        let good = format!("engine {version}\nresources bsb\ntile 1170x768\n");
        assert!(smoke_passed(&good).is_ok());
        let small = format!("engine {version}\ntile 780x512\n");
        assert!(smoke_passed(&small).is_err());
        assert!(smoke_passed("tile 1170x768\n").is_err());
    }

    #[test]
    fn smoke_without_a_bundled_set_says_how_to_make_one() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let missing = root.join("target/xtask-no-such-bundle");
        let err = smoke(root, &missing).unwrap_err().to_string();
        assert!(err.contains("cargo xtask ios bundled-set"), "{err}");
    }

    /// The bindings' file names come from `apps/obs-ffi/uniffi.toml`.
    #[test]
    fn the_swift_module_is_the_one_uniffi_toml_names() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let text = std::fs::read_to_string(root.join("apps/obs-ffi/uniffi.toml")).unwrap();
        let config: toml::Table = toml::from_str(&text).unwrap();
        assert_eq!(
            config["bindings"]["swift"]["module_name"].as_str(),
            Some(SWIFT_MODULE)
        );
        assert_eq!(C_MODULE, format!("{SWIFT_MODULE}FFI"), "UniFFI's default");
    }

    #[test]
    fn mirror_writes_only_what_changed_and_removes_what_is_gone() {
        let base = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join(format!(
                "target/xtask-tests/ios-mirror-{}",
                std::process::id()
            ));
        let _ = std::fs::remove_dir_all(&base);
        let (from, to) = (base.join("from"), base.join("to"));
        std::fs::create_dir_all(from.join("ios-arm64-simulator/Headers")).unwrap();
        std::fs::write(from.join("Info.plist"), "plist").unwrap();
        std::fs::write(from.join("ios-arm64-simulator/Headers/a.h"), "header").unwrap();
        std::fs::create_dir_all(to.join("ios-arm64/Headers")).unwrap();
        std::fs::write(to.join("ios-arm64/Headers/a.h"), "old slice").unwrap();
        std::fs::write(to.join("Info.plist"), "old plist").unwrap();

        assert_eq!(mirror(&from, &to).unwrap(), 2);
        assert_eq!(
            std::fs::read_to_string(to.join("Info.plist")).unwrap(),
            "plist"
        );
        assert!(to.join("ios-arm64-simulator/Headers/a.h").is_file());
        assert!(
            !to.join("ios-arm64").exists(),
            "a slice no longer built is removed"
        );
        // Unchanged: nothing written.
        assert_eq!(mirror(&from, &to).unwrap(), 0);

        let (file, copy) = (from.join("Info.plist"), base.join("one/ObsFFI.swift"));
        assert_eq!(mirror(&file, &copy).unwrap(), 1);
        assert_eq!(mirror(&file, &copy).unwrap(), 0);
        std::fs::remove_dir_all(&base).unwrap();
    }

    /// The staticlibs are built for the minimum iOS the app's project names.
    #[test]
    fn the_libraries_are_built_for_the_apps_minimum_ios() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let spec = std::fs::read_to_string(root.join("apps/ios/project.yml")).unwrap();
        let line = format!("iOS: \"{IOS_DEPLOYMENT_TARGET}\"");
        assert!(
            spec.lines().any(|l| l.trim() == line),
            "no `{line}` in project.yml"
        );
        let package = std::fs::read_to_string(root.join(PACKAGE).join("Package.swift")).unwrap();
        let major = IOS_DEPLOYMENT_TARGET.split('.').next().unwrap();
        assert!(package.contains(&format!(".iOS(.v{major})")), "{package}");
    }

    /// `[profile.ios]` keeps unwinding: the job queue catches a panicking job, which
    /// `panic = "abort"` would turn into an app crash.
    #[test]
    fn the_ios_profile_unwinds() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let text = std::fs::read_to_string(root.join("Cargo.toml")).unwrap();
        let manifest: toml::Table = toml::from_str(&text).unwrap();
        let ios = manifest["profile"]["ios"].as_table().unwrap();
        assert_eq!(ios["inherits"].as_str(), Some("release"));
        assert_eq!(ios["opt-level"].as_str(), Some("s"));
        assert!(
            ios.get("panic")
                .is_none_or(|p| p.as_str() == Some("unwind"))
        );
    }
}
