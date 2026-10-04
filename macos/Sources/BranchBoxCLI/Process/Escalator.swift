import Darwin
import os

/// Stops one run's process group: SIGINT, then SIGTERM after `interruptGrace`, then SIGKILL after
/// `terminateGrace`. Thread-safe; every method may be called from any thread.
///
/// - Foundation makes each child the leader of its own process group, so the group id is the child's pid and
///   `killpg` reaches every process it started, including grandchildren that ignore SIGINT (a non-interactive
///   shell runs `cmd &` with SIGINT ignored).
/// - The group keeps being signalled after the leader exits, for as long as members remain.
/// - The pid is published under the same lock that records the stop reason, so a stop requested while the
///   child is still being spawned is delivered as soon as the pid is known, exactly once.
/// - Once the group is seen gone (`ESRCH`) or the run is retired, nothing is signalled again: the pid may be
///   reused by an unrelated process from then on.
final class Escalator: Sendable {
    enum Reason: Sendable, Hashable { case cancelled, timedOut, stdoutTooLarge, shutdown }

    /// `terminateAll` caps each stage so every group gets SIGKILL within 8 s and the call returns within 10 s.
    static let shutdownInterruptGrace: Duration = .seconds(5)
    static let shutdownTerminateGrace: Duration = .seconds(3)

    private struct State: Sendable {
        var pid: pid_t = 0
        var reason: Reason?
        var leaderExited = false
        var groupGone = false
        var retired = false
        var escalation: Task<Void, Never>?
    }

    let interruptGrace: Duration
    let terminateGrace: Duration
    private let state = OSAllocatedUnfairLock(initialState: State())

    init(interruptGrace: Duration, terminateGrace: Duration) {
        self.interruptGrace = interruptGrace
        self.terminateGrace = terminateGrace
    }

    /// Why the run is being stopped; nil while it may finish on its own.
    var reason: Reason? { state.withLock { $0.reason } }

    /// The group id once the child is spawned, else nil.
    var pid: pid_t? { state.withLock { $0.pid > 0 ? $0.pid : nil } }

    /// Records the spawned child's pid. Starts the escalation now if a stop was requested during the launch.
    func publish(pid: pid_t) {
        let pending = state.withLock { state -> Reason? in
            state.pid = pid
            return state.retired ? nil : state.reason
        }
        if let pending { escalate(for: pending) }
    }

    /// Called from the termination handler once the leader has been reaped.
    func leaderDidExit() {
        state.withLock { $0.leaderExited = true }
    }

    var leaderExited: Bool { state.withLock { $0.leaderExited } }

    /// Requests a stop. The first reason wins; later calls, and calls after `retire()`, do nothing.
    func begin(_ reason: Reason) {
        let launched = state.withLock { state -> Bool? in
            guard state.reason == nil, !state.retired else { return nil }
            state.reason = reason
            return state.pid > 0
        }
        if launched == true { escalate(for: reason) }
    }

    /// Ends the run's claim on the group: no signal is sent after this.
    func retire() {
        let escalation = state.withLock { state -> Task<Void, Never>? in
            state.retired = true
            defer { state.escalation = nil }
            return state.escalation
        }
        escalation?.cancel()
    }

    /// Whether any member of the group may still be alive (EPERM counts as alive).
    var groupAlive: Bool {
        state.withLock { state in
            guard state.pid > 0, !state.groupGone else { return false }
            if killpg(state.pid, 0) == 0 || errno == EPERM { return true }
            if !state.leaderExited, kill(state.pid, 0) == 0 { return true }
            state.groupGone = true
            return false
        }
    }

    /// Waits until the group is gone, for at most both graces plus one second (the time SIGKILL needs to land).
    /// Returns whether the group is gone.
    @discardableResult
    func awaitGroupGone() async -> Bool {
        let graces = stageGraces(for: reason ?? .cancelled)
        let deadline = ContinuousClock.now + graces.interrupt + graces.terminate + .seconds(1)
        while groupAlive {
            guard ContinuousClock.now < deadline else { return false }
            try? await Task.sleep(for: .milliseconds(20))
        }
        return true
    }

    private func stageGraces(for reason: Reason) -> (interrupt: Duration, terminate: Duration) {
        guard reason == .shutdown else { return (interruptGrace, terminateGrace) }
        return (min(interruptGrace, Self.shutdownInterruptGrace), min(terminateGrace, Self.shutdownTerminateGrace))
    }

    /// Sends SIGINT now and schedules SIGTERM and SIGKILL; runs once per escalator.
    private func escalate(for reason: Reason) {
        let graces = stageGraces(for: reason)
        guard signal(SIGINT) else { return }
        let escalation = Task.detached { [self] in
            try? await Task.sleep(for: graces.interrupt)
            guard !Task.isCancelled, signal(SIGTERM) else { return }
            try? await Task.sleep(for: graces.terminate)
            guard !Task.isCancelled else { return }
            _ = signal(SIGKILL)
        }
        let stale = state.withLock { state -> Bool in
            guard !state.retired else { return true }
            state.escalation = escalation
            return false
        }
        if stale { escalation.cancel() }
    }

    /// Signals the group, then sends SIGCONT so stopped members act on it. Falls back to the leader alone if it
    /// is somehow not a group leader. Returns false once there is nothing left to signal.
    private func signal(_ signal: Int32) -> Bool {
        state.withLock { state in
            guard state.pid > 0, !state.groupGone, !state.retired else { return false }
            if killpg(state.pid, signal) == 0 {
                if signal != SIGKILL { _ = killpg(state.pid, SIGCONT) }
                return true
            }
            if errno == EPERM { return true }
            if !state.leaderExited, kill(state.pid, signal) == 0 { return true }
            state.groupGone = true
            return false
        }
    }
}
