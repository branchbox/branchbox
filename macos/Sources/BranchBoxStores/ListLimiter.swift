import Foundation

/// Caps how many `feature list` calls run at once across every project (D-17: at most 2), so a burst of
/// refreshes (app activation, the all-projects timer) never spawns one CLI per project. Waiters are served in
/// arrival order.
@MainActor final class ListLimiter {
    let limit: Int
    private(set) var running = 0
    private var waiters: [CheckedContinuation<Void, Never>] = []

    init(limit: Int = 2) {
        precondition(limit > 0, "a limiter must let at least one call through")
        self.limit = limit
    }

    /// How many callers are waiting for a slot.
    var waiting: Int { waiters.count }

    /// Runs `body` once a slot is free. Refresh loops are never cancelled, so waiting is not cancellable either.
    func run<T: Sendable>(_ body: () async throws -> T) async rethrows -> T {
        await acquire()
        defer { release() }
        return try await body()
    }

    private func acquire() async {
        if running < limit {
            running += 1
            return
        }
        await withCheckedContinuation { waiters.append($0) }
    }

    /// Hands the slot straight to the oldest waiter, so `running` never dips below the limit while some wait.
    private func release() {
        if waiters.isEmpty {
            running -= 1
        } else {
            waiters.removeFirst().resume()
        }
    }
}
