import BranchBoxKit
import Foundation

extension CLIBackend {
    // MARK: - Start

    /// `feature start` with cwd = the project root. A 0.13.x preamble line before the JSON becomes a warning and
    /// `StartSummary.preambleWarning`; `verbose` adds `--telemetry` and `RUST_LOG=debug` for this run.
    public func startFeature(_ request: StartFeatureRequest,
                             progress: @escaping ProgressSink) async throws -> StartSummary {
        let relay = ProgressRelay(progress, activity: .start)
        relay.phase(.preparing)
        let output = try await cli(CLICommand.start(request), operation: "feature start", purpose: .mutation,
                                   project: request.project, workingDirectory: request.project.root, timeout: nil,
                                   // git worktree add can be stopped before even a write-ahead CLI registers it.
                                   relay: relay, cancelNote: Self.partialWorktreeNote,
                                   environmentOverrides: request.verbose ? ["RUST_LOG": "debug"] : [:])
        var (summary, preamble) = try output.decode(StartSummary.self, what: "start summary")
        if let preamble {
            summary.preambleWarning = preamble
            relay.warning(preamble)
        }
        return summary
    }

    // MARK: - Exec

    /// The payload is read on any exit status: a failing inner command is data (`exitCode`), not an error.
    public func exec(_ request: ExecRequest, progress: @escaping ProgressSink) async throws -> ExecResult {
        let relay = ProgressRelay(progress, activity: .other)
        let worktree: String
        let workingDirectory: URL
        switch request.target {
        case .featureRuntime:
            worktree = request.feature.project.path
            workingDirectory = request.feature.project.root
        case .devcontainer:
            worktree = try await worktreePath(for: request.feature)
            workingDirectory = URL(fileURLWithPath: worktree, isDirectory: true)
        }
        let output = try await cli(CLICommand.exec(request, worktree: worktree),
                                   operation: request.target == .devcontainer ? "devcontainer exec" : "feature exec",
                                   purpose: .mutation, project: request.feature.project,
                                   workingDirectory: workingDirectory, timeout: request.timeout, relay: relay)
        return try output.decodeInBand(ExecResult.self, what: "exec result")
    }

    // MARK: - Devcontainer

    /// cwd = the worktree. The payload is decoded even on exit 1; `outcome: "error"` (Docker down prints it on
    /// stdout with an empty stderr) becomes `.commandFailed(message)`.
    public func devcontainer(_ action: DevcontainerAction, for feature: FeatureRef,
                             progress: @escaping ProgressSink) async throws -> DevcontainerResult {
        let relay = ProgressRelay(progress, activity: .other)
        let worktree = try await worktreePath(for: feature)
        let operation: String
        switch action {
        case .up:
            operation = "devcontainer up"
            relay.phase(.startingEnvironment)
        case .down:
            operation = "devcontainer down"
            relay.phase(.cleaningRuntime)
        case .build:
            operation = "devcontainer build"
            relay.phase(.building)
        }
        let output = try await cli(CLICommand.devcontainer(action, worktree: worktree), operation: operation,
                                   purpose: .mutation, project: feature.project,
                                   workingDirectory: URL(fileURLWithPath: worktree, isDirectory: true), timeout: nil,
                                   relay: relay)
        let result = try output.decodeInBand(DevcontainerResult.self, what: "\(operation) result",
                                             accept: \.isRecognized)
        if result.isError {
            throw BackendError.commandFailed(output.diagnostics(summary: result.message ?? "\(operation) failed"))
        }
        return result
    }

    // MARK: - Sync

    /// `devcontainer sync --json` with `devcontainer-sync-json` (a failed row exits 1 and still prints the report);
    /// otherwise the streamed text report, where a failed row is failed even with exit 0 (DRIFT-09).
    public func syncDevcontainers(_ request: SyncRequest, progress: @escaping ProgressSink) async throws -> SyncReport {
        let relay = ProgressRelay(progress, activity: .other)
        if supports(.devcontainerSyncJSON) {
            let output = try await cli(CLICommand.syncDevcontainers(request, json: true), operation: "devcontainer sync",
                                       purpose: .mutation, project: request.project,
                                       workingDirectory: request.project.root, timeout: nil, relay: relay)
            return try output.decodeInBand(SyncPayload.self, what: "sync report").report
        }
        guard request.features.isEmpty else {
            throw BackendError.unsupported(.devcontainerSyncJSON, minimumCLI: "0.14.0")
        }
        let output = try await cli(CLICommand.syncDevcontainers(request, json: false), operation: "devcontainer sync",
                                   purpose: .mutation, project: request.project,
                                   workingDirectory: request.project.root, timeout: nil, streamStdout: true,
                                   relay: relay)
        let report = TextParsers.syncReport(output.stdoutText, dryRun: request.dryRun,
                                            strategy: request.strategy?.rawValue)
        if !output.succeeded, report.failedCount == 0 { throw output.failure() }
        return report
    }

    // MARK: - Tunnels

    public func openTunnel(_ feature: FeatureRef, progress: @escaping ProgressSink) async throws -> TunnelChange {
        try await tunnel(CLICommand.tunnelOpen(feature), operation: "tunnel open", feature: feature, progress: progress)
    }

    public func removeTunnel(_ feature: FeatureRef, force: Bool,
                             progress: @escaping ProgressSink) async throws -> TunnelChange {
        try await tunnel(CLICommand.tunnelRemove(feature, force: force), operation: "tunnel remove", feature: feature,
                         progress: progress)
    }

    private func tunnel(_ arguments: [String], operation: String, feature: FeatureRef,
                        progress: @escaping ProgressSink) async throws -> TunnelChange {
        let relay = ProgressRelay(progress, activity: .other)
        relay.phase(.provisioningTunnel)
        return try await cli(arguments, operation: operation, purpose: .mutation, project: feature.project,
                             workingDirectory: feature.project.root, timeout: .seconds(180), relay: relay)
            .decode(TunnelChange.self, what: "\(operation) result").value
    }

    // MARK: - Init

    /// cwd = the folder; always `-y`. With `init-json` the report is decoded and `tunnelsEnabled` is applied with
    /// `config apply`; on legacy CLIs stdout is streamed as the log. Either way the workspace is re-resolved
    /// (`<folder>/main` after a reorganize).
    public func initProject(_ request: InitRequest, progress: @escaping ProgressSink) async throws -> InitReport {
        let relay = ProgressRelay(progress, activity: .other)
        relay.phase(.preparing)
        let json = supports(.initJSON)
        let output = try await cli(CLICommand.initProject(request, json: json), operation: "init", purpose: .mutation,
                                   workingDirectory: request.folder, timeout: nil, streamStdout: !json, relay: relay,
                                   cancelNote: cancelNote)
        if json {
            let (payload, preamble) = try output.decode(InitPayload.self, what: "init report")
            var warnings = payload.warnings
            if let preamble {
                warnings.append(preamble)
                relay.warning(preamble)
            }
            let workspace = (try? await resolveProject(at: URL(fileURLWithPath: payload.workspacePath)))?.project
                ?? ProjectRef(root: URL(fileURLWithPath: payload.workspacePath))
            if let tunnels = request.tunnelsEnabled, !request.dryRun {
                if let warning = await applyTunnelSetting(tunnels, to: workspace) { warnings.append(warning) }
            }
            return InitReport(workspacePath: workspace.path, reorganized: payload.reorganized, stack: payload.stack,
                              adapter: payload.adapter, modules: payload.modules, warnings: warnings,
                              nextSteps: payload.nextSteps, onePasswordStatus: payload.onePasswordStatus)
        }
        guard output.succeeded else { throw output.failure() }
        let folder = request.folder.standardizedFileURL.path
        let container = Paths.join(folder, "main")
        let reorganized = request.reorganize && fileSystem.kind(at: Paths.join(container, ".git")) != .missing
        let candidate = reorganized ? container : folder
        let workspace = (try? await resolveProject(at: URL(fileURLWithPath: candidate)))?.project.path ?? candidate
        var warnings: [String] = []
        if request.tunnelsEnabled != nil {
            warnings.append("The tunnel setting was not applied: it needs branchbox 0.14 (found \(cliVersion))")
        }
        return InitReport(workspacePath: workspace, reorganized: reorganized, warnings: warnings,
                          log: output.stdoutText.split(whereSeparator: \.isNewline).map(String.init))
    }

    /// `config apply` of `tunnel.enabled` after init; the warning to show when it could not be applied.
    private func applyTunnelSetting(_ enabled: Bool, to project: ProjectRef) async -> String? {
        guard supports(.config) else {
            return "The tunnel setting was not applied: it needs a CLI with `config` (found \(cliVersion))"
        }
        do {
            _ = try await applyConfig(ConfigPatch(changes: [ConfigChange(key: "tunnel.enabled", value: .bool(enabled))]),
                                      to: project, dryRun: false)
            return nil
        } catch {
            return "The tunnel setting was not applied: \(TeardownPreflight.summary(of: BackendError.normalize(error)))"
        }
    }

    // MARK: - Branches and strays

    public func deleteBranch(_ branch: String, in project: ProjectRef, force: Bool) async throws {
        try await git(.mutation).deleteBranch(branch, in: project.path, force: force)
    }

    /// Rechecks the stray and refuses user paths not covered by consent before `git worktree remove [--force]`.
    public func removeStray(_ stray: StrayWorktree, in project: ProjectRef, discard: DiscardConsent?) async throws {
        let git = await git(.mutation)
        let exists = fileSystem.kind(at: stray.path) == .directory
        if exists {
            let changes = try await git.status(of: stray.path).map(\.changedFile)
            let uncovered = LegacyTeardown.uncovered(changes, by: discard)
            if !uncovered.isEmpty {
                let message = "Refusing to remove \(stray.path): \(LegacyTeardown.count(uncovered.count, "uncommitted change")) would be lost (\(LegacyTeardown.list(uncovered.map(\.path)))); nothing was removed"
                throw BackendError.refused(Refusal(cause: .uncommittedChanges(files: uncovered), message: message,
                                                   diagnostics: Diagnostics(summary: message)))
            }
        }
        // A worktree whose folder is already gone only has git's bookkeeping left, which `--force` clears.
        try await git.removeWorktree(stray.path, in: project.path, force: discard != nil || !exists)
    }

    // MARK: - Copy as Command

    public func previewCommandLine(_ request: OperationRequestContext) -> String? {
        let cli = executable.path
        let render = { (argv: [String]) in self.redaction.render(argv) }
        switch request {
        case .start(let start):
            return render([cli] + CLICommand.start(start))
        case .teardown(let teardown):
            return teardownCommandLine(teardown)
        case .prune(let selection):
            let lines = selection.rows.map(teardownCommandLine)
            return lines.isEmpty ? nil : lines.joined(separator: "\n")
        case .exec(let exec):
            return render([cli] + CLICommand.exec(exec, worktree: Self.layoutPath(for: exec.feature)))
        case .devcontainer(let action, let feature):
            return render([cli] + CLICommand.devcontainer(action, worktree: Self.layoutPath(for: feature)))
        case .syncDevcontainers(let sync):
            return render([cli] + CLICommand.syncDevcontainers(sync, json: supports(.devcontainerSyncJSON)))
        case .tunnelOpen(let feature):
            return render([cli] + CLICommand.tunnelOpen(feature))
        case .tunnelRemove(let feature, let force):
            return render([cli] + CLICommand.tunnelRemove(feature, force: force))
        case .initProject(let initRequest):
            return "cd \(RedactedCommandLine.quote(initRequest.folder.standardizedFileURL.path)) && "
                + render([cli] + CLICommand.initProject(initRequest, json: supports(.initJSON)))
        case .applyConfig(let patch, let project):
            let body = (try? Self.encode(LegacyConfig.mergePatch(patch))).map { String(decoding: $0, as: UTF8.self) } ?? "{}"
            return "printf '%s' \(RedactedCommandLine.quote(body)) | "
                + render([cli] + CLICommand.configApply(repo: project.path, dryRun: false))
        case .tunnelCredentials(let credentials, let project):
            return render([cli] + CLICommand.tunnelCredentials(credentials, repo: project.path))
        case .deleteBranch(let branch, let project, let force):
            return render(["git", "-C", project.path, "branch", force ? "-D" : "-d", branch])
        case .removeStray(let stray, let project, let discard):
            return render(["git", "-C", project.path, "worktree", "remove"] + (discard != nil ? ["--force"] : []) + [stray.path])
        }
    }

    /// The CLI teardown, followed in legacy mode by the app's own branch step.
    private func teardownCommandLine(_ request: TeardownRequest) -> String {
        let mode = teardownMode
        var line = redaction.render([executable.path] + CLICommand.teardown(request, mode: mode))
        let appBranchStep = mode == .legacy || request.forceRemoval
        if appBranchStep, let flag = LegacyTeardown.branchDeletionFlag(for: request.branch),
           let branch = request.recordedBranch, !branch.isEmpty {
            line += " && " + redaction.render(["git", "-C", request.feature.project.path, "branch", flag, branch])
        }
        return line
    }
}
