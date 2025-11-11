import SwiftUI
#if os(macOS)
import AppKit
#endif

struct FeaturesView: View {
    @EnvironmentObject private var viewModel: FeatureListViewModel
    @Binding var selected: FeatureViewData?
    @State private var filter: Filter = .all
    @State private var query: String = ""

    enum Filter: String, CaseIterable, Identifiable { case all, active, removed; var id: Self { self } }

    var body: some View {
        VStack(spacing: 8) {
            HStack {
                Picker("Filter", selection: $filter) {
                    ForEach(Filter.allCases) { f in Text(f.rawValue.capitalized).tag(f) }
                }
                .pickerStyle(.segmented)
                TextField("Search", text: $query)
                    .textFieldStyle(.roundedBorder)
                Spacer()
                Button { viewModel.refresh() } label: { Label("Refresh", systemImage: "arrow.clockwise") }
                    .disabled(viewModel.isWorking)
            }
            .padding(.horizontal)

            List(filteredFeatures, selection: $selected) { feature in
                HStack(alignment: .top) {
                    VStack(alignment: .leading, spacing: 4) {
                        Text(feature.workFeature).font(.headline)
                        Text(feature.branchName).foregroundColor(.secondary).font(.caption)
                        devcontainerBadge(for: feature)
                    }
                    Spacer()
                    VStack(alignment: .trailing, spacing: 4) {
                        Text(feature.updatedAtLabel).foregroundColor(.secondary).font(.caption)
                        statusPill(for: feature)
                    }
                }
                .tag(feature)
                .contextMenu {
                    if let url = feature.featureURL, let link = URL(string: url) {
                        Button("Open feature") { openURL(link) }
                    }
                    Button("Copy branch") { copyToPasteboard(feature.branchName) }
                    if feature.worktreePath != nil {
                        Button("Reveal in Finder") { viewModel.revealFeatureInFinder(feature) }
                        Button("Open in Terminal") { viewModel.openFeatureInTerminal(feature) }
                        Button("Copy path") { copyPath(feature) }
                    }
                    Button("Sync devcontainer") { viewModel.syncDevcontainer(strategy: feature.syncStrategy) }
                    Button("Teardown…", role: .destructive) { viewModel.openTeardownSheet(for: feature) }
                }
            }
            .listStyle(.inset)
        }
        .navigationTitle("Features")
    }

    private var filteredFeatures: [FeatureViewData] {
        viewModel.features.filter { f in
            switch filter {
            case .all: true
            case .active: f.status.lowercased() == "active"
            case .removed: f.status.lowercased() != "active"
            }
        }.filter { f in
            query.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty ||
            f.workFeature.localizedCaseInsensitiveContains(query) ||
            f.branchName.localizedCaseInsensitiveContains(query)
        }
    }

    private func statusPill(for feature: FeatureViewData) -> some View {
        Text(feature.statusLabel)
            .font(.caption)
            .padding(.horizontal, 8)
            .padding(.vertical, 4)
            .background(feature.status.lowercased() == "active" ? Color.green.opacity(0.2) : Color.gray.opacity(0.2))
            .clipShape(Capsule())
    }

    private func devcontainerBadge(for feature: FeatureViewData) -> some View {
        HStack(spacing: 6) {
            Image(systemName: feature.devcontainerHasWarning ? "exclamationmark.triangle" : "shippingbox")
            Text(feature.devcontainerStatusSummary)
                .font(.caption2)
        }
        .padding(.horizontal, 6)
        .padding(.vertical, 2)
        .background(feature.devcontainerHasWarning ? Color.orange.opacity(0.2) : Color.blue.opacity(0.15))
        .clipShape(RoundedRectangle(cornerRadius: 4))
    }

    private func copyToPasteboard(_ value: String) {
        #if os(macOS)
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(value, forType: .string)
        #endif
    }

    private func openURL(_ url: URL) {
        #if os(macOS)
        NSWorkspace.shared.open(url)
        #endif
    }

    private func copyPath(_ feature: FeatureViewData) {
        guard let path = feature.worktreePath else { return }
        copyToPasteboard(path)
    }
}
