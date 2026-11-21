import Foundation
import Observation
import OpenBibleCore
import UIKit
import os

/// S07 prototype (#331): the reader screen's state over the core. It boots the core as
/// `HelloModel` does, then forwards what the user does (a typed reference, a chapter, a Page
/// tone, a Bible) to the core and shows what the core answers. Debug launch arguments set
/// the screen up for screenshots and the fling bench (`docs/dev/mobile-text-view.md`).
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
        let stub: StudySheetStub
    }

    private(set) var state: State = .starting
    private(set) var core: MobileApp?
    private(set) var reader: UInt32 = 0
    private(set) var location = ""
    var referenceError: String?
    var sheet: Sheet?
    private(set) var tones: [Choice] = []
    private(set) var tone = ""
    private(set) var bibles: [Choice] = []
    private(set) var bible = ""
    private(set) var books: [BookChapters] = []
    private(set) var metricsText = ""
    private(set) var benchResult = ""
    /// The prototype's measurements strip (frames dropped, tile times) under the text.
    var showMeasurements = UserDefaults.standard.bool(forKey: "OBSMeasurements")
    private(set) var coldStart: Duration?
    @ObservationIgnored private weak var view: ResourceTextView?
    @ObservationIgnored private var bench: FlingBench?
    /// When start-up reached each step, in ms since the process started (the bench reports them).
    @ObservationIgnored private var startupMarks: [String] = []

    private static let log = Logger(subsystem: "io.github.inhesrom.openbible", category: "reader")

    func start() async {
        let folder: URL
        do { folder = try HelloModel.appFolder() } catch {
            state = .failed(message: "\(error)")
            return
        }
        var paths = HelloModel.paths(in: folder)
        #if DEBUG
            // The debug Study set override (plan S07): a Starter library folder on the Mac.
            paths.studySet = ProcessInfo.processInfo.environment["OBS_STUDY_SET_DIR"]
        #endif
        let bundle = HelloModel.bundledSet()?.path
        let since = { ProcessStart.date.map { Int(Date().timeIntervalSince($0) * 1000) } ?? -1 }
        startupMarks.append("start() at \(since()) ms")
        do {
            let core = try await Task.detached(priority: .userInitiated) { [paths] in
                try MobileApp.boot(paths: paths, bundle: bundle)
            }.value
            self.core = core
            startupMarks.append("booted at \(since()) ms")
            for problem in core.problems() { Self.log.error("boot: \(problem, privacy: .public)") }
            let pageTones = core.pageTones()
            tones = pageTones.choices
            tone = pageTones.current
            bibles = core.bibles()
            bible = core.readerResource(reader: 0) ?? ""
            books = core.books(reader: 0)
            applyLaunchArguments(core)
            state = core.readerResource(reader: 0) == nil ? .failed(message: "No Bible in the library") : .ready
        } catch {
            state = .failed(message: Self.message(error))
        }
    }

    /// Debug launch arguments: `-OBSResource tahot`, `-OBSPageTone sepia`, `-OBSGoTo "Jn 3:16"`.
    private func applyLaunchArguments(_ core: MobileApp) {
        let defaults = UserDefaults.standard
        if let id = defaults.string(forKey: "OBSResource") { open(bible: id) }
        if let id = defaults.string(forKey: "OBSPageTone") { setTone(id) }
        if let text = defaults.string(forKey: "OBSGoTo") { goTo(text) }
    }

    static func message(_ error: Error) -> String {
        if case MobileError.Failed(let message) = error { return message }
        return "\(error)"
    }

    func attach(_ view: ResourceTextView) {
        self.view = view
        view.onLocation = { [weak self] in self?.location = $0 }
        view.onTap = { [weak self] in self?.sheet = Sheet(stub: $0) }
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

    func goTo(_ text: String) {
        guard let core else { return }
        do {
            _ = try core.goTo(reader: reader, text: text)
            referenceError = nil
            view?.adoptPosition()
        } catch {
            referenceError = Self.message(error)
        }
    }

    func goTo(book: Int, chapter: UInt16) {
        guard let core else { return }
        do {
            _ = try core.goToChapter(reader: reader, book: UInt32(book), chapter: chapter)
            referenceError = nil
            view?.adoptPosition()
        } catch {
            referenceError = Self.message(error)
        }
    }

    func setTone(_ id: String) {
        core?.setPageTone(id: id)
        tone = core?.pageTones().current ?? id
        view?.refresh()
    }

    func open(bible id: String) {
        guard let core, id != bible else { return }
        do {
            reader = try core.openReader(resourceId: id, reference: nil)
            bible = id
            books = core.books(reader: reader)
            view?.show(reader: reader)
        } catch {
            referenceError = Self.message(error)
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
