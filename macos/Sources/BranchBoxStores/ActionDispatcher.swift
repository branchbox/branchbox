import BranchBoxKit
import Foundation

public enum DispatchResult { case started(OperationRecord), queued(OperationRecord, behind: String), rejected(reason: String), unavailable(BackendError) }

/// Starts operations (§8.3). For each request it:
/// - works out the kind and target, applies admission (D-16) and creates the `OperationRecord`;
/// - runs the backend call in a task that captures the backend, once the record may run;
/// - feeds the `ProgressSink` into an `AsyncStream` read by ONE main-actor consumer, so events stay in order, and
///   applies them in `EventBatcher` batches (≤ 10 record updates per second);
/// - streams the full, redacted log to `LogArchive`;
/// - maps the typed result to the record's state, requests a refresh of the project after success, failure and
///   cancellation alike, and posts a notification when one is due.
/// Requests are run exactly as given: the dispatcher never adds discard consent or force flags.
@MainActor public final class ActionDispatcher {
    /// Whether the user is looking at BranchBox (app active and main window visible). A finished operation only
    /// notifies when this is false. SW-4 wires it; unwired, every qualifying completion notifies (the notifier's
    /// own frontmost check still applies). Additive (SW-2).
    public var isAppActive: @MainActor () -> Bool = { false }

    private let settings: AppSettings
    private let environment: EnvironmentStore
    private let projects: ProjectsStore
    private let operations: OperationStore
    private let notifier: any Notifier
    private let clock: any StoreClock
    private let logsDirectory: URL?
    private let flushInterval: Duration
    private let notificationThreshold: Duration

    init(settings: AppSettings, environment: EnvironmentStore, projects: ProjectsStore, operations: OperationStore,
         notifier: any Notifier, clock: any StoreClock = SystemClock(), logsDirectory: URL? = nil,
         flushInterval: Duration = .milliseconds(100), notificationThreshold: Duration = .seconds(10)) {
        self.settings = settings
        self.environment = environment
        self.projects = projects
        self.operations = operations
        self.notifier = notifier
        self.clock = clock
        self.logsDirectory = logsDirectory
        self.flushInterval = flushInterval
        self.notificationThreshold = notificationThreshold
    }

    /// The ONLY way views start operations. Builds the OperationRecord, applies admission, streams progress
    /// through one ordered AsyncStream consumer, refreshes the project after success/failure/cancel, notifies.
    @discardableResult public func dispatch(_ request: OperationRequestContext) -> DispatchResult {
        if environment.isTerminating { return .rejected(reason: "BranchBox is quitting") }
        let backend: any BranchBoxBackend
        do {
            backend = try environment.currentBackend()
        } catch {
            return .unavailable(BackendError.normalize(error))
        }
        let kind = request.operationKind
        let target = request.operationTarget
        if kind.isMutating {
            if let other = operations.queue.featureConflict(kind: kind, target: target) {
                return .rejected(reason: OperationStore.busyReason(other))
            }
            if let same = operations.queue.entries.first(where: { $0.context == request }) {
                return .rejected(reason: OperationStore.busyReason(same))
            }
            if let holder = operations.queue.resourceConflict(request.exclusiveKeys) {
                return .rejected(reason: OperationStore.busyReason(holder))
            }
        }
        let record = OperationRecord(kind: kind, target: target, title: title(for: request), context: request,
                                     startedAt: clock.now(), state: .queued(behind: ""))
        record.setSecrets(secrets(for: request))
        attachArchive(to: record, commandLine: backend.previewCommandLine(request))
        let blocker = operations.submit(record) { [weak self] record in self?.run(record) }
        if let blocker { return .queued(record, behind: blocker.title) }
        return .started(record)
    }

    /// Re-runs a `.retry` recovery's request; nil for the recoveries the UI performs itself.
    @discardableResult public func perform(_ recovery: RecoveryAction) -> DispatchResult? {
        guard case .retry(let request, _, _, _) = recovery else { return nil }
        return dispatch(request)
    }

    /// The record title shown in Activity and notifications, e.g. "Starting oauth".
    public func title(for request: OperationRequestContext) -> String {
        switch request {
        case .start(let request):
            "Starting \(request.name)"
        case .teardown(let request):
            "Tearing down \(request.feature.name)"
        case .prune(let selection):
            selection.rows.count == 1 ? "Pruning 1 feature" : "Pruning \(selection.rows.count) features"
        case .exec(let request):
            "Running \(request.command.joined(separator: " ")) in \(request.feature.name)"
        case .devcontainer(.up(let removeExisting, let buildNoCache), let feature):
            removeExisting && buildNoCache
                ? "Rebuilding the dev container for \(feature.name)"
                : "Starting the dev container for \(feature.name)"
        case .devcontainer(.down, let feature):
            "Stopping the dev container for \(feature.name)"
        case .devcontainer(.build, let feature):
            "Building the dev container for \(feature.name)"
        case .syncDevcontainers(let request):
            "Updating workspaces in \(projectName(request.project))"
        case .tunnelOpen(let feature):
            "Opening a tunnel for \(feature.name)"
        case .tunnelRemove(let feature, _):
            "Removing the tunnel for \(feature.name)"
        case .initProject(let request):
            switch request.mode {
            case .initialize: "Setting up BranchBox in \(request.folder.lastPathComponent)"
            case .update: "Repairing BranchBox in \(request.folder.lastPathComponent)"
            case .validate: "Checking BranchBox in \(request.folder.lastPathComponent)"
            }
        case .applyConfig(_, let project):
            "Saving settings for \(projectName(project))"
        case .tunnelCredentials(_, let project):
            "Saving tunnel credentials for \(projectName(project))"
        case .deleteBranch(let branch, _, _):
            "Deleting branch \(branch)"
        case .removeStray(let stray, _, _):
            "Removing worktree \(URL(fileURLWithPath: stray.path).lastPathComponent)"
        }
    }

    // MARK: Running

    /// What an operation ended with.
    struct Outcome: Sendable {
        var state: OperationState
        var result: OperationResult?
        var warnings: [String] = []
        var stepProgress: StepProgress?
    }

    /// Runs a record that admission let through.
    private func run(_ record: OperationRecord) {
        let backend: any BranchBoxBackend
        do {
            backend = try environment.currentBackend()               // it may have changed while the record waited
        } catch {
            complete(record, with: Outcome(state: .failed(BackendError.normalize(error))))
            return
        }
        let batcher = EventBatcher(interval: flushInterval) { [weak record] events in record?.apply(events) }
        let (stream, continuation) = AsyncStream.makeStream(of: ProgressEvent.self, bufferingPolicy: .unbounded)
        // The single consumer: events reach the record in the order the backend sent them.
        let consumer = Task { @MainActor in
            for await event in stream { batcher.add(event) }
        }
        let sink: ProgressSink = { event in continuation.yield(event) }
        let request = record.context
        record.task = Task { @MainActor [weak self] in
            let outcome = await Self.execute(request, on: backend, progress: sink)
            continuation.finish()
            await consumer.value
            batcher.flush()
            self?.complete(record, with: outcome)
        }
    }

    private func complete(_ record: OperationRecord, with outcome: Outcome) {
        record.addWarnings(outcome.warnings)
        if let progress = outcome.stepProgress { record.setStepProgress(progress) }
        var state = outcome.state
        if state == .succeeded, !record.warnings.isEmpty { state = .succeededWithWarnings }
        if case .cancelled(nil) = state { state = .cancelled(note: cancellationNote(for: record.kind)) }
        // A prune stopped while a row's teardown was running: that feature may be partly removed (D-18).
        if case .cancelled(let note) = state, case .prune(let prune)? = outcome.result,
           prune.rows.contains(where: { if case .cancelled = $0.outcome { true } else { false } }),
           let partial = cancellationNote(for: record.kind) {
            state = .cancelled(note: [note, partial].compactMap { $0 }.joined(separator: ". "))
        }
        let finishedAt = clock.now()
        record.finish(state, result: outcome.result, at: finishedAt)
        record.archive?.close(footer: Self.footer(for: record))
        operations.finished(record)
        refresh(after: record)
        notifyIfDue(record)
    }

    /// Refreshes the project after every operation (success, failure or cancellation). A finished `init` adds the
    /// project when needed and starts its registry watcher.
    private func refresh(after record: OperationRecord) {
        // While quitting, nothing new is spawned: the refresh would race `terminateAllProcesses`.
        guard !environment.isTerminating else { return }
        if case .initProject(let request) = record.context, !request.dryRun, request.mode != .validate,
           case .initProject(let report)? = record.result {
            let projects = self.projects
            Task { await projects.didInitialize(folder: request.folder, workspacePath: report.workspacePath) }
            return
        }
        guard let project = record.project else {
            projects.refreshAll(.afterOperation)
            return
        }
        projects.project(project)?.requestRefresh(.afterOperation)
    }

    /// Without `registry-lock` and `write-ahead-start`, an interrupted start or teardown leaves no trace in the
    /// registry, so the record says what may be left behind (D-18).
    private func cancellationNote(for kind: OperationKind) -> String? {
        guard !(environment.supports(.registryLock) && environment.supports(.writeAheadStart)) else { return nil }
        switch kind {
        case .start: return "Stopped while starting; a partial worktree may be left behind"
        case .teardown, .prune: return "Stopped while tearing down; a feature may be partly removed"
        default: return nil
        }
    }

    // MARK: Backend calls (off the main actor)

    nonisolated static func execute(_ request: OperationRequestContext, on backend: any BranchBoxBackend,
                                    progress: @escaping ProgressSink) async -> Outcome {
        do {
            let result: OperationResult
            switch request {
            case .start(let request):
                result = .start(try await backend.startFeature(request, progress: progress))
            case .teardown(let request):
                result = .teardown(try await backend.teardownFeature(request, progress: progress))
            case .prune(let selection):
                return await prune(selection, on: backend, progress: progress)
            case .exec(let request):
                result = .exec(try await backend.exec(request, progress: progress))
            case .devcontainer(let action, let feature):
                result = .devcontainer(try await backend.devcontainer(action, for: feature, progress: progress))
            case .syncDevcontainers(let request):
                result = .sync(try await backend.syncDevcontainers(request, progress: progress))
            case .tunnelOpen(let feature):
                result = .tunnel(try await backend.openTunnel(feature, progress: progress))
            case .tunnelRemove(let feature, let force):
                result = .tunnel(try await backend.removeTunnel(feature, force: force, progress: progress))
            case .initProject(let request):
                result = .initProject(try await backend.initProject(request, progress: progress))
            case .applyConfig(let patch, let project):
                result = .config(try await backend.applyConfig(patch, to: project, dryRun: false))
            case .tunnelCredentials(let request, let project):
                result = .credentials(try await backend.setTunnelCredentials(request, in: project))
            case .deleteBranch(let branch, let project, let force):
                try await backend.deleteBranch(branch, in: project, force: force)
                result = .message("Deleted branch \(branch)")
            case .removeStray(let stray, let project, let discardChanges):
                try await backend.removeStray(stray, in: project, discardChanges: discardChanges)
                result = .message("Removed the worktree at \(stray.path)")
            }
            return outcome(for: result)
        } catch {
            return outcome(for: BackendError.normalize(error))
        }
    }

    /// Prune (D-12): the selected rows' teardowns, one after another, exactly as selected. A refused or failed
    /// row is recorded and skipped; cancellation stops the row in flight and starts no further row.
    nonisolated static func prune(_ selection: PruneSelection, on backend: any BranchBoxBackend,
                                  progress: @escaping ProgressSink) async -> Outcome {
        let total = selection.rows.count
        var rows: [PruneRow] = []
        var stopped = false
        for (offset, request) in selection.rows.enumerated() {
            let name = request.feature.name
            if stopped || Task.isCancelled {
                stopped = true
                rows.append(PruneRow(feature: name, outcome: .skipped("Not started: the prune was stopped")))
                continue
            }
            progress(.phase(.item(index: offset + 1, of: total, name: name)))
            progress(.log(appLine("Tearing down \(name) (\(offset + 1) of \(total))")))
            do {
                let outcome = try await backend.teardownFeature(request, progress: progress)
                rows.append(PruneRow(feature: name, outcome: .removed(outcome)))
            } catch {
                let error = BackendError.normalize(error)
                switch error {
                case .cancelled:
                    rows.append(PruneRow(feature: name, outcome: .cancelled))
                    stopped = true
                case .refused(let refusal):
                    rows.append(PruneRow(feature: name, outcome: .refused(error)))
                    progress(.log(appLine("Skipped \(name): \(refusal.message)", level: .warn)))
                default:
                    rows.append(PruneRow(feature: name, outcome: .failed(error)))
                    progress(.log(appLine("\(name) failed: \(error.briefSummary)", level: .error)))
                }
            }
        }
        let result = PruneResult(rows: rows)
        let finished = rows.filter { row in
            switch row.outcome {
            case .removed, .refused, .failed: true
            case .skipped, .cancelled: false
            }
        }.count
        if stopped {
            return Outcome(state: .cancelled(note: "Stopped after \(finished) of \(total) features"), result: .prune(result),
                           stepProgress: StepProgress(completed: finished, total: total))
        }
        var outcome = outcome(for: .prune(result))
        outcome.stepProgress = StepProgress(completed: total, total: total)
        return outcome
    }

    /// Maps a typed result to the record's state: `.partial` when part of it failed (a branch that could not be
    /// deleted, prune rows that were refused or failed, sync rows that failed), `.succeededWithWarnings` when it
    /// carries warnings, else `.succeeded`.
    nonisolated static func outcome(for result: OperationResult) -> Outcome {
        var warnings: [String] = []
        var partial = false
        switch result {
        case .start(let summary):
            warnings = summary.warnings + (summary.adapter?.warnings ?? [])
            if let preamble = summary.preambleWarning { warnings.append(preamble) }
            warnings += summary.moduleOutcomes.filter { $0.status == .failed }.map { module in
                module.notes.isEmpty ? "\(module.module) failed" : "\(module.module) failed: \(module.notes.joined(separator: "; "))"
            }
        case .teardown(let outcome):
            warnings = teardownWarnings(outcome)
            if case .deleteFailed = outcome.branch { partial = true }
        case .prune(let prune):
            for row in prune.rows {
                switch row.outcome {
                case .removed(let outcome):
                    if case .deleteFailed = outcome.branch { partial = true }
                    warnings += teardownWarnings(outcome).map { "\(row.feature): \($0)" }
                case .refused, .failed:
                    partial = true
                case .skipped, .cancelled:
                    break
                }
            }
        case .exec(let result):
            if result.exitCode != 0 { warnings.append("The command exited with status \(result.exitCode)") }
        case .sync(let report):
            if report.failedCount > 0 { partial = true }
        case .tunnel(let change):
            warnings = change.warnings
        case .initProject(let report):
            warnings = report.warnings
        case .devcontainer, .config, .credentials, .message:
            break
        }
        var seen = Set<String>()
        warnings = warnings.filter { seen.insert($0).inserted }
        let state: OperationState = partial ? .partial : (warnings.isEmpty ? .succeeded : .succeededWithWarnings)
        return Outcome(state: state, result: result, warnings: warnings)
    }

    nonisolated static func outcome(for error: BackendError) -> Outcome {
        if case .cancelled(let note) = error { return Outcome(state: .cancelled(note: note)) }
        return Outcome(state: .failed(error))
    }

    nonisolated static func teardownWarnings(_ outcome: TeardownOutcome) -> [String] {
        let summary = outcome.summary
        var warnings = summary.warnings + summary.adapterCleanupWarnings
        warnings += summary.moduleReports.filter { !$0.teardownOk }.map { report in
            report.errors.isEmpty ? "\(report.name) cleanup failed" : "\(report.name) cleanup failed: \(report.errors.joined(separator: "; "))"
        }
        if let runtime = summary.runtimeTeardown, !runtime.residueFree {
            let items = runtime.residue.flatMap(\.identifiers)
            warnings.append(items.isEmpty ? "The runtime cleanup could not be verified"
                                          : "The runtime cleanup left \(items.joined(separator: ", "))")
        }
        if !outcome.worktreeGone { warnings.append("The worktree folder is still on disk") }
        return warnings
    }

    nonisolated private static func appLine(_ message: String, level: LogLevel = .info) -> LogLine {
        LogLine(timestamp: Date(), level: level, source: .app, target: nil, message: message)
    }

    // MARK: Log archive

    private func attachArchive(to record: OperationRecord, commandLine: String?) {
        guard let logsDirectory else { return }
        let subject: String
        let targetLine: String
        switch record.target {
        case .feature(let feature):
            subject = feature.name
            targetLine = "\(feature.project.path) › \(feature.name)"
        case .project(let project):
            subject = projectName(project)
            targetLine = project.path
        case .global:
            subject = "global"
            targetLine = "global"
        }
        var header = ["# \(record.title)", "# Kind: \(record.kind.rawValue)", "# Target: \(targetLine)",
                      "# Started: \(RFC3339.format(record.startedAt))"]
        if let identity = environment.identity { header.append("# BranchBox CLI \(identity.version)") }
        if let commandLine { header.append("# Command: \(commandLine)") }
        let archive = LogArchive(directory: logsDirectory,
                                 fileName: LogArchive.fileName(startedAt: record.startedAt, kind: record.kind,
                                                               subject: subject, id: record.id),
                                 header: header, secrets: record.secrets, retention: settings.logRetention)
        record.archive = archive
        record.log.setArchiveURL(archive?.url)
    }

    /// What a request's log, warnings and history must never show: the extra-environment values and its token.
    private func secrets(for request: OperationRequestContext) -> [String] {
        var secrets = Array(settings.extraEnvironment.values)
        if case .tunnelCredentials(let credentials, _) = request, let token = credentials.apiToken?.value {
            secrets.append(token)
        }
        return secrets
    }

    private static func footer(for record: OperationRecord) -> [String] {
        let finished = record.finishedAt.map(RFC3339.format) ?? "?"
        var footer = ["# Finished: \(finished) (\(stateLabel(record.state)))"]
        switch record.state {
        case .failed(let error): footer.append("# \(error.briefSummary)")
        case .cancelled(let note?): footer.append("# \(note)")
        default: break
        }
        footer += record.warnings.map { "# Warning: \($0)" }
        return footer
    }

    static func stateLabel(_ state: OperationState) -> String {
        switch state {
        case .queued: "queued"
        case .running: "running"
        case .succeeded: "succeeded"
        case .succeededWithWarnings: "succeeded with warnings"
        case .partial: "partly succeeded"
        case .failed: "failed"
        case .cancelled: "cancelled"
        }
    }

    // MARK: Notifications (§11)

    /// Posts when the user is not looking at BranchBox, notifications are on, and the operation failed or took
    /// longer than the threshold (only problems, if Settings say so). Cancellations never notify: the user
    /// asked for them.
    private func notifyIfDue(_ record: OperationRecord) {
        guard notifier.isAvailable, settings.notificationsEnabled, !isAppActive() else { return }
        let failed: Bool
        switch record.state {
        case .failed, .partial: failed = true
        case .succeeded, .succeededWithWarnings: failed = false
        case .queued, .running, .cancelled: return
        }
        let began = record.runningSince ?? record.startedAt
        let duration = (record.finishedAt ?? clock.now()).timeIntervalSince(began)
        guard failed || duration > notificationThreshold.seconds else { return }
        if settings.notifyOnlyOnProblems, !failed, record.state != .succeededWithWarnings { return }
        let note = note(for: record)
        let notifier = self.notifier
        Task {
            guard await notifier.requestAuthorizationIfNeeded() else { return }
            await notifier.post(note)
        }
    }

    func note(for record: OperationRecord) -> UserNote {
        let name: String = switch record.target {
        case .feature(let feature): feature.name
        case .project(let project): projectName(project)
        case .global: "BranchBox"
        }
        let problem: String? = switch record.state {
        case .failed(let error): error.briefSummary
        case .partial, .succeededWithWarnings: record.warnings.first
        case .queued, .running, .succeeded, .cancelled: nil
        }
        let title: String
        let body: String
        switch (record.kind, record.state) {
        case (.start, .succeeded):
            title = "\(name) is ready"
            body = "Started successfully"
        case (.start, .succeededWithWarnings):
            title = "\(name) started with problems"
            body = problem ?? "See the operation for details"
        case (.start, _):
            title = "Couldn't start \(name)"
            body = problem ?? "See the operation for details"
        case (.teardown, .succeeded), (.teardown, .succeededWithWarnings):
            title = "\(name) torn down"
            body = problem ?? "Removed successfully"
        case (.teardown, _):
            title = "Teardown of \(name) needs attention"
            body = problem ?? "See the operation for details"
        default:
            title = record.title
            switch record.state {
            case .failed: body = "Failed: \(problem ?? "see the operation for details")"
            case .partial: body = "Finished with problems" + (problem.map { ": \($0)" } ?? "")
            case .succeededWithWarnings: body = "Finished with warnings" + (problem.map { ": \($0)" } ?? "")
            default: body = "Finished"
            }
        }
        return UserNote(title: title, body: body, intent: .showActivity(operation: record.id),
                        threadID: record.project?.path ?? "global")
    }

    private func projectName(_ project: ProjectRef) -> String {
        projects.project(project)?.displayName ?? ProjectEntry.defaultDisplayName(for: project.path)
    }
}
