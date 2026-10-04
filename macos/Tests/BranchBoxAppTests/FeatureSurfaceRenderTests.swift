import AppKit
import BranchBoxKit
import BranchBoxPreview
import BranchBoxStores
import SwiftUI
import Testing
@testable import BranchBoxApp

/// Renders the feature surfaces (detail, cards, Run Command, actions menu) for visual review, on a started
/// `AppModel` over `PreviewBackend`. Run with BRANCHBOX_RENDER_DIR=<dir>.
@MainActor
@Suite(.enabled(if: SnapshotRenderer.isEnabled), .serialized)
struct FeatureSurfaceRenderTests {
    static let pane = CGSize(width: 820, height: 640)
    static let tallPane = CGSize(width: 820, height: 1900)
    static let widePane = CGSize(width: 1180, height: 1300)
    static let cardColumn = CGSize(width: 520, height: 1250)
    static let runWindow = CGSize(width: 680, height: 520)

    // MARK: Detail

    @Test func activeContainerFeature() async throws {
        let world = try await RenderWorld.start()
        let record = RenderSamples.checkout
        try SnapshotRenderer.render("feature-detail-active", size: Self.pane) { world.detail(record) }
        try SnapshotRenderer.render("feature-detail-active-full", size: Self.tallPane) { world.detail(record) }
        try SnapshotRenderer.render("feature-detail-active-wide", size: Self.widePane) { world.detail(record) }
        await world.tearDown()
    }

    @Test func attentionStates() async throws {
        let world = try await RenderWorld.start()
        var brokenGit = RenderSamples.checkout
        brokenGit.worktreeIssue = "The .git file points to Git metadata that cannot be found at /old/main/.git/worktrees/checkout-v2."
        try SnapshotRenderer.render("feature-detail-broken-git", size: Self.widePane) {
            world.detail(brokenGit, devcontainer: .unavailable("Git worktree needs repair: \(brokenGit.worktreeIssue ?? "")"))
        }
        let sbx = try #require(PreviewSamples.features.first { $0.workFeature == "sbx-demo" })
        try SnapshotRenderer.render("feature-detail-degraded-sbx", size: Self.tallPane) { world.detail(sbx) }
        try SnapshotRenderer.render("feature-detail-failed-retained", size: Self.pane) { world.detail(RenderSamples.retainedSandbox) }
        let orphan = try #require(PreviewSamples.features.first { $0.workFeature == "orphan" })
        try SnapshotRenderer.render("feature-detail-orphaned-folder-missing", size: Self.pane) {
            world.detail(orphan, folderExists: false)
        }
        try SnapshotRenderer.render("feature-detail-interrupted", size: Self.pane) {
            world.detail(PreviewSamples.interruptedFeature, devcontainer: .loaded(DevcontainerStatus(state: .notCreated)))
        }
        try SnapshotRenderer.render("feature-detail-setup-incomplete", size: Self.pane) { world.detail(RenderSamples.failedModules) }
        try SnapshotRenderer.render("feature-detail-unknown-status", size: Self.pane) { world.detail(RenderSamples.unknownStatus) }
        try SnapshotRenderer.render("feature-detail-setting-up", size: Self.pane) { world.detail(RenderSamples.settingUp) }
        await world.tearDown()
    }

    @Test func removedLongNameAndGone() async throws {
        let world = try await RenderWorld.start()
        let removed = try #require(PreviewSamples.features.first { $0.workFeature == "coding-agents" })
        try SnapshotRenderer.render("feature-detail-removed", size: Self.pane) {
            world.detail(removed, folderExists: false, branchExists: true)
        }
        try SnapshotRenderer.render("feature-detail-long-name", size: Self.pane) { world.detail(RenderSamples.longName) }
        try SnapshotRenderer.render("feature-detail-gone", size: Self.pane) {
            FeatureDetailView(feature: FeatureRef(project: PreviewSamples.project, name: "deleted-elsewhere"))
                .environment(world.model)
        }
        await world.tearDown()
    }

    @Test func busyAndCLIMissing() async throws {
        let world = try await RenderWorld.start(scenario: RenderSamples.scenario)
        await world.backend.script(.startFeature, .suspendUntilResumed)
        let oauth = PreviewSamples.interruptedFeature
        let actions = Remediation.actions(for: oauth, project: PreviewSamples.project, identity: world.model.environment.identity,
                                          folderExists: true)
        if case .resumeSetup(let request)? = actions.first { world.model.actions.dispatch(.start(request)) }
        _ = await world.backend.waitUntilSuspended(.startFeature)
        try SnapshotRenderer.render("feature-detail-busy", size: Self.pane) {
            world.detail(oauth, devcontainer: .loaded(DevcontainerStatus(state: .notCreated)))
        }
        await world.backend.resumeAll()
        await world.tearDown()

        let missing = try await RenderWorld.start(scenario: .cliMissing, addProject: false)
        try SnapshotRenderer.render("feature-detail-cli-missing", size: Self.pane) {
            missing.detail(oauth, devcontainer: .failed(.cliNotFound(searched: PreviewSamples.searchedPaths)))
        }
        await missing.tearDown()
    }

    // MARK: Cards

    @Test func environmentCardStates() async throws {
        let world = try await RenderWorld.start()
        let checkout = RenderSamples.checkout
        let sbx = try #require(PreviewSamples.features.first { $0.workFeature == "sbx-demo" })
        let retained = try #require(PreviewSamples.features.first { $0.workFeature == "retained" })
        let running = DevcontainerStatus(state: .running, containerID: "4b1f0c9e2a7d55aa",
                                         service: DevcontainerServiceInfo(serviceName: "app", port: 3000,
                                                                          serviceURL: "http://app:3000", containerUser: "vscode"))
        try SnapshotRenderer.render("feature-card-environment-container", size: Self.cardColumn) {
            ScrollView {
                VStack(spacing: 16) {
                    world.environmentCard(checkout, load: .loaded(running))
                    world.environmentCard(checkout, load: .loaded(DevcontainerStatus(state: .stopped)))
                    world.environmentCard(RenderSamples.outdated, load: .loaded(DevcontainerStatus(state: .notCreated)))
                    world.environmentCard(checkout, load: .failed(.commandFailed(Diagnostics(summary: "Docker is not available"))))
                    world.environmentCard(checkout, load: .loading)
                }
                .padding(20)
            }
        }
        try SnapshotRenderer.render("feature-card-environment-other", size: CGSize(width: 520, height: 720)) {
            VStack(spacing: 16) {
                world.environmentCard(sbx, load: .loading)
                world.environmentCard(retained, load: .loading)
            }
            .padding(20)
        }
        await world.tearDown()
    }

    @Test func sharingCardStates() async throws {
        let world = try await RenderWorld.start()
        let manual = RenderSamples.manualTunnel
        let prine = PreviewSamples.features[0]
        try SnapshotRenderer.render("feature-card-sharing", size: Self.cardColumn) {
            ScrollView {
                VStack(spacing: 16) {
                    world.tunnelCard(RenderSamples.checkout, enabled: true)
                    world.tunnelCard(manual, enabled: true)
                    world.tunnelCard(prine, enabled: true)
                    world.tunnelCard(prine, enabled: false)
                }
                .padding(20)
            }
        }
        // The provider refuses: the card offers Remove Anyway.
        let feature = FeatureRef(project: PreviewSamples.project, name: RenderSamples.checkout.workFeature)
        await world.backend.script(.removeTunnel, .fail(.commandFailed(Diagnostics(
            summary: "cloudflared: failed to delete tunnel checkout-v2: API error 1003 (Unauthorized)",
            causes: ["The API token was revoked"], exitCode: 1))))
        if case .started(let record) = world.model.actions.dispatch(.tunnelRemove(feature, force: false)) {
            try await world.until { !record.isCancellable }
        }
        try SnapshotRenderer.render("feature-card-sharing-failure", size: CGSize(width: 520, height: 620)) {
            world.tunnelCard(RenderSamples.checkout, enabled: true).padding(20)
        }
        await world.tearDown()
    }

    @Test func otherCards() async throws {
        let world = try await RenderWorld.start()
        let checkout = RenderSamples.checkout
        let sbx = try #require(PreviewSamples.features.first { $0.workFeature == "sbx-demo" })
        try SnapshotRenderer.render("feature-cards-misc", size: Self.cardColumn) {
            ScrollView {
                VStack(spacing: 16) {
                    OpenLinksCard(record: sbx, feature: world.ref(sbx), service: DevcontainerServiceInfo(
                        serviceName: "app", port: 3000, serviceURL: "http://app:3000", containerUser: "vscode"))
                    OpenLinksCard(record: RenderSamples.settingUp, feature: world.ref(RenderSamples.settingUp), service: nil)
                    AgentPromptCard(record: checkout, feature: world.ref(checkout), availability: world.availability(checkout))
                    AgentPromptCard(record: PreviewSamples.features[0], feature: world.ref(PreviewSamples.features[0]),
                                    availability: world.availability(PreviewSamples.features[0]))
                    AdapterCard(adapter: sbx.adapter ?? AdapterInfo())
                    ModulesCard(record: RenderSamples.failedModules)
                    PullRequestCard(prNumber: nil)
                }
                .padding(20)
                .environment(world.model)
            }
        }
        await world.tearDown()
    }

    // MARK: Run Command

    @Test func runCommandWindow() async throws {
        let world = try await RenderWorld.start()
        let checkout = world.ref(RenderSamples.checkout)
        let draft = RunCommandDraft(text: "bin/rails test test/models")
        let failing = ExecOutput(result: ExecResult(
            exitCode: 3,
            stdout: "Running 214 tests in parallel using 8 processes\n\n# Running:\n\n......F...........E.....\n\nFinished in 4.81s\n214 runs, 611 assertions, 1 failures, 1 errors, 0 skips\n",
            stderr: "Failure:\nOrderTest#test_total_includes_tax [test/models/order_test.rb:42]:\nExpected 107.0, got 100.0\n"),
            duration: .milliseconds(5_120))
        try SnapshotRenderer.render("run-command-idle", size: Self.runWindow) {
            world.run(checkout, phase: .idle, draft: draft)
        }
        try SnapshotRenderer.render("run-command-running", size: Self.runWindow) {
            world.run(checkout, phase: .running(since: .now.addingTimeInterval(-12)), draft: draft)
        }
        try SnapshotRenderer.render("run-command-exit-3", size: Self.runWindow) {
            world.run(checkout, phase: .finished(failing), draft: draft)
        }
        try SnapshotRenderer.render("run-command-exit-0", size: Self.runWindow) {
            world.run(checkout, phase: .finished(ExecOutput(result: ExecResult(exitCode: 0, stdout: "On branch feature/checkout-v2\nnothing to commit, working tree clean\n"),
                                                            duration: .milliseconds(85))),
                      draft: RunCommandDraft(text: "git status"))
        }
        try SnapshotRenderer.render("run-command-failed", size: Self.runWindow) {
            world.run(checkout, phase: .failed(.commandFailed(Diagnostics(summary: "Dev container for checkout-v2 is not running",
                                                                          causes: ["devcontainer exec: no running container"],
                                                                          exitCode: 1))),
                      draft: draft)
        }
        let orphan = try #require(PreviewSamples.features.first { $0.workFeature == "orphan" })
        try SnapshotRenderer.render("run-command-disabled-orphaned", size: Self.runWindow) {
            world.run(world.ref(orphan), phase: .idle, draft: RunCommandDraft())
        }
        try SnapshotRenderer.render("run-command-no-feature", size: Self.runWindow) {
            RunCommandWindow(feature: nil).environment(world.model)
        }
        await world.tearDown()
    }

    // MARK: Menu

    @Test func actionsMenuContents() async throws {
        let world = try await RenderWorld.start()
        let sbx = try #require(PreviewSamples.features.first { $0.workFeature == "sbx-demo" })
        // Menu items laid out as controls, to review which items exist and which are disabled.
        try SnapshotRenderer.render("feature-actions-menu", size: CGSize(width: 760, height: 520)) {
            HStack(alignment: .top, spacing: 24) {
                VStack(alignment: .leading, spacing: 6) {
                    Text("Container (toolbar overflow)").font(.headline)
                    FeatureActionsMenu(feature: world.ref(RenderSamples.checkout), style: .toolbarOverflow)
                }
                VStack(alignment: .leading, spacing: 6) {
                    Text("Sandbox (menu bar)").font(.headline)
                    FeatureActionsMenu(feature: world.ref(sbx), style: .menuBar)
                }
            }
            .padding(20)
            .environment(world.model)
        }
        await world.tearDown()
    }
}

// MARK: World

/// A started `AppModel` on the preview backend with the render samples listed, storing in a temporary folder.
@MainActor private struct RenderWorld {
    let directory: URL
    let backend: PreviewBackend
    let model: AppModel

    static func start(scenario: PreviewScenario = RenderSamples.scenario, addProject: Bool = true) async throws -> RenderWorld {
        let directory = FileManager.default.temporaryDirectory
            .appendingPathComponent("branchbox-tests/renders-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        let settings = AppSettings(defaults: UserDefaults(suiteName: directory.appendingPathComponent("settings").path)!)
        settings.watchProjectFiles = false
        let backend = PreviewBackend(scenario: scenario)
        let model = AppModel(settings: settings, bootstrapper: PreviewBootstrapper(backend: backend), notifier: NoopNotifier(),
                             configuration: .isolated(in: directory))
        let world = RenderWorld(directory: directory, backend: backend, model: model)
        await model.start()
        if addProject {
            _ = await model.projects.add(folder: PreviewSamples.project.root)
            let store = try #require(model.projects.project(PreviewSamples.project))
            try await world.until {
                if case .loaded = store.loadState, !store.isRefreshing { return true }
                return false
            }
            await store.reloadConfig()
        }
        return world
    }

    func tearDown() async {
        await model.prepareForTermination()
        try? FileManager.default.removeItem(at: directory)
    }

    func until(_ condition: () -> Bool) async throws {
        // Generous: the integrated render run shares the main actor with the other render suites.
        let deadline = ContinuousClock.now + .seconds(30)
        while !condition() {
            try #require(ContinuousClock.now < deadline)
            try await Task.sleep(for: .milliseconds(5))
        }
    }

    func ref(_ record: FeatureRecord) -> FeatureRef {
        FeatureRef(project: PreviewSamples.project, name: record.workFeature)
    }

    func availability(_ record: FeatureRecord, folderExists: Bool = true) -> FeatureActionAvailability {
        let ready: Bool = if case .ready = model.environment.backendState { true } else { false }
        return FeatureActionAvailability(record: record, folderExists: folderExists, backendReady: ready,
                                         preferences: LaunchPreferences(settings: model.settings, projectDefaultAgent: nil),
                                         busyWith: model.operations.active(for: .feature(ref(record)))?.title)
    }

    func detail(_ record: FeatureRecord, folderExists: Bool = true, branchExists: Bool? = nil,
                devcontainer: DevcontainerLoad? = nil) -> some View {
        let running = DevcontainerStatus(state: .running, containerID: "4b1f0c9e2a7d55aa",
                                         service: DevcontainerServiceInfo(serviceName: "app", port: 3000,
                                                                          serviceURL: "http://app:3000", containerUser: "vscode"))
        let pinned = devcontainer ?? (record.runtime.provider == .container ? .loaded(running) : nil)
        return FeatureDetailContent(feature: ref(record), record: record, folderExists: folderExists, config: .defaults,
                                    pinnedDevcontainer: pinned, pinnedBranchExists: branchExists)
            .environment(model)
    }

    func environmentCard(_ record: FeatureRecord, load: DevcontainerLoad) -> some View {
        let availability = availability(record)
        let items = RemediationPresenter.items(
            for: Remediation.actions(for: record, project: PreviewSamples.project, identity: model.environment.identity,
                                     folderExists: true),
            record: record)
        return EnvironmentCard(record: record, feature: ref(record), availability: availability, load: load,
                               startItem: items.first(where: FeatureDetailContent.startsEnvironment), onReload: {},
                               onPerform: { _ in })
            .environment(model)
    }

    func tunnelCard(_ record: FeatureRecord, enabled: Bool) -> some View {
        TunnelCard(record: record, feature: ref(record), availability: availability(record), tunnelsEnabled: enabled)
            .environment(model)
    }

    func run(_ feature: FeatureRef, phase: RunCommandPhase, draft: RunCommandDraft) -> some View {
        RunCommandView(feature: feature, pinnedPhase: phase, pinnedDevcontainerRunning: true, initialDraft: draft)
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .environment(model)
    }
}

/// Realistic records beyond the CLI fixtures: ports, an online tunnel, a PR, a long prompt, a long name.
private enum RenderSamples {
    static let root = "/Users/dev/projects/branchbox-suite/branchbox"
    static let now = Date.now

    static let checkout = FeatureRecord(
        workFeature: "checkout-v2", branchName: "feature/checkout-v2", worktreePath: "\(root)/checkout-v2", baseBranch: "main",
        featureURL: "dev-checkout-v2.localhost", composeProjectName: "branchbox-checkout-v2", envPath: "\(root)/checkout-v2/.env",
        status: .active, createdAt: now.addingTimeInterval(-2 * 86_400), updatedAt: now.addingTimeInterval(-3_600),
        tunnel: TunnelState(provider: "cloudflared", hostname: "checkout-v2.acme-preview.dev", serviceURL: "http://app:3000",
                            status: .active, lastUpdated: now.addingTimeInterval(-1_800)),
        color: "#8e44ad", lastCommit: "9f2c4e1b7a6d3c5e8f90a1b2c3d4e5f6a7b8c9d0", prNumber: 128, startMode: "full",
        promptSeed: """
            Rebuild the checkout flow on the new payments API. Keep the existing cart page, replace the address and \
            payment steps with the single-page form from the Figma file, and add tests for tax calculation in \
            OrderTest. Don't touch the admin refunds screen.
            """,
        moduleOutcomes: [
            ModuleOutcome(module: "devcontainer", status: .success, durationMs: 840),
            ModuleOutcome(module: "compose", status: .success, durationMs: 12_400),
            ModuleOutcome(module: "database", status: .success, durationMs: 3_150, notes: ["Seeded from db/seeds.rb"]),
            ModuleOutcome(module: "tunnel", status: .success, durationMs: 2_020),
            ModuleOutcome(module: "specs", status: .success, durationMs: 4),
        ],
        adapter: AdapterInfo(name: "Rails", serviceURL: "http://app:3000"),
        runtime: RuntimeInfo(provider: .container, publishedPorts: [PublishedPort(host: 49152, runtime: 3000),
                                                                    PublishedPort(host: 49153, runtime: 5432)],
                             workspaceFolder: "/workspaces/checkout-v2", containerUser: "vscode"),
        defaultAgent: DefaultAgentPlan(status: .ready, label: "claude", command: "claude",
                                       detail: "Claude Code launches in the dev container when the feature starts"))

    static let longName = FeatureRecord(
        workFeature: "payments-reconciliation-ledger-backfill-for-legacy-invoices",
        branchName: "rida/payments-reconciliation-ledger-backfill-for-legacy-invoices",
        worktreePath: "\(root)/payments-reconciliation-ledger-backfill-for-legacy-invoices", baseBranch: "release/2026.10",
        featureURL: "dev-payments-reconciliation-ledger-backfill-for-legacy-invoices.localhost", status: .active,
        createdAt: now.addingTimeInterval(-600), updatedAt: now.addingTimeInterval(-600), color: "#16a085",
        lastCommit: "51c0a2f9", startMode: "minimal", runtime: RuntimeInfo(provider: .container))

    static let retainedSandbox = FeatureRecord(
        workFeature: "search-reindex", branchName: "feature/search-reindex", worktreePath: "\(root)/search-reindex",
        status: .failedRetained, createdAt: now.addingTimeInterval(-7_200), updatedAt: now.addingTimeInterval(-7_000),
        color: "#e74c3c", startMode: "full",
        moduleOutcomes: [ModuleOutcome(module: "devcontainer", status: .success, durationMs: 1_100),
                         ModuleOutcome(module: "compose", status: .failed, durationMs: 31_000,
                                       notes: ["elasticsearch exited with code 137 (out of memory)"])],
        runtime: RuntimeInfo(provider: .sbx, runtimeID: "branchbox-search-reindex"))

    static let failedModules = FeatureRecord(
        workFeature: "invoices-pdf", branchName: "feature/invoices-pdf", worktreePath: "\(root)/invoices-pdf",
        featureURL: "dev-invoices-pdf.localhost", status: .active, createdAt: now.addingTimeInterval(-86_400),
        color: "#2980b9", startMode: "full",
        moduleOutcomes: [ModuleOutcome(module: "devcontainer", status: .success, durationMs: 700),
                         ModuleOutcome(module: "compose", status: .failed, durationMs: 1_830,
                                       notes: ["port 5432 is already in use"], forced: true),
                         ModuleOutcome(module: "tunnel", status: .skipped, durationMs: 0, notes: ["Tunnels are off"])],
        runtime: RuntimeInfo(provider: .container))

    static let unknownStatus = FeatureRecord(
        workFeature: "experiments", branchName: "feature/experiments", worktreePath: "\(root)/experiments",
        status: .unknown("paused_by_admin"), createdAt: now.addingTimeInterval(-3 * 86_400), color: "#7f8c8d", startMode: "full",
        runtime: RuntimeInfo(provider: .container))

    static let settingUp = FeatureRecord(
        workFeature: "onboarding-emails", branchName: "feature/onboarding-emails", worktreePath: "\(root)/onboarding-emails",
        status: .active, createdAt: now.addingTimeInterval(-40), color: "#f1c40f", startMode: "full",
        runtime: RuntimeInfo(provider: .container),
        setup: SetupInfo(state: .inProgress, pid: 4242, startedAt: now.addingTimeInterval(-42)))

    static let outdated = FeatureRecord(
        workFeature: "dark-mode", branchName: "feature/dark-mode", worktreePath: "\(root)/dark-mode", status: .active,
        devcontainerOutdated: true, startMode: "full", runtime: RuntimeInfo(provider: .container))

    static let manualTunnel = FeatureRecord(
        workFeature: "partner-webhooks", branchName: "feature/partner-webhooks", worktreePath: "\(root)/partner-webhooks",
        status: .active,
        tunnel: TunnelState(provider: "cloudflared", hostname: "partner-webhooks.acme-preview.dev", serviceURL: "http://app:3000",
                            status: .manual,
                            instructions: ["Run `cloudflared tunnel login` and pick the acme-preview.dev zone.",
                                           "Run `branchbox tunnel open partner-webhooks` again to finish provisioning."],
                            notes: "No API token is configured for this project", lastUpdated: now.addingTimeInterval(-300)),
        startMode: "full", runtime: RuntimeInfo(provider: .container))

    static let scenario = PreviewScenario(
        name: "renders", identity: PreviewScenario.contract.identity,
        listing: FeatureListing(features: [checkout, longName, manualTunnel, PreviewSamples.interruptedFeature]
                                    + PreviewSamples.features))
}
