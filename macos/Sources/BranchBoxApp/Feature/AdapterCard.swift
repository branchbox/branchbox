import BranchBoxKit
import BranchBoxPreview
import SwiftUI

/// The stack adapter BranchBox detected, its in-container service and any warnings it raised.
struct AdapterCard: View {
    let adapter: AdapterInfo

    var body: some View {
        FeatureCard("Adapter", systemImage: "square.stack.3d.up") {
            if !adapter.warnings.isEmpty {
                Tag(title: adapter.warnings.count == 1 ? "1 warning" : "\(adapter.warnings.count) warnings",
                    systemImage: "exclamationmark.triangle.fill", tint: .orange)
            }
        } content: {
            VStack(alignment: .leading, spacing: 10) {
                FactGrid {
                    FactRow(label: "Stack", value: adapter.name ?? "Unknown")
                    if let service = adapter.serviceURL, !service.isEmpty {
                        FactRow(label: "Service", value: service, monospaced: true)
                    }
                }
                if adapter.warnings.isEmpty {
                    Label("No warnings", systemImage: "checkmark.circle")
                        .foregroundStyle(.secondary)
                } else {
                    VStack(alignment: .leading, spacing: 6) {
                        ForEach(Array(adapter.warnings.enumerated()), id: \.offset) { _, warning in
                            Label {
                                Text(warning)
                                    .textSelection(.enabled)
                                    .fixedSize(horizontal: false, vertical: true)
                            } icon: {
                                Image(systemName: "exclamationmark.triangle.fill").foregroundStyle(.orange)
                            }
                        }
                    }
                }
            }
        }
    }
}

#Preview("Adapter") {
    AdapterCard(adapter: AdapterInfo(name: "Rails", serviceURL: "http://app:3000",
                                     warnings: ["Gemfile.lock is newer than the dev container image; rebuild to pick up new gems"]))
        .padding()
        .frame(width: 420)
}
