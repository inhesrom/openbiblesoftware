import SwiftUI

@main
struct OpenBibleApp: App {
    @State private var hello = HelloModel()
    @State private var reader = ReaderModel()

    var body: some Scene {
        WindowGroup {
            // S07 prototype: the reader. The S06 hello screen is its About sheet.
            ReaderScreen(model: reader, hello: hello)
                .task { await reader.start() }
        }
    }
}
