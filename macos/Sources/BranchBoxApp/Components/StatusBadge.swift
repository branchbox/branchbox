import BranchBoxKit
import BranchBoxPreview
import SwiftUI

/// A capsule with an icon and a word: a status is never told by colour alone (layout ported from the 0.13.4
/// `MainAppView.StatusBadge`).
struct StatusBadge: View {
    let title: String
    let systemImage: String
    let tint: Color

    init(title: String, systemImage: String, tint: Color) {
        self.title = title
        self.systemImage = systemImage
        self.tint = tint
    }

    init(status: FeatureStatus) {
        self.init(title: status.label, systemImage: status.symbol, tint: status.tint.color)
    }

    init(attention: AttentionReason) {
        self.init(title: attention.label, systemImage: attention.symbol, tint: attention.tint.color)
    }

    var body: some View {
        Label(title, systemImage: systemImage)
            .font(.caption)
            .foregroundStyle(tint)
            .padding(.horizontal, 10)
            .padding(.vertical, 4)
            .background(tint.opacity(0.15), in: Capsule())
            .accessibilityElement(children: .ignore)
            .accessibilityLabel("Status: \(title)")
    }
}

/// The small grey "Quick" capsule for features started without setup modules (sidebar rows, the project table).
struct QuickCapsule: View {
    var body: some View {
        Text("Quick")
            .font(.caption2.weight(.medium))
            .foregroundStyle(.secondary)
            .padding(.horizontal, 5)
            .padding(.vertical, 1)
            .background(.quaternary, in: Capsule())
            .fixedSize()
            .help("Started quickly, without setup modules")
    }
}

#Preview("Statuses") {
    let statuses: [FeatureStatus] = [.active, .degraded, .failedRetained, .orphaned, .removed, .unknown("paused_by_admin")]
    VStack(alignment: .leading, spacing: 8) {
        ForEach(statuses, id: \.raw) { StatusBadge(status: $0) }
        StatusBadge(attention: .folderMissing)
        ForEach(PreviewSamples.features.prefix(3)) { StatusBadge(status: $0.status) }
    }
    .padding()
}
