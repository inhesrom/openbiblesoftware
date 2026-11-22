import OpenBibleCore
import XCTest

@testable import OpenBible

/// The background processing task (S25, #333 "Storage") run in process with a stand-in task:
/// the simulator refuses `BGTaskScheduler` (`submit` fails with `BGTaskScheduler.Error`
/// `unavailable`, "BGTaskScheduler is not available on this platform", and lldb's
/// `_simulateLaunchForTaskWithIdentifier:` finds no scheduled request), so the system's launch
/// is checked on a device (`docs/dev/ios-project.md`, "Background task"). Here a core over the
/// Bundled set plus Resources without Search indexes (copied from `dist/starter`,
/// `TEST_RUNNER_OBS_STUDY_SET_DIR`, without its prebuilt indexes) has index builds pending;
/// the run steps them until the core says none is left and completes the task, or stops at
/// the expiration, completes it unsuccessfully and asks for another, and the next launch has
/// the work again.
@MainActor
final class BackgroundIndexerTests: XCTestCase {
    private var folder: URL!

    /// A `BGTask` stand-in: records each completion, and can expire as soon as the run sets
    /// its handler.
    final class StandInTask: BackgroundTaskHandle, @unchecked Sendable {
        let expireAtOnce: Bool
        var completions: [Bool] = []
        var expirationHandler: (() -> Void)? {
            didSet { if expireAtOnce { expirationHandler?() } }
        }

        init(expireAtOnce: Bool = false) {
            self.expireAtOnce = expireAtOnce
        }

        func setTaskCompleted(success: Bool) {
            completions.append(success)
        }
    }

    override func setUp() async throws {
        folder = FileManager.default.temporaryDirectory
            .appendingPathComponent("BackgroundIndexerTests-\(UUID().uuidString)", isDirectory: true)
    }

    override func tearDown() async throws {
        try? FileManager.default.removeItem(at: folder)
    }

    /// The core's library folder holding the Starter library Resources outside the Bundled set,
    /// as downloads leave them before their indexes are built.
    private func placeUnindexedResources() throws -> [String] {
        guard let dir = ProcessInfo.processInfo.environment["OBS_STUDY_SET_DIR"] else {
            throw XCTSkip("no Starter library to copy: set TEST_RUNNER_OBS_STUDY_SET_DIR")
        }
        let starter = URL(fileURLWithPath: dir, isDirectory: true)
        let library = folder.appendingPathComponent("library", isDirectory: true)
        try FileManager.default.createDirectory(at: library, withIntermediateDirectories: true)
        let bundled = try FileManager.default.contentsOfDirectory(atPath: try XCTUnwrap(AppCore.bundledSet()).path)
        var ids: [String] = []
        for name in try FileManager.default.contentsOfDirectory(atPath: starter.path)
        where name.hasSuffix(".obr") && !bundled.contains(name) {
            try FileManager.default.copyItem(at: starter.appendingPathComponent(name), to: library.appendingPathComponent(name))
            ids.append(String(name.dropLast(4)))
        }
        return ids
    }

    private func boot() async throws -> MobileApp {
        let bundle = try XCTUnwrap(AppCore.bundledSet()).path
        let folder = folder!
        return try await Task.detached { try AppCore.boot(folder: folder, bundle: bundle, studySet: nil) }.value
    }

    private func labels(_ core: MobileApp, _ ids: [String]) throws -> [String] {
        try core.storage().rows.filter { ids.contains($0.resourceId) }.compactMap { $0.index?.label }
    }

    /// S25's Done when, in process: Storage says the Resources are still being indexed, the
    /// task's run builds them, completes the task successfully and leaves nothing to ask for.
    func testTheRunBuildsTheIndexesAndCompletesTheTask() async throws {
        let ids = try placeUnindexedResources()
        XCTAssertGreaterThan(ids.count, 10)
        let core = try await boot()
        let request = try XCTUnwrap(try core.pendingBackgroundWork(), "no work pending after boot")
        XCTAssertFalse(request.requiresExternalPower)
        XCTAssertFalse(request.requiresNetwork)
        let before = try labels(core, ids)
        XCTAssertTrue(before.contains("Still being indexed"), "\(before)")
        let status = try XCTUnwrap(try core.searchIndexStatus(resourceId: try XCTUnwrap(ids.last)))
        if status.state == .building {
            XCTAssertTrue(status.note?.contains("is still being indexed") == true, status.note ?? "-")
        }

        let task = StandInTask()
        let clock = ContinuousClock()
        let began = clock.now
        await BackgroundIndexer.shared.run(task, core: core, expired: BackgroundIndexer.watchExpiration(of: task))
        print("background run: \(ids.count) indexes in \(clock.now - began)")
        XCTAssertEqual(task.completions, [true])
        XCTAssertNil(try core.pendingBackgroundWork())
        XCTAssertEqual(Set(try labels(core, ids)), ["Search index ready"])
        XCTAssertEqual(try labels(core, ids).count, ids.count)
    }

    /// The system takes the time back at once: the run stops without another step, completes
    /// the task unsuccessfully and asks for another (refused in the simulator); the work is
    /// still pending, and after a relaunch too.
    func testAnExpiredRunStopsAndTheWorkIsPendingAgainAfterARelaunch() async throws {
        _ = try placeUnindexedResources()
        var core: MobileApp? = try await boot()
        XCTAssertNotNil(try core?.pendingBackgroundWork())
        let task = StandInTask(expireAtOnce: true)
        await BackgroundIndexer.shared.run(
            task, core: try XCTUnwrap(core), expired: BackgroundIndexer.watchExpiration(of: task))
        XCTAssertEqual(task.completions, [false])
        XCTAssertNotNil(try core?.pendingBackgroundWork(), "work left after the expiration")
        let submission = try XCTUnwrap(BackgroundIndexer.shared.lastSubmissionResult)
        XCTAssertTrue(submission.hasPrefix("not submitted") || submission == "submitted", submission)
        core = nil

        let again = try await boot()
        XCTAssertNotNil(try again.pendingBackgroundWork(), "pending again at the next launch")
        let done = StandInTask()
        await BackgroundIndexer.shared.run(done, core: again, expired: BackgroundIndexer.watchExpiration(of: done))
        XCTAssertEqual(done.completions, [true])
    }

    /// The system takes the time back while the launch still boots the core: the handler set
    /// at launch catches it, so the run takes no step, completes the task once, unsuccessfully,
    /// and asks for another.
    func testAnExpirationDuringTheBootStopsTheRunBeforeItsFirstStep() async throws {
        let ids = try placeUnindexedResources()
        let task = StandInTask()
        BackgroundIndexer.shared.lastSubmission.withLock { $0 = nil }
        let folder = folder!
        let bundle = try XCTUnwrap(AppCore.bundledSet()).path
        nonisolated(unsafe) var booted: MobileApp?
        let launch = BackgroundIndexer.shared.launched(task) {
            XCTAssertNotNil(task.expirationHandler, "no expiration handler while the core boots")
            let core = try? await Task.detached { try AppCore.boot(folder: folder, bundle: bundle, studySet: nil) }.value
            task.expirationHandler?()
            booted = core
            return core
        }
        await launch.value
        let core = try XCTUnwrap(booted)
        XCTAssertEqual(task.completions, [false])
        XCTAssertNotNil(try core.pendingBackgroundWork(), "work left after the expiration")
        XCTAssertTrue(try labels(core, ids).contains("Still being indexed"))
        let submission = try XCTUnwrap(BackgroundIndexer.shared.lastSubmissionResult, "no new request")
        XCTAssertTrue(submission.hasPrefix("not submitted") || submission == "submitted", submission)
        while try core.stepBackground() {}
    }

    /// The simulator has no background processing: a submission is refused as unavailable,
    /// and the app carries on (the work is done the next time it runs).
    func testTheSimulatorRefusesTheRequest() async throws {
        _ = try placeUnindexedResources()
        let core = try await boot()
        XCTAssertNotNil(try core.pendingBackgroundWork())
        BackgroundIndexer.shared.schedule(core)
        let submission = try XCTUnwrap(BackgroundIndexer.shared.lastSubmissionResult)
        #if targetEnvironment(simulator)
            XCTAssertTrue(submission.contains("BGTaskSchedulerErrorDomain") && submission.contains("Code=1"), submission)
        #else
            XCTAssertEqual(submission, "submitted")
        #endif
        while try core.stepBackground() {}
    }
}
