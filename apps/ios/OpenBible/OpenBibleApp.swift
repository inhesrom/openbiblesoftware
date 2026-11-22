import SwiftUI
import UIKit

/// The app delegate, for what SwiftUI has no scene for: the system waking the app for its
/// background downloads' events (`DownloadCenter`), when no scene may be connected.
final class AppDelegate: NSObject, UIApplicationDelegate {
    func application(
        _ application: UIApplication, didFinishLaunchingWithOptions _: [UIApplication.LaunchOptionsKey: Any]? = nil
    ) -> Bool {
        // The background sessions are made again at once, so finished transfers are heard.
        _ = DownloadCenter.shared
        return true
    }

    func application(
        _ application: UIApplication, handleEventsForBackgroundURLSession identifier: String,
        completionHandler: @escaping () -> Void
    ) {
        // UIKit calls this on the main thread and wants its handler called there, as the
        // session does (`urlSessionDidFinishEvents`).
        nonisolated(unsafe) let handler = completionHandler
        guard DownloadCenter.shared.handleEvents(for: identifier, completion: { handler() }) else {
            completionHandler()
            return
        }
        // Launched in the background: the core takes the finished files once it is up.
        Task { @MainActor in await AppCore.shared.start() }
    }
}

@main
struct OpenBibleApp: App {
    @UIApplicationDelegateAdaptor(AppDelegate.self) private var delegate
    /// The reader screen's model; the core under it is `AppCore.shared`, one per process.
    @State private var reader = ReaderModel(app: .shared)
    /// Import from the picker and "Open in Open Bible" (S26), over the same core.
    @State private var importer = ImportModel(app: .shared)
    /// The Library, the downloads and the Study set offer (S24), over the same core.
    @State private var library = LibraryModel(app: .shared)
    @Environment(\.scenePhase) private var scenePhase

    var body: some Scene {
        WindowGroup {
            ReaderScreen(model: reader, importer: importer, library: library)
                .task {
                    reader.onImports = { [importer] in importer.refresh() }
                    reader.onLibrary = { [library] in library.libraryChanged() }
                    reader.onDownloads = { [library] in library.downloadsChanged() }
                    importer.onOpen = { [reader] id in reader.open(bible: id) }
                    await reader.start()
                    importer.coreReady()
                    library.coreReady()
                }
                .onOpenURL { importer.importFile(at: $0) }
        }
        .onChange(of: scenePhase) { _, phase in
            switch phase {
            case .background: AppCore.shared.background()
            case .active: AppCore.shared.resume()
            default: break
            }
        }
    }
}
