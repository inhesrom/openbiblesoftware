import XCTest

/// Launch smoke: the app starts on the reader, and its About sheet shows the engine's version
/// and the library it booted with (the S06 hello screen), with a screenshot in the results.
final class LaunchTests: XCTestCase {
    @MainActor
    func testLaunchShowsTheEngineAndTheBundledSet() throws {
        let app = XCUIApplication()
        app.launch()

        XCTAssertTrue(app.staticTexts["reader.location"].waitForExistence(timeout: 30), "no reader")
        app.buttons["reader.options"].tap()
        app.buttons["About"].tap()
        let engine = app.staticTexts["hello.engine"]
        XCTAssertTrue(engine.waitForExistence(timeout: 10))
        XCTAssertTrue(engine.label.hasPrefix("Engine, "), engine.label)
        let count = app.staticTexts["hello.resourceCount"]
        XCTAssertTrue(count.waitForExistence(timeout: 30), "the core did not boot")
        // The first launch's library is the Bundled set: four Resources.
        XCTAssertEqual(count.label, "Resources, 4")

        let shot = XCTAttachment(screenshot: app.screenshot())
        shot.name = "s06-hello"
        shot.lifetime = .keepAlways
        add(shot)
    }
}
