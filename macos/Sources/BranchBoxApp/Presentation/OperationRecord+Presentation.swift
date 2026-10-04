import BranchBoxKit
import BranchBoxStores
import Foundation

/// A value snapshot of an `OperationRecord`, so rows and progress views can also be built from plain data
/// (previews, persisted history).
struct OperationSummary: Sendable, Hashable, Identifiable {
    let id: UUID
    let kind: OperationKind
    let title: String
    let state: OperationState
    let startedAt: Date
    let finishedAt: Date?
    let phase: OperationPhase?
    let stepProgress: StepProgress?
    let warnings: [String]

    init(id: UUID = UUID(), kind: OperationKind, title: String, state: OperationState, startedAt: Date,
         finishedAt: Date? = nil, phase: OperationPhase? = nil, stepProgress: StepProgress? = nil, warnings: [String] = []) {
        self.id = id
        self.kind = kind
        self.title = title
        self.state = state
        self.startedAt = startedAt
        self.finishedAt = finishedAt
        self.phase = phase
        self.stepProgress = stepProgress
        self.warnings = warnings
    }

    /// Reading a record's properties here registers observation when called from a view's body.
    @MainActor init(_ record: OperationRecord) {
        self.init(id: record.id, kind: record.kind, title: record.title, state: record.state, startedAt: record.startedAt,
                  finishedAt: record.finishedAt, phase: record.phase, stepProgress: record.stepProgress,
                  warnings: record.warnings)
    }

    var isRunning: Bool {
        switch state {
        case .queued, .running: true
        case .succeeded, .succeededWithWarnings, .partial, .failed, .cancelled: false
        }
    }

    /// Time since the start, or the run's total once it finished.
    func elapsed(now: Date) -> Duration {
        .seconds(max(0, (finishedAt ?? now).timeIntervalSince(startedAt)))
    }
}

extension OperationState {
    var label: String {
        switch self {
        case .queued(let behind): "Waiting for \(behind)"
        case .running: "Running"
        case .succeeded: "Done"
        case .succeededWithWarnings: "Done with warnings"
        case .partial: "Partly done"
        case .failed: "Failed"
        case .cancelled: "Stopped"
        }
    }

    var symbol: String {
        switch self {
        case .queued: "clock"
        case .running: "arrow.triangle.2.circlepath"
        case .succeeded: "checkmark.circle.fill"
        case .succeededWithWarnings: "exclamationmark.triangle.fill"
        case .partial: "exclamationmark.circle.fill"
        case .failed: "xmark.octagon.fill"
        case .cancelled: "stop.circle"
        }
    }

    var tint: StatusTint {
        switch self {
        case .queued, .cancelled: .gray
        case .running: .blue
        case .succeeded: .green
        case .succeededWithWarnings, .partial: .orange
        case .failed: .red
        }
    }
}

extension OperationKind {
    var symbol: String {
        switch self {
        case .start: "play.circle"
        case .teardown: "trash"
        case .prune: "scissors"
        case .exec: "terminal"
        case .devcontainerUp: "power"
        case .devcontainerDown: "stop"
        case .devcontainerRebuild: "hammer"
        case .devcontainerBuild: "hammer"
        case .syncDevcontainers: "arrow.triangle.2.circlepath"
        case .tunnelOpen: "network"
        case .tunnelRemove: "network.slash"
        case .initProject: "wand.and.stars"
        case .applyConfig: "slider.horizontal.3"
        case .tunnelCredentials: "key"
        case .deleteBranch: "arrow.triangle.branch"
        case .removeStray: "folder.badge.minus"
        }
    }
}

extension OperationPhase {
    var label: String {
        switch self {
        case .preparing: "Preparing"
        case .creatingWorktree: "Creating the worktree"
        case .module(let name): "Setting up \(name)"
        case .runtime(let name): "Preparing the \(name) runtime"
        case .startingEnvironment: "Starting the environment"
        case .removingWorktree: "Removing the worktree"
        case .deletingBranch: "Deleting the branch"
        case .cleaningRuntime: "Cleaning up the runtime"
        case .building: "Building"
        case .provisioningTunnel: "Provisioning the tunnel"
        case .detectingAdapter: "Detecting the project type"
        case .item(let index, let total, let name): "\(index) of \(total): \(name)"
        case .step(let text): text
        }
    }
}

/// The Stop confirmation for a running operation (D-18).
struct CancelConfirmation: Sendable, Hashable {
    let title: String
    let message: String
    let stopLabel: String
    let keepLabel: String
    /// True when the message warns about a partial worktree and registry corruption.
    let warnsAboutCorruption: Bool

    /// A registry writer on a CLI without `registry-lock` and `write-ahead-start` can leave a partial worktree and
    /// a truncated `.branchbox/registry.json` when stopped mid-write; the confirmation says so explicitly.
    init(kind: OperationKind, title: String, capabilities: Set<Capability>) {
        self.title = "Stop “\(title)”?"
        stopLabel = "Stop"
        keepLabel = "Keep Running"
        let protected = capabilities.contains(.registryLock) && capabilities.contains(.writeAheadStart)
        warnsAboutCorruption = kind.writesRegistry && !protected
        if warnsAboutCorruption {
            message = "This BranchBox CLI can't be stopped safely mid-way: stopping now may leave a partial worktree behind "
                + "and corrupt the project's feature registry (.branchbox/registry.json). Stop only if it is stuck."
        } else if kind == .exec {
            message = "The command is interrupted; its output so far is kept."
        } else {
            message = "BranchBox interrupts the command, waits for its processes to exit, then refreshes. "
                + "Steps that already finished are not undone."
        }
    }
}

enum OperationPresentation {
    /// "0:42", "12:05", "1:02:05".
    static func elapsed(_ duration: Duration) -> String {
        let pattern: Duration.TimeFormatStyle.Pattern = duration >= .seconds(3600) ? .hourMinuteSecond : .minuteSecond
        return duration.formatted(.time(pattern: pattern))
    }

    /// "Running · Setting up compose · 0:42", the line under an operation's title.
    static func statusLine(_ summary: OperationSummary, now: Date) -> String {
        var parts = [summary.state.label]
        if summary.isRunning, let phase = summary.phase { parts.append(phase.label) }
        parts.append(elapsed(summary.elapsed(now: now)))
        return parts.joined(separator: " · ")
    }
}

/// A log line with its position in the full log, which is its identity in lists.
struct IndexedLogLine: Sendable, Hashable, Identifiable {
    let index: Int
    let line: LogLine
    var id: Int { index }
}

/// Which log lines the log view shows: optionally only warnings and errors, optionally only those containing a
/// search text (case- and diacritic-insensitive). Indices refer to the full log, so ids stay stable.
struct LogFilter: Sendable, Hashable {
    var warningsOnly = false
    var query = ""

    /// The lines to show, each indexed by its position in the whole log: `firstIndex` is how many lines the
    /// buffer dropped before `lines[0]`, so an index stays the same line while the ring drops old ones.
    func apply(to lines: [LogLine], firstIndex: Int = 0) -> [IndexedLogLine] {
        let needle = query.trimmingCharacters(in: .whitespaces)
        return lines.enumerated().compactMap { offset, line in
            if warningsOnly, line.level != .warn, line.level != .error { return nil }
            if !needle.isEmpty, line.message.range(of: needle, options: [.caseInsensitive, .diacriticInsensitive]) == nil {
                return nil
            }
            return IndexedLogLine(index: firstIndex + offset, line: line)
        }
    }

    /// The log as plain text, as copied: "12:00:01.250 WARN target: message".
    static func plainText(_ lines: [LogLine], timestamps: Bool, timeZone: TimeZone = .current) -> String {
        lines.map { line in
            var prefix = ""
            if timestamps, let date = line.timestamp { prefix += timestamp(date, timeZone: timeZone) + " " }
            if line.level != .output { prefix += line.level.rawValue.uppercased() + " " }
            if let target = line.target { prefix += target + ": " }
            return prefix + line.message
        }.joined(separator: "\n")
    }

    /// "12:00:01.250", 24-hour, in `timeZone`.
    static func timestamp(_ date: Date, timeZone: TimeZone = .current) -> String {
        let format: Date.FormatString =
            "\(hour: .twoDigits(clock: .twentyFourHour, hourCycle: .zeroBased)):\(minute: .twoDigits):\(second: .twoDigits).\(secondFraction: .fractional(3))"
        return date.formatted(Date.VerbatimFormatStyle(format: format, timeZone: timeZone,
                                                       calendar: Calendar(identifier: .gregorian)))
    }
}

extension LogLevel {
    var symbol: String {
        switch self {
        case .trace: "ant"
        case .debug: "ladybug"
        case .info: "info.circle"
        case .warn: "exclamationmark.triangle.fill"
        case .error: "xmark.octagon.fill"
        case .output: "chevron.right"
        }
    }

    var tint: StatusTint {
        switch self {
        case .trace, .debug, .output: .gray
        case .info: .blue
        case .warn: .orange
        case .error: .red
        }
    }
}
