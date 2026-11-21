import XCTest

@testable import OpenBible

/// The hello screen's model against the real Rust core and the Bundled set in the app.
@MainActor
final class HelloModelTests: XCTestCase {
    private var folder: URL!

    override func setUp() async throws {
        folder = FileManager.default.temporaryDirectory
            .appendingPathComponent("HelloModelTests-\(UUID().uuidString)", isDirectory: true)
    }

    override func tearDown() async throws {
        try? FileManager.default.removeItem(at: folder)
    }

    func testBootsOverTheBundledSetWithItsFourResources() async throws {
        let bundle = try XCTUnwrap(HelloModel.bundledSet(), "no Bundled set in the app")
        let model = HelloModel()
        XCTAssertFalse(model.engineVersion.isEmpty)

        await model.start(folder: folder, bundle: bundle)
        guard case .ready(let ids, let problems) = model.state else {
            return XCTFail("did not boot: \(model.state)")
        }
        XCTAssertEqual(
            Set(ids), ["bsb", "openbible-xrefs", "strongs-hebrew", "strongs-greek"])
        XCTAssertEqual(problems, [])
        // The first launch copies the Bundled set into the library (plan S06: at most 3 s).
        let took = try XCTUnwrap(model.bootDuration)
        print("first boot over the Bundled set: \(took)")

        // The second launch finds the library there.
        let again = HelloModel()
        await again.start(folder: folder, bundle: bundle)
        XCTAssertEqual(again.state, model.state)
        print("second boot: \(try XCTUnwrap(again.bootDuration))")
    }

    func testABootThatFailsSaysWhy() async throws {
        // A file where the library folder must go.
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        try Data().write(to: folder.appendingPathComponent("library"))
        let model = HelloModel()
        await model.start(folder: folder, bundle: nil)
        guard case .failed(let message) = model.state else {
            return XCTFail("booted anyway: \(model.state)")
        }
        XCTAssertTrue(message.contains("library"), message)
    }
}
