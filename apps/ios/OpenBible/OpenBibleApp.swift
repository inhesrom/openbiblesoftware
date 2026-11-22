import SwiftUI

@main
struct OpenBibleApp: App {
    /// The reader screen's model; the core under it is `AppCore.shared`, one per process.
    @State private var reader = ReaderModel(app: .shared)
    /// Import from the picker and "Open in Open Bible" (S26), over the same core.
    @State private var importer = ImportModel(app: .shared)
    @Environment(\.scenePhase) private var scenePhase

    var body: some Scene {
        WindowGroup {
            ReaderScreen(model: reader, importer: importer)
                .task {
                    reader.onImports = { [importer] in importer.refresh() }
                    importer.onOpen = { [reader] id in reader.open(bible: id) }
                    await reader.start()
                    importer.coreReady()
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
