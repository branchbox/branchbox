import BranchBoxKit
import BranchBoxPreview
import SwiftUI

/// The feature's runtime: a glyph with its name, or the glyph alone in dense rows (named in help and VoiceOver).
struct RuntimeBadge: View {
    enum Style { case full, glyph }

    let provider: RuntimeProvider
    var style: Style = .full

    var body: some View {
        Group {
            switch style {
            case .full:
                Label(provider.label, systemImage: provider.symbol)
            case .glyph:
                Image(systemName: provider.symbol)
            }
        }
        .font(.caption)
        .foregroundStyle(.secondary)
        .help("Runtime: \(provider.label)")
        .accessibilityElement(children: .ignore)
        .accessibilityLabel("Runtime: \(provider.label)")
    }
}

#Preview("Runtimes") {
    let providers: [RuntimeProvider] = [.container, .sbx, .localVM, .inGuest, .unknown("firecracker")]
    VStack(alignment: .leading, spacing: 8) {
        ForEach(providers, id: \.raw) { provider in
            HStack {
                RuntimeBadge(provider: provider)
                RuntimeBadge(provider: provider, style: .glyph)
            }
        }
        RuntimeBadge(provider: PreviewSamples.features.last?.runtime.provider ?? .container)
    }
    .padding()
}
