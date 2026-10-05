import BranchBoxKit
import Foundation

/// Reads the CLI's `tracing` output (DESIGN §6.3): `<RFC 3339 timestamp> <LEVEL> <target>: <message>`, for example
/// `2026-10-01T22:50:29.222458Z  INFO worktree_core::git: Created worktree at /r/alpha`. Any other line (the CLI's
/// own text, an anyhow `Error:` block, a tool's output) is kept whole at level `.output`. Lines arrive from the
/// runner already ANSI-stripped and split.
public enum TracingLineParser {
    /// Matches `^(\S+)\s+(TRACE|DEBUG|INFO|WARN|ERROR)\s+([\w:]+):\s(.*)$`, hand-rolled so no regex object has to
    /// be shared across threads.
    public static func parse(_ text: String, source: LogLine.Source) -> LogLine {
        guard let fields = fields(of: text) else {
            return LogLine(timestamp: nil, level: .output, source: source, target: nil, message: text)
        }
        return LogLine(timestamp: RFC3339.parse(fields.stamp), level: fields.level, source: source,
                       target: fields.target, message: fields.message)
    }

    private static let levels: [String: LogLevel] = [
        "TRACE": .trace, "DEBUG": .debug, "INFO": .info, "WARN": .warn, "ERROR": .error,
    ]

    private static func fields(of text: String) -> (stamp: String, level: LogLevel, target: String, message: String)? {
        var rest = Substring(text)
        func token() -> Substring? {
            let token = rest.prefix { !$0.isWhitespace }
            guard !token.isEmpty else { return nil }
            rest = rest.dropFirst(token.count)
            return token
        }
        func spaces() -> Bool {
            let run = rest.prefix { $0.isWhitespace }
            rest = rest.dropFirst(run.count)
            return !run.isEmpty
        }
        guard let stamp = token(), spaces(), let levelToken = token(), let level = levels[String(levelToken)],
              spaces() else { return nil }
        // `([\w:]+):\s` — the run of word characters and colons must end in the colon that precedes one whitespace
        // character; the backtracking alternatives can never match because whitespace is outside the run.
        let run = rest.prefix { $0 == ":" || $0 == "_" || $0.isLetter || $0.isNumber }
        guard run.count >= 2, run.last == ":" else { return nil }
        let afterRun = rest.dropFirst(run.count)
        guard let separator = afterRun.first, separator.isWhitespace else { return nil }
        return (String(stamp), level, String(run.dropLast()), String(afterRun.dropFirst()))
    }
}

/// Derives the operation phase from tracing targets (DESIGN §6.3). Stateless; `ProgressRelay` drops repeats.
public enum PhaseMapper {
    /// What the CLI is doing, which decides how runtime lines read: set-up during a start, clean-up during a
    /// teardown.
    public enum Activity: Sendable { case start, teardown, other }

    public static func phase(for line: LogLine, during activity: Activity) -> OperationPhase? {
        guard let target = line.target else { return nil }
        let message = line.message
        let segments = target.split(separator: ":", omittingEmptySubsequences: true).map(String.init)
        if let index = segments.firstIndex(of: "modules"), index + 1 < segments.count {
            return .module(segments[index + 1])
        }
        if segments.contains("runtime") || segments.contains("devcontainer_runtime") {
            if activity == .teardown || isRuntimeCleanup(message) { return .cleaningRuntime }
            let provider = segments.last.flatMap { $0 == "runtime" ? nil : $0 } ?? ""
            return .runtime(provider)
        }
        if segments.contains("adapters") || message.hasPrefix("Detected adapter") { return .detectingAdapter }
        if segments.contains("tunnel") || segments.contains("tunnels") { return .provisioningTunnel }
        if segments.last == "git" {
            if message.hasPrefix("Created worktree") { return .creatingWorktree }
            if message.hasPrefix("Removed worktree") { return .removingWorktree }
            if message.hasPrefix("Deleted branch") { return .deletingBranch }
        }
        return nil
    }

    private static func isRuntimeCleanup(_ message: String) -> Bool {
        let lowered = message.lowercased()
        return ["destroy", "tearing down", "teardown", "removing", "removed"].contains { lowered.contains($0) }
    }
}

/// Turns one operation's output lines into ordered progress events: every line as a `.log`, plus a `.phase`
/// whenever the mapped phase changes. The runner calls `onLine` serially, but the relay locks anyway so it can be
/// shared with the backend's own events.
final class ProgressRelay: @unchecked Sendable {
    private let lock = NSLock()
    private let sink: ProgressSink?
    private let activity: PhaseMapper.Activity
    private var lastPhase: OperationPhase?

    init(_ sink: ProgressSink?, activity: PhaseMapper.Activity) {
        self.sink = sink
        self.activity = activity
    }

    var isActive: Bool { sink != nil }

    func line(_ output: OutputLine) {
        guard let sink else { return }
        let source: LogLine.Source = output.channel == .stderr ? .stderr : .stdout
        let line = TracingLineParser.parse(output.text, source: source)
        lock.withLock {
            sink(.log(line))
            if let phase = PhaseMapper.phase(for: line, during: activity), phase != lastPhase {
                lastPhase = phase
                sink(.phase(phase))
            }
        }
    }

    func phase(_ phase: OperationPhase) {
        guard let sink else { return }
        lock.withLock {
            guard phase != lastPhase else { return }
            lastPhase = phase
            sink(.phase(phase))
        }
    }

    func warning(_ text: String) {
        guard let sink else { return }
        lock.withLock { sink(.warning(text)) }
    }

    /// A line the app itself logs (`source == .app`), e.g. the legacy branch step.
    func note(_ text: String, level: LogLevel = .info) {
        guard let sink else { return }
        lock.withLock { sink(.log(LogLine(timestamp: Date(), level: level, source: .app, target: nil, message: text))) }
    }
}
