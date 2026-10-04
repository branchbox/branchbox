import BranchBoxKit
import BranchBoxPreview
import BranchBoxStores
import SwiftUI

/// Sharing the feature through a tunnel (§9.3): provider, status, the public address, notes and instructions.
/// [Share via Tunnel] opens one; [Stop Sharing…] asks first. When the provider refuses to remove it, the failure
/// card offers [Remove Anyway…] with its own confirmation. Tunnels turned off in the project config point to
/// Project Settings.
struct TunnelCard: View {
    @Environment(AppModel.self) private var model
    @Environment(\.openWindow) private var openWindow

    let record: FeatureRecord
    let feature: FeatureRef
    let availability: FeatureActionAvailability
    /// `tunnel.enabled` from the project config; nil while it loads.
    let tunnelsEnabled: Bool?

    @State private var pendingConfirmation: FeatureConfirmation?
    @State private var showsInstructions = false
    @State private var dismissedFailure: UUID?

    static let tunnelKinds: Set<OperationKind> = [.tunnelOpen, .tunnelRemove]

    var body: some View {
        let state = TunnelCardState.state(tunnel: record.tunnel, tunnelsEnabled: tunnelsEnabled)
        FeatureCard("Sharing", systemImage: "network") {
            statusTag(state)
        } content: {
            VStack(alignment: .leading, spacing: 12) {
                content(for: state)
                if let failure, failure.id != dismissedFailure {
                    VStack(alignment: .trailing, spacing: 4) {
                        ResultCard(error: failure.error, context: failure.context, operationID: failure.id) { recovery in
                            perform(recovery)
                            acknowledge(failure)
                        }
                        Button("Dismiss") { acknowledge(failure) }
                            .buttonStyle(.borderless)
                            .font(.caption)
                    }
                }
            }
        }
        .featureConfirmationDialog($pendingConfirmation, model: model)
    }

    private var failure: OperationFailure? {
        OperationFailure.latest(of: Self.tunnelKinds, in: model.operations.records(for: .feature(feature)))
    }

    @ViewBuilder private func statusTag(_ state: TunnelCardState) -> some View {
        if let status = record.tunnel?.status, state != .offInConfig {
            Tag(title: status.label, systemImage: status.symbol, tint: status.tint.color)
                .accessibilityLabel("Tunnel: \(status.label)")
        } else {
            Tag(title: "Off", systemImage: "network.slash")
                .accessibilityLabel("Tunnel: Off")
        }
    }

    @ViewBuilder private func content(for state: TunnelCardState) -> some View {
        switch state {
        case .offInConfig:
            Text("Tunnels are off for this project. Turn them on in Project Settings to share features on the internet.")
                .foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)
            Button("Project Settings…") { model.post(.projectSettings(feature.project)) }
                .accessibilityIdentifier("feature.action.projectSettings")
        case .notShared:
            // The record's notes describe why it wasn't provisioned at start, which may no longer hold.
            Text("Share this feature on a public https address, for example to show it to a teammate.")
                .foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)
            HStack {
                shareButton(title: "Share via Tunnel", prominent: true)
                busyLine
            }
        case .pending, .manual, .unknown:
            facts
            instructions(alwaysShown: state == .manual)
            FlowLayout(spacing: 8) {
                shareButton(title: "Re-provision", prominent: false)
                if state != .manual, !(record.tunnel?.instructions.isEmpty ?? true) {
                    Button(showsInstructions ? "Hide Instructions" : "Show Instructions") { showsInstructions.toggle() }
                }
                stopButton
            }
            busyLine
        case .active:
            facts
            instructions(alwaysShown: false)
            HStack {
                stopButton
                busyLine
            }
        }
    }

    // MARK: Parts

    private var facts: some View {
        FactGrid {
            if let tunnel = record.tunnel {
                if let provider = tunnel.provider, !provider.isEmpty {
                    FactRow(label: "Provider", value: provider)
                }
                if let hostname = tunnel.hostname, !hostname.isEmpty, let url = URL(string: "https://\(hostname)") {
                    GridRow {
                        Text("Address").foregroundStyle(.secondary).fixedSize()
                        HStack(spacing: 6) {
                            Button(url.absoluteString) { HostLaunchFeedback.shared.open(url, for: feature) }
                                .buttonStyle(.link)
                                .lineLimit(1)
                                .truncationMode(.middle)
                                .help("Open \(url.absoluteString)")
                            Spacer(minLength: 4)
                            CopyButton(text: url.absoluteString, label: "Copy Address").buttonStyle(.borderless)
                        }
                    }
                }
                if let service = tunnel.serviceURL, !service.isEmpty {
                    FactRow(label: "Forwards to", value: service, monospaced: true)
                }
                if let notes = tunnel.notes, !notes.isEmpty {
                    FactRow(label: "Notes", value: notes)
                }
                if let updated = tunnel.lastUpdated {
                    FactRow(label: "Updated", value: FeaturePresentation.relative(updated))
                }
            }
        }
    }

    @ViewBuilder private func instructions(alwaysShown: Bool) -> some View {
        let steps = record.tunnel?.instructions ?? []
        if !steps.isEmpty, alwaysShown || showsInstructions {
            VStack(alignment: .leading, spacing: 6) {
                Text("Finish setting up the tunnel:")
                    .font(.subheadline.weight(.semibold))
                ForEach(Array(steps.enumerated()), id: \.offset) { index, step in
                    HStack(alignment: .firstTextBaseline, spacing: 8) {
                        Text("\(index + 1).")
                            .monospacedDigit()
                            .foregroundStyle(.secondary)
                            .frame(minWidth: 18, alignment: .trailing)
                        Text(step)
                            .textSelection(.enabled)
                            .fixedSize(horizontal: false, vertical: true)
                    }
                }
            }
            .padding(10)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(Color.orange.opacity(0.08), in: RoundedRectangle(cornerRadius: 8))
        }
    }

    @ViewBuilder private func shareButton(title: String, prominent: Bool) -> some View {
        let allowed = availability.tunnel
        let button = Button(title) { FeatureCommands.dispatch(.tunnelOpen(feature), model: model) }
            .disabled(!allowed.isEnabled)
            .help(allowed.disabledReason ?? "Open a tunnel for \(feature.name)")
            .accessibilityIdentifier("feature.action.tunnelOpen")
        if prominent {
            button.buttonStyle(.borderedProminent)
        } else {
            button
        }
    }

    private var stopButton: some View {
        let allowed = availability.tunnel
        return Button("Stop Sharing…", role: .destructive) {
            pendingConfirmation = FeatureConfirmations.stopSharing(feature, hostname: record.tunnel?.hostname)
        }
        .disabled(!allowed.isEnabled)
        .help(allowed.disabledReason ?? "Remove the tunnel")
        .accessibilityIdentifier("feature.action.tunnelRemove")
    }

    @ViewBuilder private var busyLine: some View {
        if let busy = availability.busyWith {
            HStack(spacing: 6) {
                ProgressView().controlSize(.small)
                Text("Waiting for “\(busy)”…").foregroundStyle(.secondary)
            }
            .font(.caption)
        }
    }

    /// Hides the failure and clears its attention (the menu bar count, the sidebar).
    private func acknowledge(_ failure: OperationFailure) {
        dismissedFailure = failure.id
        model.operations.record(failure.id)?.acknowledge()
    }

    private func perform(_ recovery: RecoveryAction) {
        HostLaunchFeedback.shared.perform(recovery, for: feature, model: model, openWindow: openWindow)
    }
}

#Preview("Sharing") {
    let record = PreviewSamples.features.first { $0.workFeature == "sbx-demo" } ?? PreviewSamples.features[0]
    TunnelCard(record: record, feature: FeatureRef(project: PreviewSamples.project, name: record.workFeature),
               availability: FeatureActionAvailability(record: record, folderExists: true, backendReady: true,
                                                       preferences: LaunchPreferences()),
               tunnelsEnabled: true)
        .environment(FeaturePreviewModel.make())
        .padding()
        .frame(width: 480)
}
