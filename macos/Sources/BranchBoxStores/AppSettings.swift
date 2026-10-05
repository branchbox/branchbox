import BranchBoxKit
import Foundation
import Observation

/// App preferences, persisted in `UserDefaults` (D-22). Every property loads in `init` and writes back on
/// change. Backend-related changes take effect through `EnvironmentStore.rebootstrap()`, which Settings calls;
/// refresh-interval and file-watching changes restart the timers and watchers at once.
@MainActor @Observable public final class AppSettings {
    /// `.standard` for a bundled `.app`; the `dev.branchbox.app.dev` suite under `swift run` and in tests, so
    /// a development build never reads or writes the installed app's preferences.
    public static func defaultsForCurrentProcess() -> UserDefaults {
        AppBundle.isBundledApp() ? .standard : UserDefaults(suiteName: "dev.branchbox.app.dev")!
    }

    /// What changed, for the stores that react without a rebootstrap.
    enum Change: Sendable { case refreshIntervals, watchProjectFiles }

    let defaults: UserDefaults
    /// Set by `AppModel`; called after a change it reacts to.
    @ObservationIgnored var onChange: ((Change) -> Void)?

    public init(defaults: UserDefaults) {
        self.defaults = defaults
        cliPathOverride = defaults.string(forKey: Key.cliPathOverride)
        extraEnvironment = Self.decoded([String: String].self, Key.extraEnvironment, from: defaults) ?? [:]
        verboseLogs = defaults.bool(forKey: Key.verboseLogs)
        preferredEditor = Self.decoded(EditorChoice.self, Key.preferredEditor, from: defaults) ?? .vscode
        editorOpenMode = Self.decoded(EditorOpenMode.self, Key.editorOpenMode, from: defaults) ?? .folder
        preferredTerminal = Self.decoded(TerminalChoice.self, Key.preferredTerminal, from: defaults) ?? .terminal
        agentChoice = Self.decoded(AgentChoice.self, Key.agentChoice, from: defaults) ?? .claude
        agentDisplayName = defaults.string(forKey: Key.agentDisplayName)
        passPromptToAgent = defaults.object(forKey: Key.passPromptToAgent) as? Bool ?? true
        notificationsEnabled = defaults.object(forKey: Key.notificationsEnabled) as? Bool ?? true
        notifyOnlyOnProblems = defaults.bool(forKey: Key.notifyOnlyOnProblems)
        notifyAttentionChanges = defaults.object(forKey: Key.notifyAttentionChanges) as? Bool ?? true
        watchProjectFiles = defaults.object(forKey: Key.watchProjectFiles) as? Bool ?? true
        selectedProjectRefresh = Self.decoded(RefreshInterval.self, Key.selectedProjectRefresh, from: defaults) ?? .m1
        otherProjectsRefresh = Self.decoded(RefreshInterval.self, Key.otherProjectsRefresh, from: defaults) ?? .m5
        showMenuBarIcon = defaults.object(forKey: Key.showMenuBarIcon) as? Bool ?? true
        logRetention = defaults.object(forKey: Key.logRetention) as? Int ?? 100
        quickCommands = Self.decoded([String: [String]].self, Key.quickCommands, from: defaults) ?? [:]
        promptHistory = defaults.stringArray(forKey: Key.promptHistory) ?? []
    }

    // Tools
    public var cliPathOverride: String? { didSet { defaults.set(cliPathOverride, forKey: Key.cliPathOverride) } }
    public var extraEnvironment: [String: String] { didSet { encode(extraEnvironment, Key.extraEnvironment) } }
    public var verboseLogs: Bool { didSet { defaults.set(verboseLogs, forKey: Key.verboseLogs) } }

    // Editors, terminal and coding agent
    public var preferredEditor: EditorChoice { didSet { encode(preferredEditor, Key.preferredEditor) } }
    public var editorOpenMode: EditorOpenMode { didSet { encode(editorOpenMode, Key.editorOpenMode) } }
    public var preferredTerminal: TerminalChoice { didSet { encode(preferredTerminal, Key.preferredTerminal) } }
    public var agentChoice: AgentChoice { didSet { encode(agentChoice, Key.agentChoice) } }
    public var agentDisplayName: String? { didSet { defaults.set(agentDisplayName, forKey: Key.agentDisplayName) } }
    public var passPromptToAgent: Bool { didSet { defaults.set(passPromptToAgent, forKey: Key.passPromptToAgent) } }

    // Notifications
    public var notificationsEnabled: Bool { didSet { defaults.set(notificationsEnabled, forKey: Key.notificationsEnabled) } }
    public var notifyOnlyOnProblems: Bool { didSet { defaults.set(notifyOnlyOnProblems, forKey: Key.notifyOnlyOnProblems) } }
    public var notifyAttentionChanges: Bool {
        didSet { defaults.set(notifyAttentionChanges, forKey: Key.notifyAttentionChanges) }
    }

    // Refresh
    public var watchProjectFiles: Bool {
        didSet {
            defaults.set(watchProjectFiles, forKey: Key.watchProjectFiles)
            onChange?(.watchProjectFiles)
        }
    }
    public var selectedProjectRefresh: RefreshInterval {            // default .m1
        didSet {
            encode(selectedProjectRefresh, Key.selectedProjectRefresh)
            onChange?(.refreshIntervals)
        }
    }
    public var otherProjectsRefresh: RefreshInterval {              // default .m5
        didSet {
            encode(otherProjectsRefresh, Key.otherProjectsRefresh)
            onChange?(.refreshIntervals)
        }
    }

    // General and advanced
    public var showMenuBarIcon: Bool { didSet { defaults.set(showMenuBarIcon, forKey: Key.showMenuBarIcon) } }
    public var logRetention: Int { didSet { defaults.set(logRetention, forKey: Key.logRetention) } }   // default 100
    public var quickCommands: [String: [String]] { didSet { encode(quickCommands, Key.quickCommands) } }  // project path → commands
    public var promptHistory: [String] { didSet { defaults.set(promptHistory, forKey: Key.promptHistory) } }  // last 10

    /// What the backend needs from these settings. The agent choice becomes `BRANCHBOX_DEFAULT_AGENT_CMD`
    /// (claude, codex or the custom command line) and its name `BRANCHBOX_DEFAULT_AGENT_NAME` (the display name
    /// if set, else the agent's own name; a custom command without a display name leaves the CLI's default label).
    public var backendSettings: BackendSettings {
        let override = cliPathOverride.flatMap { $0.trimmingCharacters(in: .whitespaces).isEmpty ? nil : $0 }
        let displayName = agentDisplayName?.trimmingCharacters(in: .whitespacesAndNewlines)
        return BackendSettings(cliPathOverride: override, extraEnvironment: extraEnvironment, verboseLogs: verboseLogs,
                               agentCommand: agentChoice.command,
                               agentName: displayName.flatMap { $0.isEmpty ? nil : $0 } ?? agentChoice.defaultName)
    }

    /// How many prompts `promptHistory` keeps.
    public static let promptHistoryLimit = 10

    /// Puts `prompt` first in `promptHistory` (once), keeping the newest `promptHistoryLimit`.
    public func recordPrompt(_ prompt: String) {
        let trimmed = prompt.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return }
        promptHistory = Self.mergedHistory([trimmed], promptHistory)
    }

    /// `newer` then `older`, without duplicates or blanks, capped at `promptHistoryLimit`.
    static func mergedHistory(_ newer: [String], _ older: [String]) -> [String] {
        var seen = Set<String>()
        let merged = (newer + older).filter { !$0.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty && seen.insert($0).inserted }
        return Array(merged.prefix(promptHistoryLimit))
    }

    private enum Key {
        static let cliPathOverride = "settings.cliPathOverride"
        static let extraEnvironment = "settings.extraEnvironment"
        static let verboseLogs = "settings.verboseLogs"
        static let preferredEditor = "settings.preferredEditor"
        static let editorOpenMode = "settings.editorOpenMode"
        static let preferredTerminal = "settings.preferredTerminal"
        static let agentChoice = "settings.agentChoice"
        static let agentDisplayName = "settings.agentDisplayName"
        static let passPromptToAgent = "settings.passPromptToAgent"
        static let notificationsEnabled = "settings.notificationsEnabled"
        static let notifyOnlyOnProblems = "settings.notifyOnlyOnProblems"
        static let notifyAttentionChanges = "settings.notifyAttentionChanges"
        static let watchProjectFiles = "settings.watchProjectFiles"
        static let selectedProjectRefresh = "settings.selectedProjectRefresh"
        static let otherProjectsRefresh = "settings.otherProjectsRefresh"
        static let showMenuBarIcon = "settings.showMenuBarIcon"
        static let logRetention = "settings.logRetention"
        static let quickCommands = "settings.quickCommands"
        static let promptHistory = "settings.promptHistory"
    }

    /// Codable values are stored as JSON so enums with payloads (`.custom(appPath:)`) survive a relaunch.
    private func encode<T: Encodable>(_ value: T, _ key: String) {
        defaults.set(try? JSONEncoder().encode(value), forKey: key)
    }

    private static func decoded<T: Decodable>(_ type: T.Type, _ key: String, from defaults: UserDefaults) -> T? {
        defaults.data(forKey: key).flatMap { try? JSONDecoder().decode(type, from: $0) }
    }
}

private extension AgentChoice {
    var command: String? {
        switch self {
        case .claude: "claude"
        case .codex: "codex"
        case .custom(let command):
            command.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty ? nil : command
        }
    }

    var defaultName: String? {
        switch self {
        case .claude: "Claude Code"
        case .codex: "Codex"
        case .custom: nil
        }
    }
}
