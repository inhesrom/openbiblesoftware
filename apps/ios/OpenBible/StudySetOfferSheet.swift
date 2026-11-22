import OpenBibleCore
import SwiftUI

/// The first-launch Study set offer (#333 "Downloads"): the core's words, its size once the
/// released list is in, the network now, Download and Not now. The core says whether there
/// is an offer to make; the sheet closes when it no longer makes one (nothing of the Study
/// set is missing) or the user answers.
struct StudySetOfferSheet: View {
    let model: LibraryModel
    let offer: StudySetOffer

    var body: some View {
        NavigationStack {
            ScrollView {
                VStack(alignment: .leading, spacing: 16) {
                    Text(offer.message)
                        .accessibilityIdentifier("offer.message")
                    if let size = offer.size {
                        Text(size)
                            .font(.headline)
                            .accessibilityIdentifier("offer.size")
                    }
                    if let status = offer.status {
                        HStack(spacing: 8) {
                            if offer.retry == nil { ProgressView() }
                            Text(status)
                        }
                        .accessibilityElement(children: .combine)
                        .accessibilityIdentifier("offer.status")
                    }
                    Text(offer.network)
                        .accessibilityIdentifier("offer.network")
                    VStack(spacing: 12) {
                        if let retry = offer.retry {
                            Button(retry) { model.offerRetry() }
                                .buttonStyle(.bordered)
                                .accessibilityIdentifier("offer.retry")
                        }
                        Button { model.offerDownload() } label: {
                            Text(offer.download).frame(maxWidth: .infinity, minHeight: 44)
                        }
                        .buttonStyle(.borderedProminent)
                        .disabled(offer.size == nil)
                        .accessibilityIdentifier("offer.download")
                        Button { model.offerNotNow() } label: {
                            Text(offer.notNow).frame(maxWidth: .infinity, minHeight: 44)
                        }
                        .buttonStyle(.bordered)
                        .accessibilityIdentifier("offer.notNow")
                    }
                    Text(offer.later)
                        .font(.footnote)
                        .accessibilityIdentifier("offer.later")
                }
                .padding()
            }
            .navigationTitle(offer.title)
            .navigationBarTitleDisplayMode(.inline)
        }
        .onAppear { model.offerShown() }
        .accessibilityIdentifier("offer")
    }
}
