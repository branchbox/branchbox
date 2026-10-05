import Foundation

public struct ProcessSpec: Sendable {
    public var executable: URL                       // absolute; never /usr/bin/env
    public var arguments: [String]
    public var environment: [String: String]         // complete child env (ChildEnvironment.make)
    public var workingDirectory: URL?                // validated to exist before spawn
    public var standardInput: Data?                  // nil = /dev/null; else written then closed (config patch, token)
    public var timeout: Duration?                    // nil = cancellable only
    public var interruptGrace: Duration = .seconds(5)    // SIGINT → SIGTERM
    public var terminateGrace: Duration = .seconds(3)    // SIGTERM → SIGKILL
    public var drainGrace: Duration = .seconds(2)        // max wait for pipe EOF after the leader exits
    public var stdoutLimit: Int = 64 << 20               // exceeding → .stdoutTooLarge
    public var stderrTailLines: Int = 400
    public var streamStdout: Bool = false                // text commands (legacy init/sync/detect) stream stdout lines too
    public init(executable: URL, arguments: [String], environment: [String: String], workingDirectory: URL?) {
        self.executable = executable
        self.arguments = arguments
        self.environment = environment
        self.workingDirectory = workingDirectory
        self.standardInput = nil
        self.timeout = nil
    }
}

public enum Termination: Sendable, Hashable { case exited(Int32), signaled(Int32) }

public struct OutputLine: Sendable, Hashable {
    public enum Channel: String, Sendable, Hashable { case stdout, stderr }
    public let channel: Channel
    public let text: String                          // ANSI-stripped; split on \n, \r\n and bare \r; ≤ 16 KiB
    public init(channel: Channel, text: String) { self.channel = channel; self.text = text }
}

public struct ProcessResult: Sendable {
    public let termination: Termination
    public let stdout: Data
    public let stderrTail: [String]
    public let duration: Duration
    public init(termination: Termination, stdout: Data, stderrTail: [String], duration: Duration) {
        self.termination = termination
        self.stdout = stdout
        self.stderrTail = stderrTail
        self.duration = duration
    }
}

public enum ProcessRunError: Error, Sendable {
    case workingDirectoryMissing(String)
    case launchFailed(executable: String, reason: String)
    case stdoutTooLarge(limit: Int)
    case cancelled(partial: ProcessResult)           // thrown only after the whole process group is gone (bounded)
    case timedOut(after: Duration, partial: ProcessResult)
}

public protocol ProcessRunning: Sendable {
    /// Throws only ProcessRunError. Never blocks a thread; never calls waitUntilExit.
    /// If the calling Task is already cancelled, throws .cancelled WITHOUT spawning.
    func run(_ spec: ProcessSpec, onLine: @escaping @Sendable (OutputLine) -> Void) async throws -> ProcessResult
    func terminateAll() async                        // SIGINT→SIGTERM→SIGKILL every live group; bounded 10 s
}

/// Implemented by EnvironmentProvider (BranchBoxCLI).
public enum EnvironmentPurpose: Sendable { case read, mutation }
public protocol EnvironmentProviding: Sendable {
    /// .read returns immediately (provisional env if capture not finished); .mutation awaits the capture (≤ 8 s + fallbacks).
    func childEnvironment(for purpose: EnvironmentPurpose, settings: BackendSettings) async -> [String: String]
    func summary() async -> EnvironmentSummary
    func recapture() async
}
