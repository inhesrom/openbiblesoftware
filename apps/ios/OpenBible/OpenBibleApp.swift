import SwiftUI

@main
struct OpenBibleApp: App {
    /// The reader screen's model; the core under it is `AppCore.shared`, one per process.
    @State private var reader = ReaderModel(app: .shared)
    @Environment(\.scenePhase) private var scenePhase

    var body: some Scene {
        WindowGroup {
            ReaderScreen(model: reader)
                .task { await reader.start() }
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
