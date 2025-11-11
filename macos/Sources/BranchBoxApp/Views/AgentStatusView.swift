import SwiftUI
#if os(macOS)
import AppKit
#endif

struct AgentStatusView: View {
    @EnvironmentObject private var viewModel: FeatureListViewModel

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text("Agent & Control Plane").font(.title2).bold()
            statusSummary
            controlPlaneHistory
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

private extension AgentStatusView {
    var statusSummary: some View {
        GroupBox("Status") {
            VStack(alignment: .leading, spacing: 6) {
                Label(viewModel.isAgentConnected ? "Direct (gRPC)" : "Fallback (CLI)", systemImage: viewModel.isAgentConnected ? "bolt.horizontal" : "arrow.triangle.2.circlepath")
                if let status = viewModel.controlPlaneStatus {
                    Label(status.controlPlaneConnected ? "Control plane connected" : "CP pending", systemImage: status.controlPlaneConnected ? "waveform.path.ecg" : "exclamationmark.triangle")
                    if let lastError = status.lastError, !lastError.isEmpty {
                        Text(lastError)
                            .font(.footnote)
                            .foregroundColor(.secondary)
                    }
                }
                let outdated = viewModel.outdatedDevcontainersCount
                Label(outdated > 0 ? "\(outdated) devcontainer(s) outdated" : "Devcontainers synced", systemImage: outdated > 0 ? "exclamationmark.triangle" : "shippingbox")
                    .foregroundColor(outdated > 0 ? .orange : .primary)
            }
            .padding(8)
        }
    }

    var controlPlaneHistory: some View {
        GroupBox("Control plane events") {
            VStack(alignment: .leading, spacing: 8) {
                if let status = viewModel.controlPlaneStatus {
                    if let ack = status.lastAckEventID {
                        infoRow(title: "Last ack ID", value: "#\(ack)") {
                            copyToPasteboard(String(ack))
                        }
                    }
                    if let delivery = formattedStatusDate(status.lastDeliveryAt) {
                        infoRow(title: "Last delivery", value: delivery.display) {
                            copyToPasteboard(delivery.raw)
                        }
                    }
                    if let failure = formattedStatusDate(status.lastFailureAt) {
                        infoRow(title: "Last failure", value: failure.display) {
                            copyToPasteboard(failure.raw)
                        }
                    }
                    if let error = status.lastError, !error.isEmpty {
                        infoRow(title: "Last error", value: error) {
                            copyToPasteboard(error)
                        }
                    }
                } else {
                    Text("No control plane data yet").foregroundColor(.secondary)
                }
            }
            .padding(8)
        }
    }

    func infoRow(title: String, value: String, copyAction: @escaping () -> Void) -> some View {
        HStack(alignment: .center) {
            VStack(alignment: .leading, spacing: 2) {
                Text(title).font(.caption).foregroundColor(.secondary)
                Text(value)
            }
            Spacer()
            Button(action: copyAction) {
                Image(systemName: "doc.on.doc")
            }
            .buttonStyle(.borderless)
        }
    }

    func formattedStatusDate(_ raw: String?) -> (display: String, raw: String)? {
        guard let raw, let date = AgentStatusView.isoFormatter.date(from: raw) else {
            return nil
        }
        let relative = AgentStatusView.relativeFormatter.localizedString(for: date, relativeTo: Date())
        let absolute = AgentStatusView.absoluteFormatter.string(from: date)
        return ("\(relative) · \(absolute)", raw)
    }

    func copyToPasteboard(_ value: String) {
        #if os(macOS)
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(value, forType: .string)
        #endif
    }

    static let isoFormatter: ISO8601DateFormatter = {
        let formatter = ISO8601DateFormatter()
        formatter.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        return formatter
    }()

    static let relativeFormatter: RelativeDateTimeFormatter = {
        let formatter = RelativeDateTimeFormatter()
        formatter.unitsStyle = .short
        return formatter
    }()

    static let absoluteFormatter: DateFormatter = {
        let formatter = DateFormatter()
        formatter.dateStyle = .medium
        formatter.timeStyle = .short
        return formatter
    }()
}
