import BackgroundTasks
import Foundation
import OpenBibleCore
import os

/// The parts of a `BGTask` a background run uses, so a test can hand in a stand-in.
protocol BackgroundTaskHandle: AnyObject {
    var expirationHandler: (() -> Void)? { get set }
    func setTaskCompleted(success: Bool)
}

extension BGTask: BackgroundTaskHandle {}

/// The background processing task (#333 "Storage", S25): the Search indexes and alias caches
/// the device builds after a download or an Import carry on when the app is closed. The core
/// says whether there is work and what the task asks the system for
/// (`pendingBackgroundWork`), and steps the work under its own deadline (`stepBackground`);
/// Swift registers the task, submits the core's request, steps until the core says the work
/// is done or the system's expiration handler fires, and completes the task.
final class BackgroundIndexer: @unchecked Sendable {
    static let shared = BackgroundIndexer()

    /// The task's identifier, `<bundle id>.index`, as `project.yml` permits it
    /// (`BGTaskSchedulerPermittedIdentifiers`).
    static var identifier: String { (Bundle.main.bundleIdentifier ?? "io.github.inhesrom.openbible") + ".index" }

    private static let log = Logger(subsystem: "io.github.inhesrom.openbible", category: "background")

    /// What the last submission did, for the log and the tests: nil until one was made.
    let lastSubmission = OSAllocatedUnfairLock<String?>(initialState: nil)

    var lastSubmissionResult: String? { lastSubmission.withLock { $0 } }

    /// Register the launch handler. UIKit wants it before the app finishes launching (the
    /// app delegate's `didFinishLaunching`), once.
    func register() {
        let registered = BGTaskScheduler.shared.register(forTaskWithIdentifier: Self.identifier, using: nil) {
            task in
            BackgroundIndexer.shared.launched(task) {
                await AppCore.shared.start()
                return AppCore.shared.core
            }
        }
        if !registered {
            Self.log.error("\(Self.identifier, privacy: .public) is not in BGTaskSchedulerPermittedIdentifiers")
        }
    }

    /// The system launched (or woke) the app for the task: watch for its expiration at once,
    /// since the boot can take seconds and a task without a handler is ended unsuccessfully
    /// (BGTask.h), then boot the core if it is not up (`boot`) and run.
    @discardableResult
    func launched(
        _ task: some BackgroundTaskHandle,
        boot: @escaping @MainActor @Sendable () async -> MobileApp?
    ) -> Task<Void, Never> {
        let expired = Self.watchExpiration(of: task)
        nonisolated(unsafe) let task = task
        return Task { @MainActor in
            guard let core = await boot() else {
                task.setTaskCompleted(success: false)
                return
            }
            await self.run(task, core: core, expired: expired)
        }
    }

    /// Set the task's expiration handler: the flag it returns turns true when the system
    /// takes the time back.
    static func watchExpiration(of task: some BackgroundTaskHandle) -> OSAllocatedUnfairLock<Bool> {
        let expired = OSAllocatedUnfairLock(initialState: false)
        task.expirationHandler = { expired.withLock { $0 = true } }
        return expired
    }

    /// Ask the system for the task while the core has work for it (the app is going to the
    /// background, or a run ended with work left). Nothing is asked when it has none.
    func schedule(_ core: MobileApp) {
        let request: BackgroundRequest?
        do { request = try core.pendingBackgroundWork() } catch {
            Self.log.error("pending work: \(AppCore.message(error), privacy: .public)")
            return
        }
        guard let request else {
            lastSubmission.withLock { $0 = "no work" }
            Self.log.info("background task: no work")
            return
        }
        let task = BGProcessingTaskRequest(identifier: Self.identifier)
        task.requiresExternalPower = request.requiresExternalPower
        task.requiresNetworkConnectivity = request.requiresNetwork
        let outcome: String
        do {
            try BGTaskScheduler.shared.submit(task)
            outcome = "submitted"
        } catch {
            // The simulator answers `unavailable` (BGTaskScheduler.h); the work then carries
            // on the next time the app runs.
            outcome = "not submitted: \(error)"
        }
        lastSubmission.withLock { $0 = outcome }
        Self.log.info("background task \(Self.identifier, privacy: .public): \(outcome, privacy: .public)")
    }

    /// Step the core's work until it says none is left or the task expires (`expired`, from
    /// `watchExpiration`), then complete the task (successful when nothing is left) and ask
    /// for another while work is left. Each step is the core's (`stepBackground`, half a
    /// second at most), so an expiration stops the run within one step, and one before the
    /// run stops it before its first.
    func run(_ task: some BackgroundTaskHandle, core: MobileApp, expired: OSAllocatedUnfairLock<Bool>) async {
        let pending = await Task.detached(priority: .utility) {
            var pending = true
            while pending, !expired.withLock({ $0 }) {
                do { pending = try core.stepBackground() } catch {
                    Self.log.error("background step: \(AppCore.message(error), privacy: .public)")
                    break
                }
            }
            return pending
        }.value
        if pending { schedule(core) }
        Self.log.info("background run ended, work left: \(pending, privacy: .public)")
        task.setTaskCompleted(success: !pending)
    }
}
