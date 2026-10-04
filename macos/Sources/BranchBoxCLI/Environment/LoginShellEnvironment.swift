import BranchBoxKit
import Darwin
import Foundation

/// The user's login-shell environment (DESIGN §7.2). An app launched from Finder or the Dock inherits launchd's
/// `PATH=/usr/bin:/bin:/usr/sbin:/sbin`, which has neither Homebrew, Docker, nvm nor cargo; the shell's rc
/// files know where those live.
public struct LoginShellEnvironment: Sendable, Hashable {
    public let variables: [String: String]
    /// `.interactiveLogin` or `.login`.
    public let source: EnvironmentSummary.Source
    public let shell: String
    public let duration: Duration
    public let capturedAt: Date

    public init(variables: [String: String], source: EnvironmentSummary.Source, shell: String, duration: Duration,
                capturedAt: Date) {
        self.variables = variables
        self.source = source
        self.shell = shell
        self.duration = duration
        self.capturedAt = capturedAt
    }

    static let beginSentinel = "__BRANCHBOX_ENV_BEGIN__"
    static let endSentinel = "__BRANCHBOX_ENV_END__"

    /// Prints the environment NUL-separated between two sentinels, so rc-file output around it is ignored and
    /// values may contain newlines.
    public static let captureScript =
        "printf '\\n\(beginSentinel)\\n'; /usr/bin/env -0; printf '\\n\(endSentinel)\\n'"

    /// `-l -i -c` runs the rc files that set up nvm and friends (`-l -c` alone misses nvm); `-l -c` is the
    /// fallback for rc files that misbehave when interactive.
    public static let interactiveTimeout: Duration = .seconds(8)
    public static let loginTimeout: Duration = .seconds(5)

    /// Shells that take `+m` (job control off) on the command line.
    static let posixShells: Set<String> = ["zsh", "bash", "sh", "ksh", "mksh", "dash"]

    /// The flags before the script for the interactive attempt. An interactive zsh or bash with job control on
    /// opens the controlling terminal, if the app has one (`swift run` from Terminal), and stops itself trying to
    /// become its foreground group, because the runner starts every child in a group of its own. `+m` turns job
    /// control off, so the capture takes about a second instead of timing out.
    static func interactiveFlags(for shell: String) -> [String] {
        let name = (shell as NSString).lastPathComponent
        return posixShells.contains(name) ? ["-l", "-i", "+m", "-c"] : ["-l", "-i", "-c"]
    }

    /// The user's login shell from the password database, then `$SHELL`, then `/bin/zsh`.
    public static func userShell(processEnvironment: [String: String] = ProcessInfo.processInfo.environment) -> String {
        if let shell = passwordDatabaseShell(), isUsableShell(shell) { return shell }
        if let shell = processEnvironment["SHELL"], isUsableShell(shell) { return shell }
        return "/bin/zsh"
    }

    /// Runs `shell -l -i -c` (`-l -i +m -c` for zsh and bash), then `shell -l -c`, and returns the first
    /// environment captured, or nil when both fail (the caller then uses the process environment). stdin is
    /// `/dev/null`, so an rc file that prompts reads EOF; `TERM=dumb` keeps prompt themes quiet.
    public static func capture(shell: String, processEnvironment: [String: String], runner: any ProcessRunning,
                               interactiveTimeout: Duration = interactiveTimeout,
                               loginTimeout: Duration = loginTimeout) async -> LoginShellEnvironment? {
        let attempts: [(flags: [String], source: EnvironmentSummary.Source, timeout: Duration)] = [
            (interactiveFlags(for: shell), .interactiveLogin, interactiveTimeout),
            (["-l", "-c"], .login, loginTimeout),
        ]
        var environment = processEnvironment.filter { !ChildEnvironment.isDropped($0.key) }
        environment["TERM"] = "dumb"
        let home = processEnvironment["HOME"].flatMap { $0.isEmpty ? nil : URL(fileURLWithPath: $0) }
        let workingDirectory = home.flatMap { FileManager.default.fileExists(atPath: $0.path) ? $0 : nil }
        for attempt in attempts {
            var spec = ProcessSpec(executable: URL(fileURLWithPath: shell), arguments: attempt.flags + [captureScript],
                                   environment: environment, workingDirectory: workingDirectory)
            spec.timeout = attempt.timeout
            // A hung rc file should not hold the capture much past its timeout.
            spec.interruptGrace = .seconds(1)
            spec.terminateGrace = .seconds(1)
            spec.drainGrace = .seconds(1)
            spec.stdoutLimit = 8 << 20
            spec.stderrTailLines = 0
            guard let result = try? await runner.run(spec, onLine: { _ in }),
                  let variables = parse(result.stdout) else { continue }
            // The sentinels prove the script ran to the end, whatever status the rc files left behind.
            return LoginShellEnvironment(variables: variables, source: attempt.source, shell: shell,
                                         duration: result.duration, capturedAt: Date())
        }
        return nil
    }

    /// The variables printed between the sentinels, or nil when either sentinel is missing or PATH is absent.
    public static func parse(_ output: Data) -> [String: String]? {
        guard let begin = output.range(of: Data("\(beginSentinel)\n".utf8)),
              let end = output.range(of: Data("\n\(endSentinel)".utf8), options: .backwards,
                                     in: begin.upperBound..<output.endIndex) else { return nil }
        var variables: [String: String] = [:]
        for entry in output[begin.upperBound..<end.lowerBound].split(separator: 0) {
            guard let equals = entry.firstIndex(of: UInt8(ascii: "=")), equals > entry.startIndex else { continue }
            let name = String(decoding: entry[entry.startIndex..<equals], as: UTF8.self)
            variables[name] = String(decoding: entry[entry.index(after: equals)...], as: UTF8.self)
        }
        guard let path = variables["PATH"], !path.isEmpty else { return nil }
        return variables
    }

    private static func isUsableShell(_ path: String) -> Bool {
        path.hasPrefix("/") && access(path, X_OK) == 0
    }

    private static func passwordDatabaseShell() -> String? {
        var buffer = [CChar](repeating: 0, count: 4096)
        return buffer.withUnsafeMutableBufferPointer { storage -> String? in
            var entry = passwd()
            var result: UnsafeMutablePointer<passwd>?
            guard let base = storage.baseAddress, getpwuid_r(getuid(), &entry, base, storage.count, &result) == 0,
                  result != nil, let shell = entry.pw_shell else { return nil }
            return String(cString: shell)
        }
    }
}
