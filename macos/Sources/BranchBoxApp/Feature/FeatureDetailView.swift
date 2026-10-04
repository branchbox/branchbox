import BranchBoxKit
import BranchBoxPreview
import BranchBoxStores
import SwiftUI

/// The selected feature: header, toolbar, health callout and cards.
///
/// The record is looked up again on every render (`ProjectStore.feature(named:)`), so a refresh, a teardown
/// elsewhere or a registry edit shows at once. A feature that is no longer listed offers to show removed
/// features. The initializer is final (DESIGN §4.11).
struct FeatureDetailView: View {
    @Environment(AppModel.self) private var model

    let feature: FeatureRef

    init(feature: FeatureRef) {
        self.feature = feature
    }

    var body: some View {
        let store = model.projects.project(feature.project)
        Group {
            if let store, let record = store.feature(named: feature.name) {
                FeatureDetailContent(feature: feature, record: record, folderExists: store.folderExists(for: record),
                                     config: store.config?.effective)
            } else if let store, store.features.isEmpty, Self.isLoading(store.loadState) {
                ProgressView("Loading \(feature.name)…")
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                missing(store)
            }
        }
        .task(id: feature.project.path) {
            if let store, store.config == nil { await store.reloadConfig() }
        }
    }

    private func missing(_ store: ProjectStore?) -> some View {
        ContentUnavailableView {
            Label("This feature no longer exists", systemImage: "questionmark.folder")
        } description: {
            Text("\(feature.name) isn't in \(store?.displayName ?? feature.project.displayName)'s registry anymore. "
                 + "It may have been torn down from the command line.")
        } actions: {
            if let store, !store.includeRemoved {
                Button("Show Removed Features") {
                    store.includeRemoved = true
                    model.post(.select(.feature(projectPath: feature.project.path, name: feature.name)))
                }
                .buttonStyle(.borderedProminent)
                .accessibilityIdentifier("feature.action.showRemoved")
            }
        }
    }

    private static func isLoading(_ state: LoadState) -> Bool {
        switch state {
        case .idle, .loading: true
        case .loaded, .failed: false
        }
    }
}

/// The detail for a resolved record. Split from `FeatureDetailView` so previews and render tests can show any
/// record and disk state directly.
struct FeatureDetailContent: View {
    @Environment(AppModel.self) private var model
    @Environment(\.openWindow) private var openWindow

    let feature: FeatureRef
    let record: FeatureRecord
    let folderExists: Bool
    /// The project's effective config (tunnels, default agent); nil while it loads.
    let config: ProjectConfig?
    /// Render tests pin the dev container's state and the branch's existence instead of reading the backend.
    var pinnedDevcontainer: DevcontainerLoad?
    var pinnedBranchExists: Bool?

    @State private var devcontainer: DevcontainerLoad = .loading
    @State private var devcontainerReloads = 0
    @State private var branchExists: Bool?
    @State private var pendingConfirmation: FeatureConfirmation?

    var body: some View {
        let availability = self.availability
        let attention = Remediation.attention(for: record, folderExists: folderExists)
        let items = remediationItems
        ScrollView {
            VStack(alignment: .leading, spacing: 20) {
                FeatureHeader(record: record, feature: feature, availability: availability, attention: attention,
                              onTearDown: { model.post(.teardown(feature, preselect: nil)) })
                launchFailure
                activeOperation
                recentFailure
                if HealthCallout.isShown(for: record, folderExists: folderExists) {
                    HealthCallout(record: record, folderExists: folderExists, items: items,
                                  availability: availability.remediation) { effect in
                        RemediationPerformer.perform(effect, model: model)
                    }
                }
                CardGridLayout {
                    cards(availability: availability, items: items)
                }
            }
            .padding(24)
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        .navigationTitle(record.workFeature)
        .navigationSubtitle(model.projects.project(feature.project)?.displayName ?? feature.project.displayName)
        .toolbar {
            ToolbarItemGroup(placement: .primaryAction) {
                Button {
                    openWindow(id: SceneID.run, value: feature)
                } label: {
                    Label("Run Command…", systemImage: "play.rectangle")
                }
                .disabled(!availability.runCommand.isEnabled)
                .help(availability.runCommand.disabledReason ?? "Run a command in \(record.workFeature)")
                .accessibilityIdentifier("feature.action.runCommand")
                Menu {
                    FeatureActionsMenu(feature: feature, style: .toolbarOverflow)
                } label: {
                    Label("More", systemImage: "ellipsis.circle")
                }
                .help("More actions for \(record.workFeature)")
                .accessibilityIdentifier("feature.action.more")
            }
        }
        .environment(\.requestFeatureConfirmation) { confirmation in pendingConfirmation = confirmation }
        .featureConfirmationDialog($pendingConfirmation, model: model)
        .task(id: devcontainerKey) { await loadDevcontainer() }
        .task(id: branchKey) { await loadBranchExists() }
    }

    // MARK: State

    private var availability: FeatureActionAvailability {
        let backendReady: Bool = if case .ready = model.environment.backendState { true } else { false }
        return FeatureActionAvailability(
            record: record, folderExists: folderExists, backendReady: backendReady,
            preferences: LaunchPreferences(settings: model.settings, projectDefaultAgent: config?.editorDefaultAgent),
            busyWith: model.operations.active(for: .feature(feature))?.title)
    }

    private var remediationItems: [RemediationItem] {
        let exists: Bool? = record.status == .removed ? (pinnedBranchExists ?? branchExists ?? false) : nil
        let actions = Remediation.actions(for: record, project: feature.project, identity: model.environment.identity,
                                          folderExists: folderExists, branchExists: exists)
        return RemediationPresenter.items(for: actions, record: record)
    }

    private var devcontainerLoad: DevcontainerLoad { pinnedDevcontainer ?? devcontainer }

    /// Changes when the dev container may have changed: a dev container operation finished, the record was
    /// updated, or the user asked.
    private var devcontainerKey: String {
        let finished = model.operations.records(for: .feature(feature))
            .filter { EnvironmentCard.devcontainerKinds.contains($0.kind) && !$0.isCancellable }.count
        return "\(feature.project.path)#\(feature.name)#\(finished)#\(devcontainerReloads)#\(record.updatedAt?.timeIntervalSince1970 ?? 0)#\(folderExists)#\(record.status)#\(record.worktreeIssue ?? "")"
    }

    private func loadDevcontainer() async {
        guard pinnedDevcontainer == nil, record.runtime.provider == .container else { return }
        guard record.status != .removed, folderExists else {
            devcontainer = .unavailable(record.status == .removed
                ? "The feature was torn down, so there is no dev container to check."
                : "The feature's folder is missing, so there is no dev container to check or start.")
            return
        }
        if let issue = record.worktreeIssue {
            devcontainer = .unavailable("Git worktree needs repair: \(issue)")
            return
        }
        if devcontainer.status == nil { devcontainer = .loading }
        do {
            let status = try await model.backend().devcontainerStatus(for: feature)
            devcontainer = .loaded(status)
        } catch {
            if Task.isCancelled { return }
            devcontainer = .failed(BackendError.normalize(error))
        }
    }

    /// Changes when the branch may have appeared or gone: the status changed, or a Delete Branch of it finished
    /// (deleting a removed feature's branch leaves its record unchanged).
    private var branchKey: String {
        let branch = record.branchName
        let finished = model.operations.records(for: .project(feature.project)).filter {
            if case .deleteBranch(let name, _, _) = $0.context { return name == branch && !$0.isCancellable }
            return false
        }.count
        return "\(record.status)#\(finished)"
    }

    private func loadBranchExists() async {
        guard pinnedBranchExists == nil, record.status == .removed, !record.branchName.isEmpty else { return }
        do {
            let branches = try await model.backend().listBranches(in: feature.project)
            branchExists = branches.local.contains(record.branchName)
        } catch {
            branchExists = nil
        }
    }

    // MARK: Banners

    @ViewBuilder private var launchFailure: some View {
        if let failure = HostLaunchFeedback.shared.failure, failure.feature == feature {
            HStack(alignment: .firstTextBaseline, spacing: 10) {
                Image(systemName: "exclamationmark.triangle.fill")
                    .foregroundStyle(.orange)
                    .accessibilityHidden(true)
                Text(failure.message)
                    .textSelection(.enabled)
                    .fixedSize(horizontal: false, vertical: true)
                Spacer(minLength: 8)
                Button("Dismiss") { HostLaunchFeedback.shared.dismiss() }
            }
            .padding(10)
            .background(Color.orange.opacity(0.1), in: RoundedRectangle(cornerRadius: 8))
            .accessibilityElement(children: .contain)
        }
    }

    @ViewBuilder private var activeOperation: some View {
        if let record = model.operations.active(for: .feature(feature)) {
            HStack(spacing: 12) {
                OperationRow(record: record)
                Button("Show Activity") { model.post(.showActivity(operation: record.id)) }
                    .accessibilityIdentifier("feature.action.showActivity")
            }
            .padding(10)
            .background(Color.accentColor.opacity(0.08), in: RoundedRectangle(cornerRadius: 8))
        }
    }

    /// The feature's most recent failed operation the user hasn't looked at (dev container and sharing failures
    /// show in their cards), plus a failed Delete Branch of a removed feature.
    @ViewBuilder private var recentFailure: some View {
        let inCards = EnvironmentCard.devcontainerKinds.union(TunnelCard.tunnelKinds)
        let branch = record.branchName
        let candidates = model.operations.records(for: .feature(feature)).filter { !inCards.contains($0.kind) }
            + model.operations.records(for: .project(feature.project)).filter {
                if case .deleteBranch(let name, _, _) = $0.context { return name == branch && !branch.isEmpty }
                return false
            }
        let latest = candidates.max { $0.startedAt < $1.startedAt }
        if let latest, latest.needsAttention, case .failed(let error) = latest.state {
            VStack(alignment: .trailing, spacing: 4) {
                ResultCard(error: error, context: latest.context, operationID: latest.id) { recovery in
                    HostLaunchFeedback.shared.perform(recovery, for: feature, model: model, openWindow: openWindow)
                    latest.acknowledge()
                }
                Button("Dismiss") { latest.acknowledge() }
                    .buttonStyle(.borderless)
                    .font(.caption)
            }
        }
    }

    // MARK: Cards

    @ViewBuilder private func cards(availability: FeatureActionAvailability, items: [RemediationItem]) -> some View {
        if record.status == .removed {
            OverviewCard(record: record, folderExists: folderExists)
            ModulesCard(record: record)
            PullRequestCard(prNumber: record.prNumber)
        } else {
            OpenLinksCard(record: record, feature: feature, service: devcontainerLoad.status?.service)
            EnvironmentCard(record: record, feature: feature, availability: availability, load: devcontainerLoad,
                            startItem: items.first(where: Self.startsEnvironment),
                            onReload: { devcontainerReloads += 1 },
                            onPerform: { effect in RemediationPerformer.perform(effect, model: model) })
            OverviewCard(record: record, folderExists: folderExists)
            AgentPromptCard(record: record, feature: feature, availability: availability,
                            autoLaunchConfigured: config?.autoLaunchAgentTerminal == true)
            TunnelCard(record: record, feature: feature, availability: availability, tunnelsEnabled: config?.tunnelEnabled)
            ModulesCard(record: record)
            if let adapter = record.adapter {
                AdapterCard(adapter: adapter)
            }
            PullRequestCard(prNumber: record.prNumber)
        }
    }

    /// The remediation that starts a non-container environment again (§9.2: "Start Environment per §9.1").
    static func startsEnvironment(_ item: RemediationItem) -> Bool {
        switch item.action {
        case .retryRetainedRuntime, .startEnvironment: true
        default: false
        }
    }
}

/// An `AppModel` on the preview backend for SwiftUI previews; storage lives in a temporary folder.
@MainActor enum FeaturePreviewModel {
    static func make(_ scenario: PreviewScenario = .contract) -> AppModel {
        let folder = FileManager.default.temporaryDirectory.appendingPathComponent("branchbox-previews", isDirectory: true)
        let defaults = UserDefaults(suiteName: folder.appendingPathComponent("settings").path) ?? .standard
        return AppModel(settings: AppSettings(defaults: defaults), bootstrapper: PreviewBootstrapper(scenario: scenario),
                        notifier: NoopNotifier(), configuration: .isolated(in: folder))
    }
}

#Preview("Active container feature") {
    let record = PreviewSamples.features[0]
    FeatureDetailContent(feature: FeatureRef(project: PreviewSamples.project, name: record.workFeature), record: record,
                         folderExists: true, config: .defaults,
                         pinnedDevcontainer: .loaded(DevcontainerStatus(state: .running, containerID: "4b1f0c9e2a7d")))
        .environment(FeaturePreviewModel.make())
        .frame(width: 1000, height: 900)
}

#Preview("Degraded sandbox") {
    let record = PreviewSamples.features.first { $0.workFeature == "sbx-demo" } ?? PreviewSamples.features[0]
    FeatureDetailContent(feature: FeatureRef(project: PreviewSamples.project, name: record.workFeature), record: record,
                         folderExists: true, config: .defaults)
        .environment(FeaturePreviewModel.make())
        .frame(width: 820, height: 900)
}

#Preview("Interrupted setup") {
    let record = PreviewSamples.interruptedFeature
    FeatureDetailContent(feature: FeatureRef(project: PreviewSamples.project, name: record.workFeature), record: record,
                         folderExists: true, config: .defaults, pinnedDevcontainer: .loaded(DevcontainerStatus(state: .notCreated)))
        .environment(FeaturePreviewModel.make(.interruptedSetup))
        .frame(width: 820, height: 900)
}

#Preview("Orphaned, folder missing") {
    let record = PreviewSamples.features.first { $0.workFeature == "orphan" } ?? PreviewSamples.features[0]
    FeatureDetailContent(feature: FeatureRef(project: PreviewSamples.project, name: record.workFeature), record: record,
                         folderExists: false, config: .defaults)
        .environment(FeaturePreviewModel.make())
        .frame(width: 820, height: 760)
}

#Preview("Removed") {
    let record = PreviewSamples.features.first { $0.status == .removed } ?? PreviewSamples.features[0]
    FeatureDetailContent(feature: FeatureRef(project: PreviewSamples.project, name: record.workFeature), record: record,
                         folderExists: false, config: .defaults, pinnedBranchExists: true)
        .environment(FeaturePreviewModel.make())
        .frame(width: 820, height: 760)
}
