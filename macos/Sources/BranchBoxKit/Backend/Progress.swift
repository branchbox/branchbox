import Foundation

public enum LogLevel: String, Sendable, Hashable, Codable { case trace, debug, info, warn, error, output }

public struct LogLine: Sendable, Hashable {
    public enum Source: String, Sendable, Hashable { case stdout, stderr, app }
    /// Longest message kept, in UTF-8 bytes; longer messages are cut at a character boundary.
    public static let maxMessageBytes = 16 * 1024
    public let timestamp: Date?
    public let level: LogLevel
    public let source: Source
    public let target: String?                       // tracing target, e.g. worktree_core::modules::compose
    public let message: String                       // ANSI-stripped, CR-trimmed, ≤ 16 KiB
    public init(timestamp: Date?, level: LogLevel, source: Source, target: String?, message: String) {
        self.timestamp = timestamp
        self.level = level
        self.source = source
        self.target = target
        self.message = LogLine.capped(message)
    }

    private static func capped(_ message: String) -> String {
        guard message.utf8.count > maxMessageBytes else { return message }
        var bytes = 0
        var end = message.startIndex
        for index in message.indices {
            let width = message[index].utf8.count
            if bytes + width > maxMessageBytes { break }
            bytes += width
            end = message.index(after: index)
        }
        return String(message[..<end])
    }
}

public enum OperationPhase: Sendable, Hashable {
    case preparing, creatingWorktree, module(String), runtime(String), startingEnvironment
    case removingWorktree, deletingBranch, cleaningRuntime, building, provisioningTunnel, detectingAdapter
    case item(index: Int, of: Int, name: String)     // prune rows
    case step(String)
}

public enum ProgressEvent: Sendable, Hashable {
    case log(LogLine)
    case phase(OperationPhase)
    case warning(String)
}

/// Called from any thread, in order, by a single producer per operation.
public typealias ProgressSink = @Sendable (ProgressEvent) -> Void
