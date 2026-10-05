import BranchBoxKit
import Foundation
import Observation

public enum OperationKind: String, Sendable, Hashable, Codable {
    case start, teardown, prune, exec, devcontainerUp, devcontainerDown, devcontainerRebuild, devcontainerBuild
    case syncDevcontainers, tunnelOpen, tunnelRemove, initProject, applyConfig, tunnelCredentials, deleteBranch, removeStray

    /// Without the `registry-lock` capability these run FIFO per project (D-16).
    public var writesRegistry: Bool {   // start, teardown, prune, tunnelOpen/Remove, syncDevcontainers, initProject, applyConfig, tunnelCredentials
        switch self {
        case .start, .teardown, .prune, .tunnelOpen, .tunnelRemove, .syncDevcontainers, .initProject, .applyConfig,
             .tunnelCredentials:
            true
        case .exec, .devcontainerUp, .devcontainerDown, .devcontainerRebuild, .devcontainerBuild, .deleteBranch, .removeStray:
            false
        }
    }

    public var isMutating: Bool {       // all but exec
        self != .exec
    }

    public var isProjectWide: Bool {    // prune, syncDevcontainers, initProject, applyConfig, tunnelCredentials
        switch self {
        case .prune, .syncDevcontainers, .initProject, .applyConfig, .tunnelCredentials:
            true
        case .start, .teardown, .exec, .devcontainerUp, .devcontainerDown, .devcontainerRebuild, .devcontainerBuild,
             .tunnelOpen, .tunnelRemove, .deleteBranch, .removeStray:
            false
        }
    }
}

public enum OperationTarget: Sendable, Hashable { case feature(FeatureRef), project(ProjectRef), global }
public enum OperationState: Sendable, Hashable { case queued(behind: String), running, succeeded, succeededWithWarnings, partial, failed(BackendError), cancelled(note: String?) }

public struct StepProgress: Sendable, Hashable {
    public let completed: Int; public let total: Int
    public init(completed: Int, total: Int) { self.completed = completed; self.total = total }
}

public enum PruneRowOutcome: Sendable, Hashable { case removed(TeardownOutcome), refused(BackendError), failed(BackendError), skipped(String), cancelled }

public struct PruneRow: Sendable, Hashable {
    public let feature: String; public let outcome: PruneRowOutcome
    public init(feature: String, outcome: PruneRowOutcome) { self.feature = feature; self.outcome = outcome }
}

public struct PruneResult: Sendable, Hashable {
    public let rows: [PruneRow]
    public init(rows: [PruneRow]) { self.rows = rows }
}

public enum OperationResult: Sendable, Hashable {
    case start(StartSummary), teardown(TeardownOutcome), prune(PruneResult), exec(ExecResult), devcontainer(DevcontainerResult)
    case sync(SyncReport), tunnel(TunnelChange), initProject(InitReport), config(ConfigApplyResult)
    case credentials(TunnelCredentialsResult), message(String)
}

/// One run of one `OperationRequestContext`: its state, progress, log and typed result.
@MainActor @Observable public final class OperationRecord: Identifiable {
    public let id: UUID
    public let kind: OperationKind
    public let target: OperationTarget
    public let title: String                                      // "Starting oauth"
    public let context: OperationRequestContext
    public let startedAt: Date
    public private(set) var state: OperationState
    public private(set) var result: OperationResult?
    public private(set) var phase: OperationPhase?
    public private(set) var stepProgress: StepProgress?
    public private(set) var warnings: [String] = []
    public let log: LogBuffer
    public private(set) var finishedAt: Date?
    public private(set) var acknowledged: Bool = false            // failed/partial count toward menu-bar attention until viewed

    /// When the work actually began (a queued record waits first); nil while queued.
    public private(set) var runningSince: Date?

    /// The task running the operation; cancelling it cancels the backend call (and its process group).
    @ObservationIgnored var task: Task<Void, Never>?
    /// Where the full log streams; closed when the record finishes.
    @ObservationIgnored var archive: LogArchive?
    /// Values never shown or stored as they are (D-20): the settings' extra-environment values and the request's
    /// token, longest first, at least 4 characters (shorter ones are ordinary words). The in-memory log, the
    /// warnings and the history entry show `<redacted>` instead, as the archive does.
    @ObservationIgnored private(set) var secrets: [String] = []

    func setSecrets(_ values: [String]) {
        secrets = Set(values.filter { $0.count >= 4 }).sorted { $0.count > $1.count }
    }

    init(id: UUID = UUID(), kind: OperationKind, target: OperationTarget, title: String,
         context: OperationRequestContext, startedAt: Date = .now, state: OperationState = .running) {
        self.id = id
        self.kind = kind
        self.target = target
        self.title = title
        self.context = context
        self.startedAt = startedAt
        self.state = state
        self.log = LogBuffer()
        if case .running = state { runningSince = startedAt }
    }

    public var isCancellable: Bool {
        switch state {
        case .queued, .running: true
        case .succeeded, .succeededWithWarnings, .partial, .failed, .cancelled: false
        }
    }

    /// Finished as failed or partial: counts toward attention until acknowledged.
    public var needsAttention: Bool {
        guard !acknowledged else { return false }
        switch state {
        case .failed, .partial: return true
        case .queued, .running, .succeeded, .succeededWithWarnings, .cancelled: return false
        }
    }

    public func acknowledge() {
        acknowledged = true
    }

    /// The project the operation acts on; nil for global ones.
    var project: ProjectRef? { target.project }

    func markQueued(behind title: String) {
        state = .queued(behind: title)
    }

    func markRunning(at date: Date) {
        state = .running
        runningSince = date
    }

    /// Applies one batch from `EventBatcher`: all its log lines in one `LogBuffer` append, the last phase, every
    /// warning, and prune progress from `.item` phases.
    func apply(_ events: [ProgressEvent]) {
        var lines: [LogLine] = []
        for event in events {
            switch event {
            case .log(let line):
                lines.append(redacted(line))
            case .phase(let phase):
                self.phase = phase
                if case .item(let index, let total, _) = phase {
                    stepProgress = StepProgress(completed: max(index - 1, 0), total: total)
                }
            case .warning(let warning):
                warnings.append(redacted(warning))
            }
        }
        log.append(lines)
        archive?.append(lines)
    }

    /// `text` with every secret replaced by `<redacted>`.
    func redacted(_ text: String) -> String {
        secrets.isEmpty ? text : LogArchive.redact(text, secrets: secrets)
    }

    private func redacted(_ line: LogLine) -> LogLine {
        guard !secrets.isEmpty else { return line }
        let message = redacted(line.message)
        guard message != line.message else { return line }
        return LogLine(timestamp: line.timestamp, level: line.level, source: line.source, target: line.target,
                       message: message)
    }

    /// Single-event convenience (tests, app-side notes).
    func apply(_ event: ProgressEvent) {
        apply([event])
    }

    func addWarnings(_ newWarnings: [String]) {
        for warning in newWarnings.map(redacted) where !warnings.contains(warning) { warnings.append(warning) }
    }

    func setStepProgress(_ progress: StepProgress) {
        stepProgress = progress
    }

    func finish(_ state: OperationState, result: OperationResult? = nil, at date: Date = .now) {
        self.state = state
        self.result = result
        finishedAt = date
    }
}

extension OperationTarget {
    /// The project of a feature or project target; nil for `.global`.
    public var project: ProjectRef? {
        switch self {
        case .feature(let feature): feature.project
        case .project(let project): project
        case .global: nil
        }
    }
}

extension OperationRequestContext {
    /// The kind of record `ActionDispatcher` makes for this request (Rebuild = `up` with both flags).
    public var operationKind: OperationKind {
        switch self {
        case .start: .start
        case .teardown: .teardown
        case .prune: .prune
        case .exec: .exec
        case .devcontainer(.up(let removeExisting, let buildNoCache), _):
            removeExisting && buildNoCache ? .devcontainerRebuild : .devcontainerUp
        case .devcontainer(.down, _): .devcontainerDown
        case .devcontainer(.build, _): .devcontainerBuild
        case .syncDevcontainers: .syncDevcontainers
        case .tunnelOpen: .tunnelOpen
        case .tunnelRemove: .tunnelRemove
        case .initProject: .initProject
        case .applyConfig: .applyConfig
        case .tunnelCredentials: .tunnelCredentials
        case .deleteBranch: .deleteBranch
        case .removeStray: .removeStray
        }
    }

    /// What the request acts on: one feature, or a whole project (`init` targets the folder being set up).
    public var operationTarget: OperationTarget {
        switch self {
        case .start(let request): .feature(FeatureRef(project: request.project, name: request.name))
        case .teardown(let request): .feature(request.feature)
        case .prune(let selection): .project(selection.project)
        case .exec(let request): .feature(request.feature)
        case .devcontainer(_, let feature): .feature(feature)
        case .syncDevcontainers(let request): .project(request.project)
        case .tunnelOpen(let feature): .feature(feature)
        case .tunnelRemove(let feature, _): .feature(feature)
        case .initProject(let request): .project(ProjectRef(root: request.folder))
        case .applyConfig(_, let project): .project(project)
        case .tunnelCredentials(_, let project): .project(project)
        case .deleteBranch(_, let project, _): .project(project)
        case .removeStray(_, let project, _): .project(project)
        }
    }
}
