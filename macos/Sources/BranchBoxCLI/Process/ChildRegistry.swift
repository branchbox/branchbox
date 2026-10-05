import Darwin
import Foundation
import os

/// The runs a `ProcessRunner` has in flight, so app quit can stop every process group it started.
///
/// A run registers its escalator before it spawns and unregisters once it has returned or thrown, so a
/// `terminateAll()` that races a launch still reaches that child as soon as its pid is published.
final class ChildRegistry: Sendable {
    /// `terminateAll()` gives up waiting after this long; SIGKILL has gone out to every group by then.
    static let terminateAllBound: Duration = .seconds(10)

    private let entries = OSAllocatedUnfairLock(initialState: [UUID: Escalator]())

    func register(_ escalator: Escalator) -> UUID {
        let id = UUID()
        entries.withLock { $0[id] = escalator }
        return id
    }

    func unregister(_ id: UUID) {
        entries.withLock { $0[id] = nil }
    }

    /// Runs still in flight, including ones not yet spawned.
    var count: Int { entries.withLock { $0.count } }

    /// Group ids of the spawned runs still in flight.
    var liveGroups: [pid_t] { entries.withLock { $0.values.compactMap(\.pid) } }

    /// Stops every registered run (SIGINT → SIGTERM → SIGKILL, stages capped at 5 s and 3 s) and waits until
    /// their groups are gone, for at most `bound`. Each stopped run throws `.cancelled`.
    func terminateAll(bound: Duration = terminateAllBound) async {
        let escalators = entries.withLock { Array($0.values) }
        guard !escalators.isEmpty else { return }
        for escalator in escalators { escalator.begin(.shutdown) }
        let deadline = ContinuousClock.now + bound
        while escalators.contains(where: \.groupAlive), ContinuousClock.now < deadline {
            try? await Task.sleep(for: .milliseconds(20))
        }
    }
}
