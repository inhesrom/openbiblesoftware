import XCTest

/// Library downloads (S24, #333 "Downloads") against a local Starter library mirror
/// (`MirrorServer` in this test runner, serving `dist/starter` on 127.0.0.1), so no test
/// touches the network: the first-launch Study set offer and the Study set downloaded in the
/// background across a suspension, verified and installed (`testLibraryDownload`); a bad
/// hash showing Retry; Wi-Fi only asking first on a simulated cellular path; Cancel; the
/// Library screens' accessibility audits and previews. Run with
/// `TEST_RUNNER_OBS_STUDY_SET_DIR=<repo>/dist/starter`; without it they are skipped. Each
/// launch boots in a fresh folder of its own (`OBS_TEST_APP_FOLDER`).
@MainActor
final class LibraryTests: XCTestCase {
    private var server: MirrorServer!

    override func tearDown() async throws {
        server?.stop()
        XCUIDevice.shared.appearance = .light
    }

    private func launch(network: String = "wifi", offer: Bool = true) throws -> XCUIApplication {
        guard let dir = ProcessInfo.processInfo.environment["OBS_STUDY_SET_DIR"] else {
            throw XCTSkip("no Starter library to serve: set TEST_RUNNER_OBS_STUDY_SET_DIR")
        }
        server = try MirrorServer(root: URL(fileURLWithPath: dir, isDirectory: true))
        let app = XCUIApplication()
        app.launchEnvironment["OBS_TEST_APP_FOLDER"] = "OpenBible-LibraryTests"
        app.launchEnvironment["OBS_STARTER_MIRROR"] = server.base
        app.launchEnvironment["OBS_TEST_NETWORK"] = network
        if !offer { app.launchEnvironment["OBS_TEST_NO_STUDY_SET_OFFER"] = "1" }
        app.launch()
        XCTAssertTrue(app.staticTexts["reader.location"].waitForExistence(timeout: 30), "no reader")
        return app
    }

    private func shot(_ app: XCUIApplication, _ name: String, dark: Bool = false) {
        let add = { (name: String) in
            let shot = XCTAttachment(screenshot: app.screenshot())
            shot.name = name
            shot.lifetime = .keepAlways
            self.add(shot)
        }
        add(name)
        if dark {
            XCUIDevice.shared.appearance = .dark
            sleep(1)
            add(name + "-dark")
            XCUIDevice.shared.appearance = .light
            sleep(1)
        }
    }

    /// The accessibility audit of what is on screen. The system toolbar's glass buttons
    /// (Done) are drawn at a fixed size with the Large Content Viewer (as S26 found), and on
    /// an iPad a sheet floats over the dimmed reader, whose drawn text has no element.
    private func audit(_ app: XCUIApplication) throws {
        let pad = UIDevice.current.userInterfaceIdiom == .pad
        try app.performAccessibilityAudit { issue in
            let id = issue.element?.identifier ?? "-"
            let toolbar = id == "library.done" && [.contrast, .dynamicType].contains(issue.auditType)
            let behind = pad && issue.element == nil && [.contrast, .elementDetection].contains(issue.auditType)
            let system = toolbar || behind
            if !system {
                print("AUDIT \(issue.auditType) \(issue.compactDescription): \(id) \(issue.element?.label ?? "-")")
            }
            return system
        }
    }

    private func element(_ app: XCUIApplication, _ id: String) -> XCUIElement {
        app.descendants(matching: .any)[id].firstMatch
    }

    private func openLibrary(_ app: XCUIApplication) {
        app.buttons["reader.options"].tap()
        let item = app.buttons["reader.library"]
        XCTAssertTrue(item.waitForExistence(timeout: 5), "no Library… in the menu")
        XCTAssertEqual(item.label, "Library…")
        item.tap()
        XCTAssertTrue(app.buttons["library.done"].waitForExistence(timeout: 10), "no Library sheet")
    }

    private func openStarterLibrary(_ app: XCUIApplication) {
        openLibrary(app)
        element(app, "library.starter").tap()
        XCTAssertTrue(app.navigationBars["Starter library"].waitForExistence(timeout: 10))
        // The released list from the mirror: the Study set's 15 Resources and their size.
        XCTAssertTrue(element(app, "starter.row.kjv-strongs").waitForExistence(timeout: 30), "no released rows")
    }

    /// Scroll the list until `id` is on screen.
    private func scrolled(to id: String, in app: XCUIApplication) -> XCUIElement {
        let target = element(app, id)
        for _ in 0..<30 where !(target.exists && target.isHittable) {
            app.swipeUp()
        }
        return target
    }

    /// #333: "At first launch the app offers the Study set"; downloads in the background,
    /// carry on while the app is suspended, are checked against their sha256 and installed.
    func testLibraryDownload() throws {
        let app = try launch()
        let size = app.staticTexts["offer.size"]
        XCTAssertTrue(size.waitForExistence(timeout: 30), "no Study set offer with its size")
        XCTAssertTrue(size.label.hasPrefix("15 Resources, "), size.label)
        XCTAssertEqual(app.staticTexts["offer.network"].label, "You are on Wi-Fi.")
        XCTAssertTrue(app.navigationBars["Get the Study set"].exists)
        // VoiceOver order: what it brings, its size, the network, then the buttons.
        let order = ["offer.message", "offer.size", "offer.network", "offer.download", "offer.notNow"]
            .map { element(app, $0).frame.minY }
        XCTAssertEqual(order, order.sorted(), "\(order)")
        shot(app, "s24-offer", dark: true)
        try audit(app)

        // Slow enough that the transfers are still running when the app is suspended.
        server.throttle(2 * 1024 * 1024)
        app.buttons["offer.download"].tap()
        XCTAssertTrue(app.buttons["offer.download"].waitForNonExistence(timeout: 10), "the offer stayed")
        let banner = app.buttons["reader.banner"]
        XCTAssertTrue(banner.waitForExistence(timeout: 20), "no download banner")
        XCTAssertTrue(banner.label.hasPrefix("Downloading 15 Resources"), banner.label)
        shot(app, "s24-banner", dark: true)

        // Suspended: the system carries the transfers on.
        XCUIDevice.shared.press(.home)
        XCTAssertTrue(app.wait(for: .runningBackgroundSuspended, timeout: 20), "not suspended")
        sleep(8)
        app.activate()
        XCTAssertTrue(app.wait(for: .runningForeground, timeout: 20))
        XCTAssertTrue(banner.waitForNonExistence(timeout: 240), "still downloading: \(banner.label)")
        let gets = server.requests.filter { $0.method == "GET" && $0.path.hasSuffix(".obr") }
        XCTAssertEqual(Set(gets.map(\.path)).count, 15, "\(gets)")

        openStarterLibrary(app)
        XCTAssertFalse(element(app, "starter.studySet.download").exists, "the Study set is in")
        XCTAssertTrue(element(app, "starter.remove.kjv-strongs").exists)
        shot(app, "s24-starter-library", dark: true)
        try audit(app)
        app.buttons["library.done"].tap()
        app.buttons["reader.options"].tap()
        app.buttons["About"].tap()
        let count = app.staticTexts["hello.resourceCount"]
        XCTAssertTrue(count.waitForExistence(timeout: 10))
        XCTAssertEqual(count.label, "Resources, 19")
    }

    /// #333: "a mismatch deletes it and offers Retry".
    func testABadHashShowsRetry() throws {
        let app = try launch(offer: false)
        server.corrupt(["hitchcock.obr"])
        openLibrary(app)
        shot(app, "s24-library", dark: true)
        try audit(app)
        element(app, "library.starter").tap()
        XCTAssertTrue(element(app, "starter.row.kjv-strongs").waitForExistence(timeout: 30))
        scrolled(to: "starter.download.hitchcock", in: app).tap()
        let retry = app.buttons["download.retry.hitchcock"]
        XCTAssertTrue(retry.waitForExistence(timeout: 60), "no Retry")
        XCTAssertEqual(retry.label, "Retry the download of Hitchcock")
        let state = app.staticTexts["download.state.hitchcock"]
        XCTAssertFalse(state.label.isEmpty)
        print("bad hash: \(state.label)")
        shot(app, "s24-retry", dark: true)
        try audit(app)
        server.corrupt([])
        retry.tap()
        XCTAssertTrue(element(app, "starter.remove.hitchcock").waitForExistence(timeout: 60), "not installed")
    }

    /// #333: "Downloads use Wi-Fi only by default … with it off, a download over cellular
    /// shows its size and asks first", on a cellular path the test stands in for.
    func testWiFiOnlyAsksFirstOnCellular() throws {
        let app = try launch(network: "cellular")
        let network = app.staticTexts["offer.network"]
        XCTAssertTrue(app.staticTexts["offer.size"].waitForExistence(timeout: 30))
        XCTAssertEqual(network.label, "You are not on Wi-Fi. Download asks before it uses cellular data.")
        app.buttons["offer.download"].tap()
        let ask = app.alerts.firstMatch
        XCTAssertTrue(ask.waitForExistence(timeout: 10), "no cellular question")
        XCTAssertTrue(ask.label.hasPrefix("Download the Study set ("), ask.label)
        XCTAssertTrue(ask.label.contains("over cellular data?"), ask.label)
        shot(app, "s24-cellular")
        ask.buttons["Not now"].tap()
        XCTAssertTrue(ask.waitForNonExistence(timeout: 5))
        XCTAssertTrue(app.buttons["offer.notNow"].exists, "the offer stays up")
        app.buttons["offer.notNow"].tap()
        XCTAssertTrue(app.buttons["offer.notNow"].waitForNonExistence(timeout: 5))
        sleep(2)
        XCTAssertEqual(server.requests.filter { $0.path.hasSuffix(".obr") }, [], "nothing downloaded")

        // With Use cellular data on, a download starts over cellular without asking.
        openLibrary(app)
        let toggle = app.switches["library.useCellular"]
        XCTAssertEqual(toggle.value as? String, "0", "off by default")
        toggle.switches.firstMatch.tap()
        XCTAssertEqual(toggle.value as? String, "1")
        element(app, "library.starter").tap()
        XCTAssertTrue(element(app, "starter.row.kjv-strongs").waitForExistence(timeout: 30))
        scrolled(to: "starter.download.theographic", in: app).tap()
        XCTAssertFalse(app.alerts.firstMatch.waitForExistence(timeout: 2), "asked anyway")
        XCTAssertTrue(element(app, "starter.remove.theographic").waitForExistence(timeout: 60), "not installed")
    }

    /// #333: "Cancel removes partial files" (the files themselves: DownloadTests).
    func testCancelStopsADownload() throws {
        let app = try launch(offer: false)
        server.throttle(256 * 1024)
        openStarterLibrary(app)
        scrolled(to: "starter.download.jfb", in: app).tap()
        let cancel = app.buttons["download.cancel.jfb"]
        XCTAssertTrue(cancel.waitForExistence(timeout: 20), "no Cancel")
        let state = app.staticTexts["download.state.jfb"]
        let moving = NSPredicate(format: "label BEGINSWITH 'Downloading · '")
        expectation(for: moving, evaluatedWith: state)
        waitForExpectations(timeout: 30)
        XCTAssertTrue(app.staticTexts["library.banner"].exists || element(app, "library.banner").exists)
        shot(app, "s24-downloading", dark: true)
        try audit(app)
        cancel.tap()
        XCTAssertTrue(element(app, "starter.download.jfb").waitForExistence(timeout: 10), "not cancelled")
        XCTAssertFalse(element(app, "library.banner").exists, "the banner stayed")
    }

    /// Get Resources (the Open catalogue with the desktop's licence filter) and Storage.
    func testGetResourcesAndStorage() throws {
        let app = try launch(offer: false)
        openLibrary(app)
        element(app, "library.getResources").tap()
        XCTAssertTrue(app.navigationBars["Get Resources"].waitForExistence(timeout: 10))
        let rows = app.descendants(matching: .any).matching(NSPredicate(format: "identifier BEGINSWITH 'open.row.'"))
        XCTAssertTrue(rows.firstMatch.waitForExistence(timeout: 10), "no Open catalogue rows")
        XCTAssertTrue(rows.firstMatch.label.contains("from "), rows.firstMatch.label)
        shot(app, "s24-get-resources", dark: true)
        try audit(app)
        app.navigationBars.buttons.element(boundBy: 0).tap()
        element(app, "library.storage").tap()
        XCTAssertTrue(app.navigationBars["Storage"].waitForExistence(timeout: 10))
        XCTAssertTrue(element(app, "storage.total").waitForExistence(timeout: 10))
        XCTAssertTrue(element(app, "storage.remove.bsb").exists)
        shot(app, "s24-storage", dark: true)
        try audit(app)
    }
}
