import OpenBibleCore
import XCTest

@testable import OpenBible

/// The Library's transfers (S24) against a local mirror (`MirrorServer`), never the network:
/// the transport's statuses as `apps/openbible/src/fetch.rs` tells them apart, Cancel
/// deleting what a transfer left, and background resume data across a GitHub-like redirect
/// to a signed URL that expires (`docs/dev/mobile-library.md` § Evidence). The tests that
/// download Starter files serve `dist/starter` (`TEST_RUNNER_OBS_STUDY_SET_DIR`), and pick
/// their Resources from the core's own Study set rows rather than by name.
@MainActor
final class DownloadTests: XCTestCase {
    private var folder: URL!
    private var server: MirrorServer?
    /// The session the delegate calls a test makes itself name (the system's would come
    /// from the center's own sessions).
    private let session = URLSession(configuration: .ephemeral)

    override func setUp() async throws {
        folder = FileManager.default.temporaryDirectory
            .appendingPathComponent("DownloadTests-\(UUID().uuidString)", isDirectory: true)
    }

    override func tearDown() async throws {
        server?.stop()
        try? FileManager.default.removeItem(at: folder)
    }

    private func starterMirror() throws -> URL {
        guard let dir = ProcessInfo.processInfo.environment["OBS_STUDY_SET_DIR"] else {
            throw XCTSkip("no Starter library to serve: set TEST_RUNNER_OBS_STUDY_SET_DIR")
        }
        return URL(fileURLWithPath: dir, isDirectory: true)
    }

    // MARK: The transport

    func testTheTransportTellsStatusesApartAsFetchDoes() async throws {
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        try Data("{}".utf8).write(to: folder.appendingPathComponent("a.json"))
        let server = try MirrorServer(root: folder)
        self.server = server
        let base = server.base
        let result = await Task.detached {
            let t = HTTPTransport(timeout: 5)
            func outcome(_ url: String) -> Result<Data, TransportError> {
                Result { try t.get(url: url) }.mapError { $0 as? TransportError ?? .Network(message: "\($0)") }
            }
            return [
                outcome(base + "a.json"), outcome(base + "r/a.json"), outcome(base + "missing"),
                outcome(base + "status/410"), outcome(base + "status/429"), outcome(base + "status/403-limited"),
                outcome(base + "status/403"), outcome(base + "status/500"), outcome("gopher://x/y"),
                outcome("http://127.0.0.1:9/x"),
            ]
        }.value
        XCTAssertEqual(try result[0].get(), Data("{}".utf8))
        XCTAssertEqual(try result[1].get(), Data("{}".utf8), "a redirect is followed")
        XCTAssertEqual(result[2], .failure(.NotFound))
        XCTAssertEqual(result[3], .failure(.NotFound))
        XCTAssertEqual(result[4], .failure(.RateLimited))
        XCTAssertEqual(result[5], .failure(.RateLimited), "GitHub's 403 with no requests remaining")
        XCTAssertEqual(result[6], .failure(.Network(message: base + "status/403: http status: 403")))
        XCTAssertEqual(result[7], .failure(.Network(message: base + "status/500: http status: 500")))
        XCTAssertEqual(result[8], .failure(.Network(message: "gopher://x/y: unsupported protocol \"gopher\"")))
        guard case .failure(.Network(let refused)) = result[9] else { return XCTFail("\(result[9])") }
        XCTAssertTrue(refused.hasPrefix("http://127.0.0.1:9/x: "), refused)
        // `file://`: read as the core reads a local mirror.
        let t = HTTPTransport()
        XCTAssertEqual(try t.get(url: folder.appendingPathComponent("a.json").absoluteString), Data("{}".utf8))
        XCTAssertThrowsError(try t.get(url: folder.appendingPathComponent("b.json").absoluteString)) {
            XCTAssertEqual($0 as? TransportError, .NotFound)
        }
    }

    // MARK: Downloads

    /// A core over `folder` with the Bundled set, whose Starter library comes from the
    /// mirror at `base`, with the released list in (unless `check` is false: a relaunch
    /// keeps the one fetched before).
    private func bootCore(base: String, check: Bool = true) async throws -> MobileApp {
        let bundle = try XCTUnwrap(AppCore.bundledSet()).path
        let folder = folder!
        let core = try await Task.detached { try AppCore.boot(folder: folder, bundle: bundle, studySet: nil) }.value
        try core.setTransport(transport: HTTPTransport(timeout: 5))
        try core.setStarterSource(base: base, releasesApi: nil)
        if check {
            XCTAssertTrue(try core.checkStarter())
            try await until(core, "the released list") { try core.starterLibrary().released }
        }
        return core
    }

    private func newCenter() -> DownloadCenter {
        DownloadCenter(identifier: "io.github.inhesrom.openbible.tests.\(UUID().uuidString)", folder: folder)
    }

    /// `bootCore`, and a download center of its own attached to it.
    private func core(base: String) async throws -> (MobileApp, DownloadCenter) {
        let core = try await bootCore(base: base)
        let center = newCenter()
        center.attach(core: core, folder: folder)
        return (core, center)
    }

    /// The Study set's Resources the core would download, largest first.
    private func studySet(_ core: MobileApp) throws -> [StarterRow] {
        let rows = try core.starterLibrary().rows.filter { $0.set == .study && $0.status == .missing }
        XCTAssertEqual(rows.count, Int(try core.starterLibrary().studySet.resources))
        return rows.sorted { $0.bytes > $1.bytes }
    }

    /// A download task as the system hands one to the delegate after a relaunch: `request`'s
    /// transfer, known by its `taskDescription` only.
    private func relaunched(_ center: DownloadCenter, _ request: DownloadRequest) throws -> URLSessionDownloadTask {
        let file = try XCTUnwrap(center.relative(request.file))
        let task = session.downloadTask(with: try XCTUnwrap(URL(string: request.url)))
        task.taskDescription = DownloadCenter.encode(
            .init(id: request.resourceId, origin: DownloadCenter.origin(request.origin), file: file, url: request.url, resumed: false))
        return task
    }

    /// A finished transfer's file where the system leaves it: a copy of the mirror's, written
    /// now (boot deletes a downloads file a day old, `STALE_DOWNLOAD`).
    private func finishedFile(_ id: String) throws -> URL {
        let location = FileManager.default.temporaryDirectory.appendingPathComponent("CFNetworkDownload_\(UUID().uuidString).tmp")
        try FileManager.default.copyItem(at: try starterMirror().appendingPathComponent("\(id).obr"), to: location)
        try FileManager.default.setAttributes([.modificationDate: Date()], ofItemAtPath: location.path)
        return location
    }

    /// Poll `core` (the test's core has no observer thread) until `done`.
    private func until(
        _ core: MobileApp, _ what: String, timeout: TimeInterval = 120, _ done: () throws -> Bool
    ) async throws {
        let end = Date().addingTimeInterval(timeout)
        while try !done() {
            if Date() > end { return XCTFail("timed out waiting for \(what)") }
            _ = try? core.pollChanged()
            try await Task.sleep(for: .milliseconds(50))
        }
    }

    private func state(_ core: MobileApp, _ id: String) -> DownloadState? {
        (try? core.downloads())?.first { $0.resourceId == id }?.state
    }

    private func plan(_ core: MobileApp, _ id: String) throws -> DownloadRequest {
        let plan = try core.planStarterDownload(
            choice: .one(resourceId: id), network: Network(isWifi: true, allowCellular: false),
            cellularConfirmed: false, freeBytes: nil)
        guard case .start(let requests) = plan, let request = requests.first else {
            throw XCTSkip("\(id) is not downloaded: \(plan)")
        }
        return request
    }

    func testCancelStopsTheTransferAndDeletesWhatItLeft() async throws {
        let server = try MirrorServer(root: try starterMirror())
        self.server = server
        let (core, center) = try await core(base: server.base)
        server.throttle(256 * 1024)
        let id = try XCTUnwrap(studySet(core).first).resourceId
        let request = try plan(core, id)
        let resume = URL(fileURLWithPath: request.file + ".resume")
        center.start([request])
        try await until(core, "progress") {
            if case .downloading(let done, _) = state(core, id) { return done > 0 }
            return false
        }
        // Resume data from an earlier, dropped transfer, and what the core had of it.
        try Data("partial".utf8).write(to: resume)
        try Data("partial".utf8).write(to: URL(fileURLWithPath: request.file))
        try center.cancel(resourceId: id, origin: .starter, core: core)
        XCTAssertNil(state(core, id), "the core forgot it")
        XCTAssertFalse(FileManager.default.fileExists(atPath: request.file), "the core's file is gone")
        XCTAssertFalse(FileManager.default.fileExists(atPath: resume.path), "its resume data is gone")
        // The transfer stopped: no more of it arrives, and nothing is handed in.
        try await Task.sleep(for: .seconds(2))
        XCTAssertNil(state(core, id))
        XCTAssertFalse(FileManager.default.fileExists(atPath: request.file))
    }

    /// Cancel tapped as the transfer finishes: its finished file reaches the delegate after
    /// Cancel, and nothing is installed (the core would take a finished file it does not
    /// know).
    func testACancelAsTheTransferFinishesInstallsNothing() async throws {
        let server = try MirrorServer(root: try starterMirror())
        self.server = server
        let (core, center) = try await core(base: server.base)
        server.throttle(256 * 1024)
        let id = try XCTUnwrap(studySet(core).first).resourceId
        let request = try plan(core, id)
        center.start([request])
        try await until(core, "progress") {
            if case .downloading(let done, _) = state(core, id) { return done > 0 }
            return false
        }
        try center.cancel(resourceId: id, origin: .starter, core: core)
        center.urlSession(session, downloadTask: try relaunched(center, request), didFinishDownloadingTo: try finishedFile(id))
        try await Task.sleep(for: .seconds(2))
        _ = try? core.pollChanged()
        XCTAssertNil(state(core, id), "the core forgot it")
        XCTAssertFalse(try core.library().contains { $0.resourceId == id }, "installed anyway")
        XCTAssertFalse(FileManager.default.fileExists(atPath: request.file), "its file is gone")
    }

    /// The system relaunches the app in the background to hand it its transfers' outcomes,
    /// before the core is up: a finished file goes straight to the core's path, a dropped
    /// transfer's resume data is kept, and the system's completion handler waits until the
    /// core is up and has been handed both. Then the file installs, and the failure shows
    /// with Retry.
    func testARelaunchBeforeTheCoreIsUpLosesNoFinishedFile() async throws {
        let server = try MirrorServer(root: try starterMirror())
        self.server = server
        let (finished, dropped) = try await plannedBeforeATermination(base: server.base)
        let center = newCenter()
        let session = center.identifier + ".wifi"
        let called = Called()
        XCTAssertTrue(center.handleEvents(for: session) { MainActor.assumeIsolated { called.value = true } })
        let location = try finishedFile(finished.resourceId)
        center.urlSession(self.session, downloadTask: try relaunched(center, finished), didFinishDownloadingTo: location)
        XCTAssertFalse(FileManager.default.fileExists(atPath: location.path))
        XCTAssertTrue(FileManager.default.fileExists(atPath: finished.file), "moved straight to the core's path")
        let resume = Data("resume data".utf8)
        let lost = NSError(
            domain: NSURLErrorDomain, code: NSURLErrorNetworkConnectionLost,
            userInfo: [NSURLSessionDownloadTaskResumeData: resume])
        center.urlSession(self.session, task: try relaunched(center, dropped), didCompleteWithError: lost)
        XCTAssertEqual(try? Data(contentsOf: URL(fileURLWithPath: dropped.file + ".resume")), resume, "resume data kept")
        center.finishedEvents(for: session)
        try await Task.sleep(for: .milliseconds(500))
        XCTAssertFalse(called.value, "the system was told before the core had the file")

        // The core comes up over the same folder (the released list kept from before).
        let core = try await bootCore(base: server.base, check: false)
        center.attach(core: core, folder: folder)
        try await until(core, "the completion handler") { called.value }
        try await until(core, "\(finished.resourceId) installed") {
            if case .failed(let message, _) = state(core, finished.resourceId) { throw Failure(message: message) }
            return try core.library().contains { $0.resourceId == finished.resourceId }
        }
        guard case .failed(_, let retry) = state(core, dropped.resourceId) else {
            return XCTFail("\(String(describing: state(core, dropped.resourceId)))")
        }
        XCTAssertTrue(retry)
        XCTAssertTrue(FileManager.default.fileExists(atPath: dropped.file + ".resume"), "Retry resumes")
    }

    /// The launch before a termination: the two smallest of the Study set planned, and the
    /// core gone with the app.
    private func plannedBeforeATermination(base: String) async throws -> (DownloadRequest, DownloadRequest) {
        let core = try await bootCore(base: base)
        let small = Array(try studySet(core).suffix(2))
        XCTAssertEqual(small.count, 2)
        return (try plan(core, small[0].resourceId), try plan(core, small[1].resourceId))
    }

    /// The Study set downloading through the app's models (the core's pump, `Changed`
    /// delivered on the main actor, `LibraryModel`): the main thread's work for it, the
    /// screens asked again, stays small however often progress arrives.
    func testMainThreadWorkDuringTheStudySetDownloadStaysSmall() async throws {
        let server = try MirrorServer(root: try starterMirror())
        self.server = server
        let folder = folder!
        let center = newCenter()
        let app = AppCore(appFolder: { folder }, studySet: nil, downloads: center)
        let reader = ReaderModel(app: app)
        let library = LibraryModel(app: app, network: NetworkMonitor(injected: true), downloads: center)
        var changes = 0
        reader.onLibrary = { library.libraryChanged() }
        reader.onDownloads = {
            changes += 1
            library.downloadsChanged()
        }
        await reader.start()
        let core = try XCTUnwrap(app.core)
        try core.setStarterSource(base: server.base, releasesApi: nil)
        library.coreReady()
        XCTAssertTrue(try core.checkStarter())
        try await wait("the released list", timeout: 60) { library.starter?.released == true }
        let count = try studySet(core).count
        server.throttle(2 * 1024 * 1024)
        let (refreshes, time) = (library.refreshes, library.refreshTime)
        let clock = ContinuousClock()
        let began = clock.now
        library.download(.set(set: .study))
        try await wait("the Study set installed", timeout: 600) {
            let rows = (try? core.downloads()) ?? []
            return rows.count == count && rows.allSatisfy { $0.state == .done }
        }
        let took = clock.now - began
        let seconds = Double(took.components.seconds) + Double(took.components.attoseconds) / 1e18
        let asked = library.refreshes - refreshes
        let work = library.refreshTime - time
        let share = (Double(work.components.seconds) + Double(work.components.attoseconds) / 1e18) / seconds
        print("Study set (\(count) Resources) in \(took): \(changes) downloads changes, \(asked) refreshes, \(work) on the main thread (\(String(format: "%.2f", share * 100)) %)")
        XCTAssertLessThan(Double(asked), seconds * 4 + Double(count) * 2 + 10, "refreshes are not coalesced")
        XCTAssertLessThan(share, 0.05, "the main thread spent \(work) of \(took) asking again")
    }

    /// Wait on the main actor (the core's pump delivers) until `done`.
    private func wait(_ what: String, timeout: TimeInterval, _ done: () -> Bool) async throws {
        let end = Date().addingTimeInterval(timeout)
        while !done() {
            if Date() > end { return XCTFail("timed out waiting for \(what)") }
            try await Task.sleep(for: .milliseconds(100))
        }
    }

    /// GitHub serves a release asset by a 302 to a signed URL that expires. Resume data names
    /// the signed URL: resumed while it is still valid, the transfer carries on with a
    /// range request; once it has expired, the resumed request is refused, and the center
    /// starts afresh from the core's URL, which redirects again. Either way the file is
    /// verified and installed.
    func testResumeDataAcrossARedirectResumesOrStartsAfresh() async throws {
        let server = try MirrorServer(root: try starterMirror())
        self.server = server
        let (core, center) = try await core(base: server.base + "r/")
        server.throttle(1024 * 1024)
        // Two of a few MB, so 512 KB of each can arrive before the drop.
        let ids = try studySet(core).filter { $0.bytes > 2_000_000 }.suffix(2).map(\.resourceId)
        XCTAssertEqual(ids.count, 2)
        for (id, lifetime) in zip(ids, [600.0, 2.0]) {
            server.signedLifetime(lifetime)
            let request = try plan(core, id)
            XCTAssertTrue(request.url.hasPrefix(server.base + "r/"), request.url)
            // A transfer dropped part way, leaving its resume data where the center keeps it.
            let data = try await ResumeData.after(bytes: 512 * 1024, of: URL(string: request.url)!)
            try data.write(to: URL(fileURLWithPath: request.file + ".resume"))
            if lifetime < 10 { try await Task.sleep(for: .seconds(lifetime + 3)) }
            let before = server.requests.count
            center.start([request])
            try await until(core, "\(id) installed", timeout: 90) {
                if case .failed(let message, _) = state(core, id) { throw Failure(message: "\(id) failed: \(message)") }
                return state(core, id) == .done || state(core, id) == .indexing
            }
            let after = Array(server.requests[before...]).filter { $0.method == "GET" }
            print("\(id) (signed URL valid \(lifetime) s): \(after.map { "\($0.path) range \($0.range.map(String.init) ?? "-")" })")
            let first = try XCTUnwrap(after.first)
            XCTAssertEqual(first.path, "/signed/\(id).obr", "resume data names the signed URL")
            XCTAssertNotNil(first.range, "and asks for the rest")
            if lifetime > 10 {
                XCTAssertEqual(after.count, 1, "resumed, not downloaded again")
            } else {
                XCTAssertEqual(after.dropFirst().map(\.path), ["/r/\(id).obr", "/signed/\(id).obr"], "afresh")
                XCTAssertNil(after.last?.range)
            }
            XCTAssertFalse(FileManager.default.fileExists(atPath: request.file + ".resume"))
            XCTAssertTrue(try core.library().contains { $0.resourceId == id })
        }
    }
}

/// Set by a completion handler the system would call, read on the main actor.
final class Called: @unchecked Sendable {
    var value = false
}

struct Failure: Error, CustomStringConvertible {
    let message: String
    var description: String { message }
}

/// Resume data the way the system leaves it after a dropped transfer: a download of `url`
/// cancelled with `cancel(byProducingResumeData:)` once `bytes` have arrived.
final class ResumeData: NSObject, URLSessionDownloadDelegate, @unchecked Sendable {
    private let bytes: Int64
    private var continuation: CheckedContinuation<Data, Error>?
    private var task: URLSessionDownloadTask?

    private init(bytes: Int64) { self.bytes = bytes }

    static func after(bytes: Int64, of url: URL) async throws -> Data {
        let probe = ResumeData(bytes: bytes)
        let session = URLSession(configuration: .ephemeral, delegate: probe, delegateQueue: nil)
        defer { session.invalidateAndCancel() }
        return try await withCheckedThrowingContinuation { continuation in
            probe.continuation = continuation
            let task = session.downloadTask(with: url)
            probe.task = task
            task.resume()
        }
    }

    func urlSession(
        _ session: URLSession, downloadTask: URLSessionDownloadTask, didWriteData _: Int64,
        totalBytesWritten: Int64, totalBytesExpectedToWrite _: Int64
    ) {
        guard totalBytesWritten >= bytes, let continuation else { return }
        self.continuation = nil
        downloadTask.cancel { data in
            if let data { continuation.resume(returning: data) } else {
                continuation.resume(throwing: URLError(.cannotCreateFile))
            }
        }
    }

    func urlSession(_ session: URLSession, downloadTask: URLSessionDownloadTask, didFinishDownloadingTo location: URL) {
        continuation?.resume(throwing: URLError(.cancelled))
        continuation = nil
    }
}
