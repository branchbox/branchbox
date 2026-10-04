import BranchBoxKit
import Foundation

/// Builds the complete environment of every child the backend spawns (DESIGN §7.3).
public enum ChildEnvironment {
    /// Kept from the process environment when the base lacks them.
    static let preservedKeys = ["HOME", "USER", "LOGNAME", "TMPDIR", "SSH_AUTH_SOCK"]

    /// State of the capturing shell or of the terminal the app was started from; never forwarded.
    static let droppedKeys: Set<String> = [
        "PWD", "OLDPWD", "SHLVL", "_", "COLORTERM", "CLICOLOR_FORCE", "PS1", "PS2", "PROMPT",
        "__CFBundleIdentifier",
    ]
    /// `TERM*` also covers `TERM_PROGRAM*`, `TERM_SESSION_ID` and `TERMINFO`.
    static let droppedPrefixes = ["TERM", "ITERM_", "XPC_"]

    static let systemDirectories = ["/usr/bin", "/bin", "/usr/sbin", "/sbin"]

    /// Where Homebrew, cargo, pipx and Docker Desktop install tools, in PATH order after the base PATH.
    public static func wellKnownDirectories(home: String) -> [String] {
        [
            "/opt/homebrew/bin", "/opt/homebrew/sbin", "/usr/local/bin",
            "\(home)/.cargo/bin", "\(home)/.local/bin", "\(home)/.docker/bin",
            "/Applications/Docker.app/Contents/Resources/bin",
        ]
    }

    static func isDropped(_ key: String) -> Bool {
        droppedKeys.contains(key) || droppedPrefixes.contains(where: key.hasPrefix)
    }

    /// - Parameters:
    ///   - base: the captured login environment, or the process environment when there is none.
    ///   - processEnvironment: the app's own environment, which supplies HOME, USER, LOGNAME, TMPDIR and
    ///     SSH_AUTH_SOCK when `base` lacks them.
    ///   - cliPath: the unresolved CLI path; its directory leads PATH so the CLI finds its siblings.
    ///   - settings: verbosity, the default agent, and the user's extra variables, which are applied last and
    ///     override everything (their values are never logged).
    ///   - home: expands `~` in the well-known directories.
    public static func make(base: [String: String], processEnvironment: [String: String], cliPath: String?,
                            settings: BackendSettings, home: String) -> [String: String] {
        var environment = base.filter { !isDropped($0.key) }
        for key in preservedKeys where environment[key] == nil {
            if let value = processEnvironment[key] { environment[key] = value }
        }

        let cliDirectory = cliPath.map { ($0 as NSString).deletingLastPathComponent }.map { [$0] } ?? []
        let basePath = (base["PATH"] ?? "").split(separator: ":").map(String.init)
        let path = cliDirectory + basePath + wellKnownDirectories(home: home) + systemDirectories
        environment["PATH"] = deduplicated(path).joined(separator: ":")

        environment["NO_COLOR"] = "1"
        environment["CLICOLOR"] = "0"
        environment["TERM"] = "dumb"
        environment["GIT_TERMINAL_PROMPT"] = "0"
        environment["RUST_BACKTRACE"] = "0"
        if environment["RUST_LOG"] == nil { environment["RUST_LOG"] = settings.verboseLogs ? "debug" : "info" }
        if let command = settings.agentCommand, !command.isEmpty {
            environment["BRANCHBOX_DEFAULT_AGENT_CMD"] = command
        }
        if let name = settings.agentName, !name.isEmpty { environment["BRANCHBOX_DEFAULT_AGENT_NAME"] = name }

        environment.merge(settings.extraEnvironment) { _, extra in extra }
        return environment
    }

    /// PATH entries in order without repeats; empty entries (which would mean the working directory) are
    /// dropped and a trailing slash does not make an entry distinct.
    static func deduplicated(_ entries: [String]) -> [String] {
        var seen: Set<String> = []
        var result: [String] = []
        for entry in entries {
            var normalized = entry
            while normalized.count > 1, normalized.hasSuffix("/") { normalized.removeLast() }
            guard !normalized.isEmpty, seen.insert(normalized).inserted else { continue }
            result.append(normalized)
        }
        return result
    }
}
