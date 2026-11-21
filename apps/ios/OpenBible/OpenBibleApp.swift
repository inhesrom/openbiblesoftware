import SwiftUI

@main
struct OpenBibleApp: App {
    @State private var model = HelloModel()

    var body: some Scene {
        WindowGroup {
            HelloView(model: model)
                .task { await model.start() }
        }
    }
}
