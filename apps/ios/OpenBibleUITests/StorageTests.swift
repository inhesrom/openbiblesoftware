import XCTest

/// Library → Storage with Search indexes being built (S25, #333 "Storage"): each row says
/// "Still being indexed" until its index is built on the device, then "Search index ready";
/// its accessibility audit and VoiceOver label, and previews. The Starter library Resources
/// outside the Bundled set are added read in place from a copy of `dist/starter` without its
/// prebuilt indexes (the Debug Study set override, `OBS_STUDY_SET_DIR`, with every one of them
/// marked Study set), so the app builds all their indexes after launch. Run with
/// `TEST_RUNNER_OBS_STUDY_SET_DIR=<repo>/dist/starter`; without it the test is skipped.
@MainActor
final class StorageTests: XCTestCase {
    private var copy: URL?

    override func tearDown() async throws {
        if let copy { try? FileManager.default.removeItem(at: copy) }
        XCUIDevice.shared.appearance = .light
    }

    /// `dist/starter`'s Resources and manifest without `search/` and `aliases/`, every
    /// Resource outside the Bundled set in the Study set (hard links where the volume allows).
    private func unindexedStarter() throws -> URL {
        guard let dir = ProcessInfo.processInfo.environment["OBS_STUDY_SET_DIR"] else {
            throw XCTSkip("no Starter library to copy: set TEST_RUNNER_OBS_STUDY_SET_DIR")
        }
        let starter = URL(fileURLWithPath: dir, isDirectory: true)
        let copy = FileManager.default.temporaryDirectory.appendingPathComponent("StorageTests-\(UUID().uuidString)")
        self.copy = copy
        try FileManager.default.createDirectory(at: copy, withIntermediateDirectories: true)
        for name in try FileManager.default.contentsOfDirectory(atPath: starter.path) where name.hasSuffix(".obr") {
            let from = starter.appendingPathComponent(name)
            let to = copy.appendingPathComponent(name)
            do { try FileManager.default.linkItem(at: from, to: to) } catch { try FileManager.default.copyItem(at: from, to: to) }
        }
        let manifest = starter.appendingPathComponent("starter-manifest.json")
        var json = try XCTUnwrap(try JSONSerialization.jsonObject(with: Data(contentsOf: manifest)) as? [String: Any])
        let resources = try XCTUnwrap(json["resources"] as? [[String: Any]])
        json["resources"] = resources.map { r -> [String: Any] in
            var r = r
            if (r["mobile_set"] as? String) != "bundled" { r["mobile_set"] = "study" }
            return r
        }
        try JSONSerialization.data(withJSONObject: json).write(to: copy.appendingPathComponent("starter-manifest.json"))
        return copy
    }

    private func shot(_ app: XCUIApplication, _ name: String) {
        for dark in [false, true] {
            XCUIDevice.shared.appearance = dark ? .dark : .light
            sleep(1)
            let shot = XCTAttachment(screenshot: app.screenshot())
            shot.name = dark ? name + "-dark" : name
            shot.lifetime = .keepAlways
            add(shot)
        }
        XCUIDevice.shared.appearance = .light
    }

    /// The accessibility audit, filtering only what `LibraryTests.audit` filters (the system
    /// toolbar's Done, and on an iPad findings with no element from the reader behind the
    /// sheet).
    private func audit(_ app: XCUIApplication) throws {
        let pad = UIDevice.current.userInterfaceIdiom == .pad
        try app.performAccessibilityAudit { issue in
            let id = issue.element?.identifier ?? "-"
            let toolbar = id == "library.done" && [.contrast, .dynamicType].contains(issue.auditType)
            let behind = pad && issue.element == nil
                && [.contrast, .elementDetection, .textClipped].contains(issue.auditType)
            if !(toolbar || behind) {
                print("AUDIT \(issue.auditType) \(issue.compactDescription): \(id) \(issue.element?.label ?? "-")")
            }
            return toolbar || behind
        }
    }

    private func rows(_ app: XCUIApplication, saying text: String) -> XCUIElementQuery {
        app.descendants(matching: .any).matching(
            NSPredicate(format: "identifier BEGINSWITH 'storage.row.' AND label CONTAINS %@", text))
    }

    func testStorageSaysStillBeingIndexedUntilTheIndexIsBuilt() throws {
        let starter = try unindexedStarter()
        let app = XCUIApplication()
        app.launchEnvironment["OBS_TEST_APP_FOLDER"] = "OpenBible-StorageTests"
        app.launchEnvironment["OBS_TEST_NO_STUDY_SET_OFFER"] = "1"
        app.launchEnvironment["OBS_STUDY_SET_DIR"] = starter.path
        app.launch()
        XCTAssertTrue(app.staticTexts["reader.location"].waitForExistence(timeout: 30), "no reader")
        app.buttons["reader.options"].tap()
        app.buttons["reader.library"].tap()
        let storage = app.descendants(matching: .any)["library.storage"].firstMatch
        XCTAssertTrue(storage.waitForExistence(timeout: 10), "no Library sheet")
        storage.tap()
        XCTAssertTrue(app.navigationBars["Storage"].waitForExistence(timeout: 10))

        // While the indexes are built: rows say so, in the core's words.
        let building = rows(app, saying: "Still being indexed").firstMatch
        XCTAssertTrue(building.waitForExistence(timeout: 20), "no row still being indexed")
        // VoiceOver reads a row as one element: its title and size, then its index; Remove
        // follows it.
        let id = building.identifier.replacingOccurrences(of: "storage.row.", with: "")
        XCTAssertTrue(building.label.hasSuffix("Still being indexed"), building.label)
        XCTAssertTrue(building.label.contains(" MB"), building.label)
        XCTAssertTrue(app.buttons["storage.remove.\(id)"].exists)
        shot(app, "s25-storage-indexing")
        try audit(app)

        // Built: no row is still being indexed, and the Bundled set's BSB reads ready.
        let none = NSPredicate(format: "count == 0")
        expectation(for: none, evaluatedWith: rows(app, saying: "Still being indexed"))
        waitForExpectations(timeout: 240)
        let bsb = app.descendants(matching: .any)["storage.row.bsb"].firstMatch
        XCTAssertTrue(bsb.waitForExistence(timeout: 10))
        XCTAssertTrue(bsb.label.hasSuffix("Search index ready"), bsb.label)
        shot(app, "s25-storage-ready")
        try audit(app)
    }
}
