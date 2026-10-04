import BranchBoxKit
import BranchBoxPreview
import SwiftUI

/// The pull request linked to the feature: "#n", or "No pull request linked".
struct PullRequestCard: View {
    let prNumber: Int?

    var body: some View {
        FeatureCard("Pull Request", systemImage: "arrow.triangle.pull") {
            if let prNumber {
                HStack(alignment: .firstTextBaseline, spacing: 8) {
                    Text("#\(String(prNumber))")
                        .font(.title3.weight(.semibold).monospacedDigit())
                        .textSelection(.enabled)
                    Spacer(minLength: 4)
                    CopyButton(text: "#\(String(prNumber))", label: "Copy Pull Request Number").buttonStyle(.borderless)
                }
                .accessibilityElement(children: .combine)
                .accessibilityLabel("Pull request \(String(prNumber))")
            } else {
                Text("No pull request linked")
                    .foregroundStyle(.secondary)
            }
        }
    }
}

#Preview("Pull requests") {
    VStack(spacing: 16) {
        PullRequestCard(prNumber: 42)
        PullRequestCard(prNumber: nil)
    }
    .padding()
    .frame(width: 360)
}
