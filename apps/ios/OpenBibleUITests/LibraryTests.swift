import XCTest

/// Library downloads (S24, #333 "Downloads") against a local Starter library mirror
/// (`MirrorServer` in this test runner, serving `dist/starter` on 127.0.0.1), so no test
/// touches the network: the first-launch Study set offer and the Study set downloaded in the
/// background across a suspension, verified and installed (`testLibraryDownload`); a bad
/// hash showing Retry; Wi-Fi only asking first on a simulated cellular path; Cancel; the
/// Library screens' accessibility audits and previews. Run with
/// `TEST_RUNNER_OBS_STUDY_SET_DIR=<repo>/dist/starter`; without it they are skipped. Each
/// launch boots in a fresh folder of its own (`OBS_TEST_APP_FOLDER`). Counts come from what
/// the app says (the offer's "N Resources") and Resources are picked from the served
/// manifest by their set and size, not by name, so a new Starter library release does not
/// break them.
@MainActor
final class LibraryTests: XCTestCase {
    private var server: MirrorServer!
    /// The served Starter library manifest, in its order.
    private var manifest: [Entry] = []

    struct Entry: Decodable {
        let resourceId: String
        let abbreviation: String
        let bytes: UInt64
        let mobileSet: String?

        enum CodingKeys: String, CodingKey {
            case resourceId = "resource_id", abbreviation, bytes, mobileSet = "mobile_set"
        }
    }

    private struct Manifest: Decodable {
        let resources: [Entry]
    }

    private func entries(_ set: String) -> [Entry] {
        manifest.filter { $0.mobileSet == set }
    }

    /// "Resources, N" in About, the library's count.
    private func resourceCount(_ app: XCUIApplication) -> Int? {
        let count = app.staticTexts["hello.resourceCount"]
        guard count.waitForExistence(timeout: 10) else { return nil }
        return Int(count.label.replacingOccurrences(of: "Resources, ", with: ""))
    }

    override func tearDown() async throws {
        server?.stop()
        XCUIDevice.shared.appearance = .light
    }

    private func launch(network: String = "wifi", offer: Bool = true) throws -> XCUIApplication {
        guard let dir = ProcessInfo.processInfo.environment["OBS_STUDY_SET_DIR"] else {
            throw XCTSkip("no Starter library to serve: set TEST_RUNNER_OBS_STUDY_SET_DIR")
        }
        let root = URL(fileURLWithPath: dir, isDirectory: true)
        manifest = try JSONDecoder().decode(
            Manifest.self, from: Data(contentsOf: root.appendingPathComponent("starter-manifest.json"))
        ).resources
        server = try MirrorServer(root: root)
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
    /// an iPad a sheet floats over the dimmed reader, whose drawn text the audit reports with
    /// no element (contrast, detection, and text the sheet covers as clipped). That iPad
    /// filter is wide: it would hide such a finding inside the sheet too. It is accepted
    /// because the iPhone runs the same audits on the same screens, full screen with nothing
    /// behind them, filtering only Done (see the S24 Outcome).
    private func audit(_ app: XCUIApplication) throws {
        let pad = UIDevice.current.userInterfaceIdiom == .pad
        try app.performAccessibilityAudit { issue in
            let id = issue.element?.identifier ?? "-"
            let toolbar = id == "library.done" && [.contrast, .dynamicType].contains(issue.auditType)
            let behind = pad && issue.element == nil
                && [.contrast, .elementDetection, .textClipped].contains(issue.auditType)
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
        // The released list from the mirror: its first Study set row.
        let first = entries("study")[0].resourceId
        XCTAssertTrue(element(app, "starter.row.\(first)").waitForExistence(timeout: 60), "no released rows")
    }

    /// Whether `element` is gone and stays gone for 3 s (a banner between two states may
    /// blink), within `timeout`.
    private func settled(_ element: XCUIElement, timeout: TimeInterval) -> Bool {
        let end = Date().addingTimeInterval(timeout)
        var gone = 0
        while gone < 3 && Date() < end {
            gone = element.exists ? 0 : gone + 1
            sleep(1)
        }
        return gone >= 3
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
        XCTAssertTrue(size.waitForExistence(timeout: 60), "no Study set offer with its size")
        // The core's count of the Study set ("15 Resources, 136.6 MB").
        let study = try XCTUnwrap(Int(size.label.components(separatedBy: " ").first ?? ""), size.label)
        XCTAssertTrue(size.label.hasPrefix("\(study) Resources, "), size.label)
        XCTAssertEqual(study, entries("study").count, "the served manifest's Study set")
        XCTAssertEqual(app.staticTexts["offer.network"].label, "You are on Wi-Fi.")
        XCTAssertTrue(app.navigationBars["Get the Study set"].exists)
        // VoiceOver order: what it brings, its size, the network, then the buttons.
        let order = ["offer.message", "offer.size", "offer.network", "offer.download", "offer.notNow"]
            .map { element(app, $0).frame.minY }
        XCTAssertEqual(order, order.sorted(), "\(order)")
        shot(app, "s24-offer", dark: true)
        try audit(app)
        XCUIDevice.shared.appearance = .dark
        sleep(1)
        try audit(app)
        XCUIDevice.shared.appearance = .light
        sleep(1)

        // Slow enough (about 45 s in all) that the transfers outlast the app's time in the
        // background before it is suspended.
        server.throttle(512 * 1024)
        app.buttons["offer.download"].tap()
        XCTAssertTrue(app.buttons["offer.download"].waitForNonExistence(timeout: 10), "the offer stayed")
        let banner = app.buttons["reader.banner"]
        XCTAssertTrue(banner.waitForExistence(timeout: 20), "no download banner")
        XCTAssertTrue(banner.label.hasPrefix("Downloading "), banner.label)
        shot(app, "s24-banner", dark: true)

        // Away: the system carries the transfers on while the app is in the background (the
        // simulator may keep it running there rather than suspend it; the state is printed).
        let before = banner.label
        sleep(2)
        let sent = server.bytesServed
        XCUIDevice.shared.press(.home)
        let suspended = app.wait(for: .runningBackgroundSuspended, timeout: 20)
        XCTAssertNotEqual(app.state, .runningForeground)
        let away = server.bytesServed - sent
        print("away: suspended \(suspended) (state \(app.state.rawValue)), \(away) bytes sent while away; banner was \(before)")
        XCTAssertGreaterThan(away, 2_000_000, "the transfers stopped while the app was away")
        app.activate()
        XCTAssertTrue(app.wait(for: .runningForeground, timeout: 20))
        // Every transfer done, checked and installed: the core's banner goes (it stays while
        // any download is queued, moving, being installed or indexed, or failed).
        XCTAssertTrue(settled(banner, timeout: 600), "still downloading: \(banner.exists ? banner.label : "-")")
        let gets = server.requests.filter { $0.method == "GET" && $0.path.hasSuffix(".obr") }
        XCTAssertEqual(Set(gets.map(\.path)).count, study, "\(gets.map(\.path))")

        openStarterLibrary(app)
        XCTAssertFalse(element(app, "starter.studySet.download").exists, "the Study set is in")
        XCTAssertTrue(element(app, "starter.remove.\(entries("study")[0].resourceId)").exists)
        shot(app, "s24-starter-library", dark: true)
        try audit(app)
        app.navigationBars.buttons["BackButton"].firstMatch.tap()
        app.buttons["library.done"].tap()
        app.buttons["reader.options"].tap()
        app.buttons["About"].tap()
        // The Bundled set and the Study set.
        XCTAssertEqual(resourceCount(app), entries("bundled").count + study)
    }

    /// #333: "a mismatch deletes it and offers Retry".
    func testABadHashShowsRetry() throws {
        let app = try launch(offer: false)
        // The first Study set row, so the screen is not scrolled for its audit.
        let first = entries("study")[0]
        let id = first.resourceId
        server.corrupt(["\(id).obr"])
        openLibrary(app)
        shot(app, "s24-library", dark: true)
        try audit(app)
        element(app, "library.starter").tap()
        XCTAssertTrue(element(app, "starter.row.\(id)").waitForExistence(timeout: 60))
        scrolled(to: "starter.download.\(id)", in: app).tap()
        let retry = app.buttons["download.retry.\(id)"]
        XCTAssertTrue(retry.waitForExistence(timeout: 120), "no Retry")
        XCTAssertEqual(retry.label, "Retry the download of \(first.abbreviation)")
        let state = app.staticTexts["download.state.\(id)"]
        XCTAssertFalse(state.label.isEmpty)
        print("bad hash: \(state.label)")
        shot(app, "s24-retry", dark: true)
        try audit(app)
        server.corrupt([])
        retry.tap()
        XCTAssertTrue(element(app, "starter.remove.\(id)").waitForExistence(timeout: 120), "not installed")
    }

    /// #333: "Downloads use Wi-Fi only by default … with it off, a download over cellular
    /// shows its size and asks first", on a cellular path the test stands in for.
    func testWiFiOnlyAsksFirstOnCellular() throws {
        let app = try launch(network: "cellular")
        let network = app.staticTexts["offer.network"]
        XCTAssertTrue(app.staticTexts["offer.size"].waitForExistence(timeout: 60))
        XCTAssertEqual(network.label, "You are not on Wi-Fi. Download asks before it uses cellular data.")
        app.buttons["offer.download"].tap()
        let ask = app.alerts.firstMatch
        XCTAssertTrue(ask.waitForExistence(timeout: 10), "no cellular question")
        XCTAssertTrue(ask.label.hasPrefix("Download the Study set ("), ask.label)
        XCTAssertTrue(ask.label.contains("over cellular data?"), ask.label)
        shot(app, "s24-cellular")
        ask.buttons["Not now"].firstMatch.tap()
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
        XCTAssertTrue(element(app, "starter.row.\(entries("study")[0].resourceId)").waitForExistence(timeout: 60))
        // The smallest of the Study set.
        let small = try XCTUnwrap(entries("study").min { $0.bytes < $1.bytes }).resourceId
        scrolled(to: "starter.download.\(small)", in: app).tap()
        XCTAssertFalse(app.alerts.firstMatch.waitForExistence(timeout: 2), "asked anyway")
        // Installed: the Bundled set and that one.
        app.navigationBars.buttons["BackButton"].firstMatch.tap()
        app.buttons["library.done"].tap()
        XCTAssertTrue(settled(app.buttons["reader.banner"], timeout: 120), "still downloading")
        app.buttons["reader.options"].tap()
        app.buttons["About"].tap()
        XCTAssertEqual(resourceCount(app), entries("bundled").count + 1)
    }

    /// #333: "Cancel removes partial files" (the files themselves: DownloadTests).
    func testCancelStopsADownload() throws {
        let app = try launch(offer: false)
        server.throttle(256 * 1024)
        openStarterLibrary(app)
        // The first large Resource outside the Study set (the World English Bible, 41.7 MB),
        // near the top, so the screen is not scrolled for its audit.
        let id = try XCTUnwrap(entries("more").first { $0.bytes > 10_000_000 }).resourceId
        scrolled(to: "starter.download.\(id)", in: app).tap()
        let cancel = app.buttons["download.cancel.\(id)"]
        XCTAssertTrue(cancel.waitForExistence(timeout: 30), "no Cancel")
        let state = app.staticTexts["download.state.\(id)"]
        let moving = NSPredicate(format: "label BEGINSWITH 'Downloading · '")
        expectation(for: moving, evaluatedWith: state)
        waitForExpectations(timeout: 60)
        // The banner: in the list above (scrolled away in a short iPad sheet) and the reader's.
        XCTAssertTrue(element(app, "library.banner").exists || app.buttons["reader.banner"].exists)
        shot(app, "s24-downloading", dark: true)
        try audit(app)
        cancel.tap()
        XCTAssertTrue(element(app, "starter.download.\(id)").waitForExistence(timeout: 20), "not cancelled")
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
