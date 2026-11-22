import OpenBibleCore
import XCTest

@testable import OpenBible

/// The Library's transfers (S24) against a local mirror (`MirrorServer`), never the network:
/// the transport's statuses as `apps/openbible/src/fetch.rs` tells them apart, Cancel
/// deleting what a transfer left, and background resume data across a GitHub-like redirect
/// to a signed URL that expires (`docs/dev/mobile-library.md` § Evidence). The tests that
/// download Starter files serve `dist/starter` (`TEST_RUNNER_OBS_STUDY_SET_DIR`).
@MainActor
final class DownloadTests: XCTestCase {
    private var folder: URL!
    private var server: MirrorServer?

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
    /// mirror at `base`, and a download center of its own.
    private func core(base: String) async throws -> (MobileApp, DownloadCenter) {
        let bundle = try XCTUnwrap(AppCore.bundledSet()).path
        let folder = folder!
        let core = try await Task.detached { try AppCore.boot(folder: folder, bundle: bundle, studySet: nil) }.value
        try core.setTransport(transport: HTTPTransport(timeout: 5))
        try core.setStarterSource(base: base, releasesApi: nil)
        let center = DownloadCenter(identifier: "io.github.inhesrom.openbible.tests.\(UUID().uuidString)")
        center.attach(core: core, folder: folder)
        XCTAssertTrue(try core.checkStarter())
        try await until(core, "the released list") { try core.starterLibrary().released }
        return (core, center)
    }

    /// Poll `core` (the test's core has no observer thread) until `done`.
    private func until(
        _ core: MobileApp, _ what: String, timeout: TimeInterval = 60, _ done: () throws -> Bool
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
        let request = try plan(core, "jfb")
        let resume = URL(fileURLWithPath: request.file + ".resume")
        center.start([request])
        try await until(core, "progress") {
            if case .downloading(let done, _) = state(core, "jfb") { return done > 0 }
            return false
        }
        // Resume data from an earlier, dropped transfer, and what the core had of it.
        try Data("partial".utf8).write(to: resume)
        try Data("partial".utf8).write(to: URL(fileURLWithPath: request.file))
        try center.cancel(resourceId: "jfb", origin: .starter, core: core)
        XCTAssertNil(state(core, "jfb"), "the core forgot it")
        XCTAssertFalse(FileManager.default.fileExists(atPath: request.file), "the core's file is gone")
        XCTAssertFalse(FileManager.default.fileExists(atPath: resume.path), "its resume data is gone")
        // The transfer stopped: no more of it arrives, and nothing is handed in.
        try await Task.sleep(for: .seconds(2))
        XCTAssertNil(state(core, "jfb"))
        XCTAssertFalse(FileManager.default.fileExists(atPath: request.file))
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
        for (id, lifetime) in [("easton", 600.0), ("smith", 2.0)] {
            server.signedLifetime(lifetime)
            let request = try plan(core, id)
            XCTAssertTrue(request.url.hasPrefix(server.base + "r/"), request.url)
            // A transfer dropped part way, leaving its resume data where the center keeps it.
            let data = try await ResumeData.after(bytes: 512 * 1024, of: URL(string: request.url)!)
            try data.write(to: URL(fileURLWithPath: request.file + ".resume"))
            if lifetime < 10 { try await Task.sleep(for: .seconds(lifetime + 1)) }
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
