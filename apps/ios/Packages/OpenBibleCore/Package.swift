// swift-tools-version: 6.0
// The Mobile app's Rust core for Swift (docs/dev/ios-project.md): the obs-ffi XCFramework
// and the Swift bindings UniFFI generates for it. `cargo xtask ios build` writes both here
// (gitignored); this file is the only one committed.

import PackageDescription

let package = Package(
    name: "OpenBibleCore",
    platforms: [.iOS(.v17)],
    products: [
        .library(name: "OpenBibleCore", targets: ["OpenBibleCore"])
    ],
    targets: [
        // libobs_ffi.a per slice, with the C module ObsFFIFFI (header + module.modulemap).
        .binaryTarget(name: "ObsFFI", path: "ObsFFI.xcframework"),
        // ObsFFI.swift, which imports ObsFFIFFI.
        .target(name: "OpenBibleCore", dependencies: ["ObsFFI"], path: "Sources/Generated"),
    ]
)
