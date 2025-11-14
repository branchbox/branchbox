import SwiftUI
#if os(macOS)
import AppKit
#endif

struct HomeView: View {
    @EnvironmentObject private var viewModel: FeatureListViewModel
    @State private var name: String = ""
    @State private var showOptions = false

    private let gridColumns: [GridItem] = [
        GridItem(.flexible(), spacing: 24),
        GridItem(.flexible(), spacing: 24)
    ]

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 24) {
                header
                if viewModel.workspaceNeedsSetup {
                    workspaceSetupCard
                }
                LazyVGrid(columns: gridColumns, spacing: 24) {
                    VStack(spacing: 24) {
                        primaryCard
                        recentCard
                    }
                    VStack(spacing: 24) {
                        workspaceHealthCard
                        if let feature = viewModel.activeFeature {
                            activeCard(feature)
                        } else {
                            emptyActiveCard
                        }
                    }
                }
            }
            .padding(24)
        }
        .sheet(isPresented: $showOptions) {
            StartFeatureSheet(name: $name)
                .environmentObject(viewModel)
        }
        .onAppear { name = viewModel.suggestedFeatureName }
    }

    private var header: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("Welcome to BranchBox")
                .font(.largeTitle).bold()
            HStack {
                Text(viewModel.workspacePath)
                    .foregroundColor(.secondary)
                    .lineLimit(1)
                    .truncationMode(.middle)
                Spacer()
                Button("Reveal in Finder") { viewModel.revealWorkspaceInFinder() }
                Button("Open in Terminal") { viewModel.openWorkspaceInTerminal() }
            }
        }
    }

    private var primaryCard: some View {
        GroupBox {
            VStack(alignment: .leading, spacing: 12) {
                Text("Start a Feature").font(.title3).bold()
                HStack(spacing: 12) {
                    TextField("Feature name", text: $name)
                        .textFieldStyle(.roundedBorder)
                        .onSubmit { quickStart() }
                    Button("Start") { quickStart() }
                        .buttonStyle(.borderedProminent)
                        .keyboardShortcut(.defaultAction)
                        .disabled(viewModel.isWorking || name.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                    Button("Options…") { showOptions = true }
                        .disabled(viewModel.isWorking)
                }
            }
            .padding()
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    private var recentCard: some View {
        GroupBox("Recent activity") {
            if viewModel.features.isEmpty {
                Text("No features yet — start one above.")
                    .foregroundColor(.secondary)
            } else {
                VStack(alignment: .leading, spacing: 8) {
                    ForEach(viewModel.features.prefix(5)) { feature in
                        HStack {
                            Text(feature.workFeature).bold()
                            Spacer()
                            Text(feature.statusLabel)
                                .font(.caption)
                                .padding(.horizontal, 8)
                                .padding(.vertical, 4)
                                .background(feature.status.lowercased() == "active" ? Color.green.opacity(0.2) : Color.gray.opacity(0.2))
                                .clipShape(Capsule())
                        }
                    }
                }
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    private var workspaceHealthCard: some View {
        GroupBox("Workspace health") {
            VStack(alignment: .leading, spacing: 12) {
                statusRow(
                    title: "Agent connection",
                    detail: viewModel.isAgentConnected ? "Online" : "Offline",
                    color: viewModel.isAgentConnected ? .green : .orange,
                    description: viewModel.isAgentConnected ? "Connected to BranchBox agent" : "Falling back to CLI output",
                    actionLabel: "Check agent",
                    action: viewModel.isAgentConnected ? nil : { viewModel.selectedSection = .agent }
                )
                statusRow(
                    title: "Cloud sync",
                    detail: viewModel.isControlPlaneHealthy ? "Delivering" : "Pending",
                    color: viewModel.isControlPlaneHealthy ? .green : .orange,
                    description: viewModel.isControlPlaneHealthy ? "Events acknowledged" : "Awaiting delivery",
                    actionLabel: "View log",
                    action: viewModel.isControlPlaneHealthy ? nil : { viewModel.selectedSection = .agent }
                )
                let hasIssues = viewModel.outdatedDevcontainersCount > 0
                statusRow(
                    title: "Workspace sync",
                    detail: hasIssues ? "Needs sync" : "Up to date",
                    color: hasIssues ? .orange : .green,
                    description: hasIssues ? "\(viewModel.outdatedDevcontainersCount) worktree(s) require sync" : "All worktrees synced",
                    actionLabel: "Sync now",
                    action: hasIssues ? { viewModel.syncDevcontainer() } : nil
                )
            }
            .padding()
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    private func activeCard(_ feature: FeatureViewData) -> some View {
        GroupBox("Active feature") {
            VStack(alignment: .leading, spacing: 8) {
                HStack {
                    VStack(alignment: .leading) {
                        Text(feature.workFeature).font(.title3).bold()
                        Text(feature.branchName).font(.caption).foregroundColor(.secondary)
                    }
                    Spacer()
                    devcontainerBadge(for: feature)
                }
                HStack(spacing: 12) {
                    if let url = feature.featureURL, let link = URL(string: url) {
                        Link("Open", destination: link)
                    }
                    Button("Sync devcontainer") { viewModel.syncDevcontainer(strategy: feature.syncStrategy) }
                        .buttonStyle(.bordered)
                        .disabled(viewModel.isWorking)
                    Button("Teardown…") { viewModel.openTeardownSheet(for: feature) }
                        .buttonStyle(.bordered)
                }
                HStack(spacing: 12) {
                    Button("Reveal in Finder") { viewModel.revealFeatureInFinder(feature) }
                        .disabled(feature.worktreePath == nil)
                    Button("Open in Terminal") { viewModel.openFeatureInTerminal(feature) }
                        .disabled(feature.worktreePath == nil)
                    Button("Copy path") { copyPath(feature) }
                        .disabled(feature.worktreePath == nil)
                }
            }
            .padding()
        }
    }

    private func devcontainerBadge(for feature: FeatureViewData) -> some View {
        HStack(spacing: 6) {
            Image(systemName: feature.devcontainerHasWarning ? "exclamationmark.triangle" : "shippingbox")
            Text(feature.devcontainerStatusSummary)
                .font(.caption)
        }
        .padding(.horizontal, 10)
        .padding(.vertical, 4)
        .background(feature.devcontainerHasWarning ? Color.orange.opacity(0.15) : Color.blue.opacity(0.15))
        .clipShape(Capsule())
    }

    private var emptyActiveCard: some View {
        GroupBox("Active feature") {
            VStack(alignment: .leading, spacing: 8) {
                Text("No active feature")
                    .font(.title3).bold()
                Text("Launch a feature from the form on the left to see runtime status, quick actions, and module health.")
                    .foregroundColor(.secondary)
            }
            .padding()
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    private func quickStart() {
        viewModel.startFeatureQuick(name: name)
        name = ""
    }

    private func statusRow(title: String, detail: String, color: Color, description: String) -> some View {
        statusRow(title: title, detail: detail, color: color, description: description, actionLabel: nil, action: nil)
    }

    private func statusRow(
        title: String,
        detail: String,
        color: Color,
        description: String,
        actionLabel: String?,
        action: (() -> Void)?
    ) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack {
                Circle().fill(color).frame(width: 10, height: 10)
                Text("\(title) • \(detail)").font(.headline)
                Spacer()
                if let actionLabel, let action {
                    Button(actionLabel, action: action)
                        .buttonStyle(.bordered)
                        .controlSize(.small)
                }
            }
            Text(description)
                .font(.caption)
                .foregroundColor(.secondary)
        }
    }

    private var workspaceSetupCard: some View {
        GroupBox("Choose a workspace") {
            VStack(alignment: .leading, spacing: 8) {
                Text("Select the repository you want BranchBox to manage. You can always change this later from Settings.")
                    .foregroundColor(.secondary)
                HStack {
                    Button("Choose workspace…") { viewModel.openWorkspacePicker() }
                        .buttonStyle(.borderedProminent)
                    Button("Open Settings") { viewModel.selectedSection = .settings }
                }
            }
            .padding()
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }
}

private extension HomeView {
    func copyPath(_ feature: FeatureViewData) {
        guard let path = feature.worktreePath else { return }
        #if os(macOS)
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(path, forType: .string)
        #endif
    }
}
