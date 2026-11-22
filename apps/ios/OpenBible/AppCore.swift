import Foundation
import Observation
import OpenBibleCore
import os

/// The one Rust core of the process (`docs/dev/mobile-core.md`): booted once, over the app's
/// folder and the Bundled set inside the app, and shared by every scene (an iPad can open
/// several windows). The core names every file under the folder; Swift only marks the
/// folders the core lists as excluded from device backups.
@MainActor
@Observable
final class AppCore {
    enum State {
        case starting
        case ready(MobileApp)
        case failed(message: String)
    }

    static let shared = AppCore()

    let engineVersion = OpenBibleCore.engineVersion()
    private(set) var state: State = .starting
    /// How long the boot took, for the log and the tests.
    private(set) var bootDuration: Duration?
    /// What the core reports from its pump thread, delivered on the main actor.
    @ObservationIgnored var onChange: ((Changed) -> Void)?
    @ObservationIgnored private var started = false
    @ObservationIgnored private var events: CoreEvents?

    private static let log = Logger(subsystem: "io.github.inhesrom.openbible", category: "boot")

    var core: MobileApp? {
        if case .ready(let core) = state { return core }
        return nil
    }

    /// Boot the core, once per process; later calls wait for nothing and return.
    func start() async {
        guard !started else { return }
        started = true
        let folder: URL
        do { folder = try Self.appFolder() } catch {
            state = .failed(message: "\(error)")
            return
        }
        var studySet: String?
        #if DEBUG
            // The debug Study set override (plan S07): a Starter library folder on the Mac.
            studySet = ProcessInfo.processInfo.environment["OBS_STUDY_SET_DIR"]
        #endif
        let bundle = Self.bundledSet()?.path
        let clock = ContinuousClock()
        let began = clock.now
        do {
            let core = try await Task.detached(priority: .userInitiated) { [studySet] in
                try Self.boot(folder: folder, bundle: bundle, studySet: studySet)
            }.value
            let took = clock.now - began
            bootDuration = took
            Self.log.info("booted in \(took.formatted(.units(allowed: [.milliseconds])), privacy: .public)")
            for problem in (try? core.problems()) ?? [] {
                Self.log.error("boot: \(problem, privacy: .public)")
            }
            let events = CoreEvents { [weak self] change in
                Task { @MainActor in self?.onChange?(change) }
            }
            try? core.setObserver(observer: events)
            self.events = events
            state = .ready(core)
        } catch {
            state = .failed(message: Self.message(error))
        }
    }

    /// Boot a core in `folder` over `bundle`, and keep the folders it lists out of device
    /// backups. Off the main thread: it reads the library and, the first time, copies the
    /// Bundled set.
    nonisolated static func boot(folder: URL, bundle: String?, studySet: String?) throws -> MobileApp {
        let core = try MobileApp.boot(root: folder.path, bundle: bundle, studySet: studySet)
        for path in try core.backupExcluded() {
            var url = URL(fileURLWithPath: path, isDirectory: true)
            var values = URLResourceValues()
            values.isExcludedFromBackup = true
            try? url.setResourceValues(values)
        }
        return core
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

    /// A core error's words.
    nonisolated static func message(_ error: Error) -> String {
        switch error {
        case MobileError.Reference(let message), MobileError.Stopped(let message),
            MobileError.Failed(let message):
            return message
        default:
            return "\(error)"
        }
    }

    /// The app went to the background: the core saves what would be lost.
    func background() {
        do { try core?.onBackground() } catch {
            Self.log.error("background: \(Self.message(error), privacy: .public)")
        }
    }

    /// The app came back to the foreground.
    func resume() {
        try? core?.onResume()
    }
}

/// The core's observer: called on its pump thread, it hands each change on.
final class CoreEvents: Observer, @unchecked Sendable {
    private let deliver: @Sendable (Changed) -> Void

    init(_ deliver: @escaping @Sendable (Changed) -> Void) {
        self.deliver = deliver
    }

    func changed(change: Changed) {
        deliver(change)
    }
}
