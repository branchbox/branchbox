import BranchBoxKit
import BranchBoxPreview
import BranchBoxTestSupport
import Foundation
import Testing

// The PreviewBackend every store suite runs on: scripting, cancellation, the call log and the scenarios.

private let prine = feature("prine")
private let remotion = feature("remotion")

private func backendError(_ error: any Error) -> BackendError? { error as? BackendError }

private func promptedStart(_ name: String = "oauth") -> StartFeatureRequest {
    var request = startRequest(name)
    request.prompt = "Add OAuth login"
    request.skipModules = ["tunnel"]
    return request
}

@Suite struct PreviewBackendTests {
    @Test func scriptedStartEmitsEventsAndReturnsASummary() async throws {
        let backend = PreviewBackend(scenario: .contract)
        let events: [ProgressEvent] = [
            .phase(.creatingWorktree),
            .log(LogLine(timestamp: nil, level: .info, source: .stderr, target: "worktree_core", message: "Creating worktree")),
            .phase(.module("compose")),
            .warning("Prompt truncated"),
        ]
        await backend.script(.startFeature, .emit(events))
        let sink = EventSink()

        let summary = try await backend.startFeature(promptedStart(), progress: sink.sink)

        #expect(sink.events == events)
        #expect(summary.workFeature == "oauth")
        #expect(summary.branchName == "feature/oauth")
        #expect(summary.worktreePath == "/Users/dev/projects/branchbox-suite/branchbox/oauth")
        #expect(summary.skippedModules == [SkippedModule(module: "tunnel", reason: "Skipped by request")])
        #expect(summary.moduleOutcomes.first { $0.module == "tunnel" }?.status == .skipped)
        let listing = try await backend.listFeatures(in: sampleProject, includeRemoved: false)
        #expect(listing.features.first { $0.workFeature == "oauth" }?.status == .active)
    }

    @Test(.timeLimit(.minutes(1)), arguments: [PreviewBehavior.suspendUntilResumed, .succeed(after: .seconds(30))])
    func cancellationThrowsCancelledWithin100ms(_ behavior: PreviewBehavior) async throws {
        let backend = PreviewBackend(scenario: .contract)
        await backend.script(.startFeature, behavior)
        let task = Task { try await backend.startFeature(promptedStart(), progress: { _ in }) }
        try await waitUntil { await backend.calls(to: .startFeature).count == 1 }
        try await Task.sleep(for: .milliseconds(10))       // let the call reach its waiting step

        let clock = ContinuousClock()
        let cancelledAt = clock.now
        task.cancel()
        let result = await task.result

        #expect(clock.now - cancelledAt < .milliseconds(100))
        guard case .failure(let error) = result else {
            Issue.record("expected .cancelled, got \(result)")
            return
        }
        #expect(backendError(error) == .cancelled(note: nil))
        let listing = try await backend.listFeatures(in: sampleProject, includeRemoved: true)
        #expect(!listing.features.contains { $0.workFeature == "oauth" })
    }

    @Test func alreadyCancelledCallThrowsCancelled() async {
        let backend = PreviewBackend(scenario: .contract)
        let task = Task {
            withUnsafeCurrentTask { $0?.cancel() }
            return try await backend.listFeatures(in: sampleProject, includeRemoved: false)
        }
        let result = await task.result
        if case .failure(let error) = result {
            #expect(backendError(error) == .cancelled(note: nil))
        } else {
            Issue.record("expected .cancelled")
        }
        #expect(await backend.calls.count == 1)               // still recorded
    }

    @Test func callLogRecordsFullRequestValues() async throws {
        let backend = PreviewBackend(scenario: .contract)
        let teardown = TeardownRequest(feature: prine, recordedBranch: "feature/prine", branch: .deleteIfMerged)
        _ = try await backend.planTeardown(teardown)
        let outcome = try await backend.teardownFeature(teardown, progress: { _ in })
        _ = try await backend.startFeature(promptedStart(), progress: { _ in })

        let calls = await backend.calls
        #expect(calls.map(\.method) == [.planTeardown, .teardownFeature, .startFeature])
        let firstTeardown = try #require(calls.compactMap(\.teardownRequest).first)
        #expect(firstTeardown == teardown)
        #expect(firstTeardown.discard == nil)
        #expect(calls.compactMap(\.startRequest).first == promptedStart())
        #expect(outcome.branch == .deleted("feature/prine", by: .cli))
        #expect(outcome.summary.branchAction == "delete")
        #expect(await backend.calls(to: .teardownFeature) == [.teardownFeature(teardown)])
    }

    @Test func scriptedFailureAppliesToTheNextCallOnly() async throws {
        let backend = PreviewBackend(scenario: .contract)
        let failure = BackendError.commandFailed(Diagnostics(summary: "Error: registry unreadable"))
        await backend.script(.listFeatures, .fail(failure))

        await #expect(throws: BackendError.self) { _ = try await backend.listFeatures(in: sampleProject, includeRemoved: false) }
        let listing = try await backend.listFeatures(in: sampleProject, includeRemoved: false)
        #expect(listing.features.count == 5)
    }

    @Test(.timeLimit(.minutes(1)))
    func resumeLetsASuspendedCallFinish() async throws {
        let backend = PreviewBackend(scenario: .contract)
        await backend.script(.listFeatures, steps: [.emit([.phase(.preparing)]), .suspendUntilResumed])
        let task = Task { try await backend.listFeatures(in: sampleProject, includeRemoved: false) }

        #expect(await backend.waitUntilSuspended(.listFeatures))
        #expect(await backend.suspendedCount(.listFeatures) == 1)
        #expect(await backend.resume(.listFeatures))
        #expect(try await task.value.features.count == 5)
        #expect(await backend.resume(.listFeatures) == false)
    }

    @Test(.timeLimit(.minutes(1)))
    func terminateAllCancelsWaitingCalls() async throws {
        let bootstrapper = PreviewBootstrapper(scenario: .contract)
        await bootstrapper.backend.script(.exec, .suspendUntilResumed)
        let request = ExecRequest(feature: prine, command: ["sleep", "60"])
        let task = Task { try await bootstrapper.backend.exec(request, progress: { _ in }) }
        #expect(await bootstrapper.backend.waitUntilSuspended(.exec))

        await bootstrapper.terminateAllProcesses()

        let result = await task.result
        if case .failure(let error) = result {
            #expect(backendError(error) == .cancelled(note: nil))
        } else {
            Issue.record("expected .cancelled")
        }
    }

    @Test func builtInScenarios() async throws {
        let legacy = try await PreviewBackend(scenario: .legacy0134).identity()
        #expect(legacy.version == SemVer(0, 13, 4))
        #expect(legacy.isLegacy)
        #expect(legacy.capabilities.isEmpty)

        let contract = try await PreviewBackend(scenario: .contract).identity()
        #expect(contract.contractVersion == 1)
        #expect(contract.capabilities.count == 14)
        #expect(contract.supports(.teardownPlan) && contract.supports(.initJSON))

        let empty = try await PreviewBackend(scenario: .emptyProject).listFeatures(in: sampleProject, includeRemoved: true)
        #expect(empty.features.isEmpty && empty.strays.isEmpty)

        let missing = PreviewBackend(scenario: .cliMissing)
        await #expect(throws: BackendError.cliNotFound(searched: PreviewSamples.searchedPaths)) {
            _ = try await missing.listFeatures(in: sampleProject, includeRemoved: false)
        }
        guard case .unavailable(.cliNotFound, nil) = await PreviewBootstrapper(scenario: .cliMissing).bootstrap(BackendSettings()) else {
            Issue.record("cliMissing should bootstrap as unavailable")
            return
        }
        for scenario in PreviewScenario.all {
            #expect(PreviewScenario.named(scenario.name) == scenario)
        }
        #expect(Set(PreviewScenario.all.map(\.name)).count == PreviewScenario.all.count)
        #expect(PreviewScenario.named("nope") == nil)
    }

    @Test func embeddedSamplesMatchTheFixtures() throws {
        let all = try CLIJSON.decode([FeatureRecord].self, from: Fixtures.data("cli-0.13.4/main_feature_list_all.json")).value
        let synthetic = try CLIJSON.decode([FeatureRecord].self,
                                           from: Fixtures.data("cli-0.13.4/synthetic_feature_list_new_statuses.json")).value
        #expect(PreviewSamples.features == all + synthetic)
        #expect(PreviewSamples.features.count == 11)
    }

    @Test func listingHidesRemovedFeaturesUnlessAsked() async throws {
        let backend = PreviewBackend(scenario: .legacy0134)
        let current = try await backend.listFeatures(in: sampleProject, includeRemoved: false)
        #expect(current.features.map(\.workFeature) == ["prine", "remotion", "sbx-demo", "retained", "orphan"])
        #expect(current.strays == [PreviewSamples.stray])
        let all = try await backend.listFeatures(in: sampleProject, includeRemoved: true)
        #expect(all.features.count == 11)
    }

    @Test func legacyRefusesContractOnlyCalls() async throws {
        let backend = PreviewBackend(scenario: .legacy0134)
        let patch = ConfigPatch(changes: [ConfigChange(key: "feature.branch_prefix", value: .string("spike"))])
        await #expect(throws: BackendError.unsupported(.config, minimumCLI: "0.14.0")) {
            _ = try await backend.applyConfig(patch, to: sampleProject, dryRun: true)
        }
        #expect(try await backend.readConfig(sampleProject).editable == false)
        #expect(try await backend.planTeardown(TeardownRequest(feature: prine, recordedBranch: nil, branch: .keep)).source == .appPreflight)

        let contract = PreviewBackend(scenario: .contract)
        let applied = try await contract.applyConfig(patch, to: sampleProject, dryRun: true)
        #expect(applied.changed == [ConfigApplyResult.Change(key: "feature.branch_prefix", old: .string("feature"), new: .string("spike"))])
    }

    @Test func teardownRemovesTheFeatureAndRefusesUnknownOnes() async throws {
        let backend = PreviewBackend(scenario: .contract)
        let outcome = try await backend.teardownFeature(TeardownRequest(feature: prine, recordedBranch: nil, branch: .keep),
                                                        progress: { _ in })
        #expect(outcome.branch == .kept("feature/prine"))
        #expect(outcome.worktreeGone)
        let all = try await backend.listFeatures(in: sampleProject, includeRemoved: true)
        #expect(all.features.first { $0.workFeature == "prine" }?.status == .removed)

        await #expect(throws: BackendError.self) {
            _ = try await backend.teardownFeature(TeardownRequest(feature: prine, recordedBranch: nil, branch: .keep),
                                                  progress: { _ in })
        }
    }

    @Test func startRefusesANameInUse() async {
        let backend = PreviewBackend(scenario: .contract)
        do {
            _ = try await backend.startFeature(startRequest("prine"), progress: { _ in })
            Issue.record("expected a refusal")
        } catch BackendError.refused(let refusal) {
            #expect(refusal.cause == .worktreeExists(path: "/Users/dev/projects/branchbox-suite/branchbox/prine"))
            #expect(refusal.message.contains("prine"))
        } catch {
            Issue.record("unexpected \(error)")
        }
    }

    @Test func previewCommandLineIsNil() {
        #expect(PreviewBackend().previewCommandLine(.start(promptedStart())) == nil)
    }

    // MARK: Scenarios added by SW-2

    @Test(arguments: ["dirtyWorktree", "legacyDirtyWorktree"])
    func dirtyTeardownIsRefusedWithThePlanUntilTheUserConsents(_ name: String) async throws {
        let scenario = try #require(PreviewScenario.named(name))
        let backend = PreviewBackend(scenario: scenario)
        var request = TeardownRequest(feature: prine, recordedBranch: "feature/prine", branch: .keep)

        let plan = try await backend.planTeardown(request)
        #expect(plan.changes.user == PreviewSamples.dirtyFiles)
        #expect(plan.blockers.map(\.kind) == ["uncommitted_changes"])
        #expect(plan.source == (scenario == .dirtyWorktree ? .cli : .appPreflight))

        do {
            _ = try await backend.teardownFeature(request, progress: { _ in })
            Issue.record("a dirty worktree must refuse a teardown without consent")
        } catch BackendError.refused(let refusal) {
            #expect(refusal.cause == .uncommittedChanges(files: PreviewSamples.dirtyFiles))
            #expect(refusal.message.contains("README.md") && refusal.message.contains("nothing was removed"))
            #expect(refusal.plan?.changes.user == PreviewSamples.dirtyFiles)
        }

        // Consent to only some of the files is not enough.
        request.discard = DiscardConsent(userFiles: ["README.md"])
        await #expect(throws: BackendError.self) { _ = try await backend.teardownFeature(request, progress: { _ in }) }

        request.discard = DiscardConsent(userFiles: PreviewSamples.dirtyFiles.map(\.path))
        let outcome = try await backend.teardownFeature(request, progress: { _ in })
        #expect(outcome.summary.discardedChanges == PreviewSamples.dirtyFiles)
        #expect(await backend.calls(to: .teardownFeature).count == 3)
    }

    @Test func unmergedBranchRefusesOnContractAndFailsToDeleteOnLegacy() async throws {
        let request = TeardownRequest(feature: remotion, recordedBranch: "feature/remotion", branch: .deleteIfMerged)

        let contract = PreviewBackend(scenario: .dirtyWorktree)
        let plan = try await contract.planTeardown(request)
        #expect(plan.branch?.merged == false && plan.branch?.ahead == 3)
        #expect(plan.blockers.map(\.kind) == ["unmerged_branch"])
        do {
            _ = try await contract.teardownFeature(request, progress: { _ in })
            Issue.record("expected an unmerged-branch refusal")
        } catch BackendError.refused(let refusal) {
            #expect(refusal.cause == .unmergedBranch(branch: "feature/remotion", ahead: 3))
            #expect(refusal.plan != nil)
        }

        let legacy = PreviewBackend(scenario: .legacyDirtyWorktree)
        let outcome = try await legacy.teardownFeature(request, progress: { _ in })
        guard case .deleteFailed("feature/remotion", let reason) = outcome.branch else {
            Issue.record("expected the app-side delete to fail, got \(outcome.branch)")
            return
        }
        #expect(reason.contains("not fully merged"))
        #expect(outcome.summary.branchDeleteError == reason)

        var force = request
        force.branch = .forceDelete
        let forced = try await PreviewBackend(scenario: .legacyDirtyWorktree).teardownFeature(force, progress: { _ in })
        #expect(forced.branch == .deleted("feature/remotion", by: .app))
    }

    @Test func interruptedSetupAndStraysScenarios() async throws {
        let interrupted = try await PreviewBackend(scenario: .interruptedSetup).listFeatures(in: sampleProject, includeRemoved: false)
        let oauth = try #require(interrupted.features.first { $0.workFeature == "oauth" })
        #expect(oauth.status == .active)
        #expect(oauth.setup?.state == .interrupted)

        let strays = try await PreviewBackend(scenario: .strays).listFeatures(in: sampleProject, includeRemoved: false)
        #expect(strays.strays == PreviewSamples.strays)
        #expect(strays.strays.contains { $0.locked } && strays.strays.contains { $0.prunable })
    }

    @Test func scenarioIdentityCanBeSwapped() async throws {
        let swapped = PreviewScenario.contract.with(identity: .success(cliIdentity), name: "cli")
        #expect(swapped.name == "cli")
        #expect(swapped.listing == PreviewScenario.contract.listing)
        #expect(try await PreviewBackend(scenario: swapped).identity() == cliIdentity)
    }

    @Test func resolutionsCanBeScripted() async throws {
        let backend = PreviewBackend(scenario: .contract)
        let folder = URL(fileURLWithPath: "/tmp/bbx/fresh")
        let resolution = ProjectResolution(project: ProjectRef(root: folder), requested: folder, normalization: .none,
                                           initialized: false)
        await backend.setResolution(resolution, for: folder)
        #expect(try await backend.resolveProject(at: URL(fileURLWithPath: "/tmp/bbx/fresh/")) == resolution)
        #expect(try await backend.resolveProject(at: URL(fileURLWithPath: "/tmp/bbx/other")).initialized)
        #expect(await backend.currentListing(for: sampleProject) == PreviewSamples.listing)
    }
}
