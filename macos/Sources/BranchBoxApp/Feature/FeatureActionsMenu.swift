import AppKit
import BranchBoxKit
import BranchBoxStores
import SwiftUI

/// Every action on one feature, shared by the sidebar context menu, the menu bar and the detail toolbar.
///
/// The body is menu content (Buttons, Menus, Dividers and Text only), placed inside a `.contextMenu`, a `Menu`
/// or a `.menu`-style `MenuBarExtra`; it never presents anything itself. Disabled items say why in `.help`.
/// Actions that need a sheet post a `WindowIntent`; from the menu bar they first bring the main window forward.
/// Destructive environment and sharing items exist only in the toolbar overflow, where the feature detail
/// confirms them (`requestFeatureConfirmation`). The initializer is final (DESIGN §4.11).
struct FeatureActionsMenu: View {
    enum Style { case contextMenu, menuBar, toolbarOverflow }

    @Environment(AppModel.self) private var model
    @Environment(\.openWindow) private var openWindow
    @Environment(\.requestFeatureConfirmation) private var requestConfirmation

    let feature: FeatureRef
    let style: Style

    init(feature: FeatureRef, style: Style) {
        self.feature = feature
        self.style = style
    }

    var body: some View {
        let store = model.projects.project(feature.project)
        if let store, let record = store.feature(named: feature.name) {
            items(record: record, availability: availability(record: record, store: store))
        } else {
            Text("\(feature.name) is no longer listed")
            if style == .menuBar {
                Button("Show in BranchBox") { showInMainWindow(.select(.project(path: feature.project.path))) }
            }
        }
    }

    private func availability(record: FeatureRecord, store: ProjectStore) -> FeatureActionAvailability {
        let backendReady: Bool = if case .ready = model.environment.backendState { true } else { false }
        return FeatureActionAvailability(
            record: record, folderExists: store.folderExists(for: record), backendReady: backendReady,
            preferences: LaunchPreferences(settings: model.settings, projectDefaultAgent: store.config?.effective.editorDefaultAgent),
            busyWith: model.operations.active(for: .feature(feature))?.title)
    }

    // MARK: Items

    @ViewBuilder private func items(record: FeatureRecord, availability: FeatureActionAvailability) -> some View {
        if !availability.isRemoved {
            launchItems(record: record, availability: availability)
            Divider()
        }
        copyMenu(record: record)
        revealButton(record: record, availability: availability)
        if style == .toolbarOverflow, !availability.isRemoved {
            Divider()
            environmentMenu(record: record, availability: availability)
            sharingMenu(record: record, availability: availability)
        }
        if let item = primaryRemediation(record: record, availability: availability) {
            Divider()
            remediationButton(item, availability: availability)
        }
        if !availability.isRemoved {
            Divider()
            Button("Tear Down…", role: .destructive) {
                perform(.teardown(feature, preselect: nil))
            }
            .disabled(!availability.teardown.isEnabled)
            .help(availability.teardown.disabledReason ?? "Remove the worktree and environment of \(feature.name)")
            .accessibilityIdentifier("feature.action.teardown")
        }
        if style == .menuBar {
            Divider()
            Button("Show in BranchBox") {
                showInMainWindow(.select(.feature(projectPath: feature.project.path, name: feature.name)))
            }
            .accessibilityIdentifier("feature.action.show")
        }
    }

    @ViewBuilder private func launchItems(record: FeatureRecord, availability: FeatureActionAvailability) -> some View {
        launchButton(availability.primaryEditorTitle, plan: availability.primaryEditor, verb: "editor")
        if record.runtime.provider == .container, !availability.primaryEditorOpensDevContainer {
            launchButton("Open in Dev Container", plan: availability.devContainer, verb: "editor.devcontainer")
        }
        launchButton("Open in Terminal", plan: availability.terminal, verb: "terminal")
        launchButton("Launch \(availability.preferences.agentName)", plan: availability.agent, verb: "agent")
        if let command = availability.terminal.copyCommand {
            Button("Copy Shell Command") { Pasteboard.general.copy(command) }
                .help("Copies: \(command)")
                .accessibilityIdentifier("feature.action.copyShellCommand")
        }
        let links = FeatureLinks.menuLinks(for: record)
        Menu("Open URL") {
            ForEach(Array(links.enumerated()), id: \.offset) { _, link in
                Button(link.title) { HostLaunchFeedback.shared.open(link.url, for: feature) }
            }
        }
        .disabled(links.isEmpty)
        .help(links.isEmpty ? "\(feature.name) has no URLs" : "Open one of the feature's URLs")
        .accessibilityIdentifier("feature.action.openURL")
    }

    private func copyMenu(record: FeatureRecord) -> some View {
        Menu("Copy") {
            Button("Branch") { Pasteboard.general.copy(record.branchName) }
                .disabled(record.branchName.isEmpty)
                .accessibilityIdentifier("feature.action.copyBranch")
            Button("Path") { Pasteboard.general.copy(record.worktreePath ?? "") }
                .disabled(record.worktreePath?.isEmpty ?? true)
                .accessibilityIdentifier("feature.action.copyPath")
            Button("Name") { Pasteboard.general.copy(record.workFeature) }
                .accessibilityIdentifier("feature.action.copyName")
            if let url = record.urls.primary {
                Button("URL") { Pasteboard.general.copy(url.absoluteString) }
                    .accessibilityIdentifier("feature.action.copyURL")
            }
        }
    }

    private func revealButton(record: FeatureRecord, availability: FeatureActionAvailability) -> some View {
        Button("Reveal in Finder") {
            if let path = record.worktreePath { HostLaunchFeedback.shared.reveal(path) }
        }
        .disabled(!availability.reveal.isEnabled)
        .help(availability.reveal.disabledReason ?? "Show the feature's folder in Finder")
        .accessibilityIdentifier("feature.action.reveal")
    }

    @ViewBuilder private func environmentMenu(record: FeatureRecord, availability: FeatureActionAvailability) -> some View {
        let allowed = availability.devcontainer
        Menu("Environment") {
            Button("Start Dev Container") {
                FeatureCommands.dispatch(.devcontainer(.up(removeExisting: false, buildNoCache: false), feature), model: model)
            }
            .accessibilityIdentifier("feature.action.devcontainerStart")
            Button("Stop Dev Container") { FeatureCommands.dispatch(.devcontainer(.down(removeVolumes: false), feature), model: model) }
                .accessibilityIdentifier("feature.action.devcontainerStop")
            if let requestConfirmation {
                Divider()
                Button("Stop and Delete Volumes…") { requestConfirmation(FeatureConfirmations.stopDeletingVolumes(feature)) }
                Button("Rebuild…") { requestConfirmation(FeatureConfirmations.rebuild(feature)) }
                    .accessibilityIdentifier("feature.action.devcontainerRebuild")
            }
        }
        .disabled(!allowed.isEnabled)
        .help(allowed.disabledReason ?? "Start, stop or rebuild the dev container")
    }

    @ViewBuilder private func sharingMenu(record: FeatureRecord, availability: FeatureActionAvailability) -> some View {
        let allowed = availability.tunnel
        let status = record.tunnel?.status
        Menu("Sharing") {
            if status == .active || status == .pending || status == .manual {
                if status != .active {
                    Button("Re-provision Tunnel") { FeatureCommands.dispatch(.tunnelOpen(feature), model: model) }
                }
                if let requestConfirmation {
                    Button("Stop Sharing…") {
                        requestConfirmation(FeatureConfirmations.stopSharing(feature, hostname: record.tunnel?.hostname))
                    }
                    .accessibilityIdentifier("feature.action.tunnelRemove")
                }
            } else {
                let tunnelsOff = model.projects.project(feature.project)?.config?.effective.tunnelEnabled == false
                Button("Share via Tunnel") { FeatureCommands.dispatch(.tunnelOpen(feature), model: model) }
                    .disabled(tunnelsOff)
                    .help(tunnelsOff ? "Tunnels are off for this project (Project Settings)" : "Share the feature through a tunnel")
                    .accessibilityIdentifier("feature.action.tunnelOpen")
            }
        }
        .disabled(!allowed.isEnabled)
        .help(allowed.disabledReason ?? "Share the feature through a tunnel")
    }

    // MARK: Remediation

    /// The callout's main fix, if any (not a teardown, which has its own item, nor a copy or log action).
    private func primaryRemediation(record: FeatureRecord, availability: FeatureActionAvailability) -> RemediationItem? {
        let actions = Remediation.actions(for: record, project: feature.project, identity: model.environment.identity,
                                          folderExists: availability.folderExists, branchExists: record.status == .removed ? false : nil)
        return RemediationPresenter.items(for: actions, record: record).first { $0.role == .primary }
    }

    private func remediationButton(_ item: RemediationItem, availability: FeatureActionAvailability) -> some View {
        Button(item.title) {
            switch item.effect {
            case .post(let intent): perform(intent)
            case .confirmThenDispatch:
                showInMainWindow(.select(.feature(projectPath: feature.project.path, name: feature.name)))
            case .dispatch, .copy, .showLog: RemediationPerformer.perform(item.effect, model: model)
            }
        }
        .disabled(item.needsBackend && !availability.remediation.isEnabled)
        .help(availability.remediation.disabledReason ?? (Remediation.callout(for: availability.record,
                                                                              folderExists: availability.folderExists) ?? item.title))
        .accessibilityIdentifier("feature.action.remediation")
    }

    // MARK: Plumbing

    private func launchButton(_ title: String, plan: HostLaunchPlan, verb: String) -> some View {
        Button(title) { HostLaunchFeedback.shared.launch(plan, for: feature) }
            .disabled(!plan.isEnabled)
            .help(plan.disabledReason ?? title)
            .accessibilityIdentifier("feature.action.\(verb)")
    }

    /// Sheet intents go to the main window; the menu bar brings it forward first.
    private func perform(_ intent: WindowIntent) {
        if style == .menuBar {
            showInMainWindow(intent)
        } else {
            model.post(intent)
        }
    }

    private func showInMainWindow(_ intent: WindowIntent) {
        openWindow(id: SceneID.main)
        NSApplication.shared.activate()
        model.post(intent)
    }
}
