import Foundation
import Observation
import OpenBibleCore
import os

/// Library → Starter library, Get Resources and Storage, the download banner and the
/// first-launch Study set offer over the shared core (`docs/dev/mobile-library.md`). Every
/// row, size, licence, state and question is the core's; this forwards the taps (with the
/// network `NetworkMonitor` sees and the free bytes the volume reports), starts the
/// transfers the core plans on `DownloadCenter`, and asks again on `Changed.downloads`.
@MainActor
@Observable
final class LibraryModel {
    /// A question or message the core answered a tap with, shown as an alert.
    enum Question: Identifiable {
        /// On cellular with Use cellular data off: the core's question with the size.
        case cellular(message: String, again: Again)
        /// Not enough free space, in the core's words.
        case wontFit(message: String)
        /// Remove's question, in the core's words.
        case remove(resourceId: String, question: String)
        /// A refusal, in the core's words.
        case failed(message: String)

        var id: String {
            switch self {
            case .cellular(let m, _), .wontFit(let m), .remove(_, let m), .failed(let m): return m
            }
        }
    }

    /// The download to plan again once the user said yes to cellular data.
    enum Again {
        case starter(StarterChoice)
        case open(String)
    }

    let app: AppCore
    let network: NetworkMonitor
    let downloads: DownloadCenter
    /// The Library sheet is up.
    var showing = false
    private(set) var starter: StarterLibraryView?
    private(set) var openRows: [OpenRow] = []
    let filters = licenceFilters()
    var filter: LicenceFilter = licenceFilters().default {
        didSet { refresh() }
    }
    private(set) var storage: StorageView?
    private(set) var banner: String?
    private(set) var useCellular = false
    /// The first-launch Study set offer, while the core makes one and its sheet is up.
    var offer: StudySetOffer?
    var question: Question?

    private static let log = Logger(subsystem: "io.github.inhesrom.openbible", category: "library")

    init(app: AppCore, network: NetworkMonitor = .shared, downloads: DownloadCenter = .shared) {
        self.app = app
        self.network = network
        self.downloads = downloads
    }

    private var core: MobileApp? { app.core }

    /// The core is ready: show the Study set offer if the core makes one.
    func coreReady() {
        network.start()
        refresh()
        #if DEBUG
            // Test harness: the UI tests that do not test the offer launch without it.
            if ProcessInfo.processInfo.environment["OBS_TEST_NO_STUDY_SET_OFFER"] == "1" { return }
        #endif
        guard let core else { return }
        offer = try? core.studySetOffer(network: network.network(core))
    }

    /// The offer's sheet appeared: the core records it, and looks up the Study set.
    func offerShown() {
        guard let core else { return }
        do { try core.studySetOfferShown() } catch {
            Self.log.error("offer: \(AppCore.message(error), privacy: .public)")
        }
        if offer?.size == nil { _ = try? core.checkStarter() }
    }

    /// The offer's Try again.
    func offerRetry() {
        _ = try? core?.checkStarter()
    }

    /// The offer's Download: the sheet closes once the download starts (it stays up for
    /// the core's cellular or free-space question).
    func offerDownload() {
        download(.set(set: .study))
        if question == nil { offer = nil }
    }

    func offerNotNow() {
        offer = nil
    }

    /// The Library sheet opened: the core fetches the released Starter library list.
    func open() {
        showing = true
        _ = try? core?.checkStarter()
        refresh()
    }

    /// Ask the core again for everything shown.
    func refresh() {
        guard let core else { return }
        starter = try? core.starterLibrary()
        openRows = (try? core.openCatalogue(licence: filter)) ?? []
        storage = try? core.storage()
        banner = try? core.downloadBanner()
        useCellular = (try? core.useCellular()) ?? false
        if offer != nil {
            offer = try? core.studySetOffer(network: network.network(core))
        }
    }

    func setUseCellular(_ on: Bool) {
        do { try core?.setUseCellular(on: on) } catch { question = .failed(message: AppCore.message(error)) }
        refresh()
    }

    /// A Starter library Download, Update, Add back or Retry (or the Study set's).
    func download(_ choice: StarterChoice, cellularConfirmed: Bool = false) {
        guard let core else { return }
        let plan = Result {
            try core.planStarterDownload(
                choice: choice, network: network.network(core), cellularConfirmed: cellularConfirmed,
                freeBytes: freeBytes())
        }
        answer(plan, again: .starter(choice))
    }

    /// An Open catalogue row's Download or Retry.
    func downloadOpen(_ id: String, cellularConfirmed: Bool = false) {
        guard let core else { return }
        let plan = Result {
            try core.planOpenDownload(
                id: id, network: network.network(core), cellularConfirmed: cellularConfirmed,
                freeBytes: freeBytes())
        }
        answer(plan, again: .open(id))
    }

    /// Yes to the core's cellular question: plan it again, confirmed.
    func confirmCellular(_ again: Again) {
        question = nil
        switch again {
        case .starter(let choice): download(choice, cellularConfirmed: true)
        case .open(let id): downloadOpen(id, cellularConfirmed: true)
        }
        if question == nil { offer = nil }
    }

    private func answer(_ plan: Result<DownloadPlan, Error>, again: Again) {
        switch plan {
        case .success(.start(let requests)):
            downloads.start(requests)
        case .success(.askCellular(_, let message)):
            question = .cellular(message: message, again: again)
        case .success(.wontFit(_, _, let message)):
            question = .wontFit(message: message)
        case .success(.nothing):
            break
        case .failure(let error):
            question = .failed(message: AppCore.message(error))
        }
        refresh()
    }

    func cancel(_ resourceId: String, origin: DownloadOrigin) {
        guard let core else { return }
        do { try downloads.cancel(resourceId: resourceId, origin: origin, core: core) } catch {
            question = .failed(message: AppCore.message(error))
        }
        refresh()
    }

    /// Remove: the core's question first, then removed once the user says yes.
    func remove(_ resourceId: String, confirmed: Bool = false) {
        guard let core else { return }
        do {
            switch try core.removeResource(resourceId: resourceId, confirmed: confirmed) {
            case .confirm(let question): self.question = .remove(resourceId: resourceId, question: question)
            case .removed: break
            }
        } catch {
            question = .failed(message: AppCore.message(error))
        }
        refresh()
    }

    /// The free bytes of the app's volume for important use (what the system would free for
    /// a download the user asked for), for the core's "won't fit" rule; nil when unknown.
    private func freeBytes() -> UInt64? {
        guard let folder = app.folder,
            let values = try? folder.resourceValues(forKeys: [.volumeAvailableCapacityForImportantUsageKey]),
            let free = values.volumeAvailableCapacityForImportantUsage
        else { return nil }
        return UInt64(max(free, 0))
    }
}
