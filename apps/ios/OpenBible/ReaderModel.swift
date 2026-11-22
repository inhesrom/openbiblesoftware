import Foundation
import Observation
import OpenBibleCore
import UIKit
import os

/// The reader screen's state over the shared core (`AppCore`): it forwards what the user
/// does (a typed reference, a chapter, a Page tone, a Bible) to the core and shows what the
/// core answers. Debug launch arguments set the screen up for screenshots and the fling bench
/// (`docs/dev/mobile-text-view.md`).
@MainActor
@Observable
final class ReaderModel {
    enum State: Equatable {
        case starting
        case ready
        case failed(message: String)
    }

    struct Sheet: Identifiable {
        let id = UUID()
        let verse: HitVerse
        let word: HitWord?
    }

    let app: AppCore
    private(set) var state: State = .starting
    /// The reader this screen shows: the first.
    let reader: UInt32 = 0
    private(set) var location = ""
    var referenceError: String?
    var sheet: Sheet?
    private(set) var tones: [Choice] = []
    private(set) var tone = ""
    private(set) var bibles: [LibraryResource] = []
    private(set) var bible = ""
    private(set) var books: [BookChapters] = []
    private(set) var metricsText = ""
    private(set) var benchResult = ""
    /// An Import's report changed (`ImportModel` shows it).
    @ObservationIgnored var onImports: (() -> Void)?
    /// The library changed (`LibraryModel` shows it, Storage too).
    @ObservationIgnored var onLibrary: (() -> Void)?
    /// A download or the Study set offer changed (`LibraryModel` shows them).
    @ObservationIgnored var onDownloads: (() -> Void)?
    /// The measurements strip (frames dropped, tile times) under the text.
    var showMeasurements = UserDefaults.standard.bool(forKey: "OBSMeasurements")
    private(set) var coldStart: Duration?
    @ObservationIgnored private weak var view: ResourceTextView?
    @ObservationIgnored private var bench: FlingBench?
    /// When start-up reached each step, in ms since the process started (the bench reports them).
    @ObservationIgnored private var startupMarks: [String] = []

    private static let log = Logger(subsystem: "io.github.inhesrom.openbible", category: "reader")

    init(app: AppCore) {
        self.app = app
    }

    var core: MobileApp? { app.core }

    func start() async {
        let since = { ProcessStart.date.map { Int(Date().timeIntervalSince($0) * 1000) } ?? -1 }
        startupMarks.append("start() at \(since()) ms")
        await app.start()
        switch app.state {
        case .starting:
            return
        case .failed(let message):
            state = .failed(message: message)
            return
        case .ready(let core):
            startupMarks.append("booted at \(since()) ms")
            app.onChange = { [weak self] change in self?.changed(change) }
            refreshChoices(core)
            applyLaunchArguments()
            let hasReader = ((try? core.readers()) ?? []).isEmpty == false
            state = hasReader ? .ready : .failed(message: "No Bible in the library")
        }
    }

    /// The Page tones, the Bibles to switch to and the grid's books, as the core lists them.
    private func refreshChoices(_ core: MobileApp) {
        if let settings = try? core.settings() {
            tones = settings.pageTone.choices
            tone = settings.pageTone.current
        }
        bibles = (try? core.parallelResources(reader: reader)) ?? []
        bible = (try? core.reader(reader: reader))?.resourceId ?? ""
        books = (try? core.books(reader: reader)) ?? []
    }

    /// What the core reported from its pump.
    private func changed(_ change: Changed) {
        switch change {
        case .reader(let r) where r == reader:
            view?.refresh()
        case .library:
            if let core { refreshChoices(core) }
            onLibrary?()
        case .downloads:
            onDownloads?()
        case .message(let text):
            Self.log.info("core: \(text, privacy: .public)")
        case .imports:
            onImports?()
        default:
            break
        }
    }

    /// Debug launch arguments: `-OBSResource tahot`, `-OBSPageTone sepia`, `-OBSGoTo "Jn 3:16"`.
    private func applyLaunchArguments() {
        let defaults = UserDefaults.standard
        if let id = defaults.string(forKey: "OBSResource") { open(bible: id) }
        if let id = defaults.string(forKey: "OBSPageTone") { setTone(id) }
        if let text = defaults.string(forKey: "OBSGoTo") { goTo(text) }
    }

    func attach(_ view: ResourceTextView) {
        self.view = view
        view.onLocation = { [weak self] in self?.location = $0 }
        view.onTap = { [weak self] hit in
            guard let verse = hit.verse else { return }
            self?.sheet = Sheet(verse: verse, word: hit.word)
        }
        view.metrics.onChange = { [weak self, weak view] in
            guard let self, let view else { return }
            self.metricsText = view.metrics.summary()
        }
        view.onFirstText = { [weak self] in self?.firstText() }
    }

    private func firstText() {
        if let started = ProcessStart.date {
            let took = Date().timeIntervalSince(started)
            coldStart = .milliseconds(Int(took * 1000))
            Self.log.info("cold start to first text: \(Int(took * 1000), privacy: .public) ms")
        }
        if let until = UserDefaults.standard.string(forKey: "OBSBenchUntil"), let view {
            let speed = UserDefaults.standard.double(forKey: "OBSBenchSpeed")
            bench = FlingBench(view: view, until: until, speed: speed > 0 ? speed : 8000) {
                [weak self] summary in
                self?.benchDone(summary)
            }
            bench?.start()
        }
    }

    private func benchDone(_ summary: String) {
        let cold = coldStart.map { "\($0.components.seconds * 1000 + $0.components.attoseconds / 1_000_000_000_000_000) ms" } ?? "?"
        let marks = startupMarks.joined(separator: ", ")
        let result = "\(summary) · cold start \(cold) (\(marks)) · footprint \(Int(Memory.footprintMB())) MB"
        benchResult = result
        showMeasurements = true
        Self.log.info("bench: \(result, privacy: .public)")
        if let docs = FileManager.default.urls(for: .documentDirectory, in: .userDomainMask).first {
            try? result.write(to: docs.appendingPathComponent("s07-bench.txt"), atomically: true, encoding: .utf8)
        }
        bench = nil
    }

    /// Go to what the user typed; the core reads it (a reference, or an entry Key).
    func goTo(_ text: String) {
        guard let core else { return }
        do {
            _ = try core.navigate(reader: reader, text: text)
            referenceError = nil
            view?.adoptPosition()
        } catch {
            referenceError = AppCore.message(error)
        }
    }

    /// Go to a chapter the grid offers (the `book`th of the core's books).
    func goTo(book: Int, chapter: UInt16) {
        guard let core, books.indices.contains(book) else { return }
        do {
            _ = try core.goToVerse(reader: reader, book: books[book].id, chapter: chapter, verse: nil)
            referenceError = nil
            view?.adoptPosition()
        } catch {
            referenceError = AppCore.message(error)
        }
    }

    func setTone(_ id: String) {
        guard let core else { return }
        if let settings = try? core.setPageTone(id: id) {
            tone = settings.pageTone.current
        }
        view?.refresh()
    }

    /// Show another Bible in the reader (the core switches it at the same place).
    func open(bible id: String) {
        guard let core, id != bible else { return }
        do {
            _ = try core.open(reader: reader, resourceId: id, at: nil)
            refreshChoices(core)
            view?.show(reader: reader)
        } catch {
            referenceError = AppCore.message(error)
        }
    }
}

/// Debug fling bench: scrolls the text view at a constant speed (points per second) from
/// where it is until the Current location starts with `until` (a book name the core shows),
/// one step per display frame, as a fling's deceleration would move it, and reports the
/// metrics.
@MainActor
final class FlingBench {
    private let view: ResourceTextView
    private let until: String
    private let speed: Double
    private let done: (String) -> Void
    private var link: CADisplayLink?
    private var last: CFTimeInterval = 0
    private var began = ContinuousClock.now

    init(view: ResourceTextView, until: String, speed: Double, done: @escaping (String) -> Void) {
        self.view = view
        self.until = until
        self.speed = speed
        self.done = done
    }

    func start() {
        view.metrics.reset()
        view.metrics.beginScroll()
        began = .now
        let link = CADisplayLink(target: BenchTarget(self), selector: #selector(BenchTarget.tick(_:)))
        link.add(to: .main, forMode: .common)
        self.link = link
    }

    fileprivate func tick(_ link: CADisplayLink) {
        defer { last = link.timestamp }
        guard last > 0 else { return }
        let step = (link.timestamp - last) * speed
        view.contentOffset.y += step
        if (view.lastFrame?.location ?? "").hasPrefix(until) {
            link.invalidate()
            self.link = nil
            view.metrics.endScroll()
            let took = ContinuousClock.now - began
            done("fling \(Int(speed)) pt/s for \(took.components.seconds) s · \(view.metrics.summary())")
        }
    }
}

@MainActor
private final class BenchTarget: NSObject {
    weak var bench: FlingBench?
    init(_ bench: FlingBench) { self.bench = bench }
    @objc func tick(_ link: CADisplayLink) { bench?.tick(link) }
}

/// When this process started, for cold start to first text.
enum ProcessStart {
    static var date: Date? {
        var info = kinfo_proc()
        var size = MemoryLayout<kinfo_proc>.stride
        var mib: [Int32] = [CTL_KERN, KERN_PROC, KERN_PROC_PID, getpid()]
        guard sysctl(&mib, 4, &info, &size, nil, 0) == 0 else { return nil }
        let t = info.kp_proc.p_un.__p_starttime
        return Date(timeIntervalSince1970: Double(t.tv_sec) + Double(t.tv_usec) / 1_000_000)
    }
}

/// The app's memory footprint (what Xcode's memory gauge shows).
enum Memory {
    static func footprintMB() -> Double {
        var info = task_vm_info_data_t()
        var count = mach_msg_type_number_t(
            MemoryLayout<task_vm_info_data_t>.size / MemoryLayout<integer_t>.size)
        let result = withUnsafeMutablePointer(to: &info) {
            $0.withMemoryRebound(to: integer_t.self, capacity: Int(count)) {
                task_info(task_self_trap(), task_flavor_t(TASK_VM_INFO), $0, &count)
            }
        }
        return result == KERN_SUCCESS ? Double(info.phys_footprint) / 1_048_576 : 0
    }
}
