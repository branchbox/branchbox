import AppKit
import BranchBoxKit
import BranchBoxStores
import SwiftUI

/// Every command the menus offer (DESIGN §9 "Scenes and menus").
enum AppCommand: String, CaseIterable, Sendable {
    case startFeature, addProject
    case refresh, showRemoved, inspector, quickOpen
    case openInEditor, terminal, launchAgent, openURL, reveal, runCommand, copyPath, copyBranch
    case startDevContainer, stopDevContainer, shareViaTunnel
    case tearDown
    case projectSettings, prune, syncDevcontainers, setUp, repair, removeProject
    case activity, diagnostics
}

/// Whether a command can run now, and why not (shown with `.help`).
struct CommandState: Sendable, Hashable {
    let isEnabled: Bool
    let reason: String?

    static let enabled = CommandState(isEnabled: true, reason: nil)
    static func disabled(_ reason: String) -> CommandState { CommandState(isEnabled: false, reason: reason) }
}

/// What the commands act on: the key main window's selection resolved against the stores, plus the settings the
/// host launches use. A value, so the enablement matrix is testable without a window.
struct CommandContext: Sendable, Hashable {
    var backendReady = false
    var hasProjects = false
    /// A main window is key (commands target it); without one, commands open it and post an intent.
    var hasMainWindow = false
    /// The selected project, or the selected feature's or stray's project.
    var project: ProjectRef?
    var projectRootExists = true
    /// The selected feature, when it still exists.
    var feature: FeatureRecord?
    var featureFolderExists = true
    var featureBusy: String?
    var editor: EditorChoice = .vscode
    var editorMode: EditorOpenMode = .folder
    var terminal: TerminalChoice = .terminal
    var agent: AgentChoice = .claude
    var passPrompt = true

    static let cliUnavailable = "The BranchBox CLI isn't available; see Diagnostics"
    static let noFeature = "Select a feature first"
    static let noProject = "Select a project first"

    func state(_ command: AppCommand) -> CommandState {
        switch command {
        case .startFeature:
            if !backendReady { return .disabled(Self.cliUnavailable) }
            if !hasProjects { return .disabled("Add a project first") }
            return .enabled
        case .addProject:
            return backendReady ? .enabled : .disabled(Self.cliUnavailable)
        case .refresh:
            if !backendReady { return .disabled(Self.cliUnavailable) }
            return hasProjects ? .enabled : .disabled("There are no projects to refresh")
        case .showRemoved:
            return project == nil ? .disabled(Self.noProject) : .enabled
        case .inspector:
            return hasMainWindow ? .enabled : .disabled("Open the BranchBox window first")
        case .quickOpen, .activity, .diagnostics:
            return .enabled
        case .openInEditor, .terminal, .launchAgent:
            guard let feature else { return .disabled(Self.noFeature) }
            if feature.status == .removed { return .disabled("This feature was torn down") }
            let plan = hostPlan(command, for: feature)
            return plan.isEnabled ? .enabled : .disabled(plan.disabledReason ?? "Not available for this feature")
        case .openURL:
            guard let feature else { return .disabled(Self.noFeature) }
            return Self.primaryURL(feature) == nil ? .disabled("This feature has no URL") : .enabled
        case .reveal:
            guard let feature else { return .disabled(Self.noFeature) }
            return featureFolderExists && feature.worktreePath != nil
                ? .enabled : .disabled("The feature's folder is missing")
        case .copyPath:
            guard let feature else { return .disabled(Self.noFeature) }
            return (feature.worktreePath ?? "").isEmpty ? .disabled("No folder is recorded for this feature") : .enabled
        case .copyBranch:
            guard let feature else { return .disabled(Self.noFeature) }
            return feature.branchName.isEmpty ? .disabled("No branch is recorded for this feature") : .enabled
        case .runCommand:
            guard let feature else { return .disabled(Self.noFeature) }
            if !backendReady { return .disabled(Self.cliUnavailable) }
            if let issue = feature.worktreeIssue { return .disabled("Git worktree needs repair: \(issue)") }
            return feature.status == .removed ? .disabled("This feature was torn down") : .enabled
        case .startDevContainer, .stopDevContainer:
            guard let feature else { return .disabled(Self.noFeature) }
            if !backendReady { return .disabled(Self.cliUnavailable) }
            if let issue = feature.worktreeIssue { return .disabled("Git worktree needs repair: \(issue)") }
            if feature.runtime.provider != .container { return .disabled("Only container features have a dev container") }
            if !featureFolderExists { return .disabled("The feature's folder is missing") }
            return featureBusy.map { .disabled($0) } ?? .enabled
        case .shareViaTunnel:
            guard let feature else { return .disabled(Self.noFeature) }
            if !backendReady { return .disabled(Self.cliUnavailable) }
            if feature.status == .removed { return .disabled("This feature was torn down") }
            if let issue = feature.worktreeIssue { return .disabled("Git worktree needs repair: \(issue)") }
            if feature.tunnel?.status == .active { return .disabled("This feature is already shared") }
            return featureBusy.map { .disabled($0) } ?? .enabled
        case .tearDown:
            guard let feature else { return .disabled(Self.noFeature) }
            if !backendReady { return .disabled(Self.cliUnavailable) }
            if feature.status == .removed { return .disabled("This feature was already torn down") }
            if let issue = feature.worktreeIssue { return .disabled("Git worktree needs repair: \(issue)") }
            return featureBusy.map { .disabled($0) } ?? .enabled
        case .projectSettings, .removeProject:
            return project == nil ? .disabled(Self.noProject) : .enabled
        case .prune, .syncDevcontainers, .setUp, .repair:
            guard project != nil else { return .disabled(Self.noProject) }
            if !backendReady { return .disabled(Self.cliUnavailable) }
            return projectRootExists ? .enabled : .disabled("The project's folder is missing")
        }
    }

    /// The host launch behind Open in Editor, Terminal and Launch Agent.
    func hostPlan(_ command: AppCommand, for feature: FeatureRecord, projectDefaultAgent: String? = nil) -> HostLaunchPlan {
        switch command {
        case .terminal:
            return HostLaunchPlan.terminal(terminal, record: feature, folderExists: featureFolderExists)
        case .launchAgent:
            let choice = HostLaunchPlan.agentChoice(projectDefault: projectDefaultAgent, fallback: agent)
            return HostLaunchPlan.agent(choice, terminal: terminal, record: feature, passPrompt: passPrompt,
                                        folderExists: featureFolderExists)
        default:
            let mode: EditorOpenMode = feature.worktreeIssue == nil ? editorMode : .folder
            return HostLaunchPlan.editor(editor, mode: mode, record: feature, folderExists: featureFolderExists)
        }
    }

    /// The feature URL, else its tunnel, else its first published port.
    static func primaryURL(_ feature: FeatureRecord) -> URL? {
        let urls = feature.urls
        return urls.primary ?? urls.tunnel ?? urls.ports.first?.url
    }
}

extension CommandContext {
    /// The context of `selection` in `model` (nil selection: no main window, or nothing selected).
    @MainActor init(model: AppModel, selection: SidebarSelection?, hasMainWindow: Bool) {
        let settings = model.settings
        backendReady = model.environment.identity != nil
        hasProjects = !model.projects.projects.isEmpty
        self.hasMainWindow = hasMainWindow
        editor = settings.preferredEditor
        editorMode = settings.editorOpenMode
        terminal = settings.preferredTerminal
        agent = settings.agentChoice
        passPrompt = settings.passPromptToAgent
        guard let ref = selection?.projectRef, let store = model.projects.project(ref) else { return }
        project = store.ref
        projectRootExists = store.rootExists
        if let featureRef = selection?.featureRef, let record = store.feature(named: featureRef.name) {
            feature = record
            featureFolderExists = store.folderExists(for: record)
            if case .rejected(let reason) = model.operations.admission(for: .teardown, target: .feature(featureRef)) {
                featureBusy = reason
            }
        }
    }
}

/// The side effects of the Feature menu, shared with notification actions and Quick Open.
@MainActor enum FeatureCommands {
    static func openInEditor(_ record: FeatureRecord, project: ProjectRef, model: AppModel) {
        launch(.openInEditor, record, project: project, model: model)
    }

    static func launch(_ command: AppCommand, _ record: FeatureRecord, project: ProjectRef, model: AppModel) {
        let store = model.projects.project(project)
        let selection = SidebarSelection.feature(projectPath: project.path, name: record.workFeature)
        let context = CommandContext(model: model, selection: selection, hasMainWindow: true)
        let plan = context.hostPlan(command, for: record, projectDefaultAgent: store?.config?.effective.editorDefaultAgent)
        Task { @MainActor in
            do {
                try await HostLauncher().launch(plan)
            } catch let error as HostLaunchError {
                ToastCenter.shared.show(title: "Couldn't open \(record.workFeature)", body: error.message)
            } catch {
                ToastCenter.shared.show(title: "Couldn't open \(record.workFeature)", body: error.localizedDescription)
            }
        }
    }

    static func openURL(_ record: FeatureRecord) {
        guard let url = CommandContext.primaryURL(record) else { return }
        NSWorkspace.shared.open(url)
    }

    static func reveal(_ record: FeatureRecord) {
        guard let path = record.worktreePath else { return }
        NSWorkspace.shared.activateFileViewerSelecting([URL(fileURLWithPath: path, isDirectory: true)])
    }

    static func copy(_ text: String, announcing what: String) {
        Pasteboard.general.copy(text)
        AccessibilityNotification.Announcement("Copied \(what)").post()
    }

    /// Dispatches `request`; a refusal to start (busy, quitting, no CLI) is reported as a toast.
    static func dispatch(_ request: OperationRequestContext, model: AppModel) {
        report(model.actions.dispatch(request), for: request, model: model)
    }

    /// Shows a dispatch that didn't start (a rejection, an unavailable backend) as a toast in the main window.
    static func report(_ result: DispatchResult, for request: OperationRequestContext, model: AppModel) {
        switch result {
        case .started, .queued:
            break
        case .rejected(let reason):
            ToastCenter.shared.show(title: model.actions.title(for: request), body: reason)
        case .unavailable(let error):
            ToastCenter.shared.show(title: model.actions.title(for: request), body: error.presentation().message)
        }
    }

    /// Removes a project from the list after a confirmation (files are never touched).
    static func confirmRemove(_ project: ProjectRef, model: AppModel) {
        let name = model.projects.project(project)?.displayName ?? project.displayName
        let alert = NSAlert()
        alert.messageText = "Remove “\(name)” from BranchBox?"
        alert.informativeText = "Its folder, features and worktrees stay on disk; you can add it again later."
        alert.addButton(withTitle: "Remove")
        alert.addButton(withTitle: "Cancel").keyEquivalent = "\u{1b}"
        guard alert.runModal() == .alertFirstButtonReturn else { return }
        model.projects.remove(project)
    }

    /// Asks for a new location of a project whose folder moved.
    static func locate(_ project: ProjectRef, model: AppModel) {
        let panel = NSOpenPanel()
        panel.canChooseDirectories = true
        panel.canChooseFiles = false
        panel.allowsMultipleSelection = false
        panel.message = "Locate the folder of \(model.projects.project(project)?.displayName ?? project.displayName)"
        panel.prompt = "Locate"
        guard panel.runModal() == .OK, let url = panel.url else { return }
        Task { @MainActor in
            switch await model.projects.relocate(project, to: url) {
            case .added, .alreadyPresent:
                break
            case .needsInit(let ref):
                model.post(.initProject(ref.root, mode: .setUp))
            case .refused(let error):
                ToastCenter.shared.show(title: "Couldn't use that folder", body: error.presentation().message)
            }
        }
    }

    /// Asks for the branchbox executable, saves it as the Settings override and re-detects (no relaunch).
    static func locateCLI(model: AppModel) {
        let panel = NSOpenPanel()
        panel.canChooseFiles = true
        panel.canChooseDirectories = false
        panel.allowsMultipleSelection = false
        panel.treatsFilePackagesAsDirectories = true
        panel.message = "Choose the branchbox command-line tool"
        panel.prompt = "Use This CLI"
        panel.directoryURL = URL(fileURLWithPath: "/opt/homebrew/bin", isDirectory: true)
        guard panel.runModal() == .OK, let url = panel.url else { return }
        model.settings.cliPathOverride = url.path
        Task { await model.environment.rebootstrap() }
    }
}

/// The app's menus and shortcuts (DESIGN §9). Commands act on the key main window through its router; with no
/// main window, they open it and post an intent, which the window performs when it appears. A disabled command
/// says why in its help tag.
struct AppCommands: Commands {
    let model: AppModel
    @FocusedValue(\.mainWindowRouter) private var router
    @Environment(\.openWindow) private var openWindow

    private var context: CommandContext {
        CommandContext(model: model, selection: router?.selection, hasMainWindow: router != nil)
    }

    var body: some Commands {
        let context = self.context
        CommandGroup(replacing: .newItem) {
            item(.startFeature, "Start Feature…", context) { post(.startFeature(project: context.project, prefill: nil)) }
                .keyboardShortcut("n")
            item(.addProject, "Add Project…", context) { post(.addProject(nil)) }
                .keyboardShortcut("o")
        }

        CommandGroup(after: .sidebar) {
            item(.refresh, "Refresh", context) { refresh(context) }
                .keyboardShortcut("r")
            item(.showRemoved, showRemovedTitle(context), context) { toggleRemoved(context) }
                .keyboardShortcut(".", modifiers: [.command, .shift])
            item(.inspector, router?.isInspectorPresented == true ? "Hide Inspector" : "Show Inspector", context) {
                router?.isInspectorPresented.toggle()
            }
            .keyboardShortcut("i", modifiers: [.command, .option])
            item(.quickOpen, "Quick Open…", context) { post(.quickOpen) }
                .keyboardShortcut("k")
            Divider()
        }

        CommandMenu("Feature") {
            featureItems(context)
        }

        CommandMenu("Project") {
            projectItems(context)
        }

        CommandGroup(before: .windowList) {
            item(.activity, "Activity", context) { openWindow(id: SceneID.activity) }
                .keyboardShortcut("l", modifiers: [.command, .option])
            item(.diagnostics, "Diagnostics", context) { openWindow(id: SceneID.diagnostics) }
            Divider()
        }
    }

    @ViewBuilder private func featureItems(_ context: CommandContext) -> some View {
        let feature = context.feature
        let project = context.project
        item(.openInEditor, "Open in Editor", context) { launch(.openInEditor, context) }
            .keyboardShortcut("e", modifiers: [.command, .control])
        item(.terminal, "Open in Terminal", context) { launch(.terminal, context) }
            .keyboardShortcut("t", modifiers: [.command, .control])
        item(.launchAgent, "Launch Agent", context) { launch(.launchAgent, context) }
            .keyboardShortcut("a", modifiers: [.command, .control])
        item(.openURL, "Open URL", context) { feature.map(FeatureCommands.openURL) }
            .keyboardShortcut("o", modifiers: [.command, .control])
        item(.reveal, "Reveal in Finder", context) { feature.map(FeatureCommands.reveal) }
            .keyboardShortcut("r", modifiers: [.command, .control])
        item(.runCommand, "Run Command…", context) {
            if let feature, let project {
                openWindow(id: SceneID.run, value: FeatureRef(project: project, name: feature.workFeature))
            }
        }
        .keyboardShortcut("r", modifiers: [.command, .option])
        Divider()
        item(.copyPath, "Copy Path", context) {
            if let path = feature?.worktreePath { FeatureCommands.copy(path, announcing: "path") }
        }
        .keyboardShortcut("c", modifiers: [.command, .option])
        item(.copyBranch, "Copy Branch", context) {
            if let branch = feature?.branchName { FeatureCommands.copy(branch, announcing: "branch") }
        }
        .keyboardShortcut("c", modifiers: [.command, .option, .shift])
        Divider()
        Menu("Environment") {
            item(.startDevContainer, "Start Dev Container", context) {
                dispatchDevcontainer(.up(removeExisting: false, buildNoCache: false), context)
            }
            item(.stopDevContainer, "Stop Dev Container", context) {
                dispatchDevcontainer(.down(removeVolumes: false), context)
            }
        }
        Menu("Sharing") {
            item(.shareViaTunnel, "Share via Tunnel", context) {
                if let feature, let project {
                    FeatureCommands.dispatch(.tunnelOpen(FeatureRef(project: project, name: feature.workFeature)), model: model)
                }
            }
        }
        Divider()
        item(.tearDown, "Tear Down…", context) {
            // ⌘⌫ typed in a text field (the sidebar filter, Quick Open) means "delete to the start of the line":
            // the menu sees the key equivalent first, so hand it back to the field editor instead of tearing down.
            if NSApp.currentEvent?.type == .keyDown, NSApp.keyWindow?.firstResponder is NSText {
                NSApp.sendAction(#selector(NSResponder.deleteToBeginningOfLine(_:)), to: nil, from: nil)
                return
            }
            if let feature, let project {
                post(.teardown(FeatureRef(project: project, name: feature.workFeature), preselect: nil))
            }
        }
        .keyboardShortcut(.delete, modifiers: .command)
    }

    @ViewBuilder private func projectItems(_ context: CommandContext) -> some View {
        let project = context.project
        item(.projectSettings, "Settings…", context) { project.map { post(.projectSettings($0)) } }
        item(.prune, "Prune…", context) { project.map { post(.prune($0)) } }
        item(.syncDevcontainers, "Update All Workspaces…", context) { project.map { post(.syncDevcontainers($0)) } }
        Divider()
        item(.setUp, "Set Up…", context) { project.map { post(.initProject($0.root, mode: .setUp)) } }
        item(.repair, "Repair…", context) { project.map { post(.initProject($0.root, mode: .repair)) } }
        Divider()
        item(.removeProject, "Remove…", context) { project.map { FeatureCommands.confirmRemove($0, model: model) } }
    }

    // MARK: Helpers

    private func item(_ command: AppCommand, _ title: String, _ context: CommandContext,
                      action: @escaping () -> Void) -> some View {
        let state = context.state(command)
        return Button(title, action: action)
            .disabled(!state.isEnabled)
            .help(state.reason ?? "")
    }

    /// Hands `intent` to the key main window, or opens the main window and lets it perform the intent on appear.
    private func post(_ intent: WindowIntent) {
        if router == nil {
            openWindow(id: SceneID.main)
            NSApp.activate()
        }
        model.post(intent)
    }

    private func refresh(_ context: CommandContext) {
        if let project = context.project, let store = model.projects.project(project) {
            store.requestRefresh(.manual)
        } else {
            model.projects.refreshAll(.manual)
        }
    }

    private func showRemovedTitle(_ context: CommandContext) -> String {
        guard let project = context.project, let store = model.projects.project(project) else { return "Show Removed" }
        return store.includeRemoved ? "Hide Removed" : "Show Removed"
    }

    private func toggleRemoved(_ context: CommandContext) {
        guard let project = context.project, let store = model.projects.project(project) else { return }
        store.includeRemoved.toggle()
    }

    private func launch(_ command: AppCommand, _ context: CommandContext) {
        guard let feature = context.feature, let project = context.project else { return }
        FeatureCommands.launch(command, feature, project: project, model: model)
    }

    private func dispatchDevcontainer(_ action: DevcontainerAction, _ context: CommandContext) {
        guard let feature = context.feature, let project = context.project else { return }
        FeatureCommands.dispatch(.devcontainer(action, FeatureRef(project: project, name: feature.workFeature)), model: model)
    }
}
