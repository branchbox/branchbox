@testable import BranchBoxApp
import BranchBoxKit
import BranchBoxPreview
import BranchBoxStores
import Foundation
import Testing

// SW-6: the Start, Teardown, Prune and Stray flows and the Stop confirmation, driven through their flow models
// against a PreviewBackend whose call log proves what reached the backend.

private let project = PreviewSamples.project
let flowSampleProject = PreviewSamples.project

private func feature(_ name: String) -> FeatureRef { FeatureRef(project: project, name: name) }

func uncommitted(_ paths: [String]) -> BackendError {
    let files = paths.map { ChangedFile(path: $0, kind: "untracked", area: "other") }
    let message = "Refusing to tear down; \(paths.count) uncommitted change(s) would be lost: \(paths.joined(separator: ", "))"
    return .refused(Refusal(cause: .uncommittedChanges(files: files), message: message, diagnostics: Diagnostics(summary: message)))
}

/// An `AppModel` on a `PreviewBackend` with the sample project added, storing everything under
/// `$TMPDIR/branchbox-tests/` (never the user's preferences or Application Support).
@MainActor final class FlowHarness {
    let directory: URL
    let defaultsName: String
    let backend: PreviewBackend
    let model: AppModel
    let memory: StartAdvancedMemory

    init(_ scenario: PreviewScenario = .contract) {
        let base = FileManager.default.temporaryDirectory.appendingPathComponent("branchbox-tests", isDirectory: true)
        directory = base.appendingPathComponent("flows-\(UUID().uuidString)", isDirectory: true)
        try? FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        defaultsName = base.appendingPathComponent("flows-settings-\(UUID().uuidString)").path
        let settings = AppSettings(defaults: UserDefaults(suiteName: defaultsName)!)
        backend = PreviewBackend(scenario: scenario)
        model = AppModel(settings: settings, bootstrapper: PreviewBootstrapper(backend: backend), notifier: NoopNotifier(),
                         configuration: .isolated(in: directory))
        memory = StartAdvancedMemory(suiteName: defaultsName)
    }

    /// Bootstraps the backend and adds the sample project, waiting for its first listing.
    @discardableResult func start() async throws -> ProjectStore {
        await model.environment.rebootstrap()
        _ = await model.projects.add(folder: flowSampleProject.root)
        let store = try #require(model.projects.project(flowSampleProject))
        try await flowWaitUntil("the project did not load") {
            if case .loaded = store.loadState, !store.isRefreshing { return true }
            return false
        }
        return store
    }

    func startFlow(prefill: StartFeatureRequest? = nil) -> StartFlow {
        StartFlow(model: model, project: flowSampleProject, prefill: prefill, memory: memory, debounce: .zero)
    }

    func tearDown() {
        UserDefaults(suiteName: defaultsName)?.removePersistentDomain(forName: defaultsName)
        try? FileManager.default.removeItem(atPath: defaultsName + ".plist")
        try? FileManager.default.removeItem(at: directory)
    }
}

/// Polls `condition` every millisecond on the main actor, failing after `timeout`.
@MainActor func flowWaitUntil(_ message: String, timeout: Duration = .seconds(30),
                                  sourceLocation: SourceLocation = #_sourceLocation,
                                  _ condition: () async -> Bool) async throws {
    let deadline = ContinuousClock.now + timeout
    while await !condition() {
        try #require(ContinuousClock.now < deadline, "\(message) within \(timeout)", sourceLocation: sourceLocation)
        try await Task.sleep(for: .milliseconds(1))
    }
}

@MainActor func flowWaitUntilFinished(_ record: OperationRecord?, sourceLocation: SourceLocation = #_sourceLocation) async throws {
    let record = try #require(record, sourceLocation: sourceLocation)
    try await flowWaitUntil("\(record.title) did not finish", sourceLocation: sourceLocation) { record.isFinished }
}

/// The discard retry among `recoveries`.
private func discardRetry(_ recoveries: [RecoveryAction]) -> RecoveryAction? {
    recoveries.first { action in
        if case .retry(.teardown(let request), _, true, _) = action { return request.discard != nil }
        if case .retry(.removeStray(_, _, true), _, true, _) = action { return true }
        return false
    }
}

// MARK: - Start

@MainActor @Suite(.serialized) struct StartFlowTests {
    @Test func cancellingLeavesNoDraftStateForTheNextPresentation() async throws {
        let harness = FlowHarness()
        defer { harness.tearDown() }
        try await harness.start()

        var flow: StartFlow? = harness.startFlow()
        flow?.setTitle("Add OAuth login")
        flow?.setPrompt("Wire up the OAuth callback")
        flow?.setMode(.minimal)
        flow?.setSkipped("compose", true)
        flow?.setVerbose(true)
        flow?.showsAdvanced = true
        await flow?.waitForPreview()
        #expect(flow?.draft.resolvedName == "add-oauth-login")
        flow = nil                                          // Cancel: the sheet drops its flow

        let next = harness.startFlow()
        #expect(next.title.isEmpty)
        #expect(next.draft.input.isEmpty)
        #expect(next.draft.prompt.isEmpty)
        #expect(next.draft.mode == .full)
        #expect(next.draft.skipModules.isEmpty)
        #expect(next.draft.verbose == false)
        #expect(next.showsAdvanced == false)
        #expect(next.record == nil)
        #expect(harness.memory.load(project) == nil, "cancelling must not remember the advanced options")
        #expect(await harness.backend.calls(to: .startFeature).isEmpty)
    }

    @Test func aPromptOf2001CharactersBlocksStart() async throws {
        let harness = FlowHarness()
        defer { harness.tearDown() }
        try await harness.start()
        let flow = harness.startFlow()
        flow.setTitle("oauth")
        await flow.waitForPreview()
        #expect(flow.canStart)

        flow.setPrompt(String(repeating: "a", count: 2001))
        #expect(flow.promptOverLimit)
        #expect(flow.canStart == false)
        #expect(flow.validationErrors.contains("The prompt is 2,001 characters; the limit is 2,000"))
        #expect(flow.start() == false)
        #expect(flow.record == nil)
        #expect(await harness.backend.calls(to: .startFeature).isEmpty)

        flow.setPrompt(String(repeating: "a", count: 2000))
        #expect(flow.canStart)
    }

    @Test func theStartRequestCarriesTheResolvedSlugNeverTheTitle() async throws {
        let harness = FlowHarness()
        defer { harness.tearDown() }
        try await harness.start()
        let flow = harness.startFlow()
        flow.setTitle("Add OAuth login!")
        await flow.waitForPreview()
        #expect(flow.draft.currentPreview?.slug == "add-oauth-login")
        #expect(flow.start())
        try await flowWaitUntilFinished(flow.record)

        let request = try #require(await harness.backend.calls(to: .startFeature).first?.startRequest)
        #expect(request.name == "add-oauth-login")
        let encoded = String(decoding: try JSONEncoder().encode(request), as: UTF8.self)
        #expect(!encoded.contains("Add OAuth"), "the title must never reach the backend")
        #expect(flow.summary?.workFeature == "add-oauth-login", "the result shows the resolved name")
        #expect(harness.model.settings.promptHistory.isEmpty)
    }

    @Test func editNameOverridesTheSlugAndRemembersAdvancedOptionsOnlyAfterStart() async throws {
        let harness = FlowHarness()
        defer { harness.tearDown() }
        try await harness.start()
        let flow = harness.startFlow()
        flow.setTitle("Add OAuth login")
        await flow.waitForPreview()
        flow.beginEditingName()
        #expect(flow.nameOverride == "add-oauth-login")
        flow.setNameOverride("oauth-v2")
        flow.setSkipped("database", true)
        flow.setPrompt("Ship it")
        await flow.waitForPreview()
        #expect(flow.start())
        try await flowWaitUntilFinished(flow.record)

        #expect(await harness.backend.calls(to: .startFeature).first?.startRequest?.name == "oauth-v2")
        #expect(harness.memory.load(project)?.skipModules == ["database"])
        #expect(harness.model.settings.promptHistory == ["Ship it"])
        #expect(harness.startFlow().draft.skipModules == ["database"])
    }

    @Test func aRegistryDuplicateBlocksStart() async throws {
        let harness = FlowHarness()
        defer { harness.tearDown() }
        try await harness.start()
        let flow = harness.startFlow()
        flow.setTitle("prine")
        await flow.waitForPreview()
        #expect(flow.validationErrors.contains("A feature named “prine” already exists in this project"))
        #expect(flow.collidingFeature?.workFeature == "prine")
        #expect(flow.canStart == false)
    }

    @Test func editAndRetryReturnsToTheFormWithTheSameRequest() async throws {
        let harness = FlowHarness()
        defer { harness.tearDown() }
        try await harness.start()
        await harness.backend.script(.startFeature, .fail(.commandFailed(Diagnostics(summary: "Docker is not available"))))
        let flow = harness.startFlow()
        flow.setTitle("oauth")
        flow.setPrompt("Try again later")
        await flow.waitForPreview()
        #expect(flow.start())
        try await flowWaitUntilFinished(flow.record)
        #expect(flow.record?.failure != nil)

        flow.editAndRetry()
        #expect(flow.record == nil)
        #expect(flow.draft.input == "oauth")
        #expect(flow.draft.prompt == "Try again later")
    }
}

// MARK: - Teardown

@MainActor @Suite(.serialized) struct TeardownFlowTests {
    @Test func theFirstAttemptNeverCarriesDiscardEvenWhenThePlanListsUserChanges() async throws {
        let harness = FlowHarness(.dirtyWorktree)
        defer { harness.tearDown() }
        try await harness.start()
        let flow = TeardownFlow(model: harness.model, feature: feature("prine"), preselect: nil)
        await flow.load()
        #expect(flow.plan?.changes.user.map(\.path) == ["README.md", "notes.txt"])
        #expect(flow.draft?.pendingDiscardWarning != nil)

        #expect(flow.tearDown())
        try await flowWaitUntilFinished(flow.record)
        let first = try #require(await harness.backend.calls(to: .teardownFeature).first?.teardownRequest)
        #expect(first.discard == nil)
        #expect(first.forceRemoval == false)

        // The refusal's recovery names exactly the refused files, and only it carries consent.
        let retry = try #require(discardRetry(flow.recoveries))
        #expect(retry.confirmationMessage?.contains("README.md") == true)
        #expect(retry.confirmationMessage?.contains("notes.txt") == true)
        flow.adopt(await FlowActions(model: harness.model).perform(retry))
        try await flowWaitUntilFinished(flow.record)
        let calls = await harness.backend.calls(to: .teardownFeature).compactMap(\.teardownRequest)
        #expect(calls.count == 2)
        #expect(calls[1].discard?.userFiles == ["README.md", "notes.txt"])
        #expect(flow.outcome?.worktreeGone == true)
        #expect(flow.earlierAttempts.count == 1)
    }

    @Test func aScriptedRefusalsRetryCarriesExactlyTheRefusedFiles() async throws {
        let harness = FlowHarness()
        defer { harness.tearDown() }
        try await harness.start()
        await harness.backend.script(.teardownFeature, .fail(uncommitted(["notes.txt"])))
        let flow = TeardownFlow(model: harness.model, feature: feature("prine"), preselect: nil)
        await flow.load()
        #expect(flow.tearDown())
        try await flowWaitUntilFinished(flow.record)
        guard case .refused(let refusal)? = flow.record?.failure, case .uncommittedChanges = refusal.cause else {
            Issue.record("expected the scripted refusal")
            return
        }

        let retry = try #require(discardRetry(flow.recoveries))
        flow.adopt(await FlowActions(model: harness.model).perform(retry))
        try await flowWaitUntilFinished(flow.record)
        let calls = await harness.backend.calls(to: .teardownFeature).compactMap(\.teardownRequest)
        #expect(calls.map(\.discard?.userFiles) == [nil, ["notes.txt"]])
        #expect(calls.allSatisfy { $0.branch == calls[0].branch }, "a retry changes one decision only")
    }

    @Test func anUnmergedBranchDefaultsToKeepAndDeleteIfMergedBlocksTearDown() async throws {
        let harness = FlowHarness(.dirtyWorktree)
        defer { harness.tearDown() }
        try await harness.start()
        let flow = TeardownFlow(model: harness.model, feature: feature("remotion"), preselect: .deleteIfMerged)
        await flow.load()
        #expect(flow.draft?.branch == .keep, "an unmerged branch is never preselected for deletion")
        #expect(flow.draft?.visibleBranchOptions == [.keep, .deleteIfMerged, .forceDelete])
        #expect(flow.canTearDown)

        flow.choose(.deleteIfMerged)
        #expect(flow.canTearDown == false)
        #expect(flow.draft?.blockingReason?.contains("3 commits") == true)
        #expect(flow.tearDown() == false)

        flow.choose(.forceDelete)
        #expect(flow.confirmingForceDelete)
        #expect(flow.draft?.branch == .deleteIfMerged, "Force-delete applies only after its confirmation")
        #expect(flow.forceDeleteMessage.contains("3 commits"))
        flow.confirmForceDelete()
        #expect(flow.draft?.branch == .forceDelete)
        #expect(flow.canTearDown)
        #expect(await harness.backend.calls(to: .teardownFeature).isEmpty)
    }

    @Test func aPreselectedForceDeleteIsIgnored() async throws {
        let harness = FlowHarness(.dirtyWorktree)
        defer { harness.tearDown() }
        try await harness.start()
        let flow = TeardownFlow(model: harness.model, feature: feature("remotion"), preselect: .forceDelete)
        await flow.load()
        #expect(flow.draft?.branch == .keep)
    }

    @Test func specNotPreservedOffersAConfirmedForcedRetryAndNotAWorktreeNever() async throws {
        let harness = FlowHarness()
        defer { harness.tearDown() }
        try await harness.start()
        let refusal: (String) -> BackendError = { code in
            .refused(Refusal(cause: .other(code: code), message: "The CLI refused (\(code))", diagnostics: Diagnostics(summary: code)))
        }
        await harness.backend.script(.teardownFeature, .fail(refusal("spec_not_preserved")))
        await harness.backend.script(.teardownFeature, .fail(refusal("not_a_worktree")))

        let spec = TeardownFlow(model: harness.model, feature: feature("prine"), preselect: nil)
        await spec.load()
        spec.tearDown()
        try await flowWaitUntilFinished(spec.record)
        let override = try #require(spec.specOverride)
        guard case .retry(.teardown(let request), _, true, let confirmation?) = override else {
            Issue.record("expected a destructive, confirmed retry")
            return
        }
        #expect(request.forceRemoval && request.branch == .keep && request.discard == nil)
        #expect(confirmation.contains("spec"))

        let worktree = TeardownFlow(model: harness.model, feature: feature("remotion"), preselect: nil)
        await worktree.load()
        worktree.tearDown()
        try await flowWaitUntilFinished(worktree.record)
        #expect(worktree.specOverride == nil)
        #expect(worktree.recoveries.allSatisfy { action in
            if case .retry(.teardown(let request), _, _, _) = action { return !request.forceRemoval }
            return true
        })
    }

    @Test func cleanupCommandsAreCopiedPerResidueKind() {
        let commands = TeardownResultView.cleanupCommands([
            ResidueItem(kind: "container", identifiers: ["abc123", "def456"]),
            ResidueItem(kind: "tool-request-volume", identifiers: ["vol one"]),
            ResidueItem(kind: "port-proxy", identifiers: ["proxy-1"]),
            ResidueItem(kind: "container-removal-error", identifiers: ["Cannot remove container abc123:\npermission denied"]),
            ResidueItem(kind: "container-cleanup-attempted", identifiers: ["already-gone"]),
        ])
        #expect(commands == "docker rm -f abc123 def456\ndocker volume rm 'vol one'\n# port-proxy: proxy-1\n"
            + "# container-removal-error: Cannot remove container abc123:\n# permission denied\n# container-cleanup-attempted: already-gone")
    }
}

// MARK: - Prune

@MainActor @Suite(.serialized) struct PruneFlowTests {
    @Test func aRefusedRowIsReportedAndTheOthersComplete() async throws {
        let harness = FlowHarness()
        defer { harness.tearDown() }
        try await harness.start()
        let flow = PruneFlow(model: harness.model, project: project)
        await flow.loadPlans()
        flow.selectNone()
        let names = flow.rows.prefix(3).map(\.feature.workFeature)
        #expect(names.count == 3)
        for name in names { flow.toggle(name) }
        #expect(flow.selected == Set(names))

        await harness.backend.script(.teardownFeature, .succeed(after: .zero))
        await harness.backend.script(.teardownFeature, .fail(uncommitted(["notes.txt"])))
        await harness.backend.script(.teardownFeature, .succeed(after: .zero))
        #expect(flow.prune())
        try await flowWaitUntilFinished(flow.record)

        let result = try #require(flow.result)
        #expect(result.rows.map(\.feature) == names)
        guard case .removed = result.rows[0].outcome, case .refused(let error) = result.rows[1].outcome,
              case .removed = result.rows[2].outcome else {
            Issue.record("expected removed, refused, removed; got \(result.rows.map(\.outcome))")
            return
        }
        #expect(PruneResultView.label(for: result.rows[1].outcome) == "Skipped: 1 uncommitted change")
        #expect(error.presentation(context: nil).details == ["notes.txt (untracked)"])
        #expect(PruneResultView.summaryLine(PruneResultView.Counts(result)) == "2 torn down · 0 partial · 1 refused · 0 failed")
        let calls = await harness.backend.calls(to: .teardownFeature).compactMap(\.teardownRequest)
        #expect(calls.map(\.feature.name) == names)
        #expect(calls.allSatisfy { $0.discard == nil && $0.branch == .keep })
    }

    @Test func checkingADirtyRowAsksAndGivesOnlyThatRowConsentForTheListedFiles() async throws {
        let harness = FlowHarness(.dirtyWorktree)
        defer { harness.tearDown() }
        try await harness.start()
        let flow = PruneFlow(model: harness.model, project: project)
        await flow.loadPlans()
        #expect(!flow.selected.contains("prine"), "a dirty row is never preselected")
        #expect(flow.rows.first { $0.feature.workFeature == "prine" }?.defaultReason == "2 uncommitted changes")

        flow.toggle("prine")
        #expect(flow.pendingConsent == "prine")
        #expect(!flow.selected.contains("prine"))
        flow.cancelConsent()
        #expect(flow.consents.isEmpty)

        flow.toggle("prine")
        let shown = flow.consentFiles("prine").map(\.path)
        flow.confirmConsent()
        #expect(flow.selected.contains("prine"))
        #expect(flow.consents["prine"]?.userFiles == shown)
        let requests = flow.selection.rows
        #expect(requests.first { $0.feature.name == "prine" }?.discard?.userFiles == ["README.md", "notes.txt"])
        #expect(requests.filter { $0.feature.name != "prine" }.allSatisfy { $0.discard == nil })

        flow.toggle("prine")
        #expect(flow.consents["prine"] == nil, "unchecking drops the consent")
    }

    @Test func plansAreCheckedAtMostFourAtATime() async throws {
        let harness = FlowHarness()
        defer { harness.tearDown() }
        try await harness.start()
        let flow = PruneFlow(model: harness.model, project: project)
        #expect(flow.features.count == 5)
        for _ in flow.features { await harness.backend.script(.planTeardown, .suspendUntilResumed) }
        let loading = Task { await flow.loadPlans() }
        #expect(await harness.backend.waitUntilSuspended(.planTeardown, count: 4))
        try await Task.sleep(for: .milliseconds(50))
        #expect(await harness.backend.suspendedCount(.planTeardown) == 4)
        await harness.backend.resumeAll()
        #expect(await harness.backend.waitUntilSuspended(.planTeardown, count: 1))
        await harness.backend.resumeAll()
        await loading.value
        #expect(flow.plans.count == 5)
        #expect(!flow.isChecking)
    }

    @Test func forceDeleteAsksFirstListingTheRows() async throws {
        let harness = FlowHarness(.dirtyWorktree)
        defer { harness.tearDown() }
        try await harness.start()
        let flow = PruneFlow(model: harness.model, project: project)
        await flow.loadPlans()
        flow.setPolicy(.forceDelete)
        flow.selectNone()
        flow.toggle("remotion")
        #expect(flow.prune() == false)
        #expect(flow.confirmingForceDelete)
        #expect(flow.forceDeleteMessage.contains("remotion: feature/remotion (3 unmerged commits)"))
        #expect(await harness.backend.calls(to: .teardownFeature).isEmpty)
    }

    @Test func aQueuedPruneThatIsStoppedFinishesWithoutRowStates() async throws {
        let harness = FlowHarness()
        defer { harness.tearDown() }
        try await harness.start()
        let flow = PruneFlow(model: harness.model, project: project)
        await flow.loadPlans()
        let names = flow.rows.map(\.feature.workFeature)
        let busy = try #require(names.first)
        let pruned = try #require(names.dropFirst().first)

        // A running teardown in the project: the prune (project-wide) queues behind it (D-16).
        await harness.backend.script(.teardownFeature, .suspendUntilResumed)
        let running = harness.model.actions.dispatch(.teardown(TeardownRequest(feature: feature(busy), recordedBranch: nil,
                                                                                branch: .keep)))
        guard case .started(let teardown) = running else {
            Issue.record("expected the teardown to start, got \(running)")
            return
        }
        #expect(await harness.backend.waitUntilSuspended(.teardownFeature))

        flow.selectNone()
        flow.toggle(pruned)
        #expect(flow.prune())
        let record = try #require(flow.record)
        guard case .queued = record.state else {
            Issue.record("expected the prune to queue, got \(record.state)")
            return
        }
        flow.stop.request(record)
        flow.stop.confirm(in: harness.model.operations)
        try await flowWaitUntilFinished(record)
        guard case .cancelled = record.state else {
            Issue.record("expected .cancelled, got \(record.state)")
            return
        }
        #expect(flow.result == nil)
        #expect(flow.runState(for: pruned) == nil, "a stopped prune shows no waiting rows")

        await harness.backend.resumeAll()
        try await flowWaitUntilFinished(teardown)
    }
}

// MARK: - Stray

@MainActor @Suite(.serialized) struct StrayFlowTests {
    @Test func removalNeverDiscardsFirstAndTheRefusalsRetryDoes() async throws {
        let harness = FlowHarness(.strays)
        defer { harness.tearDown() }
        try await harness.start()
        await harness.backend.script(.removeStray, .fail(uncommitted(["scratch.txt"])))
        let flow = StrayFlow(model: harness.model, project: project, stray: PreviewSamples.stray)
        flow.deleteBranchIfMerged = true
        #expect(flow.remove())
        try await flowWaitUntilFinished(flow.record)
        let failure = try #require(flow.record?.failure)
        let retry = try #require(discardRetry(RecoveryPlanner.recoveries(for: failure, after: flow.record?.context)))
        #expect(retry.confirmationMessage?.contains("scratch.txt") == true)
        flow.adopt(await FlowActions(model: harness.model).perform(retry))
        try await flowWaitUntilFinished(flow.record)
        flow.removalFinished()
        try await flowWaitUntilFinished(flow.branchRecord)

        let removals = await harness.backend.calls(to: .removeStray).compactMap { call -> Bool? in
            if case .removeStray(_, _, let discard) = call { return discard }
            return nil
        }
        #expect(removals == [false, true])
        let deletions = await harness.backend.calls(to: .deleteBranch)
        #expect(deletions == [.deleteBranch("spike/search", project, force: false)])
    }
}

// MARK: - Stop (D-18)

@MainActor @Suite(.serialized) struct StopConfirmationFlowTests {
    @Test func stopPresentsAConfirmationBeforeCancelling() async throws {
        let harness = FlowHarness(.legacy0134)
        defer { harness.tearDown() }
        try await harness.start()
        await harness.backend.script(.startFeature, steps: [
            .emit([.log(LogLine(timestamp: nil, level: .info, source: .stderr, target: nil, message: "Creating worktree"))]),
            .suspendUntilResumed,
        ])
        let flow = harness.startFlow()
        flow.setTitle("oauth")
        await flow.waitForPreview()
        #expect(flow.start())
        let record = try #require(flow.record)
        #expect(await harness.backend.waitUntilSuspended(.startFeature))

        flow.stop.request(record)
        #expect(flow.stop.pending === record)
        let confirmation = try #require(flow.stop.confirmation(capabilities: harness.model.environment.identity?.capabilities ?? []))
        #expect(confirmation.warnsAboutCorruption, "a legacy CLI's Stop warns about a partial worktree")
        #expect(confirmation.message.contains("partial worktree"))
        #expect(record.isCancellable, "asking to stop cancels nothing")

        flow.stop.dismiss()
        #expect(flow.stop.pending == nil)
        #expect(record.isCancellable)

        flow.stop.request(record)
        flow.stop.confirm(in: harness.model.operations)
        try await flowWaitUntilFinished(record)
        guard case .cancelled(let note) = record.state else {
            Issue.record("expected .cancelled, got \(record.state)")
            return
        }
        #expect(note?.contains("partial worktree") == true)
    }

    @Test func aContractCLIsStopConfirmationDoesNotWarnAboutCorruption() {
        let confirmation = CancelConfirmation(kind: .start, title: "Starting oauth", capabilities: PreviewSamples.allCapabilities)
        #expect(!confirmation.warnsAboutCorruption)
        #expect(confirmation.message.contains("partial worktree"))
        #expect(confirmation.message.contains("Needs attention"))
    }
}

// MARK: - Activity

@MainActor @Suite(.serialized) struct ActivityFlowTests {
    @Test func archivedLogLinesParseBack() {
        let line = ArchivedLog.parse("2026-10-02T15:30:45Z WARN   stderr compose: port 5432 is busy")
        #expect(line.level == .warn)
        #expect(line.source == .stderr)
        #expect(line.message == "compose: port 5432 is busy")
        #expect(line.timestamp != nil)
        #expect(ArchivedLog.parse("# Kind: start").message == "Kind: start")
        #expect(ArchivedLog.parse("garbage").level == .output)
    }

    @Test func entriesFilterByTargetAndAScriptedLongLogStaysWhole() async throws {
        let harness = FlowHarness()
        defer { harness.tearDown() }
        try await harness.start()
        let lines = (0..<2_000).map { index in
            ProgressEvent.log(LogLine(timestamp: nil, level: index % 100 == 0 ? .warn : .info, source: .stderr,
                                      target: nil, message: "line \(index)"))
        }
        await harness.backend.script(.startFeature, steps: [.emit(lines), .succeed(after: .zero)])
        let flow = harness.startFlow()
        flow.setTitle("long-log")
        await flow.waitForPreview()
        #expect(flow.start())
        try await flowWaitUntilFinished(flow.record)
        let record = try #require(flow.record)
        try await flowWaitUntil("the log did not fill") { record.log.lines.count >= 2_000 }

        let entries = ActivityEntry.all(in: harness.model.operations)
        #expect(entries.first?.id == record.id)
        #expect(entries.filter { $0.concerns(.feature(feature("long-log"))) }.count == 1)
        #expect(entries.filter { $0.concerns(.feature(feature("prine"))) }.isEmpty)
        #expect(entries.filter { $0.concerns(.project(project)) }.count == 1)
    }
}
