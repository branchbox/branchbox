import SwiftUI
#if os(macOS)
import AppKit
#endif

struct FeatureDetailView: View {
    let feature: FeatureViewData
    @EnvironmentObject private var viewModel: FeatureListViewModel

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                header
                basics
                devcontainer
                modules
                warnings
                actions
            }
            .padding(24)
        }
        .navigationTitle(feature.workFeature)
    }

    private var header: some View {
        HStack {
            Text(feature.workFeature).font(.title).bold()
            Spacer()
            statusPill
        }
    }

    private var basics: some View {
        GroupBox("Basics") {
            VStack(alignment: .leading, spacing: 6) {
                Label(feature.branchName, systemImage: "arrow.branch")
                if let url = feature.featureURL, let link = URL(string: url) {
                    Link(destination: link) {
                        Label(url, systemImage: "safari")
                    }
                }
                if let adapter = feature.adapterName { Label("Adapter: \(adapter)", systemImage: "square.stack.3d.up") }
                if let svc = feature.adapterServiceURL { Label(svc, systemImage: "link") }
                Text("Updated \(feature.updatedAtLabel)").foregroundColor(.secondary).font(.footnote)
            }
            .padding(8)
        }
    }

    private var devcontainer: some View {
        GroupBox("Devcontainer") {
            HStack {
                Image(systemName: feature.devcontainerHasWarning ? "exclamationmark.triangle" : "shippingbox")
                VStack(alignment: .leading, spacing: 4) {
                    Text(feature.devcontainerStatusSummary)
                        .font(.body)
                    if let strategy = feature.syncStrategy {
                        Text("Strategy: \(strategy.capitalized)")
                            .font(.caption)
                            .foregroundColor(.secondary)
                    }
                    if let lastSync = feature.lastSyncAt {
                        Text("Last sync: \(FeatureDetailView.dateFormatter.string(from: lastSync))")
                            .font(.caption)
                            .foregroundColor(.secondary)
                    }
                }
                Spacer()
            }
            .padding(8)
            .background(feature.devcontainerHasWarning ? Color.orange.opacity(0.12) : Color.blue.opacity(0.08))
            .clipShape(RoundedRectangle(cornerRadius: 8))
        }
    }

    private var modules: some View {
        GroupBox("Modules") {
            if feature.moduleOutcomes.isEmpty {
                Text("—").foregroundColor(.secondary)
            } else {
                FlowLayout(spacing: 8) {
                    ForEach(feature.moduleOutcomes, id: \.self) { outcome in
                        Text("\(outcome.name): \(outcome.status)")
                            .font(.caption)
                            .padding(.horizontal, 8)
                            .padding(.vertical, 4)
                            .background(outcome.status.lowercased() == "ok" ? Color.green.opacity(0.15) : Color.orange.opacity(0.15))
                            .clipShape(Capsule())
                    }
                }
            }
        }
    }

    private var warnings: some View {
        Group {
            if !feature.adapterWarnings.isEmpty {
                VStack(alignment: .leading, spacing: 6) {
                    Label("Adapter warnings", systemImage: "exclamationmark.triangle.fill")
                        .foregroundColor(.orange)
                    ForEach(feature.adapterWarnings, id: \.self) { w in
                        Text(w).font(.footnote)
                    }
                }
                .padding(12)
                .background(Color.orange.opacity(0.1))
                .clipShape(RoundedRectangle(cornerRadius: 8))
            }
        }
    }

    private var actions: some View {
        HStack(spacing: 12) {
            if let url = feature.featureURL, let link = URL(string: url) {
                Link("Open", destination: link)
            }
            Button("Copy branch") { copyToPasteboard(feature.branchName) }
                .buttonStyle(.bordered)
            Button("Sync devcontainer") { viewModel.syncDevcontainer(strategy: feature.syncStrategy) }
                .buttonStyle(.bordered)
                .disabled(viewModel.isWorking)
            Button("Teardown…", role: .destructive) { viewModel.openTeardownSheet(for: feature) }
                .buttonStyle(.borderedProminent)
            Spacer()
        }
    }

    private var statusPill: some View {
        Text(feature.statusLabel)
            .font(.caption)
            .padding(.horizontal, 8)
            .padding(.vertical, 4)
            .background(feature.status.lowercased() == "active" ? Color.green.opacity(0.2) : Color.gray.opacity(0.2))
            .clipShape(Capsule())
    }
}

private extension FeatureDetailView {
    func copyToPasteboard(_ value: String) {
        #if os(macOS)
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(value, forType: .string)
        #endif
    }
}

private extension FeatureDetailView {
    static let dateFormatter: DateFormatter = {
        let formatter = DateFormatter()
        formatter.dateStyle = .medium
        formatter.timeStyle = .short
        return formatter
    }()
}

// Simple flow layout for chips using the Layout protocol
struct FlowLayout<Content: View>: View {
    var spacing: CGFloat = 8
    @ViewBuilder var content: Content

    var body: some View {
        FlowLayoutLayout(spacing: spacing) {
            content
        }
    }
}

private struct FlowLayoutLayout: Layout {
    var spacing: CGFloat

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        guard !subviews.isEmpty else { return .zero }
        let maxWidth = proposal.width ?? .infinity
        var lineWidth: CGFloat = 0
        var lineHeight: CGFloat = 0
        var totalHeight: CGFloat = 0
        var measuredWidth: CGFloat = 0

        for subview in subviews {
            let size = subview.sizeThatFits(.unspecified)
            if maxWidth.isFinite && lineWidth > 0 && lineWidth + spacing + size.width > maxWidth {
                totalHeight += lineHeight + spacing
                lineWidth = 0
                lineHeight = 0
            }
            if lineWidth > 0 {
                lineWidth += spacing
            }
            lineWidth += size.width
            lineHeight = max(lineHeight, size.height)
            measuredWidth = max(measuredWidth, lineWidth)
        }

        totalHeight += lineHeight
        let finalWidth = proposal.width ?? measuredWidth
        return CGSize(width: finalWidth, height: totalHeight)
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        guard !subviews.isEmpty else { return }
        var x = bounds.minX
        var y = bounds.minY
        var lineHeight: CGFloat = 0

        for subview in subviews {
            let size = subview.sizeThatFits(.unspecified)
            if x > bounds.minX && x + size.width > bounds.maxX {
                x = bounds.minX
                y += lineHeight + spacing
                lineHeight = 0
            }
            subview.place(at: CGPoint(x: x, y: y), proposal: ProposedViewSize(width: size.width, height: size.height))
            x += size.width + spacing
            lineHeight = max(lineHeight, size.height)
        }
    }
}
