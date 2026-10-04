import BranchBoxKit
import Foundation

extension CLIBackend {
    /// The CLI's `--dry-run` plan with `teardown-plan`, else the app preflight (`source == .appPreflight`).
    public func planTeardown(_ request: TeardownRequest) async throws -> TeardownPlanDocument {
        if supports(.teardownPlan) {
            let output = try await cli(CLICommand.teardown(request, mode: teardownMode, dryRun: true),
                                       operation: "feature teardown --dry-run", purpose: .read,
                                       project: request.feature.project, timeout: .seconds(30))
            var plan = try output.decode(TeardownPlanDocument.self, what: "teardown plan").value
            plan.source = .cli
            return plan
        }
        return try await appPreflight(request)
    }

    func appPreflight(_ request: TeardownRequest) async throws -> TeardownPlanDocument {
        let preflight = TeardownPreflight(git: await git(.read), record: try await record(for: request.feature),
                                          config: LegacyConfig.effective(root: request.feature.project.path),
                                          fileSystem: fileSystem, cliVersion: cliVersion)
        return try await preflight.plan(for: request)
    }

    /// DESIGN §6.5, the safety core:
    ///
    /// 1. Re-plan immediately before spawning.
    /// 2. User changes and no consent → `.refused(.uncommittedChanges)`; nothing is spawned.
    /// 3. Consent must name every current user path (race protection).
    /// 4. Delete-if-merged of an unmerged branch → `.refused(.unmergedBranch)`.
    /// 5. Contract mode: the explicit branch flag, `--discard-changes` iff consent, `--complete-spec`.
    /// 6. Legacy mode: always `--keep-branch`; `--force` with consent, forced removal, or when the re-plan found only
    ///    BranchBox-generated (or preserved) files; then the app deletes the branch (`git branch -d`/`-D`); a failure
    ///    there is `BranchOutcome.deleteFailed`, not an error.
    /// 7. Verify on disk that the worktree is gone.
    /// 8. Surface 0.13.x's manual-removal warning.
    ///
    /// forceRemoval is honoured only for a missing worktree, a locked one, an unreadable status, or changes the
    /// consent covers. A 0.13.x dirty-module refusal is re-preflighted so recoveries know the user changes.
    public func teardownFeature(_ request: TeardownRequest,
                                progress: @escaping ProgressSink) async throws -> TeardownOutcome {
        let relay = ProgressRelay(progress, activity: .teardown)
        relay.phase(.preparing)
        let plan = try await planTeardown(request)
        if let refusal = LegacyTeardown.refusal(for: request, plan: plan, cliVersion: cliVersion) {
            throw BackendError.refused(refusal)
        }

        let mode = teardownMode
        let onlyGenerated = mode == .legacy && LegacyTeardown.onlyGeneratedChanges(plan)
        let output = try await cli(CLICommand.teardown(request, mode: mode, onlyGeneratedChanges: onlyGenerated),
                                   operation: "feature teardown",
                                   purpose: .mutation, project: request.feature.project,
                                   workingDirectory: request.feature.project.root, timeout: nil, relay: relay,
                                   cancelNote: cancelNote)
        let summary: TeardownSummary
        do {
            let decoded = try output.decode(TeardownSummary.self, what: "teardown summary")
            if let preamble = decoded.preamble { relay.warning(preamble) }
            summary = Self.teardownSummary(decoded.value, capabilities: currentIdentity.capabilities)
        } catch BackendError.refused(let refusal) {
            throw try await enriched(refusal, request: request)
        }

        let branch: BranchOutcome
        if mode == .contract, !request.forceRemoval {
            branch = Self.cliBranchOutcome(summary, request: request, plan: plan)
        } else {
            branch = await appBranchStep(request, plan: plan, relay: relay)
        }

        for warning in summary.warnings where warning.contains(LegacyTeardown.manualRemovalWarning) {
            relay.warning("The CLI deleted the worktree folder by hand after `git worktree remove` failed: \(warning)")
        }
        let worktreePath = plan.worktree.path
        return TeardownOutcome(summary: summary, branch: branch,
                               worktreeGone: fileSystem.kind(at: worktreePath) == .missing)
    }

    /// Older container providers reported a clean runtime without inspecting standalone devcontainers.
    /// Use the execution's backend identity so a later CLI switch cannot upgrade that receipt's evidence.
    static func teardownSummary(_ summary: TeardownSummary, capabilities: Set<Capability>) -> TeardownSummary {
        guard let runtime = summary.runtimeTeardown, runtime.provider.map(RuntimeProvider.init(raw:)) == .container,
              runtime.verified, !capabilities.contains(.hostContainerTeardownVerified) else { return summary }
        let unverified = RuntimeTeardownReport(provider: runtime.provider, runtimeID: runtime.runtimeID,
                                              verified: false, residueFree: runtime.residueFree, residue: runtime.residue)
        return TeardownSummary(workFeature: summary.workFeature, branchName: summary.branchName,
                               worktreeRemoved: summary.worktreeRemoved, branchDeleted: summary.branchDeleted,
                               adapterCleanupWarnings: summary.adapterCleanupWarnings, moduleReports: summary.moduleReports,
                               runtimeTeardown: unverified,
                               warnings: summary.warnings + ["This CLI cannot verify feature container cleanup. Check Docker for remaining resources; update to a CLI with host-container-teardown-verified."],
                               branchAction: summary.branchAction, branchDeleteError: summary.branchDeleteError,
                               discardedChanges: summary.discardedChanges, preserved: summary.preserved,
                               registryUpdated: summary.registryUpdated)
    }

    /// A 0.13.x "Devcontainer/compose changes detected" refusal gets the re-preflight's user changes and plan, so
    /// the recovery can offer "Discard BranchBox-generated files" only when there are none.
    private func enriched(_ refusal: Refusal, request: TeardownRequest) async throws -> BackendError {
        guard case .moduleFilesDirty(let files, _) = refusal.cause else { return .refused(refusal) }
        let plan = try await appPreflight(request)
        return .refused(Refusal(cause: .moduleFilesDirty(files: files, userChanges: plan.changes.user),
                                message: refusal.message, diagnostics: refusal.diagnostics, plan: plan))
    }

    /// The branch as the contract CLI reports it.
    static func cliBranchOutcome(_ summary: TeardownSummary, request: TeardownRequest,
                                 plan: TeardownPlanDocument) -> BranchOutcome {
        let name = summary.branchName ?? request.recordedBranch ?? plan.branch?.name ?? ""
        if summary.branchDeleted { return .deleted(name, by: .cli) }
        if let error = summary.branchDeleteError, !error.isEmpty { return .deleteFailed(name, reason: error) }
        if request.branch != .keep, plan.branch?.exists == false { return .notFound(name) }
        return .kept(name.isEmpty ? nil : name)
    }

    /// Legacy step 6 (and forced removal in contract mode): the CLI kept the branch, the app deletes it per policy.
    private func appBranchStep(_ request: TeardownRequest, plan: TeardownPlanDocument,
                               relay: ProgressRelay) async -> BranchOutcome {
        let name = request.recordedBranch.flatMap { $0.isEmpty ? nil : $0 } ?? plan.branch?.name
        guard let flag = LegacyTeardown.branchDeletionFlag(for: request.branch), let name else {
            return .kept(name)
        }
        relay.phase(.deletingBranch)
        let git = await git(.mutation)
        let root = request.feature.project.path
        do {
            guard try await git.branchExists(name, in: root) else { return .notFound(name) }
            try await git.deleteBranch(name, in: root, force: flag == "-D")
            relay.note("Deleted branch \(name) (git branch \(flag))")
            return .deleted(name, by: .app)
        } catch {
            let reason: String
            switch BackendError.normalize(error) {
            case .cancelled:
                // The worktree is already gone; only the branch step was stopped, so the branch is kept.
                relay.note("Kept branch \(name): the teardown was stopped before the branch was deleted", level: .warn)
                return .kept(name)
            case .refused(let refusal): reason = refusal.diagnostics.summary
            case let other: reason = TeardownPreflight.summary(of: other)
            }
            relay.note("Could not delete branch \(name): \(reason)", level: .warn)
            return .deleteFailed(name, reason: reason)
        }
    }
}
