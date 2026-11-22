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
    /// The app's folder the core was booted over.
    private(set) var folder: URL?
    /// How long the boot took, for the log and the tests.
    private(set) var bootDuration: Duration?
    /// What the core reports from its pump thread, delivered on the main actor.
    @ObservationIgnored var onChange: ((Changed) -> Void)?
    /// The boot, once begun: every `start()` waits for it (the background downloads' launch
    /// and the scene can both start the core).
    @ObservationIgnored private var boot: Task<Void, Never>?
    @ObservationIgnored private var events: CoreEvents?
    @ObservationIgnored private let appFolder: @Sendable () throws -> URL
    @ObservationIgnored private let studySet: String?
    @ObservationIgnored private let downloads: DownloadCenter

    private static let log = Logger(subsystem: "io.github.inhesrom.openbible", category: "boot")

    var core: MobileApp? {
        if case .ready(let core) = state { return core }
        return nil
    }

    /// `appFolder` is the folder to boot over, `studySet` the debug Study set override and
    /// `downloads` the background transfers the core is attached to (a test's own).
    init(
        appFolder: @escaping @Sendable () throws -> URL = { try AppCore.appFolder() },
        studySet: String? = AppCore.debugStudySet(),
        downloads: DownloadCenter = .shared
    ) {
        self.appFolder = appFolder
        self.studySet = studySet
        self.downloads = downloads
    }

    /// Boot the core, once per process; a call while it boots waits for that boot to end.
    func start() async {
        if boot == nil { boot = Task { await self.run() } }
        await boot?.value
    }

    private func run() async {
        let folder: URL
        do { folder = try appFolder() } catch {
            state = .failed(message: "\(error)")
            downloads.coreFailed()
            return
        }
        let studySet = studySet
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
            // The Library's transfers: small files through the shell's transport, `.obr`
            // and Open catalogue files through the background downloads.
            try? core.setTransport(transport: HTTPTransport())
            #if DEBUG
                // Test harness: a local Starter library mirror instead of the release.
                if let mirror = ProcessInfo.processInfo.environment["OBS_STARTER_MIRROR"] {
                    try? core.setStarterSource(base: mirror, releasesApi: nil)
                }
            #endif
            downloads.attach(core: core, folder: folder)
            self.folder = folder
            state = .ready(core)
        } catch {
            state = .failed(message: Self.message(error))
            downloads.coreFailed()
        }
    }

    /// The debug Study set override (plan S07): a Starter library folder on the Mac.
    nonisolated static func debugStudySet() -> String? {
        #if DEBUG
            return ProcessInfo.processInfo.environment["OBS_STUDY_SET_DIR"]
        #else
            return nil
        #endif
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

    /// The folder the core boots over: `locateAppFolder()`, made afresh for a UI test that
    /// asks for a folder of its own (Debug).
    nonisolated static func appFolder() throws -> URL {
        let folder = try locateAppFolder(create: true)
        #if DEBUG
            // Test harness: UI tests that add to the library (ImportTests) boot in a folder of
            // their own, made afresh, so the app's own library stays as other tests expect it.
            if ProcessInfo.processInfo.environment["OBS_TEST_APP_FOLDER"] != nil {
                try? FileManager.default.removeItem(at: folder)
            }
        #endif
        return folder
    }

    /// The app's own folder for the core, Application Support/OpenBible (a test's own under
    /// `OBS_TEST_APP_FOLDER`, Debug), worked out without changing anything in it: the
    /// background downloads need it before the core is up, after a relaunch.
    nonisolated static func locateAppFolder(create: Bool = false) throws -> URL {
        let support = try FileManager.default.url(
            for: .applicationSupportDirectory, in: .userDomainMask, appropriateFor: nil,
            create: create)
        #if DEBUG
            if let name = ProcessInfo.processInfo.environment["OBS_TEST_APP_FOLDER"] {
                return support.appendingPathComponent(name, isDirectory: true)
            }
        #endif
        return support.appendingPathComponent("OpenBible", isDirectory: true)
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
