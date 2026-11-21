import Foundation
import Observation
import OpenBibleCore
import os

/// What the S06 hello screen shows: the engine's version and, once the app has booted over
/// the Bundled set, the Resources in its library. S07 and S13 replace it with the reader.
@MainActor
@Observable
final class HelloModel {
    enum State: Equatable {
        case starting
        case ready(resourceIds: [String], problems: [String])
        case failed(message: String)
    }

    let engineVersion = OpenBibleCore.engineVersion()
    private(set) var state: State = .starting
    /// How long the last `start` took to boot, for the log and the tests.
    private(set) var bootDuration: Duration?
    private var app: MobileApp?

    private static let log = Logger(subsystem: "io.github.inhesrom.openbible", category: "boot")

    /// Boot the Rust core in the app's folder over the Bundled set inside the app.
    func start() async {
        do {
            await start(folder: try Self.appFolder(), bundle: Self.bundledSet())
        } catch {
            state = .failed(message: "\(error)")
        }
    }

    /// Boot the Rust core over `folder` (the app's files) and `bundle` (the Bundled set),
    /// off the main thread.
    func start(folder: URL, bundle: URL?) async {
        state = .starting
        let paths = Self.paths(in: folder)
        let bundlePath = bundle?.path
        let clock = ContinuousClock()
        let began = clock.now
        do {
            let app = try await Task.detached(priority: .userInitiated) {
                try MobileApp.boot(paths: paths, bundle: bundlePath)
            }.value
            let took = clock.now - began
            bootDuration = took
            Self.log.info("booted in \(took.formatted(.units(allowed: [.milliseconds])), privacy: .public)")
            self.app = app
            state = .ready(resourceIds: app.resourceIds(), problems: app.problems())
        } catch {
            state = .failed(message: "\(error)")
        }
    }

    /// The `Paths` the core wants, under one folder. S03 spike: S13 lets the core name them.
    nonisolated static func paths(in folder: URL) -> Paths {
        func at(_ name: String) -> String { folder.appendingPathComponent(name).path }
        return Paths(
            library: at("library"), userDb: at("user.db"), searchCache: at("search"),
            aliasCache: at("aliases"), logs: at("logs"), downloads: at("downloads"))
    }

    /// The app's own folder for the core: Application Support/OpenBible.
    nonisolated static func appFolder() throws -> URL {
        try FileManager.default.url(
            for: .applicationSupportDirectory, in: .userDomainMask, appropriateFor: nil,
            create: true
        ).appendingPathComponent("OpenBible", isDirectory: true)
    }

    /// The Bundled set inside the app (`project.yml` copies it as `starter`).
    nonisolated static func bundledSet() -> URL? {
        Bundle.main.url(forResource: "starter", withExtension: nil)
    }
}
