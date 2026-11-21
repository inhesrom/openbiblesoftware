import XCTest

/// S07 prototype smoke (#331): the text view scrolls, selects with a long-press, goes to a
/// typed reference, opens the Study sheet stub on a tap, and is one accessibility element
/// per Verse, labelled with its reference and text, in reading order.
final class PrototypeTests: XCTestCase {
    private func shot(_ app: XCUIApplication, _ name: String) {
        let shot = XCTAttachment(screenshot: app.screenshot())
        shot.name = name
        shot.lifetime = .keepAlways
        add(shot)
    }

    /// The Verse elements in view whose identifiers start with `prefix` (`Gen.1.`), with
    /// their verse numbers.
    private func verses(_ app: XCUIApplication, _ prefix: String) -> [(Int, XCUIElement)] {
        let query = app.staticTexts.matching(NSPredicate(format: "identifier BEGINSWITH %@", prefix))
        return query.allElementsBoundByIndex.compactMap { e in
            guard let n = Int(e.identifier.dropFirst(prefix.count)) else { return nil }
            return (n, e)
        }
    }

    @MainActor
    func testPrototypeSmoke() throws {
        let app = XCUIApplication()
        app.launch()

        // One element per Verse, labelled with its reference and text, in reading order.
        let first = app.staticTexts["Gen.1.1"]
        XCTAssertTrue(first.waitForExistence(timeout: 30), "no Verse elements")
        XCTAssertTrue(first.label.hasPrefix("Genesis 1:1. In the beginning God created"), first.label)
        let genesis = verses(app, "Gen.1.")
        XCTAssertGreaterThanOrEqual(genesis.count, 5)
        XCTAssertEqual(genesis.map(\.0), Array(1...genesis.count), "one per Verse, in order")
        for (n, e) in genesis {
            XCTAssertTrue(e.label.hasPrefix("Genesis 1:\(n). "), e.label)
            XCTAssertGreaterThan(e.label.count, "Genesis 1:\(n). ".count + 10, e.label)
        }
        XCTAssertEqual(app.staticTexts["reader.location"].label, "Genesis 1:1")

        // A fast fling scrolls on.
        let text = app.scrollViews.firstMatch
        text.swipeUp(velocity: .fast)
        text.swipeUp(velocity: .fast)
        let moved = NSPredicate(format: "label != %@", "Genesis 1:1")
        expectation(for: moved, evaluatedWith: app.staticTexts["reader.location"])
        waitForExpectations(timeout: 10)

        // A typed reference goes there.
        let field = app.textFields["reader.reference"]
        field.tap()
        field.typeText("Jn 3:16\n")
        let verse = app.staticTexts["John.3.16"]
        XCTAssertTrue(verse.waitForExistence(timeout: 10), "did not go to John 3:16")
        XCTAssertTrue(verse.label.hasPrefix("John 3:16. For God so loved the world"), verse.label)
        XCTAssertEqual(app.staticTexts["reader.location"].label, "John 3:16")

        // A long-press selects a word, with the system edit menu.
        verse.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.1)).press(forDuration: 1.2)
        let copy = app.menuItems["Copy"].exists ? app.menuItems["Copy"] : app.buttons["Copy"]
        XCTAssertTrue(copy.waitForExistence(timeout: 5), "no edit menu with Copy")
        shot(app, "s07-selection")

        // A tap clears the selection; the next opens the Study sheet stub for the Verse.
        verse.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.1)).tap()
        verse.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.1)).tap()
        let refs = app.staticTexts["sheet.crossReferences"]
        XCTAssertTrue(refs.waitForExistence(timeout: 5), "no Study sheet stub")
        XCTAssertTrue(refs.label.hasSuffix("cross-references"), refs.label)
        XCTAssertTrue(app.navigationBars["John 3:16 (BSB)"].exists, "the sheet names the Verse")
        shot(app, "s07-sheet")
    }

    /// With the debug Study set override (`TEST_RUNNER_OBS_STUDY_SET_DIR=<repo>/dist/starter`
    /// on the `xcodebuild test` line), a tap on a word of the KJV with Strong's opens the
    /// sheet with the word's Strong's number.
    @MainActor
    func testATaggedWordShowsItsStrongsNumber() throws {
        guard let studySet = ProcessInfo.processInfo.environment["OBS_STUDY_SET_DIR"] else {
            throw XCTSkip("no Study set: set TEST_RUNNER_OBS_STUDY_SET_DIR")
        }
        let app = XCUIApplication()
        app.launchEnvironment["OBS_STUDY_SET_DIR"] = studySet
        app.launchArguments += ["-OBSResource", "kjv-strongs", "-OBSGoTo", "Jn 3:16"]
        app.launch()
        let verse = app.staticTexts["John.3.16"]
        XCTAssertTrue(verse.waitForExistence(timeout: 30), "did not open the KJV at John 3:16")
        verse.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.1)).tap()
        let strongs = app.staticTexts["sheet.strongs"]
        XCTAssertTrue(strongs.waitForExistence(timeout: 5), "no Strong's number in the sheet")
        XCTAssertNotNil(strongs.label.range(of: #"[GH]\d+"#, options: .regularExpression), strongs.label)
        XCTAssertTrue(app.navigationBars["John 3:16 (KJV)"].exists)
        shot(app, "s07-sheet-strongs")
    }

    @MainActor
    func testAccessibilityAudit() throws {
        let app = XCUIApplication()
        app.launch()
        XCTAssertTrue(app.staticTexts["Gen.1.1"].waitForExistence(timeout: 30))
        try app.performAccessibilityAudit { issue in
            print("AUDIT \(issue.auditType) \(issue.compactDescription): \(issue.element?.identifier ?? "-") \(issue.element?.label ?? "-") \(issue.element?.frame ?? .zero)")
            return false
        }
    }
}
