import OpenBibleCore
import SwiftUI

/// The Import report sheet: the core's report of one Import, word for word, live while it
/// runs (`ImportReport`). Swift lays it out: the progress while running, the failure with its
/// Details, then each section the core lists (folded where the core says), and Open once
/// the Resource is in the library.
struct ImportReportSheet: View {
    let model: ImportModel

    var body: some View {
        NavigationStack {
            Group {
                if let report = model.report {
                    content(report)
                } else {
                    ProgressView()
                }
            }
            .navigationTitle(model.report?.heading ?? "")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Done") { model.close() }
                        .accessibilityIdentifier("import.done")
                }
                if model.report?.resourceId != nil {
                    ToolbarItem(placement: .confirmationAction) {
                        Button("Open") { model.open() }
                            .accessibilityIdentifier("import.open")
                    }
                }
            }
        }
        .accessibilityIdentifier("import.report")
    }

    private func content(_ report: ImportReport) -> some View {
        List {
            Section {
                Text(report.source)
                    .font(.headline)
                    .accessibilityIdentifier("import.source")
                switch report.state {
                case .running:
                    if let progress = report.progress {
                        ProgressView(value: Double(progress)) { Text(report.status) }
                            .accessibilityIdentifier("import.status")
                    } else {
                        ProgressView { Text(report.status) }
                            .accessibilityIdentifier("import.status")
                    }
                    Button("Cancel", role: .destructive) { model.cancel() }
                        .accessibilityIdentifier("import.cancel")
                case .complete:
                    Text(report.status).accessibilityIdentifier("import.status")
                case .failed:
                    if let failure = report.failure {
                        Text(failure)
                            .fontWeight(.semibold)
                            .accessibilityIdentifier("import.failure")
                    }
                    if let details = report.details {
                        DisclosureGroup("Details") {
                            Text(details)
                                .font(.footnote.monospaced())
                                .textSelection(.enabled)
                                .accessibilityIdentifier("import.details")
                        }
                    }
                }
            }
            ForEach(Array(report.sections.enumerated()), id: \.offset) { _, section in
                ReportSectionView(section: section)
            }
        }
    }
}

/// One section of the report: its title and lines, folded at first when the core says so.
private struct ReportSectionView: View {
    let section: ReportSection
    @State private var open: Bool

    init(section: ReportSection) {
        self.section = section
        _open = State(initialValue: !section.collapsed)
    }

    var body: some View {
        if section.collapsed {
            Section {
                DisclosureGroup(isExpanded: $open) {
                    lines
                } label: {
                    Text(section.title).font(.headline)
                }
            }
        } else {
            Section {
                // The title as the section's first row: a grouped list's header is drawn
                // as secondary text, short of contrast in the accessibility audit (S06).
                Text(section.title)
                    .font(.headline)
                    .accessibilityAddTraits(.isHeader)
                lines
            }
        }
    }

    private var lines: some View {
        ForEach(Array(section.lines.enumerated()), id: \.offset) { _, line in
            Text(line).fontWeight(section.warning ? .semibold : .regular)
        }
    }
}
