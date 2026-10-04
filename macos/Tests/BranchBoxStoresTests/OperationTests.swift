import BranchBoxKit
import BranchBoxPreview
@testable import BranchBoxStores
import Foundation
import Testing

// OperationStore + ActionDispatcher (§8.3, D-16, D-18): admission and queueing, the ordered and batched progress
// stream, result mapping, refresh after every operation, prune, cancellation, logs, history and notifications.

private let prine = feature("prine")
private let remotion = feature("remotion")

@MainActor @Suite(.timeLimit(.minutes(1))) struct OperationTests {
    // MARK: Dispatch and results

    @Test func aTeardownRunsExactlyTheRequestItWasGiven() async throws {
        let harness = Harness()
        defer { harness.tearDown() }
        _ = try await harness.startWithSampleProject()
        var request = TeardownRequest(feature: prine, recordedBranch: "feature/prine", branch: .deleteIfMerged)
        request.completeSpec = true

        let result = harness.model.actions.dispatch(.teardown(request))
        guard case .started(let record) = result else { throw Failure("expected .started, got \(result)") }
        #expect(record.kind == .teardown)
        #expect(record.target == .feature(prine))
        #expect(record.title == "Tearing down prine")
        #expect(record.context == .teardown(request))
        try await waitUntilFinished(record)

        let teardowns = await harness.backend.calls.compactMap(\.teardownRequest)
        #expect(teardowns == [request])
        #expect(teardowns.allSatisfy { $0.discard == nil && !$0.forceRemoval })
        #expect(record.state == .succeeded)
        guard case .teardown(let outcome)? = record.result else { throw Failure("expected a teardown result") }
        #expect(outcome.branch == .deleted("feature/prine", by: .cli))
        #expect(record.finishedAt != nil)
        #expect(harness.model.operations.running.isEmpty)
    }

    @Test func aDirtyTeardownFailsWithTheRefusalAndNoConsentIsAdded() async throws {
        let harness = Harness(.dirtyWorktree)
        defer { harness.tearDown() }
        _ = try await harness.startWithSampleProject()
        let request = TeardownRequest(feature: prine, recordedBranch: "feature/prine", branch: .keep)

        let record = try operation(of: harness.model.actions.dispatch(.teardown(request)))
        try await waitUntilFinished(record)
        guard case .failed(.refused(let refusal)) = record.state else { throw Failure("expected a refusal, got \(record.state)") }
        #expect(refusal.cause == .uncommittedChanges(files: PreviewSamples.dirtyFiles))
        #expect(record.needsAttention)
        record.acknowledge()
        #expect(!record.needsAttention)

        // The UI's recovery carries the consent; the dispatcher passes it on untouched.
        var retry = request
        retry.discard = DiscardConsent(userFiles: PreviewSamples.dirtyFiles.map(\.path), confirmedAt: Date(timeIntervalSince1970: 1))
        let recovery = RecoveryAction.retry(.teardown(retry), label: "Discard 2 changes and tear down", destructive: true,
                                            confirmation: "README.md and notes.txt will be lost")
        let retried = try operation(of: try #require(harness.model.actions.perform(recovery)))
        try await waitUntilFinished(retried)
        #expect(retried.state == .succeeded)
        #expect(await harness.backend.calls.compactMap(\.teardownRequest) == [request, retry])
        #expect(harness.model.actions.perform(.openDoctor) == nil)
        #expect(harness.model.actions.perform(.refresh(sampleProject)) == nil)
    }

    @Test func resultsMapToStates() {
        typealias Dispatcher = ActionDispatcher
        let clean = TeardownOutcome(summary: TeardownSummary(workFeature: "a", worktreeRemoved: true), branch: .kept("feature/a"),
                                    worktreeGone: true)
        let failedDelete = TeardownOutcome(summary: TeardownSummary(workFeature: "b", worktreeRemoved: true),
                                           branch: .deleteFailed("feature/b", reason: "not fully merged"), worktreeGone: true)
        let residue = TeardownOutcome(
            summary: TeardownSummary(workFeature: "c", worktreeRemoved: true,
                                     moduleReports: [ModuleReport(name: "compose", teardownOk: false, errors: ["volume busy"])],
                                     runtimeTeardown: RuntimeTeardownReport(provider: "container", verified: true, residueFree: false,
                                                                            residue: [ResidueItem(kind: "volume", identifiers: ["c_db"])])),
            branch: .kept("feature/c"), worktreeGone: false)

        #expect(Dispatcher.outcome(for: .teardown(clean)).state == .succeeded)
        let unverified = TeardownOutcome(summary: TeardownSummary(workFeature: "a", worktreeRemoved: true,
            runtimeTeardown: RuntimeTeardownReport(provider: "container", verified: false, residueFree: true)),
            branch: .kept("feature/a"), worktreeGone: true)
        #expect(Dispatcher.outcome(for: .teardown(unverified)).state == .succeededWithWarnings)
        #expect(Dispatcher.outcome(for: .teardown(unverified)).warnings == ["The runtime cleanup could not be verified"])
        #expect(Dispatcher.outcome(for: .teardown(failedDelete)).state == .partial)
        let messy = Dispatcher.outcome(for: .teardown(residue))
        #expect(messy.state == .succeededWithWarnings)
        #expect(messy.warnings == ["compose cleanup failed: volume busy", "The runtime cleanup left c_db",
                                   "The worktree folder is still on disk"])

        let start = StartSummary(workFeature: "s", moduleOutcomes: [ModuleOutcome(module: "compose", status: .failed, notes: ["port taken"])],
                                 warnings: ["w1"], adapter: AdapterInfo(warnings: ["w1", "a1"]), preambleWarning: "Prompt truncated")
        let started = Dispatcher.outcome(for: .start(start))
        #expect(started.state == .succeededWithWarnings)
        #expect(started.warnings == ["w1", "a1", "Prompt truncated", "compose failed: port taken"])
        #expect(Dispatcher.outcome(for: .start(StartSummary(workFeature: "s"))).state == .succeeded)

        #expect(Dispatcher.outcome(for: .exec(ExecResult(exitCode: 3))).state == .succeededWithWarnings)
        #expect(Dispatcher.outcome(for: .exec(ExecResult(exitCode: 0))).state == .succeeded)
        let sync = SyncReport(dryRun: false, rows: [SyncReport.Row(feature: "a", status: .synced), SyncReport.Row(feature: "b", status: .failed)])
        #expect(Dispatcher.outcome(for: .sync(sync)).state == .partial)
        #expect(Dispatcher.outcome(for: .tunnel(TunnelChange(workFeature: "t", state: nil, warnings: ["dns slow"]))).state
            == .succeededWithWarnings)
        #expect(Dispatcher.outcome(for: .initProject(InitReport(workspacePath: "/r"))).state == .succeeded)
        #expect(Dispatcher.outcome(for: .message("done")).state == .succeeded)

        let refusal = BackendError.refused(Refusal(cause: .confirmationRequired, message: "m", diagnostics: Diagnostics(summary: "m")))
        let prune = PruneResult(rows: [PruneRow(feature: "a", outcome: .removed(clean)), PruneRow(feature: "b", outcome: .refused(refusal))])
        #expect(Dispatcher.outcome(for: .prune(prune)).state == .partial)
        #expect(Dispatcher.outcome(for: .prune(PruneResult(rows: [PruneRow(feature: "b", outcome: .removed(failedDelete))]))).state == .partial)
        #expect(Dispatcher.outcome(for: .prune(PruneResult(rows: [PruneRow(feature: "a", outcome: .removed(clean))]))).state == .succeeded)

        #expect(Dispatcher.outcome(for: BackendError.cancelled(note: "n")).state == .cancelled(note: "n"))
        #expect(Dispatcher.outcome(for: refusal).state == .failed(refusal))
    }

    @Test func everyRequestKindRunsAndIsTitled() async throws {
        let harness = Harness()
        defer { harness.tearDown() }
        _ = try await harness.startWithSampleProject()
        let actions = harness.model.actions
        let requests: [OperationRequestContext] = [
            .exec(ExecRequest(feature: prine, command: ["ls", "-la"])),
            .devcontainer(.up(removeExisting: true, buildNoCache: true), prine),
            .devcontainer(.down(removeVolumes: false), remotion),
            .tunnelCredentials(TunnelCredentialsRequest(accountID: "acct", apiToken: SecretString("tok-12345")), sampleProject),
        ]
        let expected: [(OperationKind, String)] = [
            (.exec, "Running ls -la in prine"), (.devcontainerRebuild, "Rebuilding the dev container for prine"),
            (.devcontainerDown, "Stopping the dev container for remotion"), (.tunnelCredentials, "Saving tunnel credentials for branchbox"),
        ]
        for (request, (kind, title)) in zip(requests, expected) {
            let record = try operation(of: actions.dispatch(request))
            #expect(record.kind == kind)
            #expect(record.title == title)
            try await waitUntilFinished(record)
            #expect(record.state == .succeeded || record.state == .succeededWithWarnings, "\(title): \(record.state)")
        }
        #expect(actions.title(for: .start(startRequest("oauth"))) == "Starting oauth")
        #expect(actions.title(for: .devcontainer(.up(removeExisting: false, buildNoCache: false), prine))
            == "Starting the dev container for prine")
        #expect(actions.title(for: .devcontainer(.build(noCache: true), prine)) == "Building the dev container for prine")
        #expect(actions.title(for: .prune(PruneSelection(project: sampleProject, rows: []))) == "Pruning 0 features")
        #expect(actions.title(for: .syncDevcontainers(SyncRequest(project: sampleProject))) == "Updating workspaces in branchbox")
        #expect(actions.title(for: .tunnelRemove(prine, force: true)) == "Removing the tunnel for prine")
        #expect(actions.title(for: .initProject(InitRequest(folder: URL(fileURLWithPath: "/r/app"), mode: .update)))
            == "Repairing BranchBox in app")
        #expect(actions.title(for: .initProject(InitRequest(folder: URL(fileURLWithPath: "/r/app"), mode: .validate)))
            == "Checking BranchBox in app")
        #expect(actions.title(for: .applyConfig(ConfigPatch(changes: []), sampleProject)) == "Saving settings for branchbox")
        #expect(actions.title(for: .deleteBranch("feature/x", sampleProject, force: false)) == "Deleting branch feature/x")
        #expect(actions.title(for: .removeStray(PreviewSamples.stray, sampleProject, discardChanges: false))
            == "Removing worktree spike-search")
    }

    @Test func dispatchWithoutABackendIsUnavailable() async throws {
        let harness = Harness(.cliMissing)
        defer { harness.tearDown() }
        await harness.model.start()
        guard case .unavailable(.cliNotFound) = harness.model.actions.dispatch(.tunnelOpen(prine)) else {
            throw Failure("expected .unavailable")
        }
        #expect(harness.model.operations.records.isEmpty)
    }

    // MARK: Refresh after every operation

    @Test func refreshFollowsSuccessFailureAndCancel() async throws {
        let harness = Harness()
        defer { harness.tearDown() }
        let store = try await harness.startWithSampleProject()
        let backend = harness.backend

        func runAndCountRefreshes(_ setUp: () async -> Void, finish: (OperationRecord) async throws -> Void) async throws -> OperationRecord {
            await setUp()
            await backend.clearCalls()
            let record = try operation(of: harness.model.actions.dispatch(.tunnelOpen(prine)))
            try await finish(record)
            try await waitUntilFinished(record)
            try await waitUntil { await backend.calls(to: .listFeatures).count == 1 }
            try await waitUntilLoaded(store)
            try await Task.sleep(for: .milliseconds(30))
            #expect(await backend.calls(to: .listFeatures) == [.listFeatures(sampleProject, includeRemoved: false)])
            return record
        }

        let succeeded = try await runAndCountRefreshes({}, finish: { _ in })
        #expect(succeeded.state == .succeeded)

        let failure = BackendError.commandFailed(Diagnostics(summary: "Error: cloudflared missing"))
        let failed = try await runAndCountRefreshes({ await backend.script(.openTunnel, .fail(failure)) }, finish: { _ in })
        #expect(failed.state == .failed(failure))

        let cancelled = try await runAndCountRefreshes({ await backend.script(.openTunnel, .suspendUntilResumed) }, finish: { record in
            #expect(await backend.waitUntilSuspended(.openTunnel))
            harness.model.operations.cancel(record.id)
        })
        #expect(cancelled.state == .cancelled(note: nil))
    }

    @Test(arguments: [PreviewScenario.legacy0134, .contract])
    func cancellingAStartSaysWhatMayBeLeft(_ scenario: PreviewScenario) async throws {
        let harness = Harness(scenario)
        defer { harness.tearDown() }
        _ = try await harness.startWithSampleProject()
        await harness.backend.script(.startFeature, .suspendUntilResumed)
        let record = try operation(of: harness.model.actions.dispatch(.start(startRequest("oauth"))))
        #expect(record.isCancellable)
        #expect(await harness.backend.waitUntilSuspended(.startFeature))
        harness.model.operations.cancel(record.id)
        try await waitUntilFinished(record)
        #expect(record.state == .cancelled(note: "Stopped while starting; a partial worktree may be left behind"))
        #expect(harness.model.operations.history.first?.outcome == .cancelled)
    }

    // MARK: Admission (D-16)

    @Test func brokenGitMetadataNeedsAttentionAndRejectsGitOperationsBeforeTheBackendRuns() async throws {
        let harness = Harness()
        defer { harness.tearDown() }
        let store = try await harness.startWithSampleProject()
        var broken = try #require(store.features.first { $0.workFeature == "prine" })
        broken.worktreeIssue = "Git metadata is missing."
        await harness.backend.setListing(FeatureListing(features: [broken]), for: sampleProject)
        await store.refresh(.manual)
        #expect(store.attention.map(\.reason) == [.worktreeInvalid])
        #expect(store.features.first?.status == .active)
        await harness.backend.clearCalls()
        let ref = feature("prine")
        let teardown = TeardownRequest(feature: ref, recordedBranch: broken.branchName, branch: .keep)
        let requests: [OperationRequestContext] = [
            .start(startRequest("prine")), .teardown(teardown), .exec(ExecRequest(feature: ref, command: ["true"])),
            .devcontainer(.up(removeExisting: false, buildNoCache: false), ref),
            .prune(PruneSelection(project: sampleProject, rows: [teardown])),
            .tunnelOpen(ref), .syncDevcontainers(SyncRequest(project: sampleProject)),
        ]
        for request in requests {
            guard case .rejected(let reason) = harness.model.actions.dispatch(request) else {
                Issue.record("a broken Git worktree operation reached the backend")
                continue
            }
            #expect(reason == "Git worktree needs repair: prine: Git metadata is missing.")
        }
        #expect(await harness.backend.calls.isEmpty)
        let preview = try operation(of: harness.model.actions.dispatch(.syncDevcontainers(SyncRequest(project: sampleProject, dryRun: true))))
        try await waitUntilFinished(preview)
        #expect(preview.state == .succeeded)
        let remove = try operation(of: harness.model.actions.dispatch(.tunnelRemove(ref, force: false)))
        try await waitUntilFinished(remove)
        #expect(remove.state == .succeeded)
    }

    @Test func aBranchOrStrayIsDeletedByOneOperationAtATime() async throws {
        let harness = Harness()
        defer { harness.tearDown() }
        _ = try await harness.startWithSampleProject()
        let model = harness.model
        await harness.backend.script(.deleteBranch, .suspendUntilResumed)
        let first = try operation(of: model.actions.dispatch(.deleteBranch("feature/x", sampleProject, force: false)))
        #expect(await harness.backend.waitUntilSuspended(.deleteBranch))
        guard case .rejected = model.actions.dispatch(.deleteBranch("feature/x", sampleProject, force: true)) else {
            throw Failure("a second delete of the same branch must be rejected")
        }
        var teardown = TeardownRequest(feature: feature("x"), recordedBranch: "feature/x", branch: .forceDelete)
        guard case .rejected = model.actions.dispatch(.teardown(teardown)) else {
            throw Failure("a teardown deleting the same branch must be rejected")
        }
        teardown.branch = .keep                                           // keeps the branch: no conflict
        let kept = try operation(of: model.actions.dispatch(.teardown(teardown)))
        // Another branch, or a stray, is unaffected.
        let other = try operation(of: model.actions.dispatch(.deleteBranch("feature/y", sampleProject, force: false)))
        await harness.backend.resumeAll()
        for record in [first, kept, other] { try await waitUntilFinished(record) }
    }

    @Test func oneMutatingOperationPerFeature() async throws {
        let harness = Harness()
        defer { harness.tearDown() }
        _ = try await harness.startWithSampleProject()
        let model = harness.model
        await harness.backend.script(.startFeature, .suspendUntilResumed)
        let start = try operation(of: model.actions.dispatch(.start(startRequest("oauth"))))
        #expect(await harness.backend.waitUntilSuspended(.startFeature))
        let oauth = feature("oauth")

        #expect(model.operations.admission(for: .teardown, target: .feature(oauth))
            == .rejected(reason: "Starting oauth is still in progress; wait for it to finish or stop it first"))
        guard case .rejected = model.actions.dispatch(.teardown(TeardownRequest(feature: oauth, recordedBranch: nil, branch: .keep))) else {
            throw Failure("a second mutation of oauth must be rejected")
        }
        guard case .rejected = model.actions.dispatch(.start(startRequest("oauth"))) else {
            throw Failure("the same start twice must be rejected")
        }
        // Exec is always allowed; another feature is unaffected.
        #expect(model.operations.admission(for: .exec, target: .feature(oauth)) == .allowed)
        let exec = try operation(of: model.actions.dispatch(.exec(ExecRequest(feature: oauth, command: ["true"]))))
        let other = model.actions.dispatch(.devcontainer(.up(removeExisting: false, buildNoCache: false), prine))
        guard case .started = other else { throw Failure("prine must not wait for oauth, got \(other)") }
        try await waitUntilFinished(exec)
        #expect(model.operations.active(for: .feature(oauth)) === start)
        #expect(model.operations.records(for: .feature(oauth)).count == 2)

        await harness.backend.resumeAll()
        try await waitUntilFinished(start)
        #expect(model.operations.admission(for: .teardown, target: .feature(oauth)) == .allowed)
        #expect(model.operations.active(for: .feature(oauth)) == nil)
    }

    @Test func registryWritersRunFIFOWithoutTheRegistryLock() async throws {
        let harness = Harness(.legacy0134)
        defer { harness.tearDown() }
        _ = try await harness.startWithSampleProject()
        let model = harness.model
        let backend = harness.backend
        await backend.script(.startFeature, .suspendUntilResumed)
        await backend.script(.syncDevcontainers, .suspendUntilResumed)
        await backend.script(.openTunnel, .suspendUntilResumed)
        await backend.clearCalls()

        let start = try operation(of: model.actions.dispatch(.start(startRequest("alpha"))))
        let syncResult = model.actions.dispatch(.syncDevcontainers(SyncRequest(project: sampleProject)))
        guard case .queued(let sync, let behind) = syncResult else { throw Failure("expected the sync to queue, got \(syncResult)") }
        #expect(behind == "Starting alpha")
        let tunnelResult = model.actions.dispatch(.tunnelOpen(prine))
        guard case .queued(let tunnel, "Updating workspaces in branchbox") = tunnelResult else {
            throw Failure("expected the tunnel to queue behind the sync, got \(tunnelResult)")
        }
        #expect(sync.state == .queued(behind: "Starting alpha"))
        #expect(model.operations.admission(for: .tunnelRemove, target: .feature(remotion))
            == .queued(behind: "Opening a tunnel for prine"))
        // A non-writer only waits for the project-wide sync.
        #expect(model.operations.admission(for: .devcontainerUp, target: .feature(remotion))
            == .queued(behind: "Updating workspaces in branchbox"))
        #expect(model.operations.running.count == 3)

        #expect(await backend.waitUntilSuspended(.startFeature))
        #expect(await backend.resume(.startFeature))
        #expect(await backend.waitUntilSuspended(.syncDevcontainers))
        try await waitUntilFinished(start)
        #expect(sync.state == .running)
        #expect(tunnel.state == .queued(behind: "Updating workspaces in branchbox"))
        #expect(await backend.suspendedCount(.openTunnel) == 0)

        #expect(await backend.resume(.syncDevcontainers))
        #expect(await backend.waitUntilSuspended(.openTunnel))
        #expect(tunnel.state == .running)
        #expect(await backend.resume(.openTunnel))
        try await waitUntilFinished(tunnel)
        let mutations = await backend.calls.map(\.method).filter { $0 != .listFeatures }
        #expect(mutations == [.startFeature, .syncDevcontainers, .openTunnel])
    }

    @Test func registryWritersRunConcurrentlyWithTheRegistryLock() async throws {
        let harness = Harness(.contract)
        defer { harness.tearDown() }
        _ = try await harness.startWithSampleProject()
        let model = harness.model
        let backend = harness.backend
        await backend.script(.startFeature, .suspendUntilResumed)
        await backend.script(.startFeature, .suspendUntilResumed)
        await backend.script(.openTunnel, .suspendUntilResumed)

        let alpha = model.actions.dispatch(.start(startRequest("alpha")))
        let beta = model.actions.dispatch(.start(startRequest("beta")))
        let tunnel = model.actions.dispatch(.tunnelOpen(prine))
        for result in [alpha, beta, tunnel] {
            guard case .started = result else { throw Failure("with registry-lock every writer starts at once, got \(result)") }
        }
        #expect(await backend.waitUntilSuspended(.startFeature, count: 2))
        #expect(await backend.waitUntilSuspended(.openTunnel))
        // Sync is project-wide, so it still waits for the running mutations.
        guard case .queued(let sync, "Opening a tunnel for prine") = model.actions.dispatch(.syncDevcontainers(SyncRequest(project: sampleProject))) else {
            throw Failure("the sync must wait for the running mutations")
        }
        await backend.resumeAll()
        try await waitUntilFinished(sync)
        #expect(sync.state == .succeeded)
    }

    @Test func projectWideOperationsAreExclusive() async throws {
        let harness = Harness(.contract)
        defer { harness.tearDown() }
        _ = try await harness.startWithSampleProject()
        let model = harness.model
        let backend = harness.backend
        await backend.script(.startFeature, .suspendUntilResumed)
        await backend.script(.teardownFeature, .suspendUntilResumed)

        let start = try operation(of: model.actions.dispatch(.start(startRequest("alpha"))))
        let selection = PruneSelection(project: sampleProject,
                                       rows: [TeardownRequest(feature: feature("retained"), recordedBranch: nil, branch: .keep)])
        guard case .queued(let prune, "Starting alpha") = model.actions.dispatch(.prune(selection)) else {
            throw Failure("the prune must wait for the running start")
        }
        guard case .queued(let up, "Pruning 1 feature") = model.actions.dispatch(.devcontainer(.up(removeExisting: false, buildNoCache: false), prine)) else {
            throw Failure("a mutation must wait for the queued prune")
        }
        // Exec is never blocked, and another project is unaffected.
        guard case .started = model.actions.dispatch(.exec(ExecRequest(feature: prine, command: ["date"]))) else {
            throw Failure("exec must not wait")
        }
        let elsewhere = feature("x", in: ProjectRef(root: URL(fileURLWithPath: "/tmp/bbx-other/main")))
        #expect(model.operations.admission(for: .start, target: .feature(elsewhere)) == .allowed)
        #expect(model.operations.admission(for: .applyConfig, target: .project(sampleProject)) == .queued(behind: "Starting the dev container for prine"))

        #expect(await backend.waitUntilSuspended(.startFeature))
        #expect(await backend.resume(.startFeature))
        #expect(await backend.waitUntilSuspended(.teardownFeature))
        #expect(prune.state == .running)
        #expect(up.state == .queued(behind: "Pruning 1 feature"))
        #expect(await backend.resume(.teardownFeature))
        try await waitUntilFinished(prune)
        try await waitUntilFinished(up)
        try await waitUntilFinished(start)
        let mutations = await backend.calls.map(\.method).filter { [.startFeature, .teardownFeature, .devcontainer].contains($0) }
        #expect(mutations == [.startFeature, .teardownFeature, .devcontainer])
    }

    @Test func cancellingAQueuedOperationDropsIt() async throws {
        let harness = Harness(.legacy0134)
        defer { harness.tearDown() }
        _ = try await harness.startWithSampleProject()
        let model = harness.model
        await harness.backend.script(.startFeature, .suspendUntilResumed)
        let start = try operation(of: model.actions.dispatch(.start(startRequest("alpha"))))
        let queued = try operation(of: model.actions.dispatch(.tunnelOpen(prine)))
        #expect(queued.state == .queued(behind: "Starting alpha"))

        await harness.backend.clearCalls()
        model.operations.cancel(queued.id)
        #expect(queued.state == .cancelled(note: nil))
        #expect(model.operations.record(queued.id) === queued)
        #expect(await harness.backend.waitUntilSuspended(.startFeature))
        await harness.backend.resumeAll()
        try await waitUntilFinished(start)
        try await Task.sleep(for: .milliseconds(30))
        #expect(await harness.backend.calls(to: .openTunnel).isEmpty)
        #expect(model.operations.history.map(\.id).contains(queued.id))
    }

    // MARK: Prune

    @Test func pruneSkipsRefusalsAndContinues() async throws {
        let harness = Harness(.dirtyWorktree)
        defer { harness.tearDown() }
        _ = try await harness.startWithSampleProject()
        let rows = [
            TeardownRequest(feature: prine, recordedBranch: "feature/prine", branch: .keep),           // dirty: refused
            TeardownRequest(feature: feature("retained"), recordedBranch: "feature/retained", branch: .keep),
            TeardownRequest(feature: feature("orphan"), recordedBranch: "feature/orphan", branch: .keep),
        ]
        let record = try operation(of: harness.model.actions.dispatch(.prune(PruneSelection(project: sampleProject, rows: rows))))
        #expect(record.kind == .prune && record.target == .project(sampleProject))
        try await waitUntilFinished(record)

        #expect(await harness.backend.calls.compactMap(\.teardownRequest) == rows)
        #expect(record.state == .partial)
        guard case .prune(let result)? = record.result else { throw Failure("expected a prune result") }
        #expect(result.rows.map(\.feature) == ["prine", "retained", "orphan"])
        guard case .refused(.refused(let refusal)) = result.rows[0].outcome else { throw Failure("prine must be refused") }
        #expect(refusal.cause == .uncommittedChanges(files: PreviewSamples.dirtyFiles))
        guard case .removed = result.rows[1].outcome, case .removed = result.rows[2].outcome else {
            throw Failure("the other rows must be removed")
        }
        #expect(record.phase == .item(index: 3, of: 3, name: "orphan"))
        #expect(record.stepProgress == StepProgress(completed: 3, total: 3))
        #expect(record.log.lines.map(\.message).contains { $0.hasPrefix("Skipped prine:") })
    }

    @Test func pruneStopsBetweenRowsWhenCancelled() async throws {
        let harness = Harness(.contract)
        defer { harness.tearDown() }
        _ = try await harness.startWithSampleProject()
        await harness.backend.script(.teardownFeature, .succeed(after: .zero))
        await harness.backend.script(.teardownFeature, .suspendUntilResumed)
        let rows = ["prine", "remotion", "retained"].map { TeardownRequest(feature: feature($0), recordedBranch: nil, branch: .keep) }
        let record = try operation(of: harness.model.actions.dispatch(.prune(PruneSelection(project: sampleProject, rows: rows))))

        #expect(await harness.backend.waitUntilSuspended(.teardownFeature))
        harness.model.operations.cancel(record.id)
        try await waitUntilFinished(record)

        #expect(await harness.backend.calls.compactMap(\.teardownRequest) == Array(rows.prefix(2)))
        guard case .prune(let result)? = record.result else { throw Failure("expected the rows so far") }
        guard case .removed = result.rows[0].outcome else { throw Failure("prine was removed before the cancel") }
        #expect(result.rows[1].outcome == .cancelled)
        #expect(result.rows[2].outcome == .skipped("Not started: the prune was stopped"))
        #expect(record.state == .cancelled(note: "Stopped after 1 of 3 features"))
        #expect(record.stepProgress == StepProgress(completed: 1, total: 3))
    }

    @Test func aLegacyPruneStoppedMidRowSaysAFeatureMayBePartlyRemoved() async throws {
        let harness = Harness(.legacy0134)
        defer { harness.tearDown() }
        _ = try await harness.startWithSampleProject()
        await harness.backend.script(.teardownFeature, .suspendUntilResumed)
        let rows = ["prine", "remotion"].map { TeardownRequest(feature: feature($0), recordedBranch: nil, branch: .keep) }
        let record = try operation(of: harness.model.actions.dispatch(.prune(PruneSelection(project: sampleProject, rows: rows))))
        #expect(await harness.backend.waitUntilSuspended(.teardownFeature))
        harness.model.operations.cancel(record.id)
        try await waitUntilFinished(record)
        #expect(record.state == .cancelled(note: "Stopped after 0 of 2 features. Stopped while tearing down; a feature may be partly removed"))
    }

    // MARK: Progress stream

    @Test func twoThousandLinesCauseAtMost25StoreMutations() async throws {
        let harness = Harness()
        defer { harness.tearDown() }
        _ = try await harness.startWithSampleProject()
        var steps: [PreviewBehavior] = []
        for burst in 0..<20 {
            steps.append(.emit((0..<100).map { .log(logLine("line \(burst * 100 + $0)")) }))
            steps.append(.succeed(after: .milliseconds(10)))
        }
        await harness.backend.script(.exec, steps: steps)
        let record = try operation(of: harness.model.actions.dispatch(.exec(ExecRequest(feature: prine, command: ["make", "logs"]))))
        try await waitUntilFinished(record)

        #expect(record.log.lines.count == 2000)
        #expect(record.log.lines.map(\.message) == (0..<2000).map { "line \($0)" })
        #expect(record.log.revision <= 25, "\(record.log.revision) log revisions")
        #expect(record.log.revision >= 1)
    }

    @Test func eventsStayInOrder() async throws {
        let harness = Harness()
        defer { harness.tearDown() }
        _ = try await harness.startWithSampleProject()
        var events: [ProgressEvent] = []
        for index in 0..<300 {
            switch index % 3 {
            case 0: events.append(.log(logLine("line \(index)")))
            case 1: events.append(.phase(.step("step \(index)")))
            default: events.append(.warning("warning \(index)"))
            }
        }
        await harness.backend.script(.startFeature, steps: [.emit(Array(events.prefix(150))), .succeed(after: .milliseconds(150)),
                                                            .emit(Array(events.suffix(150)))])
        let record = try operation(of: harness.model.actions.dispatch(.start(startRequest("ordered"))))
        try await waitUntilFinished(record)

        #expect(record.log.lines.map(\.message) == stride(from: 0, to: 300, by: 3).map { "line \($0)" })
        #expect(record.warnings == stride(from: 2, to: 300, by: 3).map { "warning \($0)" })
        #expect(record.phase == .step("step 298"))
        #expect(record.state == .succeededWithWarnings)                // progress warnings count
    }

    @Test func theLogBufferIsARing() {
        let buffer = LogBuffer()
        buffer.append((0..<LogBuffer.capacity).map { logLine("a\($0)") })
        buffer.append((0..<5).map { logLine("b\($0)") })
        buffer.append([])
        #expect(buffer.lines.count == LogBuffer.capacity)
        #expect(buffer.lines.first?.message == "a5")
        #expect(buffer.lines.last?.message == "b4")
        #expect(buffer.droppedLines == 5)
        #expect(buffer.revision == 2)
    }

    @Test func theBatcherDeliversInOrderAtMostEveryInterval() async throws {
        var batches: [[ProgressEvent]] = []
        let batcher = EventBatcher(interval: .milliseconds(50)) { batches.append($0) }
        batcher.add(.warning("1"))
        batcher.add(.warning("2"))
        #expect(batches.isEmpty)
        try await waitUntil { batches.count == 1 }
        #expect(batches == [[.warning("1"), .warning("2")]])
        batcher.add(.warning("3"))
        batcher.flush()
        batcher.flush()
        #expect(batches == [[.warning("1"), .warning("2")], [.warning("3")]])
        #expect(batcher.deliveries == 2)
        try await Task.sleep(for: .milliseconds(80))
        #expect(batcher.deliveries == 2)                             // a flushed batch is not delivered twice
    }

    // MARK: Logs and history

    @Test func theFullLogIsArchivedRedactedAndRetained() async throws {
        let harness = Harness()
        defer { harness.tearDown() }
        _ = try await harness.startWithSampleProject()
        harness.settings.extraEnvironment = ["API_TOKEN": "s3cret-value"]
        harness.settings.logRetention = 2
        await harness.backend.script(.exec, .emit([.log(logLine("using s3cret-value now")), .log(logLine("done"))]))

        let first = try operation(of: harness.model.actions.dispatch(.exec(ExecRequest(feature: prine, command: ["env"]))))
        try await waitUntilFinished(first)
        first.archive?.drain()
        let url = try #require(first.log.archiveURL)
        #expect(url.lastPathComponent.hasSuffix("-exec-prine-\(first.id.uuidString.lowercased()).log"))
        let text = try String(contentsOf: url, encoding: .utf8)
        #expect(text.contains("# Running env in prine"))
        #expect(text.contains("using <redacted> now"))
        #expect(!text.contains("s3cret-value"))
        #expect(text.contains("# Finished: "))
        let permissions = try FileManager.default.attributesOfItem(atPath: url.path)[.posixPermissions] as? Int
        #expect(permissions == 0o600)
        // The in-memory log (Activity, Copy Log) is redacted too.
        #expect(first.log.lines.contains { $0.message == "using <redacted> now" })
        #expect(!first.log.lines.contains { $0.message.contains("s3cret-value") })

        for _ in 0..<2 {
            let next = try operation(of: harness.model.actions.dispatch(.exec(ExecRequest(feature: prine, command: ["true"]))))
            try await waitUntilFinished(next)
            next.archive?.drain()
        }
        let logs = try FileManager.default.contentsOfDirectory(atPath: harness.sandbox.configuration.logsDirectory.path)
        #expect(logs.count == 2)
        #expect(!FileManager.default.fileExists(atPath: url.path))  // the oldest was pruned
    }

    @Test func archiveNamesAreSafe() {
        let date = Date(timeIntervalSince1970: 1_790_000_000)
        let id = UUID(uuidString: "6A1F4B0E-2D3C-4E5F-8A9B-0C1D2E3F4A5B")!
        #expect(LogArchive.fileName(startedAt: date, kind: .start, subject: "oauth/../x y", id: id)
            == "20260921T141320Z-start-oauth-..-x-y-6a1f4b0e-2d3c-4e5f-8a9b-0c1d2e3f4a5b.log")
        #expect(LogArchive.sanitized("///") == "operation")
        #expect(LogArchive.redact("a tok b tok", secrets: ["tok"]) == "a <redacted> b <redacted>")
    }

    @Test func historyIsPersistedAndReloaded() async throws {
        let harness = Harness()
        defer { harness.tearDown() }
        _ = try await harness.startWithSampleProject()
        let failure = BackendError.commandFailed(Diagnostics(summary: "Error: docker is not running"))
        await harness.backend.script(.devcontainer, .fail(failure))
        let record = try operation(of: harness.model.actions.dispatch(.devcontainer(.build(noCache: false), prine)))
        try await waitUntilFinished(record)

        let summary = try #require(harness.model.operations.history.first)
        #expect(summary.id == record.id)
        #expect(summary.outcome == .failed)
        #expect(summary.detail == "Error: docker is not running")
        #expect(summary.feature == "prine" && summary.projectPath == sampleProject.path)
        #expect(summary.logPath == record.log.archiveURL?.path)

        let reloaded = OperationStore(historyURL: harness.sandbox.configuration.projectsDirectory.appendingPathComponent("operations.json"))
        reloaded.loadHistory()
        #expect(reloaded.history == harness.model.operations.history)
    }

    @Test func warningsAndTheHistoryDetailAreRedacted() async throws {
        let harness = Harness()
        defer { harness.tearDown() }
        _ = try await harness.startWithSampleProject()
        harness.settings.extraEnvironment = ["API_TOKEN": "s3cret-value"]
        await harness.backend.script(.devcontainer, steps: [
            .emit([.warning("retrying with s3cret-value")]),
            .fail(.commandFailed(Diagnostics(summary: "Error: s3cret-value was rejected"))),
        ])
        let record = try operation(of: harness.model.actions.dispatch(.devcontainer(.build(noCache: false), prine)))
        try await waitUntilFinished(record)

        #expect(record.warnings == ["retrying with <redacted>"])
        let summary = try #require(harness.model.operations.history.first)
        #expect(summary.detail == "Error: <redacted> was rejected")
    }

    // MARK: Notifications (§11)

    @Test func theNotificationActiveCheckFollowsTheAppByDefault() async throws {
        let harness = Harness()
        defer { harness.tearDown() }
        _ = try await harness.startWithSampleProject()
        #expect(!harness.model.actions.isAppActive())
        harness.model.appDidBecomeActive()
        #expect(harness.model.actions.isAppActive())
        harness.model.appDidResignActive()
        #expect(!harness.model.actions.isAppActive())
    }

    @Test func onlyTheReleasedBundleUsesTheBranchBoxFolders() {
        #expect(AppModel.Configuration.folderName(isBundledApp: true, bundleIdentifier: "dev.branchbox.app") == "BranchBox")
        #expect(AppModel.Configuration.folderName(isBundledApp: true, bundleIdentifier: "dev.branchbox.app.dev")
                == "BranchBox Dev")
        #expect(AppModel.Configuration.folderName(isBundledApp: false, bundleIdentifier: "dev.branchbox.app")
                == "BranchBox Dev")
        #expect(AppModel.Configuration.folderName(isBundledApp: false, bundleIdentifier: nil) == "BranchBox Dev")
    }

    @Test func longOrFailedOperationsNotifyWhileTheAppIsInactive() async throws {
        let clock = ManualClock()
        let spy = SpyNotifier(isAvailable: true)
        let harness = Harness(notifier: spy) { $0.clock = clock }
        defer { harness.tearDown() }
        _ = try await harness.startWithSampleProject()
        #expect(harness.model.notifier is SpyNotifier)
        let backend = harness.backend

        // Quick success: no note.
        let quick = try operation(of: harness.model.actions.dispatch(.start(startRequest("quick"))))
        try await waitUntilFinished(quick)

        // Took longer than 10 s.
        await backend.script(.startFeature, .suspendUntilResumed)
        let slow = try operation(of: harness.model.actions.dispatch(.start(startRequest("oauth"))))
        #expect(await backend.waitUntilSuspended(.startFeature))
        clock.advance(by: .seconds(11))
        #expect(await backend.resume(.startFeature))
        try await waitUntilFinished(slow)
        try await waitUntil { spy.posted.count == 1 }
        #expect(spy.posted[0] == UserNote(title: "oauth is ready", body: "Started successfully",
                                          intent: .showActivity(operation: slow.id), threadID: sampleProject.path))

        // Failed quickly.
        let failure = BackendError.refused(Refusal(cause: .branchExists("feature/beta"), message: "Branch feature/beta already exists",
                                                   diagnostics: Diagnostics(summary: "x")))
        await backend.script(.startFeature, .fail(failure))
        let failed = try operation(of: harness.model.actions.dispatch(.start(startRequest("beta"))))
        try await waitUntilFinished(failed)
        try await waitUntil { spy.posted.count == 2 }
        #expect(spy.posted[1].title == "Couldn't start beta")
        #expect(spy.posted[1].body == "Branch feature/beta already exists")

        // Not while the user is looking, not when turned off, and never for a cancellation.
        harness.model.actions.isAppActive = { true }
        await backend.script(.startFeature, .fail(failure))
        try await waitUntilFinished(try operation(of: harness.model.actions.dispatch(.start(startRequest("gamma")))))
        harness.model.actions.isAppActive = { false }
        harness.settings.notificationsEnabled = false
        await backend.script(.startFeature, .fail(failure))
        try await waitUntilFinished(try operation(of: harness.model.actions.dispatch(.start(startRequest("delta")))))
        harness.settings.notificationsEnabled = true
        await backend.script(.startFeature, .suspendUntilResumed)
        let cancelled = try operation(of: harness.model.actions.dispatch(.start(startRequest("epsilon"))))
        #expect(await backend.waitUntilSuspended(.startFeature))
        harness.model.operations.cancel(cancelled.id)
        try await waitUntilFinished(cancelled)

        // Only problems: a long success stays quiet, a teardown problem does not.
        harness.settings.notifyOnlyOnProblems = true
        await backend.script(.startFeature, .suspendUntilResumed)
        let long = try operation(of: harness.model.actions.dispatch(.start(startRequest("zeta"))))
        #expect(await backend.waitUntilSuspended(.startFeature))
        clock.advance(by: .seconds(30))
        #expect(await backend.resume(.startFeature))
        try await waitUntilFinished(long)
        try await Task.sleep(for: .milliseconds(30))
        #expect(spy.posted.count == 2)
        #expect(spy.authorizationCount == 2)
    }

    @Test func notesNameTheResult() async throws {
        let harness = Harness()
        defer { harness.tearDown() }
        let actions = harness.model.actions
        let teardown = OperationRecord(kind: .teardown, target: .feature(prine), title: "Tearing down prine",
                                       context: .teardown(TeardownRequest(feature: prine, recordedBranch: nil, branch: .keep)))
        teardown.finish(.succeeded)
        #expect(actions.note(for: teardown).title == "prine torn down")
        let partial = OperationRecord(kind: .teardown, target: .feature(prine), title: "Tearing down prine",
                                      context: .teardown(TeardownRequest(feature: prine, recordedBranch: nil, branch: .deleteIfMerged)))
        partial.finish(.partial)
        #expect(actions.note(for: partial).title == "Teardown of prine needs attention")
        let warned = OperationRecord(kind: .start, target: .feature(prine), title: "Starting prine", context: .start(startRequest("prine")))
        warned.addWarnings(["compose failed"])
        warned.finish(.succeededWithWarnings)
        #expect(actions.note(for: warned) == UserNote(title: "prine started with problems", body: "compose failed",
                                                     intent: .showActivity(operation: warned.id), threadID: sampleProject.path))
        let sync = OperationRecord(kind: .syncDevcontainers, target: .project(sampleProject), title: "Updating workspaces in branchbox",
                                   context: .syncDevcontainers(SyncRequest(project: sampleProject)))
        sync.finish(.failed(.unsupported(.devcontainerSyncJSON, minimumCLI: "0.14.0")))
        #expect(actions.note(for: sync).body == "Failed: This needs BranchBox CLI 0.14.0 or later (devcontainer-sync-json)")
    }

    // MARK: Termination

    @Test func prepareForTerminationCancelsEverythingWithinItsBound() async throws {
        let harness = Harness { $0.terminationTimeout = .seconds(2) }
        defer { harness.tearDown() }
        _ = try await harness.startWithSampleProject()
        await harness.backend.script(.exec, .suspendUntilResumed)
        await harness.backend.script(.startFeature, .suspendUntilResumed)
        let exec = try operation(of: harness.model.actions.dispatch(.exec(ExecRequest(feature: prine, command: ["sleep", "60"]))))
        let start = try operation(of: harness.model.actions.dispatch(.start(startRequest("oauth"))))
        #expect(await harness.backend.waitUntilSuspended(.exec))
        #expect(await harness.backend.waitUntilSuspended(.startFeature))
        #expect(harness.model.operations.running.count == 2)

        await harness.backend.clearCalls()
        let began = ContinuousClock.now
        await harness.model.prepareForTermination()
        #expect(ContinuousClock.now - began < .seconds(5))
        #expect(exec.state == .cancelled(note: nil))
        #expect(start.state == .cancelled(note: "Stopped while starting; a partial worktree may be left behind"))
        #expect(harness.model.operations.running.isEmpty)
        #expect(!harness.model.coordinator.isRunning)
        // Quitting spawns nothing new: no refresh after the cancelled operations, and no new operation.
        try await Task.sleep(for: .milliseconds(100))
        #expect(await harness.backend.calls(to: .listFeatures).isEmpty)
        guard case .rejected = harness.model.actions.dispatch(.exec(ExecRequest(feature: prine, command: ["true"]))) else {
            throw Failure("dispatch must be refused while quitting")
        }
    }

    @Test func prepareForTerminationReturnsEvenIfTheBackendHangs() async throws {
        let sandbox = Sandbox()
        defer { sandbox.remove() }
        var configuration = sandbox.configuration
        configuration.terminationTimeout = .milliseconds(200)
        let model = AppModel(settings: AppSettings(defaults: sandbox.defaults), bootstrapper: HangingBootstrapper(),
                             notifier: NoopNotifier(), configuration: configuration)
        await model.start()
        let began = ContinuousClock.now
        await model.prepareForTermination()
        #expect(ContinuousClock.now - began < .seconds(1))
    }
}

/// A bootstrapper whose process teardown never finishes.
private struct HangingBootstrapper: BackendBootstrapping {
    func bootstrap(_ settings: BackendSettings) async -> BackendBootstrap {
        .unavailable(.cliNotFound(searched: []), nil)
    }

    func environmentSummary() async -> EnvironmentSummary? { nil }
    func recaptureEnvironment() async {}

    func terminateAllProcesses() async {
        try? await Task.sleep(for: .seconds(3600))
    }
}
