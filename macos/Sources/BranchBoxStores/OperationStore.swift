import BranchBoxKit
import Foundation
import Observation
import os

public enum Admission: Sendable, Hashable { case allowed, queued(behind: String), rejected(reason: String) }

/// A finished operation as kept in the persisted history (`operations.json`, newest first, at most 100), so
/// Activity and Diagnostics can list earlier sessions' operations. Additive (SW-2).
public struct OperationSummary: Sendable, Hashable, Codable, Identifiable {
    public enum Outcome: String, Sendable, Hashable, Codable { case succeeded, succeededWithWarnings, partial, failed, cancelled }
    public let id: UUID
    public let kind: OperationKind
    public let title: String
    public let projectPath: String?
    public let feature: String?
    public let startedAt: Date
    public let finishedAt: Date
    public let outcome: Outcome
    /// The failure's cause or the cancellation note.
    public let detail: String?
    /// The full log (`LogArchive`), if one was written and retention has not removed it yet.
    public let logPath: String?

    public init(id: UUID, kind: OperationKind, title: String, projectPath: String?, feature: String?, startedAt: Date,
                finishedAt: Date, outcome: Outcome, detail: String?, logPath: String?) {
        self.id = id
        self.kind = kind
        self.title = title
        self.projectPath = projectPath
        self.feature = feature
        self.startedAt = startedAt
        self.finishedAt = finishedAt
        self.outcome = outcome
        self.detail = detail
        self.logPath = logPath
    }
}

/// Every operation of this session, newest first, plus the persisted history of finished ones.
///
/// `ActionDispatcher` submits records here; the D-16 rules (`MutationQueue`) decide whether each runs at once,
/// waits as `.queued(behind:)`, or is rejected. When a record finishes, the queue is re-evaluated in dispatch
/// order.
@MainActor @Observable public final class OperationStore {
    public private(set) var records: [OperationRecord] = []       // newest first; history cap 100 (persisted summaries)
    /// Finished operations, this session's and earlier ones, newest first, at most `historyLimit`. Additive (SW-2).
    public private(set) var history: [OperationSummary] = []

    static let historyLimit = 100

    @ObservationIgnored let queue = MutationQueue()
    @ObservationIgnored private var starters: [UUID: (OperationRecord) -> Void] = [:]
    @ObservationIgnored private var draining = false
    private let registryLock: () -> Bool
    private let historyURL: URL?
    private let clock: any StoreClock

    private static let logger = Logger(subsystem: "dev.branchbox.app", category: "operations")

    /// `registryLock` reports whether the current CLI serializes registry writes itself (`registry-lock`).
    init(registryLock: @escaping () -> Bool = { false }, historyURL: URL? = nil, clock: any StoreClock = SystemClock()) {
        self.registryLock = registryLock
        self.historyURL = historyURL
        self.clock = clock
    }

    /// Queued and running operations.
    public var running: [OperationRecord] {
        records.filter(\.isCancellable)
    }

    public func records(for target: OperationTarget) -> [OperationRecord] {
        records.filter { $0.target == target }
    }

    public func active(for target: OperationTarget) -> OperationRecord? {
        records.first { $0.target == target && $0.isCancellable }
    }

    /// Additive (SW-2): a record of this session by id, e.g. for `.showActivity(operation:)`.
    public func record(_ id: OperationRecord.ID) -> OperationRecord? {
        records.first { $0.id == id }
    }

    /// What dispatching an operation of `kind` on `target` would do now (D-16). Reads and exec are always allowed.
    public func admission(for kind: OperationKind, target: OperationTarget) -> Admission {
        guard kind.isMutating else { return .allowed }
        if let other = queue.featureConflict(kind: kind, target: target) { return .rejected(reason: Self.busyReason(other)) }
        if let blocker = queue.blocker(kind: kind, target: target, registryLock: registryLock()) {
            return .queued(behind: blocker.title)
        }
        return .allowed
    }

    /// The UI confirms first (D-18). A queued operation is dropped before it starts; a running one has its task
    /// cancelled, and the backend stops its process group (SIGINT, then SIGTERM, then SIGKILL) before the record
    /// becomes `.cancelled`.
    public func cancel(_ id: OperationRecord.ID) {
        guard let record = record(id), record.isCancellable else { return }
        if case .queued = record.state {
            starters[id] = nil
            queue.remove(record)
            record.finish(.cancelled(note: nil), at: clock.now())
            record.archive?.close(footer: ["# Cancelled before it started"])
            appendHistory(record)
            schedule()
        } else {
            record.task?.cancel()
        }
    }

    /// Cancels every queued and running operation and waits for the running ones to stop (app quit).
    public func cancelAll() async {
        draining = true
        defer {
            draining = false
            schedule()
        }
        for record in records where record.isCancellable {
            if case .queued = record.state { cancel(record.id) }
        }
        let running = records.filter(\.isCancellable)
        for record in running { record.task?.cancel() }
        for record in running { await record.task?.value }
    }

    static func busyReason(_ other: OperationRecord) -> String {
        "\(other.title) is still in progress; wait for it to finish or stop it first"
    }

    // MARK: Dispatcher interface

    /// Adds `record` and runs `start` now or, when it must wait, later; returns what it waits behind.
    func submit(_ record: OperationRecord, start: @escaping (OperationRecord) -> Void) -> OperationRecord? {
        insert(record)
        queue.append(record)
        if let blocker = queue.blocker(for: record, registryLock: registryLock()) {
            record.markQueued(behind: blocker.title)
            starters[record.id] = start
            return blocker
        }
        record.markRunning(at: clock.now())
        start(record)
        return nil
    }

    /// Called once a started record has its final state.
    func finished(_ record: OperationRecord) {
        queue.remove(record)
        appendHistory(record)
        schedule()
    }

    /// Starts every queued record that no longer waits for anything, in dispatch order, and updates what the
    /// others wait behind.
    private func schedule() {
        guard !draining else { return }
        let registryLock = registryLock()
        for record in queue.entries {
            guard case .queued = record.state, let start = starters[record.id] else { continue }
            if let blocker = queue.blocker(for: record, registryLock: registryLock) {
                if record.state != .queued(behind: blocker.title) { record.markQueued(behind: blocker.title) }
            } else {
                starters[record.id] = nil
                record.markRunning(at: clock.now())
                start(record)
            }
        }
    }

    private func insert(_ record: OperationRecord) {
        records.insert(record, at: 0)
        guard records.count > Self.historyLimit else { return }
        // Drop the oldest finished records; queued and running ones always stay.
        var excess = records.count - Self.historyLimit
        var index = records.count - 1
        while excess > 0, index >= 0 {
            if !records[index].isCancellable {
                records.remove(at: index)
                excess -= 1
            }
            index -= 1
        }
    }

    // MARK: Persisted history

    func loadHistory() {
        guard let historyURL, let data = try? Data(contentsOf: historyURL) else { return }
        do {
            let loaded = try JSONDecoder().decode([Lossy<OperationSummary>].self, from: data).compactMap(\.value)
            let known = Set(history.map(\.id))
            history = Array((history + loaded.filter { !known.contains($0.id) }).prefix(Self.historyLimit))
        } catch {
            Self.logger.error("operations.json is unreadable; starting a new history: \(error, privacy: .public)")
        }
    }

    private func appendHistory(_ record: OperationRecord) {
        guard let summary = Self.summary(of: record) else { return }
        history.insert(summary, at: 0)
        if history.count > Self.historyLimit { history.removeLast(history.count - Self.historyLimit) }
        guard let historyURL else { return }
        do {
            try FileManager.default.createDirectory(at: historyURL.deletingLastPathComponent(), withIntermediateDirectories: true)
            try JSONEncoder().encode(history).write(to: historyURL, options: .atomic)
        } catch {
            Self.logger.error("Saving operations.json failed: \(error, privacy: .public)")
        }
    }

    static func summary(of record: OperationRecord) -> OperationSummary? {
        let outcome: OperationSummary.Outcome
        var detail: String?
        switch record.state {
        case .queued, .running: return nil
        case .succeeded: outcome = .succeeded
        case .succeededWithWarnings: outcome = .succeededWithWarnings
        case .partial: outcome = .partial
        case .failed(let error):
            outcome = .failed
            detail = error.briefSummary
        case .cancelled(let note):
            outcome = .cancelled
            detail = note
        }
        let feature: String? = if case .feature(let feature) = record.target { feature.name } else { nil }
        return OperationSummary(id: record.id, kind: record.kind, title: record.title, projectPath: record.project?.path,
                                feature: feature, startedAt: record.startedAt, finishedAt: record.finishedAt ?? record.startedAt,
                                outcome: outcome, detail: detail.map(record.redacted), logPath: record.log.archiveURL?.path)
    }
}
