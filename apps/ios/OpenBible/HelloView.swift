import SwiftUI

/// The S06 hello screen: the engine's version, then the library the core booted with.
struct HelloView: View {
    let model: HelloModel

    var body: some View {
        NavigationStack {
            List {
                Section {
                    LabeledContent("Engine", value: model.engineVersion)
                        .accessibilityIdentifier("hello.engine")
                }
                switch model.state {
                case .starting:
                    Section {
                        ProgressView("Starting…")
                    }
                case .ready(let resourceIds, let problems):
                    Section("Library") {
                        LabeledContent("Resources", value: "\(resourceIds.count)")
                            .accessibilityIdentifier("hello.resourceCount")
                        ForEach(resourceIds, id: \.self) { id in
                            Text(id).font(.body.monospaced())
                        }
                    }
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
