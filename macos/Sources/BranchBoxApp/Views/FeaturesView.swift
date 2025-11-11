import SwiftUI

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
                HStack {
                    VStack(alignment: .leading) {
                        Text(feature.workFeature).font(.headline)
                        Text(feature.branchName).foregroundColor(.secondary).font(.caption)
                    }
                    Spacer()
                    Text(feature.updatedAtLabel).foregroundColor(.secondary).font(.caption)
                    statusPill(for: feature)
                }
                .tag(feature)
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
}

