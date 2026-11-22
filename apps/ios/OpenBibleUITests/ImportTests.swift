import XCTest

/// Import (S26, #333): a USFM zip, a SWORD module zip and a Resource file picked in the
/// system document picker from the Files app's "On My iPhone" (or iPad), each with the core's
/// Import report in a sheet; the sheet's accessibility audit and labels. The files are put
/// there first by `apps/ios/scripts/place-import-files.sh <udid>`; run with
/// `TEST_RUNNER_OBS_IMPORT_FILES=1` (docs/dev/ios-project.md, "Import tests"). The app boots
/// in a fresh folder of its own (`OBS_TEST_APP_FOLDER`), so the other tests' library stays
/// the Bundled set.
@MainActor
final class ImportTests: XCTestCase {
    private func launch() throws -> XCUIApplication {
        guard ProcessInfo.processInfo.environment["OBS_IMPORT_FILES"] == "1" else {
            throw XCTSkip("no Import files: run place-import-files.sh and set TEST_RUNNER_OBS_IMPORT_FILES=1")
        }
        let app = XCUIApplication()
        app.launchEnvironment["OBS_TEST_APP_FOLDER"] = "OpenBible-ImportTests"
        app.launch()
        XCTAssertTrue(app.staticTexts["reader.location"].waitForExistence(timeout: 30), "no reader")
        return app
    }

    private func shot(_ app: XCUIApplication, _ name: String) {
        let shot = XCTAttachment(screenshot: app.screenshot())
        shot.name = name
        shot.lifetime = .keepAlways
        add(shot)
    }

    /// Reading options → Import…, then the file named `name` in the picker's local storage.
    private func pick(_ app: XCUIApplication, _ name: String, menuShot: String? = nil) {
        app.buttons["reader.options"].tap()
        let item = app.buttons["reader.import"]
        XCTAssertTrue(item.waitForExistence(timeout: 5), "no Import… in the menu")
        XCTAssertEqual(item.label, "Import…")
        if let menuShot { shot(app, menuShot) }
        item.tap()
        let file = app.cells.matching(NSPredicate(format: "identifier BEGINSWITH %@", "\(name),")).firstMatch
        if !file.waitForExistence(timeout: 5) {
            let browse = app.tabBars["DOC.browsingModeTabBar"].buttons["Browse"]
            if browse.exists { browse.tap() }
            for place in ["On My iPhone", "On My iPad"] where !file.waitForExistence(timeout: 2) {
                let row = app.cells.containing(.staticText, identifier: place).firstMatch
                if row.exists { row.tap() } else if app.buttons[place].exists { app.buttons[place].tap() }
            }
        }
        XCTAssertTrue(file.waitForExistence(timeout: 10), "\(name) is not in the picker")
        file.tap()
    }

    /// The report sheet once the Import is over: its heading.
    private func finished(_ app: XCUIApplication) -> String {
        XCTAssertTrue(app.buttons["import.done"].waitForExistence(timeout: 30), "no Import report")
        XCTAssertTrue(app.buttons["import.cancel"].waitForNonExistence(timeout: 30), "still running")
        let bar = app.navigationBars.matching(NSPredicate(format: "identifier BEGINSWITH 'Import '")).firstMatch
        XCTAssertTrue(bar.waitForExistence(timeout: 5), "no heading")
        return bar.identifier
    }

    /// The accessibility audit of the report sheet. Its Done and Open are the system
    /// toolbar's buttons, which iOS 27 draws on glass at a fixed size (the Large Content
    /// Viewer stands in for Dynamic Type): their contrast and Dynamic Type findings are the
    /// system's, not the sheet's.
    private func audit(_ app: XCUIApplication) throws {
        try app.performAccessibilityAudit { issue in
            let id = issue.element?.identifier ?? "-"
            let system = ["import.done", "import.open"].contains(id)
                && [.contrast, .dynamicType].contains(issue.auditType)
            if !system {
                print("AUDIT \(issue.auditType) \(issue.compactDescription): \(id) \(issue.element?.label ?? "-")")
            }
            return system
        }
    }

    private func line(_ app: XCUIApplication, startingWith text: String) -> XCUIElement {
        app.staticTexts.matching(NSPredicate(format: "label BEGINSWITH %@", text)).firstMatch
    }

    func testImportsAUSFMZipAndOpensIt() throws {
        let app = try launch()
        pick(app, "WEB excerpt.zip", menuShot: "s26-menu")
        XCTAssertEqual(finished(app), "Import complete")
        XCTAssertEqual(app.staticTexts["import.source"].label, "WEB excerpt.zip")
        XCTAssertTrue(line(app, startingWith: "5 Bible books · ").exists, "no counts")
        XCTAssertTrue(line(app, startingWith: "5 source files · usfm").exists)
        XCTAssertTrue(line(app, startingWith: "Licence: unknown").exists, "no licence warning")
        shot(app, "s26-report-usfm")
        // VoiceOver order: the heading, the file, the status, then the sections.
        let order = ["import.source", "import.status"].map {
            app.descendants(matching: .any)[$0].firstMatch.frame.minY
        }
        XCTAssertLessThan(order[0], order[1])
        XCTAssertLessThan(order[1], line(app, startingWith: "5 Bible books").frame.minY)
        try audit(app)
        // The warnings, dropped markup and files start folded.
        app.swipeUp()
        let files = app.buttons.matching(NSPredicate(format: "label BEGINSWITH 'Source files and Bible books'")).firstMatch
        XCTAssertTrue(files.waitForExistence(timeout: 5), "no folded files section")
        XCTAssertFalse(line(app, startingWith: "GEN · ").exists, "the files are folded")
        files.tap()
        app.swipeUp()
        XCTAssertTrue(line(app, startingWith: "GEN · ").waitForExistence(timeout: 5), "unfolded")
        app.buttons["import.open"].tap()
        XCTAssertTrue(app.buttons["import.done"].waitForNonExistence(timeout: 10))
        // The reader shows the WEB excerpt's Genesis ("In the beginning, God …").
        let verse = app.staticTexts.matching(
            NSPredicate(format: "identifier == 'Gen.1.1' AND label CONTAINS 'In the beginning, God'")
        ).firstMatch
        XCTAssertTrue(verse.waitForExistence(timeout: 10), "the WEB excerpt is not open")
    }

    func testImportsASWORDModuleZip() throws {
        let app = try launch()
        pick(app, "KJV module.zip")
        XCTAssertEqual(finished(app), "Import complete")
        XCTAssertTrue(app.staticTexts["KJV module.zip"].exists)
        XCTAssertTrue(line(app, startingWith: "1 source file · sword").exists, "not read as SWORD")
        XCTAssertTrue(line(app, startingWith: "Licence recorded").exists)
        shot(app, "s26-report-sword")
        try audit(app)
        app.buttons["import.done"].tap()
        XCTAssertTrue(app.buttons["import.done"].waitForNonExistence(timeout: 10))
    }

    func testImportsAResourceFileAsItIs() throws {
        let app = try launch()
        pick(app, "KJV excerpt.obr")
        XCTAssertEqual(finished(app), "Import complete")
        XCTAssertTrue(app.staticTexts["KJV excerpt"].exists)
        XCTAssertTrue(app.staticTexts["Bible · KJVX"].exists)
        XCTAssertTrue(app.staticTexts["Added to your Library as it is."].exists)
        shot(app, "s26-report-obr")
        try audit(app)
        app.buttons["import.open"].tap()
        XCTAssertTrue(app.buttons["import.done"].waitForNonExistence(timeout: 10))
    }
}
