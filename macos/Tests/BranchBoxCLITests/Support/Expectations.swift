import BranchBoxKit
import Foundation
import Testing

/// The `BackendError` that `body` throws, recording an issue when it returns or throws anything else. Works the same
/// on Swift 6.0 and 6.2, unlike the error-returning and error-validating `#expect(throws:)` overloads.
func backendError<T>(sourceLocation: SourceLocation = #_sourceLocation,
                     _ body: () async throws -> T) async -> BackendError? {
    do {
        _ = try await body()
        Issue.record("expected a BackendError, but the call succeeded", sourceLocation: sourceLocation)
        return nil
    } catch let error as BackendError {
        return error
    } catch {
        Issue.record("expected a BackendError, got \(error)", sourceLocation: sourceLocation)
        return nil
    }
}

extension BackendError {
    /// The refusal of a `.refused` error.
    var refusal: Refusal? {
        if case .refused(let refusal) = self { return refusal }
        return nil
    }

    var diagnostics: Diagnostics? {
        switch self {
        case .refused(let refusal): return refusal.diagnostics
        case .partial(let partial): return partial.remaining.diagnostics
        case .commandFailed(let diagnostics), .decodeFailed(_, _, let diagnostics), .registryCorrupted(_, let diagnostics),
             .timedOut(_, _, let diagnostics):
            return diagnostics
        default: return nil
        }
    }
}

/// Collects the events a backend method reports through its `ProgressSink`.
final class ProgressCollector: @unchecked Sendable {
    private let lock = NSLock()
    private var collected: [ProgressEvent] = []

    var sink: ProgressSink { { [self] event in lock.withLock { collected.append(event) } } }
    var events: [ProgressEvent] { lock.withLock { collected } }

    var phases: [OperationPhase] {
        events.compactMap { if case .phase(let phase) = $0 { return phase } else { return nil } }
    }

    var logs: [LogLine] {
        events.compactMap { if case .log(let line) = $0 { return line } else { return nil } }
    }

    var warnings: [String] {
        events.compactMap { if case .warning(let text) = $0 { return text } else { return nil } }
    }
}
