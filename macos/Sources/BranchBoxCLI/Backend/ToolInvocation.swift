import BranchBoxKit
import Foundation

/// Runs one child process for the backend (the CLI, git, docker, …) and maps the runner's errors to
/// `BackendError` (DESIGN §6.3 step 1). It does not interpret the exit status; callers do.
struct ToolInvocation: Sendable {
    let runner: any ProcessRunning
    let environment: [String: String]
    let redaction: RedactedCommandLine
    let cliVersion: String?

    init(runner: any ProcessRunning, environment: [String: String], redaction: RedactedCommandLine = RedactedCommandLine(),
         cliVersion: String? = nil) {
        self.runner = runner
        self.environment = environment
        self.redaction = redaction
        self.cliVersion = cliVersion
    }

    /// One run's parameters besides the argv.
    struct Options: Sendable {
        var workingDirectory: URL?
        var timeout: Duration?
        var standardInput: Data?
        var streamStdout = false
        /// Named in timeout errors, e.g. "feature list".
        var operation: String
        /// `BackendError.cancelled(note:)` when the run is cancelled.
        var cancelNote: String?
        var stdoutLimit: Int?
        /// Shorter escalation for quick probes; nil keeps the runner's defaults (5 s, then 3 s).
        var interruptGrace: Duration?
        var terminateGrace: Duration?

        init(operation: String, workingDirectory: URL? = nil, timeout: Duration? = nil) {
            self.operation = operation
            self.workingDirectory = workingDirectory
            self.timeout = timeout
        }
    }

    /// `Diagnostics.invocation` for this argv: redacted, each argument cut at 200 characters.
    func invocation(_ executable: URL, _ arguments: [String]) -> String {
        redaction.render([executable.path] + arguments, argumentLimit: RedactedCommandLine.diagnosticsArgumentLimit)
    }

    func run(_ executable: URL, _ arguments: [String], _ options: Options,
             onLine: @escaping @Sendable (OutputLine) -> Void = { _ in }) async throws -> ProcessResult {
        var spec = ProcessSpec(executable: executable, arguments: arguments, environment: environment,
                               workingDirectory: options.workingDirectory)
        spec.timeout = options.timeout
        spec.standardInput = options.standardInput
        spec.streamStdout = options.streamStdout
        if let limit = options.stdoutLimit { spec.stdoutLimit = limit }
        if let grace = options.interruptGrace { spec.interruptGrace = grace }
        if let grace = options.terminateGrace { spec.terminateGrace = grace }
        do {
            return try await runner.run(spec, onLine: onLine)
        } catch let error as ProcessRunError {
            throw map(error, executable: executable, arguments: arguments, options: options)
        } catch {
            throw BackendError.normalize(error)
        }
    }

    func map(_ error: ProcessRunError, executable: URL, arguments: [String], options: Options) -> BackendError {
        switch error {
        case .cancelled:
            return .cancelled(note: options.cancelNote)
        case .timedOut(let after, let partial):
            let diagnostics = Diagnostics(summary: "\(options.operation) did not finish within \(Self.describe(after))",
                                          exitCode: nil, signal: Self.signal(of: partial.termination),
                                          logTail: partial.stderrTail, invocation: invocation(executable, arguments),
                                          cliVersion: cliVersion)
            return .timedOut(operation: options.operation, after: after, diagnostics: diagnostics)
        case .workingDirectoryMissing(let path):
            return .projectInvalid(.workingDirectoryMissing(path))
        case .launchFailed(let executable, let reason):
            return .launchFailed(executable: executable, reason: reason)
        case .stdoutTooLarge(let limit):
            return .commandFailed(Diagnostics(summary: "\(options.operation) printed more than \(limit) bytes of output",
                                              invocation: invocation(executable, arguments), cliVersion: cliVersion))
        }
    }

    static func signal(of termination: Termination) -> Int32? {
        if case .signaled(let signal) = termination { return signal }
        return nil
    }

    static func exitCode(of termination: Termination) -> Int32? {
        if case .exited(let code) = termination { return code }
        return nil
    }

    /// "10 s", "1.5 s", "250 ms".
    static func describe(_ duration: Duration) -> String {
        let (seconds, attoseconds) = duration.components
        let milliseconds = seconds * 1000 + attoseconds / 1_000_000_000_000_000
        if milliseconds < 1000 { return "\(milliseconds) ms" }
        if milliseconds % 1000 == 0 { return "\(milliseconds / 1000) s" }
        return String(format: "%.1f s", Double(milliseconds) / 1000)
    }
}
