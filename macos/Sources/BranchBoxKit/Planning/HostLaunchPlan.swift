import Foundation

/// What opening a feature on the host means: which app opens which folder or URL, or which shell script a
/// terminal runs. Pure; `HostLauncher` (App/Services) executes it.
///
/// Terminal scripts are `cd <folder> && exec <command>` with every value single-quoted, so paths and prompts with
/// spaces, quotes or `$` reach the program unchanged. Interactive shells inside a Docker Sandbox need
/// `branchbox feature exec --interactive` (planned), so sandbox terminals and agents are disabled and offer the
/// `sbx exec` command to copy instead.
///
/// The `record`-only builders assume the folder exists when the record names one. Callers that checked the disk
/// (`ProjectStore.folderExists(for:)`) use the `folderExists:` variants, which disable every host action for a
/// missing folder.
public struct HostLaunchPlan: Sendable, Hashable {     // executed by App/Services/HostLauncher
    public enum Kind: Sendable, Hashable { case openFolder(appBundleID: String?, appPath: String?, path: String)
        case openURL(URL); case terminalScript(script: String, terminal: TerminalChoice); case disabled(reason: String) }
    public let kind: Kind
    /// The folder a terminal script starts in; a custom terminal template receives it as `{path}`.
    public let workingDirectory: String?
    /// A command to copy instead, offered alongside a disabled plan (`sbx exec <runtime_id> bash`).
    public let copyCommand: String?

    public init(kind: Kind, workingDirectory: String? = nil, copyCommand: String? = nil) {
        self.kind = kind
        self.workingDirectory = workingDirectory
        self.copyCommand = copyCommand
    }

    public static let vscodeBundleID = "com.microsoft.VSCode"
    public static let cursorBundleID = "com.todesktop.230313mzl4w4u92"
    /// What sandbox terminals and agents need from the CLI first.
    public static let sandboxShellUnavailable =
        "Opening a shell in a Docker Sandbox needs `branchbox feature exec --interactive` (planned)"

    public var isEnabled: Bool {
        if case .disabled = kind { return false }
        return true
    }

    public var disabledReason: String? {
        if case .disabled(let reason) = kind { return reason }
        return nil
    }

    // MARK: Editor

    public static func editor(_ choice: EditorChoice, mode: EditorOpenMode, record: FeatureRecord) -> HostLaunchPlan {
        editor(choice, mode: mode, record: record, folderExists: record.worktreePath != nil)
    }

    /// The folder in the chosen editor, or (container runtime, VS Code or Cursor) the folder reopened in its dev
    /// container through the editor's `vscode-remote` deep link.
    public static func editor(_ choice: EditorChoice, mode: EditorOpenMode, record: FeatureRecord,
                              folderExists: Bool) -> HostLaunchPlan {
        if let disabled = unavailable(record, folderExists: folderExists) { return disabled }
        let path = record.worktreePath ?? ""
        switch mode {
        case .folder:
            switch choice {
            case .vscode: return HostLaunchPlan(kind: .openFolder(appBundleID: vscodeBundleID, appPath: nil, path: path))
            case .cursor: return HostLaunchPlan(kind: .openFolder(appBundleID: cursorBundleID, appPath: nil, path: path))
            case .custom(let appPath):
                guard !appPath.isEmpty else { return disabledPlan("Choose an editor app in Settings") }
                return HostLaunchPlan(kind: .openFolder(appBundleID: nil, appPath: appPath, path: path))
            }
        case .devContainer:
            guard record.runtime.provider == .container else {
                return disabledPlan("Only features on the container runtime open in a dev container")
            }
            let scheme: String
            switch choice {
            case .vscode: scheme = "vscode"
            case .cursor: scheme = "cursor"
            case .custom: return disabledPlan("Open in Dev Container needs VS Code or Cursor")
            }
            guard let url = devContainerDeepLink(scheme: scheme, worktreePath: path,
                                                 workspaceFolder: workspaceFolder(for: record)) else {
                return disabledPlan("The folder \(path) can't be opened in a dev container")
            }
            return HostLaunchPlan(kind: .openURL(url))
        }
    }

    /// `vscode-remote://dev-container+<hex(worktree)><workspace_folder>`: the folder URI VS Code opens inside the
    /// worktree's dev container. The hex is the UTF-8 of the host path.
    public static func devContainerURI(worktreePath: String, workspaceFolder: String) -> URL? {
        URL(string: "vscode-remote://dev-container+\(hex(worktreePath))\(encodedPath(workspaceFolder))")
    }

    /// The same folder as an app link (`vscode://vscode-remote/dev-container+<hex><folder>`), which VS Code and its
    /// forks turn back into the `vscode-remote` folder URI above. Unlike that URI it has an app registered for it.
    public static func devContainerDeepLink(scheme: String, worktreePath: String, workspaceFolder: String) -> URL? {
        guard !worktreePath.isEmpty else { return nil }
        return URL(string: "\(scheme)://vscode-remote/dev-container+\(hex(worktreePath))\(encodedPath(workspaceFolder))")
    }

    /// The folder the worktree is mounted at inside its container: the recorded one, else BranchBox's template
    /// default `/workspaces/${localWorkspaceFolderBasename}`.
    public static func workspaceFolder(for record: FeatureRecord) -> String {
        if let folder = record.runtime.workspaceFolder, !folder.isEmpty { return folder }
        let basename = URL(fileURLWithPath: record.worktreePath ?? record.workFeature).lastPathComponent
        return "/workspaces/\(basename)"
    }

    // MARK: Terminal and agent

    public static func terminal(_ choice: TerminalChoice, record: FeatureRecord) -> HostLaunchPlan {          // sbx → disabled
        terminal(choice, record: record, folderExists: record.worktreePath != nil)
    }

    /// A login shell in the worktree folder, in the chosen terminal.
    public static func terminal(_ choice: TerminalChoice, record: FeatureRecord, folderExists: Bool) -> HostLaunchPlan {
        if let disabled = unavailable(record, folderExists: folderExists) ?? sandboxUnavailable(record) { return disabled }
        return scriptPlan(cd: record.worktreePath, command: loginShellCommand, terminal: choice)
    }

    public static func agent(_ choice: AgentChoice, terminal: TerminalChoice, record: FeatureRecord, passPrompt: Bool) -> HostLaunchPlan {
        agent(choice, terminal: terminal, record: record, passPrompt: passPrompt, folderExists: record.worktreePath != nil)
    }

    /// A login shell in a project's main folder (the sidebar's project menu), in the chosen terminal. Additive (SW-4).
    public static func folderTerminal(_ choice: TerminalChoice, path: String, folderExists: Bool) -> HostLaunchPlan {
        guard folderExists else { return disabledPlan("The folder \(path) is missing") }
        return scriptPlan(cd: path, command: loginShellCommand, terminal: choice)
    }

    /// The coding agent in the worktree folder. With `passPrompt`, the record's prompt seed is the first argument,
    /// after `--` for the built-in agents so a prompt starting with `-` is never read as a flag. A custom agent
    /// command is the user's own shell text (App Settings) and is used as written; only the prompt is quoted.
    public static func agent(_ choice: AgentChoice, terminal: TerminalChoice, record: FeatureRecord, passPrompt: Bool,
                             folderExists: Bool) -> HostLaunchPlan {
        if let disabled = unavailable(record, folderExists: folderExists) ?? sandboxUnavailable(record) { return disabled }
        var command: String
        var endOfOptions = false
        switch choice {
        case .claude:
            command = "claude"
            endOfOptions = true
        case .codex:
            command = "codex"
            endOfOptions = true
        case .custom(let text):
            command = text.trimmingCharacters(in: .whitespacesAndNewlines)
            guard !command.isEmpty else { return disabledPlan("Set a coding agent command in Settings") }
        }
        if passPrompt, let prompt = record.promptSeed?.trimmingCharacters(in: .whitespacesAndNewlines), !prompt.isEmpty {
            command += (endOfOptions ? " -- " : " ") + shellQuote(prompt)
        }
        return scriptPlan(cd: record.worktreePath, command: command, terminal: terminal)
    }

    /// The agent to launch: the project's `editor.default_agent` when set, else the app setting.
    ///
    /// `editor.default_agent` comes from the repository (`.branchbox/config.json` can be committed and cloned), so
    /// it is an agent slug, never shell text: "claude" and "codex" map to the built-in agents, any other bare
    /// executable name (`aider`, `gemini`) runs as that one word, and anything else (spaces, `;`, `$()`, `|`, a
    /// leading `-`) is ignored in favour of the app setting. Only the user's own App Settings command is shell text.
    public static func agentChoice(projectDefault: String?, fallback: AgentChoice) -> AgentChoice {
        guard let value = projectDefault?.trimmingCharacters(in: .whitespacesAndNewlines), !value.isEmpty else {
            return fallback
        }
        switch value.lowercased() {
        case "claude": return .claude
        case "codex": return .codex
        default: return isAgentSlug(value) ? .custom(command: value) : fallback
        }
    }

    /// A bare executable name: a letter, digit or `_`, then letters, digits, `.`, `_`, `+` or `-` (at most 64).
    static func isAgentSlug(_ value: String) -> Bool {
        guard let first = value.unicodeScalars.first, value.unicodeScalars.count <= 64,
              agentSlugStart.contains(first) else { return false }
        return value.unicodeScalars.allSatisfy { agentSlugScalars.contains($0) }
    }

    private static let agentSlugStart = Set("abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_".unicodeScalars)
    private static let agentSlugScalars = agentSlugStart.union(".+-".unicodeScalars)

    // MARK: Dev container shell

    /// `docker exec -it -u <user> <containerId> bash -l` in the running dev container. Without the record the
    /// workspace folder is unknown, so docker's working directory is the container's default.
    public static func devcontainerShell(_ status: DevcontainerStatus, terminal: TerminalChoice) -> HostLaunchPlan {
        devcontainerShellPlan(status, terminal: terminal, user: status.service?.containerUser, workspaceFolder: nil, cd: nil)
    }

    /// `docker exec -it -u <remoteUser> -w <remoteWorkspaceFolder> <containerId> bash -l`, started from the
    /// worktree folder.
    public static func devcontainerShell(_ status: DevcontainerStatus, terminal: TerminalChoice, record: FeatureRecord,
                                         folderExists: Bool) -> HostLaunchPlan {
        if let disabled = unavailable(record, folderExists: folderExists) { return disabled }
        guard record.runtime.provider == .container else {
            return disabledPlan("Only features on the container runtime have a dev container")
        }
        let user = status.service?.containerUser ?? record.runtime.containerUser
        return devcontainerShellPlan(status, terminal: terminal, user: user, workspaceFolder: workspaceFolder(for: record),
                                     cd: record.worktreePath)
    }

    // MARK: Scripts and quoting

    /// The command a sandbox's shell needs today, offered for copying.
    public static func sandboxShellCommand(runtimeID: String) -> String {
        "sbx exec \(shellQuote(runtimeID)) bash"
    }

    /// The `.command` file a terminal runs: a shebang, then `cd <folder> && exec <command>` (just `exec <command>`
    /// without a folder). `command` is shell text whose arguments are already quoted.
    public static func script(cd folder: String?, exec command: String) -> String {
        let line = folder.map { "cd \(shellQuote($0)) && exec \(command)" } ?? "exec \(command)"
        return "#!/bin/sh\n\(line)\n"
    }

    /// POSIX single quoting: `'…'` with each `'` written as `'\''`. Words made only of safe characters stay bare.
    public static func shellQuote(_ word: String) -> String {
        if !word.isEmpty, word.unicodeScalars.allSatisfy({ safeShellScalars.contains($0) }) { return word }
        return "'" + word.replacingOccurrences(of: "'", with: "'\\''") + "'"
    }

    /// argv as one shell command line.
    public static func shellCommand(_ argv: [String]) -> String {
        argv.map(shellQuote).joined(separator: " ")
    }

    private static let safeShellScalars = Set("abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789@%+=:,./_-".unicodeScalars)

    /// The user's login shell, expanded by the script's own `sh`.
    static let loginShellCommand = "\"${SHELL:-/bin/zsh}\" -l"

    private static func scriptPlan(cd folder: String?, command: String, terminal: TerminalChoice) -> HostLaunchPlan {
        HostLaunchPlan(kind: .terminalScript(script: script(cd: folder, exec: command), terminal: terminal),
                       workingDirectory: folder)
    }

    private static func devcontainerShellPlan(_ status: DevcontainerStatus, terminal: TerminalChoice, user: String?,
                                              workspaceFolder: String?, cd folder: String?) -> HostLaunchPlan {
        guard status.state == .running, let containerID = status.containerID, !containerID.isEmpty else {
            return disabledPlan("The dev container isn't running; start it first")
        }
        var argv = ["docker", "exec", "-it"]
        if let user, !user.isEmpty { argv += ["-u", user] }
        if let workspaceFolder, !workspaceFolder.isEmpty { argv += ["-w", workspaceFolder] }
        argv += [containerID, "bash", "-l"]
        return scriptPlan(cd: folder, command: shellCommand(argv), terminal: terminal)
    }

    /// Disabled when the feature is gone or its folder is missing.
    private static func unavailable(_ record: FeatureRecord, folderExists: Bool) -> HostLaunchPlan? {
        if record.status == .removed { return disabledPlan("\(record.workFeature) has been torn down") }
        guard let path = record.worktreePath, !path.isEmpty else {
            return disabledPlan("\(record.workFeature) has no folder")
        }
        return folderExists ? nil : disabledPlan("The folder \(path) is missing")
    }

    private static func sandboxUnavailable(_ record: FeatureRecord) -> HostLaunchPlan? {
        guard record.runtime.provider == .sbx else { return nil }
        let copy = record.runtime.runtimeID.flatMap { $0.isEmpty ? nil : sandboxShellCommand(runtimeID: $0) }
        return HostLaunchPlan(kind: .disabled(reason: sandboxShellUnavailable), copyCommand: copy)
    }

    private static func disabledPlan(_ reason: String) -> HostLaunchPlan {
        HostLaunchPlan(kind: .disabled(reason: reason))
    }

    private static func hex(_ text: String) -> String {
        text.utf8.map { byte in
            let digits = String(byte, radix: 16)
            return digits.count == 1 ? "0" + digits : digits
        }.joined()
    }

    private static func encodedPath(_ folder: String) -> String {
        let absolute = folder.hasPrefix("/") ? folder : "/" + folder
        return absolute.addingPercentEncoding(withAllowedCharacters: .urlPathAllowed) ?? absolute
    }
}
