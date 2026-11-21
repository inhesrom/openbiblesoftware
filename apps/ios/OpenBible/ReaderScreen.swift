import OpenBibleCore
import SwiftUI

/// S07 prototype (#331): the reader screen. The chrome is deliberately plain (the screen
/// anatomy is #335's); every choice and answer in it comes from the core. Its own labels and
/// chrome colours are Swift's, an accepted prototype gap (`docs/dev/mobile-text-view.md`).
struct ReaderScreen: View {
    @Bindable var model: ReaderModel
    @State private var reference = ""
    @State private var showBooks = false
    @State private var showAbout = false
    let hello: HelloModel

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
                        ForEach(model.bibles, id: \.id) { Text($0.label).tag($0.id) }
                    }
                    Picker("Page tone", selection: Binding(get: { model.tone }, set: { model.setTone($0) })) {
                        ForEach(model.tones, id: \.id) { Text($0.label).tag($0.id) }
                    }
                    Toggle("Show measurements", isOn: $model.showMeasurements)
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
            StudySheetStubView(stub: sheet.stub)
                .presentationDetents([.medium, .large])
        }
        .sheet(isPresented: $showBooks) {
            BookGrid(books: model.books) { book, chapter in
                showBooks = false
                model.goTo(book: book, chapter: chapter)
            }
        }
        .sheet(isPresented: $showAbout) {
            // Boots a second core over the same folder: the S06 check, kept for comparison.
            HelloView(model: hello).task { await hello.start() }
        }
    }
}

/// The Study sheet stub (S16 builds the real one): what the core says a tap opened.
struct StudySheetStubView: View {
    let stub: StudySheetStub

    var body: some View {
        NavigationStack {
            List {
                if let word = stub.word {
                    Section("Word") {
                        Text(word).font(.title2).accessibilityIdentifier("sheet.word")
                        if let strongs = stub.strongs {
                            LabeledContent("Strong's", value: strongs).accessibilityIdentifier("sheet.strongs")
                        }
                        if let lemma = stub.lemma { LabeledContent("Lemma", value: lemma) }
                        if let gloss = stub.gloss { LabeledContent("Gloss", value: gloss) }
                    }
                }
                Section("Verse") {
                    Text(stub.verseText)
                    Text(stub.crossReferenceLabel).accessibilityIdentifier("sheet.crossReferences")
                }
            }
            .navigationTitle(stub.title)
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
