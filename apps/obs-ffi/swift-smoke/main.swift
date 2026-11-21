// The "Hello engine" Swift smoke (S03 of docs/plans/mobile-ios.md), built and run by
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
let root = URL(fileURLWithPath: args[2], isDirectory: true)
func at(_ name: String) -> String { root.appendingPathComponent(name).path }

print("engine \(engineVersion())")
do {
    let paths = Paths(
        library: at("library"), userDb: at("user.db"), searchCache: at("search"),
        aliasCache: at("aliases"), logs: at("logs"), downloads: at("downloads"))
    let app = try MobileApp.boot(paths: paths, bundle: bundle)
    for problem in app.problems() {
        print("problem: \(problem)")
    }
    print("resources \(app.resourceIds().joined(separator: " "))")
    let reader = try app.openReader(resourceId: "bsb", reference: "Gen 1")
    let tile = try app.layoutTile(reader: reader, top: 0, width: 390, height: 256, scale: 3)
    guard tile.rgba.count == Int(tile.width) * Int(tile.height) * 4 else {
        print("tile has \(tile.rgba.count) bytes for \(tile.width)x\(tile.height)")
        exit(1)
    }
    print("tile \(tile.width)x\(tile.height)")
} catch {
    print("failed: \(error)")
    exit(1)
}
