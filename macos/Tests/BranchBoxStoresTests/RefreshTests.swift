import BranchBoxKit
import BranchBoxPreview
@testable import BranchBoxStores
import Foundation
import Testing

// ProjectStore.refresh (§8.2, D-17): single flight with one coalesced re-run, the generation guard, last-good data
// on failure, the shared ListLimiter, ordering and attention; plus the RefreshCoordinator's timers.

@MainActor @Suite(.timeLimit(.minutes(1))) struct RefreshTests {
    @Test func threeOverlappingRefreshesListExactlyTwice() async throws {
        let harness = Harness()
        defer { harness.tearDown() }
        let store = try await harness.startWithSampleProject()
        await harness.backend.clearCalls()
        await harness.backend.script(.listFeatures, .suspendUntilResumed)

        let first = Task { await store.refresh(.manual) }
        #expect(await harness.backend.waitUntilSuspended(.listFeatures))
        let second = Task { await store.refresh(.registryChanged) }
        let third = Task { await store.refresh(.timer) }
        store.requestRefresh(.appActivated)
        try await Task.sleep(for: .milliseconds(20))
        #expect(await harness.backend.calls(to: .listFeatures).count == 1)   // the others only marked a re-run

        #expect(await harness.backend.resume(.listFeatures))
        await first.value
        await second.value
        await third.value

        #expect(await harness.backend.calls(to: .listFeatures).count == 2)
        #expect(store.features.count == 5)
        #expect(store.passes >= 2)
    }

    @Test func aStaleGenerationIsDropped() async throws {
        let harness = Harness()
        defer { harness.tearDown() }
        let store = try await harness.startWithSampleProject()
        guard case .loaded(let loadedAt) = store.loadState else { throw Failure("not loaded") }
        await harness.backend.clearCalls()
        // The pass in flight would show only prine; the next pass fails, so whatever is shown afterwards must be the
        // data from before the stale pass.
        await harness.backend.setListing(FeatureListing(features: PreviewSamples.features.filter { $0.workFeature == "prine" }),
                                         for: sampleProject)
        await harness.backend.script(.listFeatures, .suspendUntilResumed)
        let failure = BackendError.commandFailed(Diagnostics(summary: "Error: registry unreadable"))
        await harness.backend.script(.listFeatures, .fail(failure))

        let first = Task { await store.refresh(.manual) }
        #expect(await harness.backend.waitUntilSuspended(.listFeatures))
        store.includeRemoved = true                               // new generation; requests the --all re-run
        #expect(await harness.backend.resume(.listFeatures))
        await first.value

        #expect(await harness.backend.calls(to: .listFeatures)
            == [.listFeatures(sampleProject, includeRemoved: false), .listFeatures(sampleProject, includeRemoved: true)])
        #expect(store.features.count == 5)                         // the stale [prine] was dropped
        #expect(store.loadState == .failed(failure, lastGood: loadedAt))

        await store.refresh(.manual)
        #expect(store.features.map(\.workFeature) == ["prine"])
        #expect(store.includeRemoved)
    }

    @Test func aFailedLoadKeepsTheLastGoodData() async throws {
        let harness = Harness()
        defer { harness.tearDown() }
        let store = try await harness.startWithSampleProject()
        guard case .loaded(let loadedAt) = store.loadState else { throw Failure("not loaded") }

        let failure = BackendError.commandFailed(Diagnostics(summary: "Error: registry unreadable"))
        await harness.backend.script(.listFeatures, .fail(failure))
        await store.refresh(.manual)

        #expect(store.loadState == .failed(failure, lastGood: loadedAt))
        #expect(store.lastLoadedAt == loadedAt)
        #expect(store.features.count == 5)
        #expect(store.strays == [PreviewSamples.stray])

        await store.refresh(.manual)
        guard case .loaded = store.loadState else { throw Failure("expected a recovery, got \(store.loadState)") }
    }

    @Test func aFirstFailureHasNoLastGoodDate() async throws {
        let harness = Harness()
        defer { harness.tearDown() }
        await harness.model.start()
        let failure = BackendError.registryCorrupted(path: "/r/.branchbox/registry.json", diagnostics: Diagnostics(summary: "bad"))
        await harness.backend.script(.listFeatures, .fail(failure))
        _ = await harness.model.projects.add(folder: sampleProject.root)
        let store = try #require(harness.model.projects.project(sampleProject))
        try await waitUntil { store.loadState == .failed(failure, lastGood: nil) }
        #expect(store.features.isEmpty)
    }

    @Test func listingWithoutABackendFailsWithTheBootstrapError() async throws {
        let harness = Harness(.cliMissing)
        defer { harness.tearDown() }
        await harness.model.start()
        let entry = ProjectEntry(root: sampleProject.path, displayName: "branchbox", addedAt: .now)
        let store = ProjectStore(entry: entry, environment: harness.model.environment, limiter: ListLimiter(), clock: SystemClock())
        await store.refresh(.manual)
        #expect(store.loadState == .failed(.cliNotFound(searched: PreviewSamples.searchedPaths), lastGood: nil))
    }

    @Test func featuresAreOrderedAttentionFirstThenNewest() async throws {
        let harness = Harness(.interruptedSetup)
        defer { harness.tearDown() }
        let store = try await harness.startWithSampleProject()

        // Attention first (newest first, then by name), then the rest newest first.
        #expect(store.features.map(\.workFeature) == ["oauth", "sbx-demo", "orphan", "retained", "prine", "remotion"])
        #expect(store.attention.map(\.reason) == [.interrupted, .degraded, .orphaned, .failedRetained, .unregisteredWorktree])
        #expect(store.attention.last?.featureOrPath == PreviewSamples.stray.path)
        #expect(harness.model.projects.attentionCount == 5)
        #expect(store.strays == [PreviewSamples.stray])
        #expect(store.droppedRecords == 0)
    }

    @Test func missingFoldersNeedAttentionUnderTheCLI() async throws {
        let sandbox = Sandbox()
        defer { sandbox.remove() }
        let present = sandbox.folder("present")
        let records = [
            FeatureRecord(workFeature: "present", branchName: "feature/present", worktreePath: present.path, status: .active,
                          updatedAt: Date(timeIntervalSince1970: 100)),
            FeatureRecord(workFeature: "gone", branchName: "feature/gone", worktreePath: sandbox.directory.appendingPathComponent("gone").path,
                          status: .active, updatedAt: Date(timeIntervalSince1970: 50)),
            FeatureRecord(workFeature: "broken", branchName: "feature/broken", worktreePath: present.path, status: .active,
                          moduleOutcomes: [ModuleOutcome(module: "compose", status: .failed)]),
            FeatureRecord(workFeature: "old", status: .removed),
        ]
        let scenario = PreviewScenario(name: "cli", identity: .success(cliIdentity),
                                       listing: FeatureListing(features: records, droppedRecords: 2, warnings: ["w"]))
        let harness = Harness(scenario)
        defer { harness.tearDown() }
        let store = try await harness.startWithSampleProject()

        #expect(store.features.map(\.workFeature) == ["gone", "broken", "present"])
        #expect(store.attention.map(\.reason) == [.folderMissing, .setupIncomplete(module: "compose")])
        #expect(store.folderExists(for: records[0]))
        #expect(!store.folderExists(for: records[1]))
        #expect(!store.folderExists(for: FeatureRecord(workFeature: "nowhere")))
        #expect(store.droppedRecords == 2)
        #expect(store.listWarnings == ["w"])
        #expect(!store.rootExists)                                 // the sample root does not exist on this Mac
    }

    @Test(arguments: [
        (FeatureStatus.unknown("paused"), AttentionReason?.some(.unknownStatus("paused"))),
        (.removed, nil), (.active, nil), (.degraded, .degraded),
    ])
    func attentionTable(_ status: FeatureStatus, _ expected: AttentionReason?) {
        #expect(ProjectStore.attentionReason(for: FeatureRecord(workFeature: "x", status: status), folderExists: true) == expected)
    }

    @Test func interruptedSetupWinsOverStatus() {
        let record = FeatureRecord(workFeature: "x", status: .degraded, setup: SetupInfo(state: .interrupted))
        #expect(ProjectStore.attentionReason(for: record, folderExists: false) == .interrupted)
        let running = FeatureRecord(workFeature: "y", status: .active, setup: SetupInfo(state: .inProgress))
        #expect(ProjectStore.attentionReason(for: running, folderExists: true) == nil)
    }

    @Test func selectionLookupIsNilAfterRemoval() async throws {
        let harness = Harness()
        defer { harness.tearDown() }
        let store = try await harness.startWithSampleProject()
        #expect(store.feature(named: "prine") != nil)

        let teardown = try operation(of: harness.model.actions.dispatch(
            .teardown(TeardownRequest(feature: feature("prine"), recordedBranch: "feature/prine", branch: .keep))))
        try await waitUntilFinished(teardown)
        try await waitUntil { store.feature(named: "prine") == nil }

        store.includeRemoved = true
        try await waitUntil { store.feature(named: "prine")?.status == .removed }

        harness.model.projects.remove(sampleProject)
        #expect(harness.model.projects.project(sampleProject) == nil)
    }

    @Test func configAndDetectReloadThroughTheBackend() async throws {
        let harness = Harness(.legacy0134)
        defer { harness.tearDown() }
        let store = try await harness.startWithSampleProject()
        await store.reloadConfig()
        await store.reloadDetect()
        #expect(store.config?.editable == false)
        #expect(store.detect?.rawText != nil)                     // legacy detect is text
        #expect(store.configError == nil && store.detectError == nil)

        let failure = BackendError.commandFailed(Diagnostics(summary: "Error: no config"))
        await harness.backend.script(.readConfig, .fail(failure))
        await store.reloadConfig()
        #expect(store.configError == failure)
        #expect(store.config != nil)                               // the last good document stays
        #expect(await harness.backend.calls(to: .detect) == [.detect(sampleProject.root)])
    }

    // MARK: ListLimiter

    /// Which bodies the limiter let in, and the continuations that let them finish.
    @MainActor private final class LimiterProbe {
        var started: [Int] = []
        var finishers: [Int: CheckedContinuation<Void, Never>] = [:]

        func body(_ index: Int) async {
            started.append(index)
            await withCheckedContinuation { finishers[index] = $0 }
        }

        func finish(_ index: Int) {
            finishers.removeValue(forKey: index)?.resume()
        }
    }

    @Test func listLimiterRunsAtMostItsLimitInArrivalOrder() async throws {
        let limiter = ListLimiter(limit: 2)
        let probe = LimiterProbe()
        var tasks: [Task<Void, Never>] = []
        for index in 0..<5 {
            tasks.append(Task { await limiter.run { await probe.body(index) } })
            try await waitUntil { limiter.running + limiter.waiting == index + 1 }
        }
        try await waitUntil { probe.finishers.count == 2 }
        #expect(limiter.running == 2)
        #expect(limiter.waiting == 3)
        #expect(probe.started == [0, 1])

        probe.finish(1)
        try await waitUntil { probe.started.count == 3 && probe.finishers.count == 2 }
        #expect(probe.started == [0, 1, 2])
        #expect(limiter.running == 2)

        for index in [0, 2, 3, 4] {
            try await waitUntil { probe.finishers[index] != nil }
            probe.finish(index)
        }
        for task in tasks { await task.value }
        #expect(probe.started == [0, 1, 2, 3, 4])
        #expect(limiter.running == 0 && limiter.waiting == 0)
    }

    @Test func atMostTwoListsRunAcrossProjects() async throws {
        let harness = Harness()
        defer { harness.tearDown() }
        await harness.model.start()
        let roots = (0..<3).map { URL(fileURLWithPath: "/tmp/bbx-limit/p\($0)/main") }
        for _ in roots { await harness.backend.script(.listFeatures, .suspendUntilResumed) }
        for root in roots { _ = await harness.model.projects.add(folder: root) }

        #expect(await harness.backend.waitUntilSuspended(.listFeatures, count: 2))
        try await Task.sleep(for: .milliseconds(30))
        #expect(await harness.backend.calls(to: .listFeatures).count == 2)   // the third waits in the limiter

        #expect(await harness.backend.resume(.listFeatures))
        #expect(await harness.backend.waitUntilSuspended(.listFeatures, count: 2))
        #expect(await harness.backend.calls(to: .listFeatures).count == 3)
        await harness.backend.resumeAll()
        for root in roots {
            try await waitUntilLoaded(try #require(harness.model.projects.project(ProjectRef(root: root))))
        }
    }

    // MARK: RefreshCoordinator

    @Test func timersFollowSettingsAndActivation() async throws {
        let clock = ManualClock()
        let harness = Harness { $0.clock = clock }
        defer { harness.tearDown() }
        let store = try await harness.startWithSampleProject()
        let model = harness.model
        model.projects.selectedProject = sampleProject
        try await waitUntil { clock.sleeperCount == 2 }           // the selected-project and all-projects timers
        await harness.backend.clearCalls()

        // Inactive: the selected-project timer (60 s) does nothing.
        clock.advance(by: .seconds(60))
        try await waitUntil { clock.sleeperCount == 2 }
        try await Task.sleep(for: .milliseconds(20))
        #expect(await harness.backend.calls(to: .listFeatures).isEmpty)

        // Active: it refreshes the selected project.
        model.appDidBecomeActive()                                 // data is 60 s old: refreshed on activation too
        try await waitUntil { await harness.backend.calls(to: .listFeatures).count == 1 }
        try await waitUntilLoaded(store)
        clock.advance(by: .seconds(60))
        try await waitUntil { await harness.backend.calls(to: .listFeatures).count == 2 }
        try await waitUntilLoaded(store)

        // The all-projects timer (300 s) runs whether or not the app is active.
        model.appDidResignActive()
        try await waitUntil { clock.sleeperCount == 2 }
        clock.advance(by: .seconds(180))                           // 300 s since start
        try await waitUntil { await harness.backend.calls(to: .listFeatures).count == 3 }

        // Manual turns a timer off; the change applies at once.
        harness.settings.otherProjectsRefresh = .manual
        harness.settings.selectedProjectRefresh = .manual
        try await waitUntil { clock.sleeperCount == 0 }
        #expect(!model.coordinator.isRunning)
        harness.settings.selectedProjectRefresh = .s30
        try await waitUntil { clock.sleeperCount == 1 }
    }

    @Test func activationRefreshesOnlyStaleProjects() async throws {
        let clock = ManualClock()
        let harness = Harness { $0.clock = clock }
        defer { harness.tearDown() }
        let store = try await harness.startWithSampleProject()
        _ = await harness.model.projects.add(folder: URL(fileURLWithPath: "/tmp/bbx-stale/main"))
        let other = try #require(harness.model.projects.project(ProjectRef(root: URL(fileURLWithPath: "/tmp/bbx-stale/main"))))
        try await waitUntilLoaded(other)
        await harness.backend.clearCalls()

        clock.advance(by: .seconds(3))
        harness.model.appDidBecomeActive()
        try await Task.sleep(for: .milliseconds(30))
        #expect(await harness.backend.calls(to: .listFeatures).isEmpty)   // 3 s old: fresh enough

        clock.advance(by: .seconds(3))
        await other.refresh(.manual)                                // other is fresh again
        await harness.backend.clearCalls()
        harness.model.appDidBecomeActive()
        try await waitUntil { await harness.backend.calls(to: .listFeatures).count == 1 }
        #expect(await harness.backend.calls(to: .listFeatures) == [.listFeatures(store.ref, includeRemoved: false)])
        #expect(harness.model.isAppActive)
    }
}
