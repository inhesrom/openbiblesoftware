import OpenBibleCore
import SwiftUI

/// The Library sheet (S24): Starter library, Get Resources and Storage as pushed lists, the
/// download banner and Use cellular data. It is a sheet from the reader's options menu until
/// the screen anatomy (#335, S12/S14a) gives the Library its place; S14b moves Use cellular
/// data into Settings. Every row, size, licence, state and question is the core's; Swift
/// lays them out, and its own words are the buttons' (Download, Update, Add back, Remove,
/// Cancel, Retry) and the screens' titles.
struct LibrarySheet: View {
    @Bindable var model: LibraryModel

    enum Page: Hashable {
        case starter, getResources, storage
    }

    var body: some View {
        NavigationStack {
            List {
                if let banner = model.banner {
                    DownloadBanner(text: banner)
                }
                NavigationLink(value: Page.starter) { Text("Starter library") }
                    .accessibilityIdentifier("library.starter")
                NavigationLink(value: Page.getResources) { Text("Get Resources") }
                    .accessibilityIdentifier("library.getResources")
                NavigationLink(value: Page.storage) {
                    LabeledContent("Storage", value: model.storage?.size ?? "")
                }
                .accessibilityIdentifier("library.storage")
                Toggle("Use cellular data", isOn: Binding(get: { model.useCellular }, set: { model.setUseCellular($0) }))
                    .accessibilityIdentifier("library.useCellular")
            }
            .navigationTitle("Library")
            .navigationBarTitleDisplayMode(.inline)
            .navigationDestination(for: Page.self) { page in
                switch page {
                case .starter: StarterLibraryScreen(model: model)
                case .getResources: GetResourcesScreen(model: model)
                case .storage: StorageScreen(model: model)
                }
            }
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Done") { model.showing = false }
                        .accessibilityIdentifier("library.done")
                }
            }
        }
        .libraryQuestions(model, active: model.offer == nil)
        .accessibilityIdentifier("library")
    }
}

/// The core's banner text: downloads under way, indexes building, failures.
struct DownloadBanner: View {
    let text: String

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: 8) {
            Image(systemName: "arrow.down.circle").accessibilityHidden(true)
            Text(text).fixedSize(horizontal: false, vertical: true)
        }
        .accessibilityElement(children: .combine)
        .accessibilityIdentifier("library.banner")
    }
}

/// Library → Starter library: the Study set first while some of it is missing, then every
/// row of the core's list in its order.
struct StarterLibraryScreen: View {
    let model: LibraryModel

    var body: some View {
        List {
            if let banner = model.banner {
                DownloadBanner(text: banner)
            }
            if let view = model.starter {
                if view.checking && !view.released {
                    ProgressView().accessibilityIdentifier("starter.checking")
                }
                if let error = view.error {
                    Text(error)
                        .fontWeight(.semibold)
                        .accessibilityIdentifier("starter.error")
                }
                if view.studySet.resources > 0 {
                    Section {
                        VStack(alignment: .leading, spacing: 4) {
                            Text("Study set").font(.headline).accessibilityAddTraits(.isHeader)
                            Text(view.studySet.label).accessibilityIdentifier("starter.studySet.size")
                        }
                        .accessibilityElement(children: .combine)
                        Button("Download") { model.download(.set(set: .study)) }
                            .accessibilityIdentifier("starter.studySet.download")
                            .accessibilityLabel("Download the Study set")
                    }
                }
                Section {
                    ForEach(view.rows, id: \.resourceId) { row in
                        StarterRowView(row: row, model: model)
                    }
                }
            }
        }
        .navigationTitle("Starter library")
        .navigationBarTitleDisplayMode(.inline)
        .onAppear { model.starterOpened() }
        .accessibilityIdentifier("starter.list")
    }
}

/// One Starter library row: title, abbreviation and size, licence, what it powers, then its
/// download or its action.
struct StarterRowView: View {
    let row: StarterRow
    let model: LibraryModel

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            VStack(alignment: .leading, spacing: 2) {
                Text(row.title).font(.headline).fixedSize(horizontal: false, vertical: true)
                Text([row.abbreviation, row.size].joined(separator: " · "))
                if !row.license.isEmpty { Text(row.license) }
                ForEach(row.features, id: \.self) { Text($0).font(.footnote) }
            }
            .accessibilityElement(children: .combine)
            .accessibilityIdentifier("starter.row.\(row.resourceId)")
            DownloadControls(
                id: row.resourceId, origin: .starter, title: row.abbreviation, state: row.download, model: model
            ) {
                switch row.status {
                case .inLibrary:
                    HStack {
                        Text(downloadLabel(state: .done)).accessibilityIdentifier("starter.inLibrary.\(row.resourceId)")
                        Spacer()
                        Button("Remove", role: .destructive) { model.remove(row.resourceId) }
                            .accessibilityIdentifier("starter.remove.\(row.resourceId)")
                            .accessibilityLabel("Remove \(row.abbreviation)")
                    }
                case .outdated:
                    Button("Update") { model.download(.one(resourceId: row.resourceId)) }
                        .accessibilityIdentifier("starter.update.\(row.resourceId)")
                        .accessibilityLabel("Update \(row.abbreviation)")
                case .removed:
                    Button("Add back") { model.download(.one(resourceId: row.resourceId)) }
                        .accessibilityIdentifier("starter.addBack.\(row.resourceId)")
                        .accessibilityLabel("Add back \(row.abbreviation)")
                case .missing:
                    Button("Download") { model.download(.one(resourceId: row.resourceId)) }
                        .accessibilityIdentifier("starter.download.\(row.resourceId)")
                        .accessibilityLabel("Download \(row.abbreviation)")
                }
            }
        }
        .buttonStyle(.borderless)
        .padding(.vertical, 2)
    }
}

/// A row's download while there is one (its state in the core's words, progress, Cancel or
/// Retry), else `idle`, the row's own action.
struct DownloadControls<Idle: View>: View {
    let id: String
    let origin: DownloadOrigin
    let title: String
    let state: DownloadState?
    let model: LibraryModel
    @ViewBuilder let idle: () -> Idle

    var body: some View {
        switch state {
        case .none, .done:
            idle()
        case .queued, .downloading:
            VStack(alignment: .leading, spacing: 4) {
                if case .downloading(let done, let total) = state, total > 0 {
                    ProgressView(value: Double(done), total: Double(total)) { label }
                } else {
                    ProgressView { label }
                }
                Button("Cancel", role: .destructive) { model.cancel(id, origin: origin) }
                    .accessibilityIdentifier("download.cancel.\(id)")
                    .accessibilityLabel("Cancel the download of \(title)")
            }
        case .verifying, .installing, .indexing:
            ProgressView { label }
        case .failed(_, let retry):
            VStack(alignment: .leading, spacing: 4) {
                label.fontWeight(.semibold)
                if retry {
                    Button("Retry") { retryDownload() }
                        .accessibilityIdentifier("download.retry.\(id)")
                        .accessibilityLabel("Retry the download of \(title)")
                }
            }
        }
    }

    private var label: some View {
        Text(state.map { downloadLabel(state: $0) } ?? "")
            .accessibilityIdentifier("download.state.\(id)")
    }

    private func retryDownload() {
        switch origin {
        case .starter: model.download(.one(resourceId: id))
        case .open: model.downloadOpen(id)
        }
    }
}

/// Get Resources: the Open catalogue, with the desktop's licence filter and its words.
struct GetResourcesScreen: View {
    @Bindable var model: LibraryModel

    var body: some View {
        List {
            Picker("Licence", selection: $model.filter) {
                ForEach(model.filters.choices, id: \.filter) { Text($0.label).tag($0.filter) }
            }
            .accessibilityIdentifier("open.filter")
            if let choice = model.filters.choices.first(where: { $0.filter == model.filter }) {
                Text(choice.detail).font(.footnote)
            }
            ForEach(model.openRows, id: \.id) { row in
                OpenRowView(row: row, model: model)
            }
        }
        .navigationTitle("Get Resources")
        .navigationBarTitleDisplayMode(.inline)
        .accessibilityIdentifier("open.list")
    }
}

struct OpenRowView: View {
    let row: OpenRow
    let model: LibraryModel

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            VStack(alignment: .leading, spacing: 2) {
                Text(row.title).font(.headline).fixedSize(horizontal: false, vertical: true)
                Text([row.abbreviation, row.publisher].joined(separator: " · "))
                Text([row.licence, licenceClassLabel(class: row.licenceClass)].joined(separator: " · "))
            }
            .accessibilityElement(children: .combine)
            .accessibilityIdentifier("open.row.\(row.id)")
            if let evidence = URL(string: row.evidenceUrl) {
                Link("Licence evidence", destination: evidence)
                    .accessibilityLabel("Licence evidence for \(row.abbreviation)")
            }
            DownloadControls(id: row.id, origin: .open, title: row.abbreviation, state: row.download, model: model) {
                if row.installed != nil {
                    Text(downloadLabel(state: .done))
                } else {
                    Button("Download") { model.downloadOpen(row.id) }
                        .accessibilityIdentifier("open.download.\(row.id)")
                        .accessibilityLabel("Download \(row.abbreviation)")
                }
            }
        }
        .buttonStyle(.borderless)
        .padding(.vertical, 2)
    }
}

/// Library → Storage: the space each Resource takes, and the total, as the core counts it.
struct StorageScreen: View {
    let model: LibraryModel

    var body: some View {
        List {
            if let storage = model.storage {
                LabeledContent("Total", value: storage.size)
                    .accessibilityIdentifier("storage.total")
                ForEach(storage.rows, id: \.resourceId) { row in
                    HStack {
                        LabeledContent(row.title, value: row.size)
                        Button("Remove", role: .destructive) { model.remove(row.resourceId) }
                            .accessibilityLabel("Remove \(row.title)")
                            .accessibilityIdentifier("storage.remove.\(row.resourceId)")
                    }
                    .buttonStyle(.borderless)
                }
            }
        }
        .navigationTitle("Storage")
        .navigationBarTitleDisplayMode(.inline)
        .accessibilityIdentifier("storage.list")
    }
}

extension View {
    /// The core's questions and refusals for the Library's taps, as alerts, from the view on
    /// top (`active`: the reader, the Library sheet or the offer's). The message is the
    /// core's; the buttons are Swift's.
    func libraryQuestions(_ model: LibraryModel, active: Bool) -> some View {
        alert(
            model.question?.id ?? "",
            isPresented: Binding(
                get: { active && model.question != nil }, set: { if !$0 { model.question = nil } }),
            presenting: model.question
        ) { question in
            switch question {
            case .cellular(_, let again):
                Button("Download") { model.confirmCellular(again) }
                    .accessibilityIdentifier("cellular.download")
                Button("Not now", role: .cancel) {}
                    .accessibilityIdentifier("cellular.notNow")
            case .remove(let id, _):
                Button("Remove", role: .destructive) { model.remove(id, confirmed: true) }
                    .accessibilityIdentifier("remove.confirm")
                Button("Cancel", role: .cancel) {}
            case .wontFit, .failed:
                Button("OK", role: .cancel) {}
            }
        }
    }
}
