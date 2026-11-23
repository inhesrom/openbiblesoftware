//! PROTOTYPE: if the -dev symlinks of gpui's native libraries are missing (a runtime-only
//! install, no `libxcb1-dev` etc.), link against the versioned .so files through `lib*.so`
//! symlinks made in OUT_DIR. Harmless where the -dev packages are installed.
fn main() {
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let dirs = [
        "/usr/lib/x86_64-linux-gnu",
        "/usr/lib/aarch64-linux-gnu",
        "/usr/lib64",
        "/usr/lib",
    ];
    let libs = [
        "xkbcommon",
        "xkbcommon-x11",
        "xcb",
        "xcb-xkb",
        "X11",
        "X11-xcb",
        "wayland-client",
        "fontconfig",
        "freetype",
        "vulkan",
    ];
    for lib in libs {
        if dirs
            .iter()
            .any(|d| std::path::Path::new(&format!("{d}/lib{lib}.so")).exists())
        {
            continue;
        }
        let Some(target) = dirs.iter().find_map(|d| {
            let mut found: Vec<_> = std::fs::read_dir(d)
                .ok()?
                .filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| {
                    p.file_name()
                        .and_then(|n| n.to_str())
                        .is_some_and(|n| n.starts_with(&format!("lib{lib}.so.")))
                })
                .collect();
            found.sort();
            found.into_iter().next()
        }) else {
            continue;
        };
        let link = out.join(format!("lib{lib}.so"));
        let _ = std::fs::remove_file(&link);
        #[cfg(unix)]
        let _ = std::os::unix::fs::symlink(target, &link);
    }
    println!("cargo:rustc-link-search=native={}", out.display());
}
