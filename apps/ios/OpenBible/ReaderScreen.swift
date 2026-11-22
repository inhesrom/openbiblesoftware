import OpenBibleCore
import SwiftUI

/// The reader screen (S07's, on the designed core surface; S14a builds the real one). The
/// chrome is deliberately plain (the screen anatomy is #335's); every choice and answer in it
/// comes from the core. Its own labels and chrome colours are Swift's, an accepted prototype
/// gap (`docs/dev/mobile-text-view.md`).
struct ReaderScreen: View {
    @Bindable var model: ReaderModel
    /// Import (S26): reached from the options menu until the screen anatomy (#335, S12/S14a)
    /// gives it its place.
    @Bindable var importer: ImportModel
    /// The Library (S24): a sheet from the options menu until the screen anatomy (#335,
    /// S12/S14a) gives it its place; the first-launch Study set offer is a sheet over the
    /// reader.
    @Bindable var library: LibraryModel
    @State private var reference = ""
    @State private var showBooks = false
    @State private var showAbout = false

    var body: some View {
        switch model.state {
        case .starting:
            ProgressView("Starting…")
        case .failed(let message):
            ContentUnavailableView("Could not start", systemImage: "exclamationmark.triangle", description: Text(message))
        case .ready:
            reader
        }
    }

    private var reader: some View {
        VStack(spacing: 0) {
            HStack(spacing: 8) {
                Button { showBooks = true } label: {
                    Image(systemName: "square.grid.3x3")
                        .frame(minWidth: 44, minHeight: 44)
                        .contentShape(Rectangle())
                }
                .accessibilityLabel("Books and chapters")
                .accessibilityIdentifier("reader.books")
                // Vertical, so the field grows with Dynamic Type instead of clipping its text.
                TextField("Go to (Jn 3:16)", text: $reference, axis: .vertical)
                    .lineLimit(1...3)
                    .padding(.horizontal, 10)
                    .padding(.vertical, 6)
                    .frame(minHeight: 44)
                    .background(RoundedRectangle(cornerRadius: 10).fill(Color(.secondarySystemBackground)))
                    .submitLabel(.go)
                    .autocorrectionDisabled()
                    .textInputAutocapitalization(.never)
                    .accessibilityIdentifier("reader.reference")
                    .onSubmit { model.goTo(reference) }
                    // A vertical field types Return as a newline: it means Go here.
                    .onChange(of: reference) { _, text in
                        guard text.contains("\n") else { return }
                        reference = text.replacingOccurrences(of: "\n", with: "")
                        model.goTo(reference)
                    }
                Menu {
                    Picker("Bible", selection: Binding(get: { model.bible }, set: { model.open(bible: $0) })) {
                        ForEach(model.bibles, id: \.resourceId) { Text($0.abbreviation).tag($0.resourceId) }
                    }
                    Picker("Page tone", selection: Binding(get: { model.tone }, set: { model.setTone($0) })) {
                        ForEach(model.tones, id: \.id) { Text($0.label).tag($0.id) }
                    }
                    Toggle("Show measurements", isOn: $model.showMeasurements)
                    Button("Library…") { library.open() }
                        .accessibilityIdentifier("reader.library")
                    Button("Import…") { importer.picking = true }
                        .accessibilityIdentifier("reader.import")
                    Button("About") { showAbout = true }
                } label: {
                    Image(systemName: "textformat.size")
                        .frame(minWidth: 44, minHeight: 44)
                        .contentShape(Rectangle())
                }
                .accessibilityLabel("Reading options")
                .accessibilityIdentifier("reader.options")
            }
            .padding(.horizontal)
            .padding(.vertical, 6)
            HStack {
                Text(model.location)
                    .font(.headline)
                    .accessibilityIdentifier("reader.location")
                Spacer()
            }
            .padding(.horizontal)
            .padding(.bottom, 4)
            if let error = model.referenceError {
                Text(error)
                    .font(.footnote)
                    .foregroundStyle(.red)
                    .padding(.horizontal)
                    .accessibilityIdentifier("reader.referenceError")
            }
            ResourceText(model: model)
                .ignoresSafeArea(edges: .bottom)
                .safeAreaInset(edge: .bottom) {
                    if let banner = library.banner {
                        Button { library.open() } label: {
                            DownloadBanner(text: banner)
                                .frame(maxWidth: .infinity, minHeight: 44, alignment: .leading)
                                .padding(.horizontal)
                                .contentShape(Rectangle())
                        }
                        .buttonStyle(.plain)
                        .background(.regularMaterial)
                        .accessibilityIdentifier("reader.banner")
                    }
                }
            if model.showMeasurements && (!model.benchResult.isEmpty || !model.metricsText.isEmpty) {
                    Text(model.benchResult.isEmpty ? model.metricsText : model.benchResult)
                        .font(.caption2.monospaced())
                        .foregroundStyle(.primary)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .padding(4)
                        .background(.thinMaterial)
                        .accessibilityIdentifier(model.benchResult.isEmpty ? "reader.metrics" : "bench.result")
            }
        }
        .sheet(item: $model.sheet) { sheet in
            StudySheetStubView(verse: sheet.verse, word: sheet.word)
                .presentationDetents([.medium, .large])
        }
        .sheet(isPresented: $showBooks) {
            BookGrid(books: model.books) { book, chapter in
                showBooks = false
                model.goTo(book: book, chapter: chapter)
            }
        }
        .sheet(isPresented: $showAbout) {
            AboutView(app: model.app)
        }
        .fileImporter(
            isPresented: $importer.picking, allowedContentTypes: ImportTypes.all,
            allowsMultipleSelection: false
        ) { importer.picked($0) }
        .sheet(item: $importer.shown) { _ in
            ImportReportSheet(model: importer)
        }
        .sheet(isPresented: $library.showing) {
            LibrarySheet(model: library)
        }
        .sheet(isPresented: Binding(get: { library.offer != nil }, set: { if !$0 { library.offerNotNow() } })) {
            if let offer = library.offer {
                StudySetOfferSheet(model: library, offer: offer)
                    .libraryQuestions(library, active: true)
            }
        }
        .libraryQuestions(library, active: !library.showing && library.offer == nil)
    }
}

/// The Study sheet stub (S16 builds the real one): what the core says a tap is on.
struct StudySheetStubView: View {
    let verse: HitVerse
    let word: HitWord?

    var body: some View {
        NavigationStack {
            List {
                if let word {
                    Section("Word") {
                        Text(word.written).font(.title2).accessibilityIdentifier("sheet.word")
                        LabeledContent("Strong's", value: word.strongs).accessibilityIdentifier("sheet.strongs")
                        if let lemma = word.lemma { LabeledContent("Lemma", value: lemma) }
                        if let gloss = word.gloss { LabeledContent("Gloss", value: gloss) }
                    }
                }
                Section("Verse") {
                    Text(verse.text)
                    Text(verse.crossReferenceLabel).accessibilityIdentifier("sheet.crossReferences")
                }
            }
            .navigationTitle(verse.title)
            .navigationBarTitleDisplayMode(.inline)
            .accessibilityIdentifier("sheet")
        }
    }
}

/// The book and chapter grid stub: the core's books, each with its chapters.
struct BookGrid: View {
    let books: [BookChapters]
    let go: (Int, UInt16) -> Void
    @State private var open: Int?

    var body: some View {
        NavigationStack {
            List {
                ForEach(Array(books.enumerated()), id: \.offset) { i, book in
                    DisclosureGroup(isExpanded: Binding(get: { open == i }, set: { open = $0 ? i : nil })) {
                        LazyVGrid(columns: [GridItem(.adaptive(minimum: 44))], spacing: 8) {
                            ForEach(1...max(1, Int(book.chapters)), id: \.self) { c in
                                Button("\(c)") { go(i, UInt16(c)) }
                                    .buttonStyle(.bordered)
                                    .frame(minWidth: 44, minHeight: 44)
                            }
                        }
                    } label: {
                        Text(book.name)
                    }
                }
            }
            .navigationTitle("Books")
            .navigationBarTitleDisplayMode(.inline)
        }
    }
}
