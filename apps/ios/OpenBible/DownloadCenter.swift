import Foundation
import OpenBibleCore
import os

/// The background transfers of `.obr` and Open catalogue files (#333 "How downloads behave";
/// `docs/dev/mobile-library.md`). The core plans each download (the URL, the file in its
/// `downloads/` folder, whether it may use cellular data) and owns every rule and state;
/// this only moves bytes: a `URLSessionConfiguration.background` download task per request,
/// carried on by the system while the app is suspended or closed, its progress handed to
/// the core, the finished file moved to the path the core named and handed in with
/// `installDownloaded` (the core checks its sha256 and adds it), a failure handed in with
/// `downloadFailed`. Resume data the system leaves after a dropped transfer is kept beside
/// the core's file (`<file>.resume`), so Retry carries on where it stopped; a resumed
/// transfer that cannot be used (an expired redirect, a changed file) starts afresh once.
/// Cancel stops the transfer and deletes its resume data; the core deletes its file.
///
/// Two sessions, one that may use cellular data and one that may not, so a Wi-Fi-only
/// transfer waits for Wi-Fi whichever of the session's and the request's
/// `allowsCellularAccess` the system honours. A transfer that finishes while the app is not
/// running relaunches it in the background (`handleEventsForBackgroundURLSession`): the app
/// folder is known without the core (`AppCore.locateAppFolder`), so a finished file goes
/// straight to the core's path and resume data is kept before the core is up, and the
/// system's completion handler waits until every file that came in has been handed to the
/// core (`attach`), so a suspension or termination right after it loses none.
///
/// Release builds transfer `https` only (#333; the core's URLs are `https`).
final class DownloadCenter: NSObject, URLSessionDownloadDelegate, @unchecked Sendable {
    /// What a task carries in its `taskDescription`, so it is known again after a relaunch.
    struct Item: Codable, Equatable {
        var id: String
        var origin: String
        /// The core's file, relative to the app folder (the container's path can change
        /// between launches).
        var file: String
        var url: String
        /// Started from resume data.
        var resumed: Bool
    }

    static let shared = DownloadCenter(
        identifier: (Bundle.main.bundleIdentifier ?? "openbible") + ".downloads",
        folder: try? AppCore.locateAppFolder())

    /// The two sessions' identifiers: `<identifier>.wifi` and `<identifier>.cellular`.
    let identifier: String
    private let lock = NSLock()
    /// Held while a finished file is handed to the core and while Cancel tells the core, so
    /// one of them sees the other (a Cancel tapped as a transfer finishes installs nothing).
    private let handOff = NSLock()
    private var core: MobileApp?
    private var folder: URL?
    private var wifi: URLSession!
    private var cellular: URLSession!
    /// Running transfers by the core's (relative) file.
    private var tasks: [String: URLSessionDownloadTask] = [:]
    /// The core's (relative) file of each download, by `origin/id`.
    private var files: [String: String] = [:]
    /// Tasks Cancel stopped (`session/taskIdentifier`).
    private var cancelled: Set<String> = []
    /// The core's (relative) files whose download Cancel stopped, until a transfer of them
    /// begins again.
    private var cancelledFiles: Set<String> = []
    /// Tasks whose outcome was handed to the core already.
    private var handled: Set<String> = []
    private var lastProgress: [String: Date] = [:]
    /// Finished files and failures that came in before the core was ready.
    private var waiting: [() -> Void] = []
    /// The system's completion handlers for background events, by session identifier.
    private var completions: [String: @Sendable () -> Void] = [:]
    /// Completion handlers whose events left something in `waiting`: called once it has been
    /// handed to the core.
    private var held: [@Sendable () -> Void] = []

    private static let log = Logger(subsystem: "io.github.inhesrom.openbible", category: "downloads")
    /// How often progress reaches the core, per transfer.
    private static let progressInterval: TimeInterval = 0.25

    /// `folder`: the app folder the core will boot over, when it is known before the core.
    init(identifier: String, folder: URL? = nil) {
        self.identifier = identifier
        self.folder = folder
        super.init()
        let queue = OperationQueue()
        queue.maxConcurrentOperationCount = 1
        queue.name = identifier
        wifi = Self.session(identifier + ".wifi", cellular: false, delegate: self, queue: queue)
        cellular = Self.session(identifier + ".cellular", cellular: true, delegate: self, queue: queue)
        for session in [wifi!, cellular!] {
            session.getAllTasks { [weak self] tasks in
                self?.reconnect(tasks.compactMap { $0 as? URLSessionDownloadTask })
            }
        }
    }

    private static func session(
        _ id: String, cellular: Bool, delegate: URLSessionDelegate, queue: OperationQueue
    ) -> URLSession {
        let config = URLSessionConfiguration.background(withIdentifier: id)
        config.allowsCellularAccess = cellular
        config.sessionSendsLaunchEvents = true
        config.isDiscretionary = false
        config.httpCookieStorage = nil
        config.httpShouldSetCookies = false
        config.urlCredentialStorage = nil
        config.urlCache = nil
        config.httpAdditionalHeaders = ["User-Agent": "OpenBible/\(OpenBibleCore.engineVersion())"]
        return URLSession(configuration: config, delegate: delegate, delegateQueue: queue)
    }

    /// The core is ready, over `folder`: what came in before it is handed in now, then the
    /// system is told the events it woke the app for are handled.
    func attach(core: MobileApp, folder: URL) {
        let (pending, release): ([() -> Void], [@Sendable () -> Void]) = locked {
            self.core = core
            self.folder = folder
            defer {
                waiting = []
                held = []
            }
            return (waiting, held)
        }
        for hand in pending { hand() }
        for completion in release { DispatchQueue.main.async { completion() } }
    }

    /// The core could not boot: the system is not kept waiting for it.
    func coreFailed() {
        let release: [@Sendable () -> Void] = locked {
            defer { held = [] }
            return held
        }
        for completion in release { DispatchQueue.main.async { completion() } }
    }

    /// The system woke the app for session `identifier`'s events.
    func handleEvents(for identifier: String, completion: @escaping @Sendable () -> Void) -> Bool {
        guard identifier.hasPrefix(self.identifier + ".") else { return false }
        locked { completions[identifier] = completion }
        return true
    }

    // MARK: Starting and cancelling

    /// Start the transfers the core planned (`DownloadPlan.start`). One whose transfer is
    /// running already (after a relaunch the core had forgotten it) carries on.
    func start(_ requests: [DownloadRequest]) {
        for request in requests { start(request) }
    }

    private func start(_ request: DownloadRequest) {
        let origin = Self.origin(request.origin)
        guard let file = relative(request.file) else {
            // The core queued it: it fails there, so its row is not left waiting.
            Self.log.error("download: \(request.file, privacy: .public) is outside the app folder")
            let item = Item(id: request.resourceId, origin: origin, file: request.file, url: request.url, resumed: false)
            fail(item, message: "\(request.file) is outside the app's folder")
            return
        }
        let item = Item(id: request.resourceId, origin: origin, file: file, url: request.url, resumed: false)
        let stale: URLSessionDownloadTask? = locked {
            files["\(origin)/\(request.resourceId)"] = file
            return tasks[file].flatMap { $0.state == .running || $0.state == .suspended ? $0 : nil }
        }
        if let stale {
            // The same file from the same URL: it carries on. From another (a new release, a
            // mirror), it is replaced.
            if Self.decode(stale)?.url == request.url { return }
            locked { _ = cancelled.insert(Self.key(stale)) }
            stale.cancel()
        }
        begin(item, cellular: request.cellular)
    }

    /// Start `item`'s transfer: from its resume data when it has some (unless `afresh`),
    /// else from the core's URL.
    private func begin(_ item: Item, cellular: Bool, afresh: Bool = false) {
        guard let url = URL(string: item.url) else {
            fail(item, message: "\(item.url): not a URL")
            return
        }
        #if !DEBUG
            // Release builds: https only (Debug tests use a local http mirror).
            guard url.scheme?.lowercased() == "https" else {
                fail(item, message: "\(item.url): unsupported protocol \"\(url.scheme ?? "")\"")
                return
            }
        #endif
        let session = cellular ? self.cellular! : wifi!
        var item = item
        let task: URLSessionDownloadTask
        if !afresh, let resumeFile = path(item.file + ".resume"), let data = try? Data(contentsOf: resumeFile) {
            item.resumed = true
            task = session.downloadTask(withResumeData: data)
        } else {
            item.resumed = false
            var request = URLRequest(url: url)
            request.allowsCellularAccess = cellular
            task = session.downloadTask(with: request)
        }
        task.taskDescription = Self.encode(item)
        locked {
            tasks[item.file] = task
            cancelledFiles.remove(item.file)
        }
        task.resume()
    }

    /// Cancel: stop the transfer of `resourceId` and delete its resume data; the core forgets
    /// the download and deletes its file.
    func cancel(resourceId: String, origin: DownloadOrigin, core: MobileApp) throws {
        handOff.lock()
        defer { handOff.unlock() }
        let key = "\(Self.origin(origin))/\(resourceId)"
        let (task, file): (URLSessionDownloadTask?, String?) = locked {
            let file = files[key]
            let task = file.flatMap { tasks[$0] }
            if let task { cancelled.insert(Self.key(task)) }
            if let file { cancelledFiles.insert(file) }
            return (task, file)
        }
        task?.cancel()
        if let file, let resume = path(file + ".resume") {
            try? FileManager.default.removeItem(at: resume)
        }
        try core.cancelDownload(resourceId: resourceId, origin: origin)
    }

    // MARK: URLSessionDownloadDelegate

    func urlSession(
        _ session: URLSession, downloadTask: URLSessionDownloadTask, didWriteData bytesWritten: Int64,
        totalBytesWritten: Int64, totalBytesExpectedToWrite: Int64
    ) {
        guard let item = Self.decode(downloadTask) else { return }
        let key = Self.key(downloadTask)
        let now = Date()
        let due: Bool = locked {
            // The last update always goes; with no length known, none is the last.
            if let last = lastProgress[key], now.timeIntervalSince(last) < Self.progressInterval,
                totalBytesExpectedToWrite <= 0 || totalBytesWritten < totalBytesExpectedToWrite
            {
                return false
            }
            lastProgress[key] = now
            return true
        }
        guard due else { return }
        let total = UInt64(max(totalBytesExpectedToWrite, 0))
        let done = UInt64(max(totalBytesWritten, 0))
        withCore { core in
            // A transfer the core does not know (from before a relaunch) is adopted.
            try? core.downloadProgress(
                resourceId: item.id, origin: Self.origin(item.origin), done: done, total: total)
        }
    }

    func urlSession(
        _ session: URLSession, downloadTask: URLSessionDownloadTask, didFinishDownloadingTo location: URL
    ) {
        guard let item = Self.decode(downloadTask) else { return }
        let key = Self.key(downloadTask)
        locked { _ = handled.insert(key) }
        if let refused = HTTPStatus.check(url: item.url, response: downloadTask.response) {
            if item.resumed {
                restart(item, session: session, why: HTTPStatus.message(refused))
            } else {
                fail(item, message: HTTPStatus.message(refused))
            }
            return
        }
        // The system deletes `location` when this returns: move it now, to the core's path
        // (known before the core is up, after a relaunch).
        guard let destination = path(item.file) else {
            fail(item, message: "\(item.url): the app's folder is not known")
            return
        }
        let files = FileManager.default
        try? files.removeItem(at: destination.appendingPathExtension("resume"))
        do {
            try files.createDirectory(at: destination.deletingLastPathComponent(), withIntermediateDirectories: true)
            try? files.removeItem(at: destination)
            try files.moveItem(at: location, to: destination)
        } catch {
            fail(item, message: HTTPStatus.message(HTTPStatus.failure(url: item.url, error)))
            return
        }
        withCore { [self] core in install(item, at: destination, core: core) }
    }

    /// Hand the finished file of `item` to the core, unless Cancel stopped its download.
    private func install(_ item: Item, at destination: URL, core: MobileApp) {
        handOff.lock()
        defer { handOff.unlock() }
        if locked({ cancelledFiles.contains(item.file) }) {
            try? FileManager.default.removeItem(at: destination)
            return
        }
        do {
            _ = try core.installDownloaded(
                resourceId: item.id, path: destination.path, origin: Self.origin(item.origin))
        } catch {
            Self.log.error("install \(item.id, privacy: .public): \(AppCore.message(error), privacy: .public)")
        }
    }

    func urlSession(_ session: URLSession, task: URLSessionTask, didCompleteWithError error: Error?) {
        guard let download = task as? URLSessionDownloadTask, let item = Self.decode(download) else { return }
        let key = Self.key(download)
        let (wasCancelled, wasHandled): (Bool, Bool) = locked {
            if tasks[item.file] === download { tasks[item.file] = nil }
            lastProgress[key] = nil
            return (cancelled.remove(key) != nil, handled.remove(key) != nil)
        }
        guard let error, !wasCancelled, !wasHandled else { return }
        let nsError = error as NSError
        if let data = nsError.userInfo[NSURLSessionDownloadTaskResumeData] as? Data,
            let resume = path(item.file + ".resume")
        {
            try? data.write(to: resume, options: .atomic)
        } else if item.resumed {
            restart(item, session: session, why: error.localizedDescription)
            return
        }
        fail(item, message: HTTPStatus.message(HTTPStatus.failure(url: item.url, error)))
    }

    func urlSessionDidFinishEvents(forBackgroundURLSession session: URLSession) {
        finishedEvents(for: session.configuration.identifier ?? "")
    }

    /// Session `identifier`'s events are all delivered: the system's completion handler is
    /// called (it may suspend the app then), or, while what they brought waits for the core,
    /// held until `attach` has handed it in.
    func finishedEvents(for identifier: String) {
        let completion: (@Sendable () -> Void)? = locked {
            guard let completion = completions.removeValue(forKey: identifier) else { return nil }
            if core == nil && !waiting.isEmpty {
                held.append(completion)
                return nil
            }
            return completion
        }
        if let completion { DispatchQueue.main.async { completion() } }
    }

    // MARK: Helpers

    /// A resumed transfer that could not be used: its resume data goes, and it starts afresh
    /// from the core's URL, once (the fresh one is not a resumed one).
    private func restart(_ item: Item, session: URLSession, why: String) {
        Self.log.info("resume of \(item.id, privacy: .public) unusable (\(why, privacy: .public)): starting afresh")
        if let resume = path(item.file + ".resume") { try? FileManager.default.removeItem(at: resume) }
        begin(item, cellular: session === cellular, afresh: true)
    }

    private func fail(_ item: Item, message: String) {
        withCore { core in
            try? core.downloadFailed(resourceId: item.id, origin: Self.origin(item.origin), message: message)
        }
    }

    /// Run `body` with the core now, or once it is ready.
    private func withCore(_ body: @escaping (MobileApp) -> Void) {
        let core: MobileApp? = locked {
            if core == nil { waiting.append { [weak self] in self?.withCore(body) } }
            return core
        }
        if let core { body(core) }
    }

    /// Transfers that were running before a relaunch: known again by their files.
    private func reconnect(_ found: [URLSessionDownloadTask]) {
        #if DEBUG
            // Test harness: a launch in a fresh test folder (`OBS_TEST_APP_FOLDER`) has no
            // download from before; an earlier test's transfers are stopped.
            if ProcessInfo.processInfo.environment["OBS_TEST_APP_FOLDER"] != nil {
                locked { for task in found { cancelled.insert(Self.key(task)) } }
                for task in found { task.cancel() }
                return
            }
        #endif
        locked {
            for task in found {
                guard let item = Self.decode(task), task.state == .running || task.state == .suspended else { continue }
                tasks[item.file] = task
                files["\(item.origin)/\(item.id)"] = item.file
            }
        }
    }

    /// `path` relative to the app folder, when it is in it.
    func relative(_ path: String) -> String? {
        guard let folder = locked({ folder }) else { return nil }
        let root = folder.standardizedFileURL.path + "/"
        let file = URL(fileURLWithPath: path).standardizedFileURL.path
        return file.hasPrefix(root) ? String(file.dropFirst(root.count)) : nil
    }

    private func path(_ relative: String) -> URL? {
        locked({ folder })?.appendingPathComponent(relative)
    }

    private static func key(_ task: URLSessionTask) -> String {
        "\(task.taskIdentifier)/\(task.taskDescription ?? "")"
    }

    static func origin(_ origin: DownloadOrigin) -> String {
        switch origin {
        case .starter: return "starter"
        case .open: return "open"
        }
    }

    static func origin(_ name: String) -> DownloadOrigin {
        name == "open" ? .open : .starter
    }

    static func encode(_ item: Item) -> String {
        (try? JSONEncoder().encode(item)).flatMap { String(data: $0, encoding: .utf8) } ?? ""
    }

    private static func decode(_ task: URLSessionTask) -> Item? {
        task.taskDescription.flatMap { $0.data(using: .utf8) }.flatMap { try? JSONDecoder().decode(Item.self, from: $0) }
    }

    private func locked<T>(_ body: () -> T) -> T {
        lock.lock()
        defer { lock.unlock() }
        return body()
    }
}
