import SwiftUI
#if os(macOS)
import AppKit
#endif

struct StatusMenuView: View {
    @EnvironmentObject private var viewModel: FeatureListViewModel
    @State private var name: String = ""
    @State private var showOptions = false

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            header
            if let active = viewModel.activeFeature {
                activeCard(active)
            }
            startQuick
            Divider()
            actions
        }
        .padding(12)
        .frame(width: 340)
        .sheet(isPresented: $showOptions) {
            StartFeatureSheet(name: $name).environmentObject(viewModel)
        }
        .sheet(item: $viewModel.pendingTeardown) { feature in
            TeardownSheetView(
                feature: feature,
                options: $viewModel.teardownOptions,
                onConfirm: { viewModel.performPendingTeardown() },
                onCancel: { viewModel.cancelPendingTeardown() }
            )
        }
        .task { await viewModel.loadIfNeeded() }
        .onAppear { name = viewModel.suggestedFeatureName }
    }

    private var header: some View {
        HStack {
            Circle().fill(viewModel.isAgentConnected ? Color.green : Color.orange)
                .frame(width: 8, height: 8)
            Text(viewModel.isAgentConnected ? "Direct" : "Fallback")
                .font(.caption)
                .foregroundColor(.secondary)
            Spacer()
            Button { viewModel.refresh() } label: { Image(systemName: "arrow.clockwise") }
                .buttonStyle(.borderless)
                .disabled(viewModel.isWorking)
        }
    }

    private func activeCard(_ feature: FeatureViewData) -> some View {
        GroupBox("Active feature") {
            VStack(alignment: .leading, spacing: 6) {
                Text(feature.workFeature).bold()
                Text(feature.branchName).foregroundColor(.secondary).font(.caption)
                devcontainerBadge(for: feature)
                HStack {
                    if let url = feature.featureURL, let link = URL(string: url) { Link("Open", destination: link) }
                    Spacer()
                    Button("Teardown…") { viewModel.openTeardownSheet(for: feature) }
                }
            }
            .padding(8)
        }
    }

    private var startQuick: some View {
        GroupBox("Start feature") {
            HStack(spacing: 8) {
                TextField("Name", text: $name)
                    .textFieldStyle(.roundedBorder)
                    .onSubmit { quickStart() }
                Button("Start") { quickStart() }
                    .buttonStyle(.borderedProminent)
                    .disabled(name.trimmed.isEmpty || viewModel.isWorking)
                Button("⋯") { showOptions = true }
                    .help("Options…")
            }
            .padding(8)
        }
    }

    private var actions: some View {
        HStack {
            Button("Open BranchBox…") {
                viewModel.selectedSection = .home
                NSApp.activate(ignoringOtherApps: true)
            }
            Spacer()
            Menu {
                Button("Copy strategy") { viewModel.setDevcontainerStrategy("copy"); viewModel.syncDevcontainer(strategy: "copy") }
                Button("Symlink strategy") { viewModel.setDevcontainerStrategy("symlink"); viewModel.syncDevcontainer(strategy: "symlink") }
                Divider()
                Button("Dry Run") { viewModel.syncDevcontainer(strategy: viewModel.devcontainerStrategy, dryRun: true) }
            } label: {
                Label("Sync (\(viewModel.devcontainerStrategy))", systemImage: "arrow.triangle.2.circlepath")
            }
            .disabled(viewModel.isWorking)
        }
    }

    private func quickStart() { viewModel.startFeatureQuick(name: name); name = "" }

    private func devcontainerBadge(for feature: FeatureViewData) -> some View {
        HStack {
            Image(systemName: feature.devcontainerHasWarning ? "exclamationmark.triangle" : "shippingbox")
            Text(feature.devcontainerStatusSummary)
                .font(.caption)
        }
        .padding(.horizontal, 8)
        .padding(.vertical, 4)
        .background(feature.devcontainerHasWarning ? Color.orange.opacity(0.2) : Color.blue.opacity(0.15))
        .clipShape(Capsule())
    }
}
