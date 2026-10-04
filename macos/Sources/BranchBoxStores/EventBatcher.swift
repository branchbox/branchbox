import BranchBoxKit
import Foundation

/// Collects an operation's progress events and hands them to the record in batches, at most once per `interval`
/// (100 ms: ≤ 10 UI invalidations per second however fast docker logs). Events keep their order within and
/// across batches; `flush()` delivers whatever is pending at once, e.g. when the operation ends.
@MainActor final class EventBatcher {
    private let interval: Duration
    private let deliver: ([ProgressEvent]) -> Void
    private var pending: [ProgressEvent] = []
    private var timer: Task<Void, Never>?

    /// How many batches were delivered; each is one round of record mutations.
    private(set) var deliveries = 0

    init(interval: Duration = .milliseconds(100), deliver: @escaping ([ProgressEvent]) -> Void) {
        self.interval = interval
        self.deliver = deliver
    }

    func add(_ event: ProgressEvent) {
        pending.append(event)
        guard timer == nil else { return }
        timer = Task { [weak self, interval] in
            do { try await Task.sleep(for: interval) } catch { return }    // cancelled by flush()
            self?.timerFired()
        }
    }

    func flush() {
        timer?.cancel()
        timer = nil
        guard !pending.isEmpty else { return }
        let batch = pending
        pending.removeAll(keepingCapacity: true)
        deliveries += 1
        deliver(batch)
    }

    private func timerFired() {
        timer = nil
        flush()
    }
}
