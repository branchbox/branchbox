import Foundation

/// The time source behind the refresh timers, data-age checks and operation durations, so tests can run them on
/// a manual clock instead of waiting.
protocol StoreClock: Sendable {
    func now() -> Date
    /// Throws `CancellationError` when the waiting task is cancelled.
    func sleep(for duration: Duration) async throws
}

/// Wall-clock time and `Task.sleep`.
struct SystemClock: StoreClock {
    func now() -> Date { Date() }

    func sleep(for duration: Duration) async throws {
        try await Task.sleep(for: duration)
    }
}

extension Duration {
    /// The duration in seconds, for `Date` arithmetic.
    var seconds: TimeInterval {
        let (seconds, attoseconds) = components
        return TimeInterval(seconds) + TimeInterval(attoseconds) / 1e18
    }
}

/// Runs `work` in its own task and returns once it finishes or `timeout` passes, whichever comes first. The work
/// keeps running after a timeout; this only stops waiting for it.
func waitAtMost(_ timeout: Duration, _ work: @escaping @Sendable () async -> Void) async {
    let gate = FirstResume()
    await withCheckedContinuation { (continuation: CheckedContinuation<Void, Never>) in
        gate.arm(continuation)
        Task {
            await work()
            gate.resume()
        }
        Task {
            try? await Task.sleep(for: timeout)
            gate.resume()
        }
    }
}

/// Resumes its continuation on the first `resume()` and ignores the rest.
private final class FirstResume: @unchecked Sendable {
    private let lock = NSLock()
    private var continuation: CheckedContinuation<Void, Never>?
    private var resumed = false

    func arm(_ continuation: CheckedContinuation<Void, Never>) {
        let resumeNow: Bool = lock.withLock {
            if resumed { return true }
            self.continuation = continuation
            return false
        }
        if resumeNow { continuation.resume() }
    }

    func resume() {
        let continuation: CheckedContinuation<Void, Never>? = lock.withLock {
            guard !resumed else { return nil }
            resumed = true
            defer { self.continuation = nil }
            return self.continuation
        }
        continuation?.resume()
    }
}
