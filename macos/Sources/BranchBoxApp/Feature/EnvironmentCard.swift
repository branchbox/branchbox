import BranchBoxKit
import BranchBoxPreview
import BranchBoxStores
import SwiftUI

/// The feature's environment (§9.2). Container runtime: the dev container's state from `devcontainerStatus`,
/// [Start], [Stop] (deleting volumes only behind a confirmation), [Rebuild…] (confirmed), and a shell inside
/// it. Docker Sandbox: the record's state and Start Environment (§9.1); Stop and Rebuild wait for
/// `branchbox feature env`. Local VM and in-guest are read-only. An out-of-date config links to Update All
/// Workspaces. Below, the runtime's facts (provider, mode, identifiers, workspace); ports live in Open, and the
/// user and container show once, with the running container's facts when it has them.
struct EnvironmentCard: View {
    @Environment(AppModel.self) private var model
    @Environment(\.openWindow) private var openWindow

    let record: FeatureRecord
    let feature: FeatureRef
    let availability: FeatureActionAvailability
    let load: DevcontainerLoad
    /// Start Environment for runtimes other than the container (§9.1), when `Remediation` offers one.
    let startItem: RemediationItem?
    let onReload: () -> Void
    let onPerform: (RemediationEffect) -> Void

    @State private var pendingConfirmation: FeatureConfirmation?
    @State private var dismissedFailure: UUID?

    static let sandboxEnvUnavailable = "Not available yet (planned: branchbox feature env)"
    static let explanation = "Starting a feature doesn't start its dev container; start it here when you need it."
    static let devcontainerKinds: Set<OperationKind> = [.devcontainerUp, .devcontainerDown, .devcontainerRebuild, .devcontainerBuild]

    var body: some View {
        FeatureCard("Environment", systemImage: "cube.transparent") {
            stateAccessory
        } content: {
            VStack(alignment: .leading, spacing: 12) {
                switch record.runtime.provider {
                case .container: containerContent
                case .sbx: sandboxContent
                default: readOnlyContent
                }
                if record.devcontainerOutdated, record.status != .removed {
                    outdatedRow
                }
                Divider()
                runtimeFacts
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
        OperationFailure.latest(of: Self.devcontainerKinds, in: model.operations.records(for: .feature(feature)))
    }

    // MARK: State

    @ViewBuilder private var stateAccessory: some View {
        switch record.runtime.provider {
        case .container:
            switch load {
            case .loading:
                ProgressView().controlSize(.small).accessibilityLabel("Checking the dev container")
            case .loaded(let status):
                Tag(title: status.state.label, systemImage: status.state.symbol, tint: status.state.tint.color)
                    .accessibilityLabel("Dev container: \(status.state.label)")
            case .failed:
                Tag(title: "Unknown", systemImage: "questionmark.circle")
            case .unavailable:
                Tag(title: "Unavailable", systemImage: "folder.badge.questionmark")
            }
        default:
            let state = Self.recordState(record)
            Tag(title: "Recorded: \(state.label)", systemImage: state.symbol, tint: state.tint.color)
                .help("From the feature registry; this runtime is not checked live by BranchBox for Mac")
        }
    }

    /// A runtime's state as the registry records it (no live probe outside the container runtime).
    static func recordState(_ record: FeatureRecord) -> (label: String, symbol: String, tint: StatusTint) {
        switch record.status {
        case .active: ("Running", "play.circle.fill", .green)
        case .degraded: ("Not running", "exclamationmark.triangle.fill", .orange)
        case .failedRetained: ("Failed (kept)", "xmark.octagon.fill", .red)
        case .orphaned: ("Gone", "diamond.fill", .purple)
        case .removed: ("Removed", "archivebox", .gray)
        case .unknown(let raw): (FeatureStatus.unknown(raw).label, "questionmark.circle", .gray)
        }
    }

    // MARK: Container

    @ViewBuilder private var containerContent: some View {
        if case .unavailable(let reason) = load {
            Text(reason)
                .foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)
        } else if !load.isRunning {
            Text(Self.explanation)
                .foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)
        }
        switch load {
        case .failed(let error):
            Label {
                Text("Couldn't read the dev container's state: \(error.presentation().title)")
                    .fixedSize(horizontal: false, vertical: true)
            } icon: {
                Image(systemName: "exclamationmark.triangle").foregroundStyle(.orange)
            }
        case .loaded(let status):
            if status.state == .running {
                FactGrid {
                    if let id = status.containerID, !id.isEmpty {
                        FactRow(label: "Container", value: String(id.prefix(12)), monospaced: true) {
                            CopyButton(text: id, label: "Copy Container ID").buttonStyle(.borderless)
                        }
                    }
                    if let service = status.service?.serviceName {
                        FactRow(label: "Service", value: service + (status.service?.port.map { " :\(String($0))" } ?? ""),
                                monospaced: true)
                    }
                    if let user = status.service?.containerUser ?? record.runtime.containerUser {
                        FactRow(label: "User", value: user, monospaced: true)
                    }
                }
            }
        case .loading, .unavailable:
            EmptyView()
        }
        containerButtons
        busyLine
    }

    private var containerButtons: some View {
        let allowed = availability.devcontainer
        let state = load.status?.state
        return FlowLayout(spacing: 8) {
            if state == .running {
                Button {
                    if let status = load.status {
                        HostLaunchFeedback.shared.launch(
                            HostLaunchPlan.devcontainerShell(status, terminal: availability.preferences.terminal,
                                                             record: record, folderExists: availability.folderExists),
                            for: feature)
                    }
                } label: {
                    Label("Open Shell", systemImage: "terminal")
                }
                .help("Open a shell inside the dev container")
                .accessibilityIdentifier("feature.action.devcontainerShell")
                Menu {
                    Button("Stop and Delete Volumes…") { pendingConfirmation = FeatureConfirmations.stopDeletingVolumes(feature) }
                } label: {
                    Label("Stop", systemImage: "stop.fill")
                } primaryAction: {
                    FeatureCommands.dispatch(.devcontainer(.down(removeVolumes: false), feature), model: model)
                }
                .menuStyle(.button)
                .fixedSize()
                .disabled(!allowed.isEnabled)
                .help(allowed.disabledReason ?? "Stop the dev container; its volumes are kept")
                .accessibilityIdentifier("feature.action.devcontainerStop")
            } else {
                Button {
                    FeatureCommands.dispatch(.devcontainer(.up(removeExisting: false, buildNoCache: false), feature), model: model)
                } label: {
                    Label("Start", systemImage: "play.fill")
                }
                .buttonStyle(.borderedProminent)
                .disabled(!allowed.isEnabled)
                .help(allowed.disabledReason ?? "Start the dev container (devcontainer up)")
                .accessibilityIdentifier("feature.action.devcontainerStart")
            }
            Button("Rebuild…") { pendingConfirmation = FeatureConfirmations.rebuild(feature) }
                .disabled(!allowed.isEnabled)
                .help(allowed.disabledReason ?? "Remove the container and rebuild its image without cache")
                .accessibilityIdentifier("feature.action.devcontainerRebuild")
            Button(action: onReload) {
                Image(systemName: "arrow.clockwise")
            }
            .buttonStyle(.borderless)
            .help("Check the dev container again")
            .accessibilityLabel("Check the dev container again")
            .accessibilityIdentifier("feature.action.devcontainerRefresh")
        }
    }

    // MARK: Sandbox and read-only

    @ViewBuilder private var sandboxContent: some View {
        Text("BranchBox reads the sandbox's state from the registry. Stopping or rebuilding a sandbox from BranchBox isn't available yet (planned: branchbox feature env).")
            .foregroundStyle(.secondary)
            .fixedSize(horizontal: false, vertical: true)
        FlowLayout(spacing: 8) {
            if let startItem {
                Button("Start Environment") { onPerform(startItem.effect) }
                    .buttonStyle(.borderedProminent)
                    .disabled(!availability.remediation.isEnabled)
                    .help(availability.remediation.disabledReason ?? "Start the sandbox again (\(startItem.title))")
                    .accessibilityIdentifier("feature.action.startEnvironment")
            }
            Button("Stop") {}
                .disabled(true)
                .help(Self.sandboxEnvUnavailable)
            Button("Rebuild…") {}
                .disabled(true)
                .help(Self.sandboxEnvUnavailable)
        }
        busyLine
    }

    @ViewBuilder private var readOnlyContent: some View {
        Text("\(record.runtime.provider.label) environments are managed outside BranchBox for Mac; this shows what the registry records.")
            .foregroundStyle(.secondary)
            .fixedSize(horizontal: false, vertical: true)
    }

    // MARK: Runtime facts

    /// The running container already lists its container and user; otherwise they come from the record.
    private var showsLiveContainerFacts: Bool {
        record.runtime.provider == .container && load.status?.state == .running
    }

    private var runtimeFacts: some View {
        let runtime = record.runtime
        return FactGrid {
            FactRow(label: "Runtime", value: runtime.provider.label)
            FactRow(label: "Mode", value: FeaturePresentation.isQuick(record) ? "Quick" : "Full")
            if let id = runtime.runtimeID, !id.isEmpty {
                FactRow(label: "Runtime ID", value: id, monospaced: true) {
                    CopyButton(text: id, label: "Copy Runtime ID").buttonStyle(.borderless)
                }
            }
            if !showsLiveContainerFacts, let container = runtime.containerID, !container.isEmpty {
                FactRow(label: "Container", value: String(container.prefix(12)), monospaced: true) {
                    CopyButton(text: container, label: "Copy Container ID").buttonStyle(.borderless)
                }
            }
            if let folder = runtime.workspaceFolder, !folder.isEmpty {
                FactRow(label: "Workspace", value: folder, monospaced: true)
            }
            if !showsLiveContainerFacts, let user = runtime.containerUser, !user.isEmpty {
                FactRow(label: "User", value: user, monospaced: true)
            }
            if let config = runtime.configPath, !config.isEmpty {
                FactRow(label: "Config", value: config, monospaced: true)
            }
        }
    }

    private var outdatedRow: some View {
        HStack(alignment: .firstTextBaseline, spacing: 8) {
            Label("Config out of date", systemImage: "arrow.triangle.2.circlepath")
                .foregroundStyle(.orange)
            Spacer(minLength: 8)
            Button("Update All Workspaces…") { model.post(.syncDevcontainers(feature.project)) }
                .help("Copy the project's .devcontainer into every active feature")
                .accessibilityIdentifier("feature.action.syncDevcontainers")
        }
        .padding(10)
        .background(Color.orange.opacity(0.08), in: RoundedRectangle(cornerRadius: 8))
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

#Preview("Container, stopped") {
    let record = PreviewSamples.features[0]
    let feature = FeatureRef(project: PreviewSamples.project, name: record.workFeature)
    EnvironmentCard(record: record, feature: feature,
                    availability: FeatureActionAvailability(record: record, folderExists: true, backendReady: true,
                                                            preferences: LaunchPreferences()),
                    load: .loaded(DevcontainerStatus(state: .stopped)), startItem: nil, onReload: {}, onPerform: { _ in })
        .environment(FeaturePreviewModel.make())
        .padding()
        .frame(width: 480)
}
