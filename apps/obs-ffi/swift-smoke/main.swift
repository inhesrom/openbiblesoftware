// The Swift smoke of the FFI surface (S03, S13 of docs/plans/mobile-ios.md), built and run by
// `cargo xtask ios smoke`: Swift → UniFFI → obs-mobile → engine → layout → tile bytes, on the
// macOS host. Compiled together with the generated target/ios/bindings/ObsFFI.swift.
//
// Usage: swift-smoke <Bundled set folder> <empty folder for the app's files>

import Foundation

let args = CommandLine.arguments
guard args.count == 3 else {
    FileHandle.standardError.write("usage: swift-smoke <bundle> <app folder>\n".data(using: .utf8)!)
    exit(2)
}
let bundle = args[1]
let root = args[2]

print("engine \(engineVersion())")
do {
    let app = try MobileApp.boot(root: root, bundle: bundle, studySet: nil)
    for problem in try app.problems() {
        print("problem: \(problem)")
    }
    print("resources \(try app.library().map(\.resourceId).joined(separator: " "))")
    let reader = try app.navigate(reader: 0, text: "Gen 1")
    print("reader \(reader.resourceId) at \(reader.location)")
    // The largest Text size, so Genesis 1 has blocks taller than a whole 256 pt band.
    _ = try app.dynamicType(category: "UICTContentSizeCategoryAccessibilityXXXL")
    let viewport = Viewport(width: 390, height: 700, scale: 3)
    let frame = try app.frame(reader: 0, offset: 0, viewport: viewport, appearance: SystemAppearance(dark: false))
    guard let band = frame.tiles.first(where: { $0.height == 256 }),
        let timed = try app.tile(reader: 0, key: band.key, scale: 3)
    else {
        print("no full band in \(frame.tiles.count) tiles")
        exit(1)
    }
    let tile = timed.tile
    guard tile.rgba.count == Int(tile.width) * Int(tile.height) * 4 else {
        print("tile has \(tile.rgba.count) bytes for \(tile.width)x\(tile.height)")
        exit(1)
    }
    print("tile \(tile.width)x\(tile.height)")
} catch {
    print("failed: \(error)")
    exit(1)
}
