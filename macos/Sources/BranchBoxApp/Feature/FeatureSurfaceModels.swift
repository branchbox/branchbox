import BranchBoxKit
import BranchBoxStores
import Foundation

// Value-level decisions behind the feature surfaces (detail, actions menu, Run Command): what is available and
// why not, which request a remediation runs, which links are links, and how an exec result reads. Views render
// these; FeatureSurfaceTests check them without a window.

// MARK: Availability

/// Whether an action can run now; a disabled action always says why (shown in `.help`).
struct ActionAvailability: Sendable, Hashable {
    let disabledReason: String?

    var isEnabled: Bool { disabledReason == nil }

    static let enabled = ActionAvailability(disabledReason: nil)
    static func disabled(_ reason: String) -> ActionAvailability { ActionAvailability(disabledReason: reason) }

    /// The first disabled one wins.
    static func first(_ checks: ActionAvailability...) -> ActionAvailability {
        checks.first { !$0.isEnabled } ?? .enabled
    }
}

/// The launch-related settings as values: App Settings plus the project's `editor.default_agent` slug.
struct LaunchPreferences: Sendable, Hashable {
    var editor: EditorChoice = .vscode
    var editorMode: EditorOpenMode = .folder
    var terminal: TerminalChoice = .terminal
    var agent: AgentChoice = .claude
    var agentDisplayName: String?
    var passPrompt = true
    /// `editor.default_agent` from the project config; a slug, never shell text.
    var projectDefaultAgent: String?

    init(editor: EditorChoice = .vscode, editorMode: EditorOpenMode = .folder, terminal: TerminalChoice = .terminal,
         agent: AgentChoice = .claude, agentDisplayName: String? = nil, passPrompt: Bool = true,
         projectDefaultAgent: String? = nil) {
        self.editor = editor
        self.editorMode = editorMode
        self.terminal = terminal
        self.agent = agent
        self.agentDisplayName = agentDisplayName
        self.passPrompt = passPrompt
        self.projectDefaultAgent = projectDefaultAgent
    }

    @MainActor init(settings: AppSettings, projectDefaultAgent: String?) {
        self.init(editor: settings.preferredEditor, editorMode: settings.editorOpenMode, terminal: settings.preferredTerminal,
                  agent: settings.agentChoice, agentDisplayName: settings.agentDisplayName,
                  passPrompt: settings.passPromptToAgent, projectDefaultAgent: projectDefaultAgent)
    }

    /// The agent Launch Agent runs (through `HostLaunchPlan.agentChoice`, never built here).
    var resolvedAgent: AgentChoice {
        HostLaunchPlan.agentChoice(projectDefault: projectDefaultAgent, fallback: agent)
    }

    /// "Claude Code", "Codex", or the display name / command word of a custom agent.
    var agentName: String {
        switch resolvedAgent {
        case .claude: return "Claude Code"
        case .codex: return "Codex"
        case .custom(let command):
            if let name = agentDisplayName?.trimmingCharacters(in: .whitespaces), !name.isEmpty { return name }
            return command.split(separator: " ").first.map(String.init) ?? "Agent"
        }
    }

    /// "VS Code", "Cursor", or the custom app's name.
    static func editorName(_ choice: EditorChoice) -> String {
        switch choice {
        case .vscode: "VS Code"
        case .cursor: "Cursor"
        case .custom(let appPath):
            appPath.isEmpty ? "Editor" : URL(fileURLWithPath: appPath).deletingPathExtension().lastPathComponent
        }
    }
}

/// Everything one feature offers, decided once per render from the record, the disk and the app state.
struct FeatureActionAvailability: Sendable, Hashable {
    let record: FeatureRecord
    let folderExists: Bool
    let backendReady: Bool
    let preferences: LaunchPreferences
    /// The title of an operation still running or queued on this feature.
    let busyWith: String?

    init(record: FeatureRecord, folderExists: Bool, backendReady: Bool, preferences: LaunchPreferences,
         busyWith: String? = nil) {
        self.record = record
        self.folderExists = folderExists
        self.backendReady = backendReady
        self.preferences = preferences
        self.busyWith = busyWith
    }

    var isRemoved: Bool { record.status == .removed }

    // MARK: Host launches (HostLaunchPlan decides; the folder check comes from the disk)

    /// The preferred editor in the preferred mode; a dev-container preference falls back to the folder for
    /// features that have no dev container.
    var primaryEditor: HostLaunchPlan {
        let mode: EditorOpenMode = preferences.editorMode == .devContainer && record.runtime.provider == .container
            ? .devContainer : .folder
        return HostLaunchPlan.editor(preferences.editor, mode: mode, record: record, folderExists: folderExists)
    }

    var primaryEditorTitle: String {
        primaryEditorOpensDevContainer
            ? "Open in Dev Container"
            : "Open in \(LaunchPreferences.editorName(preferences.editor))"
    }

    var primaryEditorOpensDevContainer: Bool {
        preferences.editorMode == .devContainer && record.runtime.provider == .container
    }

    func editor(_ choice: EditorChoice) -> HostLaunchPlan {
        HostLaunchPlan.editor(choice, mode: .folder, record: record, folderExists: folderExists)
    }

    /// The folder reopened in its dev container (VS Code unless Cursor is preferred).
    var devContainer: HostLaunchPlan {
        let choice: EditorChoice = preferences.editor == .cursor ? .cursor : .vscode
        return HostLaunchPlan.editor(choice, mode: .devContainer, record: record, folderExists: folderExists)
    }

    var terminal: HostLaunchPlan {
        HostLaunchPlan.terminal(preferences.terminal, record: record, folderExists: folderExists)
    }

    var agent: HostLaunchPlan {
        HostLaunchPlan.agent(preferences.resolvedAgent, terminal: preferences.terminal, record: record,
                             passPrompt: preferences.passPrompt, folderExists: folderExists)
    }

    /// Launch Agent with the feature's prompt seed, whatever the "pass prompt" setting says.
    var agentWithPrompt: HostLaunchPlan {
        guard hasPrompt else { return HostLaunchPlan(kind: .disabled(reason: "\(record.workFeature) has no prompt")) }
        return HostLaunchPlan.agent(preferences.resolvedAgent, terminal: preferences.terminal, record: record,
                                    passPrompt: true, folderExists: folderExists)
    }

    var hasPrompt: Bool {
        record.promptSeed?.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty == false
    }

    /// Reveal needs the folder on disk.
    var reveal: ActionAvailability {
        if isRemoved { return .disabled("\(record.workFeature) has been torn down") }
        guard let path = record.worktreePath, !path.isEmpty else { return .disabled("\(record.workFeature) has no folder") }
        return folderExists ? .enabled : .disabled("The folder \(path) is missing")
    }

    // MARK: Operations (through ActionDispatcher)

    private var backendCheck: ActionAvailability {
        backendReady ? .enabled : .disabled("The BranchBox CLI isn't available")
    }

    private var busyCheck: ActionAvailability {
        busyWith.map { .disabled("Waiting for “\($0)” to finish") } ?? .enabled
    }

    private var removedCheck: ActionAvailability {
        isRemoved ? .disabled("\(record.workFeature) has been torn down") : .enabled
    }

    /// Run Command: not for torn-down or orphaned features, and not without the CLI.
    var runCommand: ActionAvailability {
        let state: ActionAvailability
        if record.status == .orphaned {
            state = .disabled("The runtime of \(record.workFeature) no longer exists; recreate it first")
        } else if record.setup?.state == .inProgress {
            state = .disabled("\(record.workFeature) is still being set up")
        } else if !folderExists, !isRemoved {
            state = .disabled("The folder \(record.worktreePath ?? record.workFeature) is missing")
        } else {
            state = .enabled
        }
        return .first(backendCheck, removedCheck, state)
    }

    /// Tear Down opens the sheet; a torn-down feature has nothing left to tear down.
    var teardown: ActionAvailability {
        .first(removedCheck, backendCheck)
    }

    /// Dev container Start / Stop / Rebuild: the container runtime with its folder present.
    var devcontainer: ActionAvailability {
        let runtime: ActionAvailability = record.runtime.provider == .container
            ? .enabled : .disabled("Only features on the container runtime have a dev container")
        let folder: ActionAvailability = folderExists ? .enabled : .disabled("The folder \(record.worktreePath ?? "") is missing")
        let setup: ActionAvailability = record.setup?.state == .inProgress
            ? .disabled("\(record.workFeature) is still being set up") : .enabled
        return .first(removedCheck, runtime, folder, setup, backendCheck, busyCheck)
    }

    /// Share via Tunnel / Stop Sharing.
    var tunnel: ActionAvailability {
        .first(removedCheck, backendCheck, busyCheck)
    }

    /// Remediation buttons wait for the feature's running operation.
    var remediation: ActionAvailability {
        .first(backendCheck, busyCheck)
    }
}

// MARK: Remediation

/// What pressing a remediation button does.
enum RemediationEffect: Sendable, Hashable {
    /// Runs the request through `ActionDispatcher`.
    case dispatch(OperationRequestContext)
    /// Asks first (a confirmation dialog), then dispatches.
    case confirmThenDispatch(OperationRequestContext, title: String, message: String, confirmLabel: String)
    /// Opens a sheet or window through the main window's router.
    case post(WindowIntent)
    case copy(String)
    /// The feature's most recent operation in Activity.
    case showLog(FeatureRef)
}

/// One button of the health callout or the menus' remediation item.
struct RemediationItem: Sendable, Hashable, Identifiable {
    enum Role: Sendable, Hashable { case primary, secondary }

    let action: RemediationAction
    let title: String
    let effect: RemediationEffect
    let role: Role
    /// Tear Down–like actions open the teardown sheet; they are styled apart from the fix.
    let isTeardown: Bool

    var id: String { title }

    /// Copy and log actions work without the CLI and while an operation runs.
    var needsBackend: Bool {
        switch effect {
        case .copy, .showLog: false
        case .post(.showDiagnostics): false
        case .dispatch, .confirmThenDispatch, .post: true
        }
    }
}

/// Turns `Remediation.actions` into buttons, exactly as returned (§9.1): no action is added or dropped here, and
/// no request carries discard consent or force flags beyond what `Remediation` built.
enum RemediationPresenter {
    static func items(for actions: [RemediationAction], record: FeatureRecord) -> [RemediationItem] {
        var items = actions.map { item(for: $0, record: record, role: .secondary) }
        // The first fix (never a teardown, a copy or the log) is the one prominent button.
        if let index = items.firstIndex(where: { !$0.isTeardown && isFix($0.effect) }) {
            let item = items[index]
            items[index] = RemediationItem(action: item.action, title: item.title, effect: item.effect, role: .primary,
                                           isTeardown: false)
        }
        return items
    }

    static func item(for action: RemediationAction, record: FeatureRecord, role: RemediationItem.Role = .secondary)
        -> RemediationItem {
        let make = { (title: String, effect: RemediationEffect, teardown: Bool) in
            RemediationItem(action: action, title: title, effect: effect, role: role, isTeardown: teardown)
        }
        switch action {
        case .resumeSetup(let request):
            return make("Resume Setup", .dispatch(.start(request)), false)
        case .retryRetainedRuntime(let request):
            return make(record.status == .failedRetained ? "Retry" : "Retry Setup", .dispatch(.start(request)), false)
        case .recreateRuntime(let request):
            return make("Recreate Runtime", .dispatch(.start(request)), false)
        case .rerunSetup(let request):
            // §9.1 "[Re-run Setup…]": re-running modules can rewrite generated files (such as .env), so confirm.
            return make("Re-run Setup…", .confirmThenDispatch(
                .start(request),
                title: "Re-run setup for \(request.name)?",
                message: "BranchBox runs the project's setup steps again in the existing worktree. Your files, commits "
                    + "and dev container config are kept; files the setup generates (such as .env) may be rewritten.",
                confirmLabel: "Re-run Setup"), false)
        case .startEnvironment(let feature):
            return make("Start Environment", .dispatch(.devcontainer(.up(removeExisting: false, buildNoCache: false), feature)), false)
        case .teardown(let feature, let preselect):
            let title = switch record.status {
            case .failedRetained: "Discard…"
            case .orphaned: "Clean Up…"
            default: "Tear Down…"
            }
            return make(title, .post(.teardown(feature, preselect: preselect)), true)
        case .cleanUpMissingFolder(let request):
            // Nothing is left to lose (the folder is gone and the branch is kept), so this is the fix itself.
            return make("Clean Up", .dispatch(.teardown(request)), false)
        case .syncDevcontainers(let project):
            return make("Update All Workspaces…", .post(.syncDevcontainers(project)), false)
        case .runDoctor:
            return make("Run Doctor", .post(.showDiagnostics), false)
        case .deleteLeftoverBranch(let branch, let project):
            return make("Delete Branch…", .confirmThenDispatch(
                .deleteBranch(branch, project, force: false),
                title: "Delete the branch \(branch)?",
                message: "BranchBox deletes it only if it is merged (git branch -d). If it has commits that aren't merged, "
                    + "nothing is deleted and you can choose to force-delete it.",
                confirmLabel: "Delete Branch"), true)
        case .copyCommand(let command, let label):
            return make(label, .copy(command), false)
        case .showLog(let feature):
            return make("Show Log", .showLog(feature), false)
        }
    }

    private static func isFix(_ effect: RemediationEffect) -> Bool {
        switch effect {
        case .dispatch, .confirmThenDispatch: true
        case .post(.syncDevcontainers), .post(.showDiagnostics): true
        case .post, .copy, .showLog: false
        }
    }

    /// A second line under the callout: what happened and what the fix does, in plain words.
    static func explanation(for record: FeatureRecord, folderExists: Bool) -> String? {
        if record.status == .removed {
            return "Its worktree is gone. The record stays so you can see what it was."
        }
        if record.setup?.state == .inProgress {
            return "BranchBox is creating the worktree and environment. This view updates when setup finishes."
        }
        switch Remediation.attention(for: record, folderExists: folderExists) {
        case .folderMissing?:
            return "The worktree folder was moved or deleted outside BranchBox. Clean Up forgets the worktree and keeps "
                + "the branch; there is nothing left to discard."
        case .interrupted?:
            return "BranchBox stopped before setup finished, so parts of the environment may be missing. Resume Setup "
                + "re-runs it on the existing folder without touching your files."
        case .failedRetained?:
            return record.runtime.provider == .sbx
                ? "Retry reuses the kept sandbox. Inspect it first with the copied command, or discard it with the feature."
                : "Tear it down to discard the kept runtime; your branch is kept."
        case .orphaned?:
            return "The runtime was removed outside BranchBox (for example by Docker cleanup). Recreate it on the same "
                + "folder, or clean up the feature and keep its branch."
        case .degraded?:
            return "The sandbox stopped or couldn't start. Retry Setup starts it again and keeps your files."
        case .setupIncomplete?:
            return "The feature works, but some setup steps didn't. Re-run Setup repeats them and keeps your dev "
                + "container config."
        case .unknownStatus?:
            return "A newer BranchBox CLI may have written it. Update the app, or run the doctor to check your installation."
        case .unregisteredWorktree?, nil:
            return record.devcontainerOutdated && folderExists
                ? "The project's .devcontainer changed since this feature was created. Updating copies it into the "
                    + "feature's worktree."
                : nil
        }
    }
}

/// Runs a remediation item's effect. Never adds consent or force: requests go out exactly as built.
@MainActor enum RemediationPerformer {
    /// Returns the dispatch result for `.dispatch`; nil for the other effects (`.confirmThenDispatch` is the
    /// caller's to confirm first, then pass `.dispatch`).
    @discardableResult
    static func perform(_ effect: RemediationEffect, model: AppModel, pasteboard: Pasteboard = .general) -> DispatchResult? {
        switch effect {
        case .dispatch(let request), .confirmThenDispatch(let request, _, _, _):
            let result = model.actions.dispatch(request)
            FeatureCommands.report(result, for: request, model: model)
            return result
        case .post(let intent):
            model.post(intent)
        case .copy(let text):
            pasteboard.copy(text)
        case .showLog(let feature):
            let latest = model.operations.records(for: .feature(feature)).first
            model.post(.showActivity(operation: latest?.id))
        }
        return nil
    }
}

// MARK: Links

/// One row of the Open card. Only real host URLs are links; container-internal addresses are copy-only.
struct FeatureLinkRow: Sendable, Hashable, Identifiable {
    enum Kind: Sendable, Hashable { case primary, tunnel, port, containerService, inContainer }

    let kind: Kind
    let title: String
    let value: String
    let url: URL?
    let subtitle: String?

    var id: String { "\(kind)-\(value)" }
    var isLink: Bool { url != nil }
}

enum FeatureLinks {
    static let containerServiceNote = "inside container — open if forwarded"

    /// Feature URL, tunnel, published ports, then the dev container's service and the adapter's in-container
    /// URL (both text: those hosts only resolve inside the container network).
    static func rows(for record: FeatureRecord, service: DevcontainerServiceInfo?) -> [FeatureLinkRow] {
        let urls = record.urls
        var rows: [FeatureLinkRow] = []
        if let primary = urls.primary {
            rows.append(FeatureLinkRow(kind: .primary, title: "Feature URL", value: primary.absoluteString, url: primary,
                                       subtitle: nil))
        }
        if let tunnel = urls.tunnel {
            rows.append(FeatureLinkRow(kind: .tunnel, title: "Tunnel", value: tunnel.absoluteString, url: tunnel,
                                       subtitle: "shared on the internet"))
        }
        for port in urls.ports {
            rows.append(FeatureLinkRow(kind: .port, title: port.label, value: port.url.absoluteString, url: port.url,
                                       subtitle: "→ container :\(String(port.runtimePort))"))
        }
        let serviceURL = service?.serviceURL?.trimmingCharacters(in: .whitespaces)
        if let serviceURL, !serviceURL.isEmpty {
            let name = service?.serviceName.map { "Container service (\($0))" } ?? "Container service"
            rows.append(FeatureLinkRow(kind: .containerService, title: name, value: serviceURL, url: nil,
                                       subtitle: containerServiceNote))
        }
        if let inside = urls.inContainerServiceURL, inside != serviceURL {
            rows.append(FeatureLinkRow(kind: .inContainer, title: "App inside the container", value: inside, url: nil,
                                       subtitle: "copy only; reachable from inside the container"))
        }
        return rows
    }

    /// The Open URL menu: every host link, with the http:// alternative after the feature URL.
    static func menuLinks(for record: FeatureRecord) -> [(title: String, url: URL)] {
        let urls = record.urls
        var links: [(title: String, url: URL)] = []
        if let primary = urls.primary { links.append((primary.host ?? primary.absoluteString, primary)) }
        if let http = urls.primaryHTTP { links.append(("Open with http://", http)) }
        if let tunnel = urls.tunnel { links.append(("Tunnel: \(tunnel.host ?? tunnel.absoluteString)", tunnel)) }
        for port in urls.ports { links.append(("\(port.label) → :\(String(port.runtimePort))", port.url)) }
        return links
    }
}

// MARK: Run Command

/// A finished command's result as the Run Command window shows it. A non-zero exit is data, not an error.
struct ExecOutput: Sendable, Hashable {
    /// Output beyond this is cut by the backend; the window says so.
    static let truncationLimit = 32 << 20

    let result: ExecResult
    let duration: Duration?

    var exitCode: Int32 { result.exitCode }
    var succeeded: Bool { result.exitCode == 0 }
    var exitLabel: String { "Exit \(result.exitCode)" }
    var tint: StatusTint { succeeded ? .green : .red }
    var isTruncated: Bool {
        result.stdout.utf8.count >= Self.truncationLimit || result.stderr.utf8.count >= Self.truncationLimit
    }

    var durationLabel: String? {
        guard let duration else { return nil }
        let seconds = Double(duration.components.seconds) + Double(duration.components.attoseconds) / 1e18
        if seconds < 1 { return "\(Int((seconds * 1000).rounded())) ms" }
        if seconds < 60 {
            return seconds.formatted(.number.precision(.fractionLength(1)).locale(Locale(identifier: "en_US"))) + " s"
        }
        return OperationPresentation.elapsed(duration)
    }
}

/// Where a Run Command operation is.
enum RunCommandPhase: Sendable, Hashable {
    case idle
    case queued(behind: String)
    case running(since: Date?)
    case finished(ExecOutput)
    case failed(BackendError)
    case cancelled(note: String?)

    /// Only a failure to run is an error; a command that ran and exited non-zero is `.finished`.
    var isError: Bool {
        if case .failed = self { return true }
        return false
    }

    var isRunning: Bool {
        switch self {
        case .queued, .running: true
        case .idle, .finished, .failed, .cancelled: false
        }
    }

    static func phase(state: OperationState, result: OperationResult?, runningSince: Date?, finishedAt: Date?) -> RunCommandPhase {
        switch state {
        case .queued(let behind): return .queued(behind: behind)
        case .running: return .running(since: runningSince)
        case .cancelled(let note): return .cancelled(note: note)
        case .failed(let error): return .failed(error)
        case .succeeded, .succeededWithWarnings, .partial:
            guard case .exec(let exec)? = result else {
                return .failed(.commandFailed(Diagnostics(summary: "The command finished without a result")))
            }
            let duration: Duration? = if let runningSince, let finishedAt {
                .seconds(max(0, finishedAt.timeIntervalSince(runningSince)))
            } else {
                nil
            }
            return .finished(ExecOutput(result: exec, duration: duration))
        }
    }

    @MainActor static func phase(of record: OperationRecord?) -> RunCommandPhase {
        guard let record else { return .idle }
        return phase(state: record.state, result: record.result, runningSince: record.runningSince ?? record.startedAt,
                     finishedAt: record.finishedAt)
    }
}

/// The command as typed, and the argv it becomes.
struct RunCommandDraft: Sendable, Hashable {
    var text = ""
    /// Default on: `["/bin/sh", "-lc", text]`, so pipes, globs and `&&` work as in Terminal.
    var runThroughShell = true
    var target: ExecRequest.Target = .featureRuntime

    var trimmed: String { text.trimmingCharacters(in: .whitespacesAndNewlines) }

    /// nil when there is nothing to run or the text can't be split into words (an open quote).
    var argv: [String]? {
        guard !trimmed.isEmpty else { return nil }
        if runThroughShell { return ["/bin/sh", "-lc", trimmed] }
        guard let words = Self.words(trimmed), !words.isEmpty else { return nil }
        return words
    }

    func makeRequest(for feature: FeatureRef) -> ExecRequest? {
        argv.map { ExecRequest(feature: feature, command: $0, target: target) }
    }

    /// POSIX-like word splitting without a shell: whitespace separates words; single quotes are literal; double
    /// quotes and backslashes escape. nil for an unterminated quote.
    static func words(_ text: String) -> [String]? {
        var words: [String] = []
        var current = ""
        var inWord = false
        var quote: Character?
        var escaping = false
        for character in text {
            if escaping {
                current.append(character)
                escaping = false
                inWord = true
                continue
            }
            switch (quote, character) {
            case ("'", "'"), ("\"", "\""):
                quote = nil
            case ("'", _):
                current.append(character)
            case (_, "\\"):
                escaping = true
            case ("\"", _):
                current.append(character)
            case (nil, "'"), (nil, "\""):
                quote = character
                inWord = true
            case (nil, _) where character.isWhitespace:
                if inWord { words.append(current) }
                current = ""
                inWord = false
            default:
                current.append(character)
                inWord = true
            }
        }
        if quote != nil || escaping { return nil }
        if inWord { words.append(current) }
        return words
    }
}

/// Per-feature command history, newest first, without duplicates.
enum RunCommandHistory {
    static let limit = 20

    static func key(for feature: FeatureRef) -> String {
        "runCommand.history.\(feature.project.path)#\(feature.name)"
    }

    static func adding(_ command: String, to history: [String]) -> [String] {
        let trimmed = command.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return history }
        return Array(([trimmed] + history.filter { $0 != trimmed }).prefix(limit))
    }

    static func load(for feature: FeatureRef, from defaults: UserDefaults) -> [String] {
        defaults.stringArray(forKey: key(for: feature)) ?? []
    }

    static func record(_ command: String, for feature: FeatureRef, in defaults: UserDefaults) -> [String] {
        let history = adding(command, to: load(for: feature, from: defaults))
        defaults.set(history, forKey: key(for: feature))
        return history
    }
}

// MARK: Sharing

/// What the Sharing card shows (§9.3).
enum TunnelCardState: Sendable, Hashable {
    /// `tunnel.enabled` is false in the project config.
    case offInConfig
    /// No tunnel, or a disabled one: [Share via Tunnel].
    case notShared
    case pending
    case manual
    case active
    case unknown(String)

    static func state(tunnel: TunnelState?, tunnelsEnabled: Bool?) -> TunnelCardState {
        switch tunnel?.status {
        case .active?: return .active
        case .pending?: return .pending
        case .manual?: return .manual
        case .unknown(let raw)?: return .unknown(raw)
        case .disabled?, nil: return tunnelsEnabled == false ? .offInConfig : .notShared
        }
    }
}

/// The failure a tunnel or dev container card shows inline: its latest operation of those kinds, if that failed.
struct OperationFailure: Sendable, Hashable, Identifiable {
    let id: UUID
    let error: BackendError
    let context: OperationRequestContext

    /// The recoveries `ResultCard` will offer for it.
    var recoveries: [RecoveryAction] { RecoveryPlanner.recoveries(for: error, after: context) }

    @MainActor static func latest(of kinds: Set<OperationKind>, in records: [OperationRecord]) -> OperationFailure? {
        guard let record = records.first(where: { kinds.contains($0.kind) }) else { return nil }
        // An acknowledged failure (dismissed here, or viewed in Activity) no longer shows in the card.
        guard record.needsAttention, case .failed(let error) = record.state else { return nil }
        return OperationFailure(id: record.id, error: error, context: record.context)
    }
}

// MARK: Dev container

/// The dev container's state as the Environment card loads it.
enum DevcontainerLoad: Sendable, Hashable {
    case loading
    case loaded(DevcontainerStatus)
    case failed(BackendError)
    /// Nothing to check: the feature's folder is missing (or the feature was removed).
    case unavailable(String)

    var status: DevcontainerStatus? {
        if case .loaded(let status) = self { return status }
        return nil
    }

    var isRunning: Bool { status?.state == .running }
}

extension DevcontainerStatus.State {
    var label: String {
        switch self {
        case .running: "Running"
        case .stopped: "Stopped"
        case .notCreated: "Not created"
        case .unknown: "Unknown"
        }
    }

    var symbol: String {
        switch self {
        case .running: "play.circle.fill"
        case .stopped: "stop.circle"
        case .notCreated: "circle.dashed"
        case .unknown: "questionmark.circle"
        }
    }

    var tint: StatusTint {
        switch self {
        case .running: .green
        case .stopped: .orange
        case .notCreated, .unknown: .gray
        }
    }
}

extension AgentPlanStatus {
    var label: String {
        switch self {
        case .ready: "Ready"
        case .waiting: "Waiting"
        case .blocked: "Blocked"
        case .disabled: "Not configured"
        case .unknown(let raw): raw.isEmpty ? "Unknown" : raw.replacingOccurrences(of: "_", with: " ")
        }
    }

    var symbol: String {
        switch self {
        case .ready: "checkmark.circle.fill"
        case .waiting: "hourglass"
        case .blocked: "exclamationmark.octagon.fill"
        case .disabled: "minus.circle"
        case .unknown: "questionmark.circle"
        }
    }

    var tint: StatusTint {
        switch self {
        case .ready: .green
        case .waiting: .blue
        case .blocked: .red
        case .disabled, .unknown: .gray
        }
    }
}
