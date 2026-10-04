import AppKit
@testable import BranchBoxApp
import BranchBoxKit
import BranchBoxPreview
import BranchBoxStores
import SwiftUI
import Testing

/// Renders every SW-6 sheet, result and Activity surface in its significant states for visual review.
/// Run with BRANCHBOX_RENDER_DIR=<dir>.
@MainActor
@Suite(.enabled(if: SnapshotRenderer.isEnabled), .serialized)
struct FlowRenderTests {
    private static let startSize = CGSize(width: 640, height: 720)
    private static let teardownSize = CGSize(width: 600, height: 640)
    private static let pruneSize = CGSize(width: 780, height: 600)
    private static let straySize = CGSize(width: 560, height: 500)

    private func feature(_ name: String) -> FeatureRef { FeatureRef(project: flowSampleProject, name: name) }

    private func render<V: View>(_ name: String, _ size: CGSize, _ harness: FlowHarness, @ViewBuilder _ view: () -> V) throws {
        try SnapshotRenderer.render(name, size: size) {
            view().environment(harness.model)
        }
    }

    private func logEvents(_ count: Int, phase: OperationPhase) -> [ProgressEvent] {
        let messages = ["Creating worktree at ../oauth", "Copying .devcontainer", "Allocating ports 49152-49153",
                        "Writing .env", "docker compose up -d db", "Waiting for postgres to accept connections",
                        "Running bin/setup", "Installing gems (bundle install)"]
        var events: [ProgressEvent] = [.phase(phase)]
        for index in 0..<count {
            let level: LogLevel = index == 5 ? .warn : .info
            events.append(.log(LogLine(timestamp: Date(timeIntervalSince1970: 1_790_000_000 + Double(index)), level: level,
                                       source: .stderr, target: "worktree_core::modules::compose",
                                       message: messages[index % messages.count])))
        }
        return events
    }

    // MARK: Start

    @Test func startForm() async throws {
        let harness = FlowHarness()
        defer { harness.tearDown() }
        try await harness.start()
        let empty = harness.startFlow()
        await empty.prepare()
        try render("start-form-empty", Self.startSize, harness) { StartFeatureSheet(flow: empty) }

        harness.model.settings.recordPrompt("Implement the OAuth callback and add request specs")
        let filled = harness.startFlow()
        await filled.prepare()
        filled.setTitle("Add OAuth login with Google and GitHub for the admin dashboard")
        filled.setPrompt("Implement the OAuth callback controller, store the provider tokens encrypted, and add request "
            + "specs for the happy path and a revoked token.")
        filled.setSkipped("tunnel", true)
        filled.showsAdvanced = true
        await filled.waitForPreview()
        try render("start-form-filled", CGSize(width: 640, height: 900), harness) { StartFeatureSheet(flow: filled) }

        let invalid = harness.startFlow()
        await invalid.prepare()
        invalid.setTitle("prine")
        invalid.setPrompt(String(repeating: "Refactor the session store. ", count: 80))
        await invalid.waitForPreview()
        try render("start-form-errors", Self.startSize, harness) { StartFeatureSheet(flow: invalid) }
    }

    @Test func startFormVariants() async throws {
        let harness = FlowHarness()
        defer { harness.tearDown() }
        try await harness.start()
        _ = await harness.model.projects.add(folder: URL(fileURLWithPath: "/Users/dev/projects/remotion/main"))
        let picker = StartFlow(model: harness.model, project: nil, prefill: nil, memory: harness.memory, debounce: .zero)
        await picker.prepare()
        try render("start-form-project-picker", Self.startSize, harness) { StartFeatureSheet(flow: picker) }

        var sbx = StartFeatureRequest(project: flowSampleProject, name: "sbx-retry", runtime: .sbx)
        sbx.mode = .minimal
        sbx.useDefaultPrompt = true
        sbx.keepRuntimeOnFailure = true
        let prefilled = harness.startFlow(prefill: sbx)
        await prefilled.prepare()
        prefilled.showsAdvanced = true
        await prefilled.waitForPreview()
        try render("start-form-prefill-sbx", CGSize(width: 640, height: 860), harness) { StartFeatureSheet(flow: prefilled) }
    }

    @Test func startWithoutProjectsOrCLI() async throws {
        let missing = FlowHarness(.cliMissing)
        defer { missing.tearDown() }
        await missing.model.start()
        let flow = StartFlow(model: missing.model, project: nil, prefill: nil, memory: missing.memory, debounce: .zero)
        try render("start-cli-missing", Self.startSize, missing) { StartFeatureSheet(flow: flow) }

        let empty = FlowHarness(.emptyProject)
        defer { empty.tearDown() }
        await empty.model.start()                      // loaded, with no projects: the real empty state
        let none = StartFlow(model: empty.model, project: nil, prefill: nil, memory: empty.memory, debounce: .zero)
        try render("start-no-projects", Self.startSize, empty) { StartFeatureSheet(flow: none) }
    }

    @Test func startRunningAndResults() async throws {
        let harness = FlowHarness(.legacy0134)
        defer { harness.tearDown() }
        try await harness.start()
        await harness.backend.script(.startFeature, steps: [.emit(logEvents(40, phase: .module("compose"))), .suspendUntilResumed])
        let running = harness.startFlow()
        running.setTitle("oauth")
        await running.waitForPreview()
        running.start()
        _ = await harness.backend.waitUntilSuspended(.startFeature)
        try await Task.sleep(for: .milliseconds(200))
        try render("start-running-legacy", Self.startSize, harness) { StartFeatureSheet(flow: running) }
        running.stop.request(try #require(running.record))
        running.stop.confirm(in: harness.model.operations)
        try await flowWaitUntilFinished(running.record)
        try render("start-cancelled-legacy", Self.startSize, harness) { StartFeatureSheet(flow: running) }

        await harness.backend.script(.startFeature, .fail(.refused(Refusal(
            cause: .worktreeExists(path: "/Users/dev/projects/branchbox-suite/branchbox/oauth"),
            message: "Worktree path /Users/dev/projects/branchbox-suite/branchbox/oauth already exists",
            diagnostics: Diagnostics(summary: "Worktree path already exists", exitCode: 1,
                                     invocation: "branchbox feature start oauth --runtime container --json")))))
        let failed = harness.startFlow()
        failed.setTitle("oauth")
        await failed.waitForPreview()
        failed.start()
        try await flowWaitUntilFinished(failed.record)
        try render("start-failed-folder-exists", Self.startSize, harness) { StartFeatureSheet(flow: failed) }

        let done = harness.startFlow()
        done.setTitle("Add OAuth login with Google and GitHub for the admin dashboard")
        await done.waitForPreview()
        done.start()
        try await flowWaitUntilFinished(done.record)
        try render("start-result-plain", Self.startSize, harness) { StartFeatureSheet(flow: done) }
        let record = try #require(done.record)
        try render("start-result-warnings", CGSize(width: 640, height: 940), harness) {
            StartResultView(flow: done, summary: Self.richSummary, record: record, actions: FlowActions(model: harness.model),
                            onDone: {})
        }
    }

    private static let richSummary: StartSummary = {
        let name = "oauth-google-github-admin-dashboard"
        return StartSummary(
            workFeature: name, branchName: "feature/\(name)",
            worktreePath: "/Users/dev/projects/branchbox-suite/branchbox/\(name)", mode: "full",
            featureURL: "dev-oauth.localhost",
            runtime: RuntimeInfo(provider: .container, publishedPorts: [PublishedPort(host: 49152, runtime: 3000),
                                                                        PublishedPort(host: 49153, runtime: 5432)]),
            moduleOutcomes: [
                ModuleOutcome(module: "devcontainer", status: .success, durationMs: 420),
                ModuleOutcome(module: "compose", status: .success, durationMs: 18_230, notes: ["db, redis"]),
                ModuleOutcome(module: "database", status: .success, durationMs: 6_100),
                ModuleOutcome(module: "specs", status: .success, durationMs: 12),
            ],
            skippedModules: [SkippedModule(module: "tunnel", reason: "Tunnels are off for this project")],
            warnings: ["Stashed 2 uncommitted changes from main as stash@{0}; run `git stash pop` in main to restore them",
                       "compose took 18 s; consider caching the postgres image"],
            adapter: AdapterInfo(name: "rails", serviceURL: "http://app:3000",
                                 warnings: ["Adapter: no service URL detected for the worker service"]),
            tunnel: TunnelState(provider: "cloudflared", hostname: "oauth.example.dev", status: .active))
    }()

    // MARK: Teardown

    @Test func teardownPlans() async throws {
        let harness = FlowHarness(.dirtyWorktree)
        defer { harness.tearDown() }
        try await harness.start()
        await harness.backend.script(.planTeardown, .suspendUntilResumed)
        let checking = TeardownFlow(model: harness.model, feature: feature("prine"), preselect: nil)
        let loading = Task { await checking.load() }
        _ = await harness.backend.waitUntilSuspended(.planTeardown)
        try render("teardown-checking", Self.teardownSize, harness) { TeardownSheet(flow: checking) }
        await harness.backend.resumeAll()
        await loading.value
        try render("teardown-dirty", Self.teardownSize, harness) { TeardownSheet(flow: checking) }

        let unmerged = TeardownFlow(model: harness.model, feature: feature("remotion"), preselect: nil)
        await unmerged.load()
        unmerged.choose(.deleteIfMerged)
        try render("teardown-unmerged-blocked", Self.teardownSize, harness) { TeardownSheet(flow: unmerged) }

        let clean = TeardownFlow(model: harness.model, feature: feature("sbx-demo"), preselect: .deleteIfMerged)
        await clean.load()
        try render("teardown-clean-sbx-tunnel", Self.teardownSize, harness) { TeardownSheet(flow: clean) }

        checking.tearDown()
        try await flowWaitUntilFinished(checking.record)
        try render("teardown-refused-dirty", Self.teardownSize, harness) { TeardownSheet(flow: checking) }
    }

    @Test func teardownLegacyAndResults() async throws {
        let legacy = FlowHarness(.legacyDirtyWorktree)
        defer { legacy.tearDown() }
        try await legacy.start()
        let plan = TeardownFlow(model: legacy.model, feature: feature("prine"), preselect: nil)
        await plan.load()
        try render("teardown-legacy-preflight", Self.teardownSize, legacy) { TeardownSheet(flow: plan) }

        let harness = FlowHarness(.dirtyWorktree)
        defer { harness.tearDown() }
        try await harness.start()
        let flow = TeardownFlow(model: harness.model, feature: feature("remotion"), preselect: nil)
        await flow.load()
        flow.tearDown()
        try await flowWaitUntilFinished(flow.record)
        try render("teardown-result-kept", Self.teardownSize, harness) { TeardownSheet(flow: flow) }
        let record = try #require(flow.record)
        try render("teardown-result-problems", CGSize(width: 600, height: 860), harness) {
            TeardownResultView(flow: flow, outcome: Self.problemOutcome, record: record,
                               actions: FlowActions(model: harness.model), onDone: {})
        }
        try render("teardown-result-clean", Self.teardownSize, harness) {
            TeardownResultView(flow: flow, outcome: Self.cleanOutcome, record: record,
                               actions: FlowActions(model: harness.model), onDone: {})
        }
    }

    private static let problemOutcome = TeardownOutcome(
        summary: TeardownSummary(
            workFeature: "remotion", branchName: "feature/remotion", worktreeRemoved: true,
            moduleReports: [ModuleReport(name: "compose", teardownOk: true),
                            ModuleReport(name: "database", teardownOk: false, errors: ["drop database failed: connection refused"])],
            runtimeTeardown: RuntimeTeardownReport(provider: "container", verified: true, residueFree: false, residue: [
                ResidueItem(kind: "container", identifiers: ["branchbox-remotion-app-1", "branchbox-remotion-db-1"]),
                ResidueItem(kind: "volume", identifiers: ["branchbox-remotion_pgdata"]),
            ]),
            warnings: ["Worktree removed manually after git removal failed",
                       "Adapter cleanup skipped tmp/cache: permission denied"]),
        branch: .deleteFailed("feature/remotion", reason: "error: the branch 'feature/remotion' is not fully merged"),
        worktreeGone: true)

    private static let cleanOutcome = TeardownOutcome(
        summary: TeardownSummary(
            workFeature: "remotion", branchName: "feature/remotion", worktreeRemoved: true, branchDeleted: true,
            moduleReports: [ModuleReport(name: "compose", teardownOk: true), ModuleReport(name: "database", teardownOk: true)],
            runtimeTeardown: RuntimeTeardownReport(provider: "container", verified: true, residueFree: true),
            discardedChanges: [ChangedFile(path: "notes.txt", kind: "untracked", area: "other")],
            preserved: [PreservedFile(path: "docs/features/in-progress/remotion.md",
                                      destination: "docs/features/backlog/remotion.md")]),
        branch: .deleted("feature/remotion", by: .cli), worktreeGone: true)

    // MARK: Prune

    @Test func pruneStates() async throws {
        let harness = FlowHarness(.dirtyWorktree)
        defer { harness.tearDown() }
        try await harness.start()
        for _ in 0..<5 { await harness.backend.script(.planTeardown, .suspendUntilResumed) }
        let flow = PruneFlow(model: harness.model, project: flowSampleProject)
        let loading = Task { await flow.loadPlans() }
        _ = await harness.backend.waitUntilSuspended(.planTeardown, count: 4)
        try render("prune-checking", Self.pruneSize, harness) { PruneSheet(flow: flow) }
        await harness.backend.resumeAll()
        _ = await harness.backend.waitUntilSuspended(.planTeardown, count: 1)
        await harness.backend.resumeAll()
        await loading.value
        try render("prune-ready", Self.pruneSize, harness) { PruneSheet(flow: flow) }

        flow.setPolicy(.deleteIfMerged)
        flow.toggle("prine")
        try render("prune-consent-popover", CGSize(width: 380, height: 260), harness) {
            ConsentPopover(flow: flow, name: "prine")
        }
        flow.confirmConsent()
        try render("prune-selected-delete-if-merged", Self.pruneSize, harness) { PruneSheet(flow: flow) }

        await harness.backend.script(.teardownFeature, .succeed(after: .zero))
        await harness.backend.script(.teardownFeature, steps: [.emit(logEvents(12, phase: .removingWorktree)),
                                                               .suspendUntilResumed])
        flow.prune()
        _ = await harness.backend.waitUntilSuspended(.teardownFeature)
        try await Task.sleep(for: .milliseconds(200))
        try render("prune-running", Self.pruneSize, harness) { PruneSheet(flow: flow) }
        await harness.backend.resumeAll()
        try await flowWaitUntilFinished(flow.record)
        try render("prune-result", Self.pruneSize, harness) { PruneSheet(flow: flow) }
    }

    @Test func pruneRefusedAndEmpty() async throws {
        let harness = FlowHarness()
        defer { harness.tearDown() }
        try await harness.start()
        let flow = PruneFlow(model: harness.model, project: flowSampleProject)
        await flow.loadPlans()
        flow.selectAll()
        await harness.backend.script(.teardownFeature, .succeed(after: .zero))
        await harness.backend.script(.teardownFeature, .fail(uncommitted(["notes.txt", "tmp/scratch.rb"])))
        await harness.backend.script(.teardownFeature, .fail(.commandFailed(Diagnostics(
            summary: "sbx rm failed: sandbox is busy", exitCode: 1, logTail: ["Error: sbx rm failed: sandbox is busy"]))))
        flow.prune()
        try await flowWaitUntilFinished(flow.record)
        try render("prune-result-refused", CGSize(width: 780, height: 760), harness) { PruneSheet(flow: flow) }

        let empty = FlowHarness(.emptyProject)
        defer { empty.tearDown() }
        try await empty.start()
        let none = PruneFlow(model: empty.model, project: flowSampleProject)
        try render("prune-empty", Self.pruneSize, empty) { PruneSheet(flow: none) }
    }

    // MARK: Stray

    @Test func strayStates() async throws {
        let harness = FlowHarness(.strays)
        defer { harness.tearDown() }
        try await harness.start()
        let plain = StrayFlow(model: harness.model, project: flowSampleProject, stray: PreviewSamples.stray)
        plain.deleteBranchIfMerged = true
        try render("stray-details", Self.straySize, harness) { StrayWorktreeSheet(flow: plain) }

        let locked = StrayFlow(model: harness.model, project: flowSampleProject, stray: PreviewSamples.strays[1])
        try render("stray-locked", Self.straySize, harness) { StrayWorktreeSheet(flow: locked) }

        await harness.backend.script(.removeStray, .fail(uncommitted(["scratch.txt", "spike/notes.md"])))
        plain.remove()
        try await flowWaitUntilFinished(plain.record)
        try render("stray-refused-dirty", Self.straySize, harness) { StrayWorktreeSheet(flow: plain) }

        await harness.backend.script(.deleteBranch, .fail(.refused(Refusal(
            cause: .unmergedBranch(branch: "feature/old-spike", ahead: 2),
            message: "error: the branch 'feature/old-spike' is not fully merged", diagnostics: Diagnostics(summary: "not merged")))))
        let prunable = StrayFlow(model: harness.model, project: flowSampleProject, stray: PreviewSamples.strays[2])
        prunable.deleteBranchIfMerged = true
        prunable.remove()
        try await flowWaitUntilFinished(prunable.record)
        prunable.removalFinished()
        try await flowWaitUntilFinished(prunable.branchRecord)
        try render("stray-removed-branch-unmerged", Self.straySize, harness) { StrayWorktreeSheet(flow: prunable) }
    }

    // MARK: Activity

    @Test func activity() async throws {
        let harness = FlowHarness(.dirtyWorktree)
        defer { harness.tearDown() }
        ActivitySelection.defaults = UserDefaults(suiteName: harness.defaultsName)!
        try await harness.start()
        try render("activity-inspector-empty", CGSize(width: 340, height: 520), harness) {
            ActivityInspector(target: .feature(feature("prine")))
        }

        let teardown = TeardownFlow(model: harness.model, feature: feature("prine"), preselect: nil)
        await teardown.load()
        teardown.tearDown()
        try await flowWaitUntilFinished(teardown.record)

        await harness.backend.script(.devcontainer, .fail(.commandFailed(Diagnostics(
            summary: "Docker is not available", causes: ["connect: no such file or directory"], exitCode: 1))))
        harness.model.actions.dispatch(.devcontainer(.up(removeExisting: false, buildNoCache: false), feature("prine")))
        await harness.backend.script(.exec, .suspendUntilResumed)
        if case .started(let exec) = harness.model.actions.dispatch(.exec(ExecRequest(feature: feature("remotion"),
                                                                                      command: ["make", "test"]))) {
            _ = await harness.backend.waitUntilSuspended(.exec)
            harness.model.operations.cancel(exec.id)
            try await flowWaitUntilFinished(exec)
        }
        let done = harness.startFlow()
        done.setTitle("billing-v2")
        await done.waitForPreview()
        done.start()
        try await flowWaitUntilFinished(done.record)

        await harness.backend.script(.startFeature, steps: [.emit(logEvents(30, phase: .module("database"))), .suspendUntilResumed])
        let running = harness.startFlow()
        running.setTitle("Add OAuth login")
        await running.waitForPreview()
        running.start()
        _ = await harness.backend.waitUntilSuspended(.startFeature)
        try await Task.sleep(for: .milliseconds(300))

        try render("activity-inspector-feature", CGSize(width: 340, height: 720), harness) {
            ActivityInspector(target: .feature(feature("prine")))
        }
        try render("activity-inspector-project", CGSize(width: 340, height: 720), harness) {
            ActivityInspector(target: .project(flowSampleProject))
        }
        ActivitySelection.select(try #require(running.record?.id))
        try render("activity-window-running", CGSize(width: 1000, height: 680), harness) { ActivityWindow() }
        ActivitySelection.select(try #require(teardown.record?.id))
        try render("activity-window-refused", CGSize(width: 1000, height: 680), harness) { ActivityWindow() }
        try render("activity-popover", CGSize(width: 360, height: 440), harness) { ActivityPopover() }
        ActivitySelection.select(nil)
        await harness.backend.terminateAll()
        try await flowWaitUntilFinished(running.record)
    }
}
