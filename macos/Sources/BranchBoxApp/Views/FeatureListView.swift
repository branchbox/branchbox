import SwiftUI
#if os(macOS)
import AppKit
#endif

struct FeatureListView: View {
    @EnvironmentObject private var viewModel: FeatureListViewModel
    @State private var didPromptWorkspace = false

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            header
            workspaceSection
            startForm
            Divider()
            featureList
        }
        .padding(24)
        .frame(minWidth: 760, minHeight: 560)
        .task {
            await viewModel.loadIfNeeded()
        }
        .onAppear {
            // If the configured workspace path doesn't exist locally, prompt the user to choose one.
            if !didPromptWorkspace && !FileManager.default.fileExists(atPath: viewModel.workspacePath) {
                didPromptWorkspace = true
                viewModel.openWorkspacePicker()
            }
        }
        .alert(item: $viewModel.activeAlert) { alert in
            Alert(title: Text(alert.title), message: Text(alert.message), dismissButton: .default(Text("OK")))
        }
    }

    private var header: some View {
        HStack {
            VStack(alignment: .leading) {
                Text("BranchBox Features")
                    .font(.largeTitle)
                    .bold()
                Text("Agent-backed worktrees \u{2022} macOS preview")
                    .foregroundColor(.secondary)
            }
            Spacer()
            Button {
                viewModel.refresh()
            } label: {
                if viewModel.isLoading {
                    ProgressView()
                        .progressViewStyle(.circular)
                        .controlSize(.small)
                } else {
                    Label("Refresh", systemImage: "arrow.clockwise")
                }
            }
            .disabled(viewModel.isWorking)
            transportBadge
            controlPlaneBadge
        }
    }

    private var workspaceSection: some View {
        GroupBox("Workspace") {
            VStack(alignment: .leading, spacing: 8) {
                Text(viewModel.workspacePath)
                    .font(.callout)
                    .lineLimit(2)
                HStack {
                    Button("Choose…") { viewModel.openWorkspacePicker() }
                    Button("Reload") {
                        viewModel.refresh()
                    }
                }
            }
            .padding(.top, 4)
        }
    }

    private var startForm: some View {
        GroupBox("Start feature") {
            VStack(alignment: .leading, spacing: 12) {
                HStack {
                    TextField("Feature name", text: $viewModel.newFeatureName)
                        .textFieldStyle(.roundedBorder)
                    TextField("Optional title", text: $viewModel.newFeatureTitle)
                        .textFieldStyle(.roundedBorder)
                }

                HStack {
                    TextField("Branch prefix", text: $viewModel.branchPrefix)
                        .textFieldStyle(.roundedBorder)
                    Toggle("Reuse existing worktree", isOn: $viewModel.reuseExisting)
                }

                HStack {
                    Toggle("Minimal mode", isOn: $viewModel.useMinimalMode)
                    Spacer()
                    TextField("Prompt seed", text: $viewModel.promptSeed)
                        .textFieldStyle(.roundedBorder)
                        .frame(width: 260)
                }

                if !viewModel.promptHistory.isEmpty {
                    Menu("Prompt history") {
                        ForEach(viewModel.promptHistory, id: \.self) { seed in
                            Button(seed) {
                                viewModel.applyPromptHistory(seed)
                            }
                        }
                    }
                }

                HStack {
                    Menu {
                        ForEach(viewModel.availableModules, id: \.self) { module in
                            let binding = Binding(
                                get: { viewModel.skipModules.contains(module) },
                                set: { newValue in
                                    if newValue {
                                        viewModel.skipModules.insert(module)
                                    } else {
                                        viewModel.skipModules.remove(module)
                                    }
                                }
                            )
                            Toggle(module.capitalized, isOn: binding)
                        }
                    } label: {
                        Label("Skip modules", systemImage: "slider.horizontal.3")
                    }

                    if !viewModel.skipModules.isEmpty {
                        ScrollView(.horizontal, showsIndicators: false) {
                            HStack {
                                ForEach(Array(viewModel.skipModules).sorted(), id: \.self) { module in
                                    Text(module)
                                        .font(.caption)
                                        .padding(.horizontal, 8)
                                        .padding(.vertical, 4)
                                        .background(Color.blue.opacity(0.1))
                                        .clipShape(Capsule())
                                }
                            }
                        }
                    }
                }

                Button {
                    viewModel.startFeature()
                } label: {
                    Label("Start feature", systemImage: "play.fill")
                }
                .buttonStyle(.borderedProminent)
                .disabled(viewModel.isWorking)
            }
            .padding()
        }
    }

    private var featureList: some View {
        Group {
            if viewModel.isLoading && viewModel.features.isEmpty {
                VStack {
                    ProgressView()
                    Text("Loading features from the agent…")
                        .foregroundColor(.secondary)
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else if viewModel.features.isEmpty {
                VStack(spacing: 8) {
                    Image(systemName: "tray")
                        .font(.system(size: 42))
                        .foregroundColor(.secondary)
                    Text("No features yet")
                        .font(.title3)
                    Text("Start a feature above to see it tracked here.")
                        .foregroundColor(.secondary)
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                List(viewModel.features) { feature in
                    FeatureRow(
                        feature: feature,
                        onTeardown: { viewModel.openTeardownSheet(for: feature) }
                    )
                }
                .listStyle(.inset)
            }
        }
        .sheet(item: $viewModel.pendingTeardown) { feature in
            TeardownSheetView(
                feature: feature,
                options: $viewModel.teardownOptions,
                onConfirm: { viewModel.performPendingTeardown() },
                onCancel: { viewModel.cancelPendingTeardown() }
            )
        }
    }
}

private struct FeatureRow: View {
    let feature: FeatureViewData
    let onTeardown: () -> Void

    var body: some View {
        HStack(alignment: .top, spacing: 12) {
            VStack(alignment: .leading, spacing: 4) {
                HStack {
                    Text(feature.workFeature)
                        .font(.headline)
                    if feature.source == .cliFallback {
                        Label("CLI", systemImage: "arrow.triangle.2.circlepath")
                            .labelStyle(.titleAndIcon)
                            .font(.caption)
                            .padding(4)
                            .background(.yellow.opacity(0.2))
                            .clipShape(RoundedRectangle(cornerRadius: 4))
                    }
                }
                Text("Branch: \(feature.branchName)")
                    .foregroundColor(.secondary)
                if let prompt = feature.promptSeed, !prompt.isEmpty {
                    Text("Prompt: \(prompt)")
                        .font(.footnote)
                        .foregroundColor(.secondary)
                        .lineLimit(2)
                }
                HStack(spacing: 12) {
                    Text("Modules: \(feature.moduleSummary)")
                        .font(.footnote)
                        .foregroundColor(.secondary)
                    if let provider = feature.tunnelProvider, !provider.isEmpty {
                        Text("Tunnel: \(provider)")
                            .font(.footnote)
                            .foregroundColor(.secondary)
                    }
                }
                if let adapter = feature.adapterName {
                    Text("Adapter: \(adapter)")
                        .font(.footnote)
                        .foregroundColor(.secondary)
                }
                if let url = feature.adapterServiceURL, !url.isEmpty {
                    Text(url)
                        .font(.footnote)
                        .foregroundColor(.secondary)
                }
                if !feature.adapterWarnings.isEmpty {
                    VStack(alignment: .leading, spacing: 2) {
                        ForEach(feature.adapterWarnings, id: \.self) { warning in
                            Text("⚠️  \(warning)")
                                .font(.footnote)
                                .foregroundColor(.orange)
                        }
                    }
                }
                Text("Updated \(feature.updatedAtLabel)")
                    .font(.footnote)
                    .foregroundColor(.secondary)
                devcontainerStatus
            }

            Spacer()

            VStack(alignment: .trailing, spacing: 6) {
                statusPill
                if let url = feature.featureURL, let link = URL(string: url) {
                    Link("Open feature", destination: link)
                        .font(.callout)
                }
                Button("Teardown") {
                    onTeardown()
                }
                .buttonStyle(.bordered)
            }
        }
        .padding(4)
    }

    private var statusPill: some View {
        Text(feature.statusLabel)
            .font(.caption)
            .padding(.horizontal, 8)
            .padding(.vertical, 4)
            .background(statusColor)
            .foregroundColor(.white)
            .clipShape(Capsule())
    }

    private var statusColor: Color {
        feature.status.lowercased() == "active" ? .green : .gray
    }

    private var devcontainerStatus: some View {
        let strategy = feature.syncStrategy?.capitalized
        return HStack(spacing: 8) {
            Image(systemName: feature.devcontainerHasWarning ? "exclamationmark.triangle" : "shippingbox")
            Text("Devcontainer: \(feature.devcontainerStatusSummary)\(strategy.map { " • \($0)" } ?? "")")
                .font(.footnote)
        }
        .padding(.horizontal, 8)
        .padding(.vertical, 4)
        .background(feature.devcontainerHasWarning ? Color.orange.opacity(0.15) : Color.blue.opacity(0.12))
        .clipShape(RoundedRectangle(cornerRadius: 6))
    }
}

private extension FeatureListView {
    @ViewBuilder
    var transportBadge: some View {
        let isGrpc = viewModel.transportStatus == .grpc
        Label(isGrpc ? "gRPC" : "CLI fallback", systemImage: isGrpc ? "bolt.horizontal" : "arrow.triangle.2.circlepath")
            .font(.caption)
            .padding(.horizontal, 8)
            .padding(.vertical, 4)
            .background(isGrpc ? Color.green.opacity(0.2) : Color.orange.opacity(0.2))
            .clipShape(Capsule())
    }

    @ViewBuilder
    var controlPlaneBadge: some View {
        if let status = viewModel.controlPlaneStatus {
            let connected = status.controlPlaneConfigured && status.controlPlaneConnected
            Label(
                connected ? "Control plane" : "CP pending",
                systemImage: connected ? "waveform.path.ecg" : "exclamationmark.triangle"
            )
            .font(.caption)
            .padding(.horizontal, 8)
            .padding(.vertical, 4)
            .background(connected ? Color.green.opacity(0.2) : Color.orange.opacity(0.2))
            .clipShape(Capsule())
        }
    }

}
