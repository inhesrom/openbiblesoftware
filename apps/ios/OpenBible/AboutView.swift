import OpenBibleCore
import SwiftUI

/// The About sheet (the S06 hello screen): the engine's version, then the library the core
/// booted with, in the core's order.
struct AboutView: View {
    let app: AppCore

    var body: some View {
        NavigationStack {
            List {
                Section {
                    LabeledContent("Engine", value: app.engineVersion)
                        .accessibilityIdentifier("hello.engine")
                }
                switch app.state {
                case .starting:
                    Section {
                        ProgressView("Starting…")
                    }
                case .ready(let core):
                    let library = (try? core.library()) ?? []
                    Section("Library") {
                        LabeledContent("Resources", value: "\(library.count)")
                            .accessibilityIdentifier("hello.resourceCount")
                        ForEach(library, id: \.resourceId) { item in
                            Text(item.resourceId).font(.body.monospaced())
                        }
                    }
                    let problems = (try? core.problems()) ?? []
                    if !problems.isEmpty {
                        Section("Problems") {
                            ForEach(problems, id: \.self) { Text($0) }
                        }
                    }
                case .failed(let message):
                    Section("Could not start") {
                        Text(message)
                    }
                }
            }
            .navigationTitle("Open Bible")
        }
    }
}
