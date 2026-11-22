import Foundation
import Observation
import OpenBibleCore
import UniformTypeIdentifiers
import os

/// The file kinds Import offers (#333 "Import"), as `project.yml` declares them: USFM books
/// (`.usfm`, `.sfm`), zips (of USFM books or of one SWORD module) and Resource files
/// (`.obr`). The picker shows only these; what a file holds is the core's to tell.
enum ImportTypes {
    static let usfm = UTType(importedAs: "org.ubsicap.usfm")
    static let resource = UTType(exportedAs: "io.github.inhesrom.openbible.obr")
    static let all: [UTType] = [usfm, .zip, resource]
}

/// Import from the document picker or "Open in Open Bible" (`docs/dev/mobile-library.md`
/// § Import): the core names where the file goes, Swift copies it there inside the file's
/// security-scoped access (the original stays where it was), the core imports it, and the
/// Import report sheet shows the core's report as it moves on.
@MainActor
@Observable
final class ImportModel {
    /// The report sheet's Import.
    struct Shown: Identifiable, Equatable {
        let id: UInt64
    }

    let app: AppCore
    /// The document picker is up.
    var picking = false
    /// The report sheet is up, for this Import.
    var shown: Shown?
    /// The shown Import's report, as the core last gave it.
    private(set) var report: ImportReport?
    /// Files shared before the core was ready.
    @ObservationIgnored private var waiting: [URL] = []
    /// What to do when the report's Open is tapped.
    @ObservationIgnored var onOpen: ((String) -> Void)?

    private static let log = Logger(subsystem: "io.github.inhesrom.openbible", category: "import")

    init(app: AppCore) {
        self.app = app
    }

    /// The picker's answer.
    func picked(_ result: Result<[URL], Error>) {
        switch result {
        case .success(let urls):
            for url in urls { importFile(at: url) }
        case .failure(let error):
            Self.log.error("picker: \(error.localizedDescription, privacy: .public)")
        }
    }

    /// Import the file at `url` (from the picker, or shared to the app); one shared before
    /// the core is ready waits for it.
    func importFile(at url: URL) {
        guard let core = app.core else {
            waiting.append(url)
            return
        }
        Task { await self.copyAndImport(url, core: core) }
    }

    /// The core is ready: import what was shared before.
    func coreReady() {
        let urls = waiting
        waiting = []
        for url in urls { importFile(at: url) }
    }

    private func copyAndImport(_ url: URL, core: MobileApp) async {
        let destination: String
        do { destination = try core.importDestination(fileName: url.lastPathComponent) } catch {
            Self.log.error("import: \(AppCore.message(error), privacy: .public)")
            return
        }
        do {
            try await Task.detached(priority: .userInitiated) {
                try Self.copy(url, to: URL(fileURLWithPath: destination))
            }.value
        } catch {
            // The report says so, in the core's words, with the error under Details.
            if let number = try? core.importCopyFailed(path: destination, error: error.localizedDescription) {
                show(number)
            }
            return
        }
        do {
            show(try core.startImport(path: destination))
        } catch {
            Self.log.error("import: \(AppCore.message(error), privacy: .public)")
        }
    }

    /// Copy the picked or shared file into the app, inside its security-scoped access and a
    /// coordinated read (the file may be in iCloud Drive or another provider). A file the
    /// system put in the app's `Documents/Inbox` for "Open in" (it copies there when the app
    /// does not open documents in place) is the app's own copy, deleted once copied.
    nonisolated static func copy(_ url: URL, to destination: URL) throws {
        let scoped = url.startAccessingSecurityScopedResource()
        defer { if scoped { url.stopAccessingSecurityScopedResource() } }
        var coordinated: NSError?
        var copied: Error?
        NSFileCoordinator().coordinate(readingItemAt: url, options: [], error: &coordinated) { from in
            do { try FileManager.default.copyItem(at: from, to: destination) } catch { copied = error }
        }
        if let error = coordinated ?? copied { throw error }
        if let inbox = try? FileManager.default.url(
            for: .documentDirectory, in: .userDomainMask, appropriateFor: nil, create: false
        ).appendingPathComponent("Inbox", isDirectory: true),
            url.standardizedFileURL.path.hasPrefix(inbox.standardizedFileURL.path + "/")
        {
            try? FileManager.default.removeItem(at: url)
        }
    }

    private func show(_ number: UInt64) {
        shown = Shown(id: number)
        refresh()
    }

    /// The core said an Import's report changed.
    func refresh() {
        guard let core = app.core, let shown else { return }
        report = try? core.importReport(import: shown.id)
    }

    func cancel() {
        guard let core = app.core, let shown else { return }
        try? core.cancelImport(import: shown.id)
    }

    func open() {
        guard let id = report?.resourceId else { return }
        shown = nil
        onOpen?(id)
    }

    func close() {
        shown = nil
        report = nil
    }
}
