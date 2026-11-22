import OpenBibleCore
import XCTest

@testable import OpenBible

/// The core's boot as `AppCore` does it, against the real Rust core and the Bundled set in
/// the app, in a folder of its own (the app's shared core uses Application Support).
@MainActor
final class AppCoreTests: XCTestCase {
    private var folder: URL!

    override func setUp() async throws {
        folder = FileManager.default.temporaryDirectory
            .appendingPathComponent("AppCoreTests-\(UUID().uuidString)", isDirectory: true)
    }

    override func tearDown() async throws {
        try? FileManager.default.removeItem(at: folder)
    }

    private func boot() async throws -> (MobileApp, Duration) {
        let bundle = try XCTUnwrap(AppCore.bundledSet(), "no Bundled set in the app").path
        let folder = folder!
        let clock = ContinuousClock()
        let began = clock.now
        let core = try await Task.detached {
            try AppCore.boot(folder: folder, bundle: bundle, studySet: nil)
        }.value
        return (core, clock.now - began)
    }

    func testBootsOverTheBundledSetAndOpensItsBible() async throws {
        let (core, took) = try await boot()
        print("first boot over the Bundled set: \(took)")
        XCTAssertEqual(
            Set(try core.library().map(\.resourceId)),
            ["bsb", "openbible-xrefs", "strongs-hebrew", "strongs-greek"])
        XCTAssertEqual(try core.problems(), [])
        let reader = try core.reader(reader: 0)
        XCTAssertEqual(reader.resourceId, "bsb")
        XCTAssertEqual(reader.location, "Genesis 1:1")
        // The folders the core lists are kept out of device backups; the User database is not.
        for path in try core.backupExcluded() {
            let values = try URL(fileURLWithPath: path).resourceValues(forKeys: [.isExcludedFromBackupKey])
            XCTAssertEqual(values.isExcludedFromBackup, true, path)
        }
        _ = try core.navigate(reader: 0, text: "Jn 3:16")
        try core.saveSession()

        // The second launch finds the library there, and the reader where it was.
        let (again, second) = try await boot()
        print("second boot: \(second)")
        XCTAssertEqual(try again.library().count, 4)
        XCTAssertEqual(try again.reader(reader: 0).location, "John 3:16")
    }

    func testABootThatFailsSaysWhy() async throws {
        // A file where the library folder must go.
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        try Data().write(to: folder.appendingPathComponent("library"))
        do {
            _ = try await boot()
            XCTFail("booted anyway")
        } catch {
            let message = AppCore.message(error)
            XCTAssertTrue(message.contains("library"), message)
        }
    }

    func testAReferenceErrorIsTheParsers() async throws {
        let (core, _) = try await boot()
        XCTAssertThrowsError(try core.navigate(reader: 0, text: "Hezekiah 4")) { error in
            guard case MobileError.Reference(let message) = error else {
                return XCTFail("\(error)")
            }
            XCTAssertTrue(message.contains("Hezekiah"), message)
        }
    }
}
