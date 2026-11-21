import XCTest

/// Launch smoke: the app starts, shows the engine's version and the library it booted with,
/// and leaves a screenshot in the test results. (No `performAccessibilityAudit()` yet: it
/// flags the system List's secondary text, "Contrast nearly passed"; the accessibility bar is
/// ADR 0023's and S14b's.)
final class LaunchTests: XCTestCase {
    @MainActor
    func testLaunchShowsTheEngineAndTheBundledSet() throws {
        let app = XCUIApplication()
        app.launch()

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
