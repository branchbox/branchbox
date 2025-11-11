import SwiftUI

struct AgentStatusView: View {
    @EnvironmentObject private var viewModel: FeatureListViewModel

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text("Agent & Control Plane").font(.title2).bold()
            GroupBox("Status") {
                VStack(alignment: .leading, spacing: 6) {
                    Label(viewModel.isAgentConnected ? "Direct (gRPC)" : "Fallback (CLI)", systemImage: viewModel.isAgentConnected ? "bolt.horizontal" : "arrow.triangle.2.circlepath")
                    if let status = viewModel.controlPlaneStatus {
                        Label(status.controlPlaneConnected ? "Control plane connected" : "CP pending", systemImage: status.controlPlaneConnected ? "waveform.path.ecg" : "exclamationmark.triangle")
                        if let lastError = status.lastError, !lastError.isEmpty {
                            Text(lastError).font(.footnote).foregroundColor(.secondary)
                        }
                    }
                    let outdated = viewModel.outdatedDevcontainersCount
                    Label(outdated > 0 ? "\(outdated) devcontainer(s) outdated" : "Devcontainers synced", systemImage: outdated > 0 ? "exclamationmark.triangle" : "shippingbox")
                        .foregroundColor(outdated > 0 ? .orange : .primary)
                }
                .padding(8)
            }
            HStack(spacing: 12) {
                Menu {
                    Button("Copy strategy") { viewModel.setDevcontainerStrategy("copy"); viewModel.syncDevcontainer(strategy: "copy") }
                    Button("Symlink strategy") { viewModel.setDevcontainerStrategy("symlink"); viewModel.syncDevcontainer(strategy: "symlink") }
                    Divider()
                    Button("Dry Run") { viewModel.syncDevcontainer(strategy: viewModel.devcontainerStrategy, dryRun: true) }
                } label: {
                    Label("Sync (\(viewModel.devcontainerStrategy))", systemImage: "arrow.triangle.2.circlepath")
                }
                .disabled(viewModel.isWorking)

                Button("Refresh") { viewModel.refresh() }
                    .disabled(viewModel.isWorking)
                Spacer()
            }
            Spacer()
        }
        .padding(24)
        .navigationTitle("Agent")
    }
}
