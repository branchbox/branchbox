import BranchBoxKit
import BranchBoxPreview
import SwiftUI

/// A red "!" capsule with a count, for project rows and the menu bar. Hidden when nothing needs attention.
struct AttentionBadge: View {
    let count: Int
    /// What is counted, singular: "feature", "item".
    var noun: String = "feature"

    var body: some View {
        if count > 0 {
            Label("\(count)", systemImage: "exclamationmark")
                .labelStyle(BadgeLabelStyle())
                .font(.caption2.weight(.bold))
                .foregroundStyle(.white)
                .padding(.horizontal, 6)
                .padding(.vertical, 2)
                .background(Color.red, in: Capsule())
                .accessibilityElement(children: .ignore)
                .accessibilityLabel(Self.accessibilityText(count: count, noun: noun))
                .help(Self.accessibilityText(count: count, noun: noun))
        }
    }

    /// "1 feature needs attention", "3 features need attention".
    nonisolated static func accessibilityText(count: Int, noun: String) -> String {
        count == 1 ? "1 \(noun) needs attention" : "\(count) \(noun)s need attention"
    }

    private struct BadgeLabelStyle: LabelStyle {
        func makeBody(configuration: Configuration) -> some View {
            HStack(spacing: 2) {
                configuration.icon
                configuration.title.monospacedDigit()
            }
        }
    }
}

#Preview("Attention badges") {
    let attention = PreviewSamples.features.filter { $0.status.needsAttention }.count
    VStack(alignment: .leading, spacing: 12) {
        HStack { Text("main"); AttentionBadge(count: attention) }
        HStack { Text("Single"); AttentionBadge(count: 1) }
        HStack { Text("Healthy (no badge)"); AttentionBadge(count: 0) }
    }
    .padding()
}
