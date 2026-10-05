import AppKit
@testable import BranchBoxApp
import BranchBoxCLI
import BranchBoxKit
import BranchBoxPreview
import BranchBoxStores
import BranchBoxTestSupport
import SwiftUI
import Testing

/// Renders every SW-7 screen, sheet and significant state for visual review. Run with BRANCHBOX_RENDER_DIR=<dir>.
/// NavigationSplitView sidebars and Lists render blank offscreen, so panes are rendered on their own.
@MainActor
@Suite(.enabled(if: SnapshotRenderer.isEnabled), .serialized)
struct ProjectsSettingsRenderTests {
    static let pane = CGSize(width: 820, height: 640)
    static let settingsTab = CGSize(width: 620, height: 480)

    // MARK: Welcome and doctor

    @Test func welcome() async throws {
        let resolving = RenderHarness(.contract)                     // not started: still locating the CLI
        try SnapshotRenderer.render("welcome-resolving", size: CGSize(width: 820, height: 760)) {
            WelcomeView().environment(resolving.model)
        }

        let missing = try await RenderHarness.started(.cliMissing)
        try SnapshotRenderer.render("welcome-cli-missing", size: CGSize(width: 820, height: 760)) {
            WelcomeView().environment(missing.model)
        }

        let legacy = try await RenderHarness.started(RenderHarness.cliScenario(.legacy0134))
        await legacy.model.environment.runDoctor(for: nil)
        try SnapshotRenderer.render("welcome-legacy-ready", size: CGSize(width: 820, height: 900)) {
            WelcomeView().environment(legacy.model)
        }

        let done = try await RenderHarness.started(RenderHarness.cliScenario(.contract), addSample: true)
        await done.model.environment.runDoctor(for: nil)
        try SnapshotRenderer.render("welcome-all-done", size: CGSize(width: 820, height: 900)) {
            WelcomeView().environment(done.model)
        }
        for harness in [resolving, missing, legacy, done] { await harness.tearDown() }
    }

    @Test func doctorChecklistWithProblems() async throws {
        let harness = try await RenderHarness.started(.contract)
        let checks = [
            DoctorCheck(id: "git", title: "Git", required: true, status: .ok, path: "/usr/bin/git", version: "2.50.1"),
            DoctorCheck(id: "docker.cli", title: "Docker CLI", required: true, status: .ok, path: "/usr/local/bin/docker",
                        version: "28.3.2"),
            DoctorCheck(id: "docker.daemon", title: "Docker daemon", required: true, status: .error,
                        path: "/usr/local/bin/docker",
                        detail: "Cannot connect to the Docker daemon at unix:///var/run/docker.sock. Is the docker daemon running?",
                        remediation: "Start Docker Desktop"),
            DoctorCheck(id: "docker.compose", title: "Docker Compose", required: false, status: .skipped,
                        detail: "Needs the Docker daemon"),
            DoctorCheck(id: "devcontainer.cli", title: "Dev Container CLI", required: false, status: .warn,
                        detail: "devcontainer was not found on PATH", remediation: "npm install -g @devcontainers/cli"),
            DoctorCheck(id: "runtime.sbx", title: "Docker Sandboxes", required: false, status: .warn, path: "/opt/homebrew/bin/sbx",
                        detail: "not authenticated", remediation: "Sign in with: sbx login"),
            DoctorCheck(id: "gh", title: "GitHub CLI", required: false, status: .ok, path: "/opt/homebrew/bin/gh", version: "2.76.0"),
        ]
        try SnapshotRenderer.render("doctor-docker-down", size: CGSize(width: 600, height: 460)) {
            DoctorChecklist(checks: checks, offersNotifications: true, onAllowNotifications: {})
                .padding(20)
                .environment(harness.model)
        }
        await harness.tearDown()
    }

    // MARK: Project detail

    @Test func projectDetail() async throws {
        let busy = try await RenderHarness.started(.interruptedSetup, addSample: true)
        await busy.backend?.setListing(RenderHarness.busyListing, for: PreviewSamples.project)
        try await busy.loadSample()
        try SnapshotRenderer.render("project-detail-contract", size: Self.pane) {
            ProjectDetailView(project: PreviewSamples.project).environment(busy.model)
        }
        try SnapshotRenderer.render("project-toolbar", size: CGSize(width: 520, height: 60)) {
            HStack { ProjectActionButtons(store: busy.store!, confirmingRemove: .constant(false)) }
                .padding(12)
                .environment(busy.model)
        }

        let legacy = try await RenderHarness.started(.legacy0134, addSample: true)
        try await legacy.loadSample()
        try SnapshotRenderer.render("project-detail-legacy", size: Self.pane) {
            ProjectDetailView(project: PreviewSamples.project).environment(legacy.model)
        }

        let empty = try await RenderHarness.started(.emptyProject, addSample: true)
        try await empty.loadSample()
        try SnapshotRenderer.render("project-detail-empty", size: Self.pane) {
            ProjectDetailView(project: PreviewSamples.project).environment(empty.model)
        }

        let uninitialized = try await RenderHarness.started(.contract)
        await uninitialized.backend?.script(.listFeatures, .fail(.projectInvalid(.notInitialized(PreviewSamples.project.path))))
        _ = await uninitialized.model.projects.add(folder: PreviewSamples.project.root)
        try await uninitialized.waitForLoad()
        try SnapshotRenderer.render("project-detail-not-set-up", size: Self.pane) {
            ProjectDetailView(project: PreviewSamples.project).environment(uninitialized.model)
        }

        let stale = try await RenderHarness.started(.contract, addSample: true)
        try await stale.loadSample()
        let corrupted = BackendError.registryCorrupted(path: "\(PreviewSamples.project.path)/.branchbox/registry.json",
                                                       diagnostics: Diagnostics(summary: "expected value at line 1 column 1"))
        await stale.backend?.script(.listFeatures, .fail(corrupted))
        await stale.store?.refresh(.manual)
        try SnapshotRenderer.render("project-detail-load-error", size: Self.pane) {
            ProjectDetailView(project: PreviewSamples.project).environment(stale.model)
        }

        let missing = try await RenderHarness.started(RenderHarness.cliScenario(.contract), addSample: true)
        try SnapshotRenderer.render("project-detail-missing-folder", size: Self.pane) {
            ProjectDetailView(project: PreviewSamples.project).environment(missing.model)
        }
        for harness in [busy, legacy, empty, uninitialized, stale, missing] { await harness.tearDown() }
    }

    // MARK: Add project

    @Test func addProject() async throws {
        let harness = try await RenderHarness.started(.contract)
        let folder = URL(fileURLWithPath: "/Users/dev/projects/branchbox-suite/branchbox/payments-reconciliation-rewrite",
                         isDirectory: true)
        let main = PreviewSamples.project
        let note = "This is a feature worktree of \(main.path); adding \(main.path) instead"
        let phases: [(String, AddProjectModel.Phase)] = [
            ("add-project-choose", .choosing),
            ("add-project-resolving", .resolving(folder)),
            ("add-project-added-note", .finished(folder, .added(main, note: note))),
            ("add-project-already-present", .finished(folder, .alreadyPresent(main))),
            ("add-project-needs-setup", .finished(URL(fileURLWithPath: "/Users/dev/projects/acme-storefront"),
                                                  .needsInit(ProjectRef(root: URL(fileURLWithPath: "/Users/dev/projects/acme-storefront"))))),
            ("add-project-refused", .finished(URL(fileURLWithPath: "/Users/dev/Documents/notes"),
                                              .refused(.projectInvalid(.notGitRepository("/Users/dev/Documents/notes"))))),
        ]
        for (name, phase) in phases {
            try SnapshotRenderer.render(name, size: CGSize(width: 520, height: 400)) {
                AddProjectSheet(initialFolder: nil, model: AddProjectModel(phase: phase)).environment(harness.model)
            }
        }
        await harness.tearDown()
    }

    // MARK: Set up (init)

    @Test func initSheet() async throws {
        let sheetSize = CGSize(width: 600, height: 640)
        let folder = URL(fileURLWithPath: "/Users/dev/projects/acme-storefront")
        let detected = DetectReport(project: folder.path, gitRepository: true, initialized: false, stack: "rails", adapter: "rails",
                                    modules: ["devcontainer", "compose", "database", "specs"])
        let contract = try await RenderHarness.started(.contract)

        try SnapshotRenderer.render("init-setup-form", size: sheetSize) {
            InitProjectSheet(model: InitSheetModel(folder: folder, mode: .setUp, detected: detected)).environment(contract.model)
        }

        let moved = InitSheetModel(folder: folder, mode: .setUp, detected: detected)
        moved.draft.confirmMoveIntoParent()
        moved.draft.tunnelsEnabled = true
        moved.draft.usesOnePassword = true
        moved.draft.gitHubRef = "ghp_pasted_by_mistake"
        try SnapshotRenderer.render("init-move-and-1password", size: CGSize(width: 600, height: 900)) {
            InitProjectSheet(model: moved).environment(contract.model)
        }

        let legacy = try await RenderHarness.started(.legacy0134)
        try SnapshotRenderer.render("init-setup-legacy", size: sheetSize) {
            InitProjectSheet(model: InitSheetModel(folder: folder, mode: .setUp, detected: detected)).environment(legacy.model)
        }
        try SnapshotRenderer.render("init-repair-form", size: sheetSize) {
            InitProjectSheet(model: InitSheetModel(folder: PreviewSamples.project.root, mode: .repair, detected: detected))
                .environment(contract.model)
        }

        // Preview: a dry run with its log.
        let log = ["Detecting project at \(folder.path)", "Stack: Rails (Gemfile, config/application.rb)",
                   "[DRY RUN] Would create .branchbox/config.json", "[DRY RUN] Would create .devcontainer/devcontainer.json",
                   "[DRY RUN] Would create .devcontainer/compose.yaml", "[DRY RUN] Would append BranchBox entries to .gitignore",
                   "[DRY RUN] Would create .env from .env.example (ports 3000, 5432)"]
        await contract.backend?.script(.initProject, steps: [.emit(log.map { .log(RenderHarness.line($0)) })])
        let previewing = InitSheetModel(folder: folder, mode: .setUp, detected: detected)
        previewing.startPreview(using: contract.model)
        try await RenderHarness.finish(previewing.preview)
        try SnapshotRenderer.render("init-preview-log", size: sheetSize) {
            InitProjectSheet(model: previewing).environment(contract.model)
        }

        // Running, then the result.
        await contract.backend?.script(.initProject, steps: [.emit(Array(log.prefix(3)).map { .log(RenderHarness.line($0)) }),
                                                             .suspendUntilResumed])
        let running = InitSheetModel(folder: folder, mode: .setUp, detected: detected)
        running.initialize(using: contract.model)
        _ = await contract.backend?.waitUntilSuspended(.initProject)
        try await Task.sleep(for: .milliseconds(150))
        try SnapshotRenderer.render("init-running", size: sheetSize) {
            InitProjectSheet(model: running).environment(contract.model)
        }
        await contract.backend?.resume(.initProject)
        try await RenderHarness.finish(running.run)
        try SnapshotRenderer.render("init-result", size: sheetSize) {
            InitProjectSheet(model: running).environment(contract.model)
        }
        try SnapshotRenderer.render("init-result-warnings", size: sheetSize) {
            ProjectInitResultView(report: InitReport(workspacePath: "/Users/dev/projects/acme-storefront/main", reorganized: true,
                                                     stack: "rails", adapter: "Rails",
                                                     modules: ["devcontainer", "compose", "database", "tunnel", "specs"],
                                                     warnings: ["Tunnels are off; turn them on in Project Settings to share features",
                                                                "1Password CLI not found; skipped GitHub token setup"],
                                                     nextSteps: ["Start your first feature", "Commit .branchbox/config.json and .devcontainer"],
                                                     onePasswordStatus: "skipped"),
                                  mode: .initialize)
        }

        await contract.backend?.script(.initProject, .fail(.commandFailed(Diagnostics(
            summary: "Not a git repository: /Users/dev/projects/acme-storefront",
            causes: ["git rev-parse --show-toplevel failed"], exitCode: 1,
            invocation: "branchbox init -y --json", cliVersion: "0.14.0"))))
        let failing = InitSheetModel(folder: folder, mode: .setUp, detected: detected)
        failing.initialize(using: contract.model)
        try await RenderHarness.finish(failing.run)
        try SnapshotRenderer.render("init-failed", size: sheetSize) {
            InitProjectSheet(model: failing).environment(contract.model)
        }
        await contract.tearDown()
        await legacy.tearDown()
    }

    // MARK: Update all workspaces

    @Test func syncSheet() async throws {
        let size = CGSize(width: 560, height: 520)
        let harness = try await RenderHarness.started(.contract, addSample: true)
        try await harness.loadSample()
        try SnapshotRenderer.render("sync-start", size: size) {
            SyncDevcontainersSheet(project: PreviewSamples.project).environment(harness.model)
        }
        let previewing = SyncSheetModel(strategy: .symlink)
        previewing.startPreview(project: PreviewSamples.project, using: harness.model)
        try await RenderHarness.finish(previewing.preview)
        try SnapshotRenderer.render("sync-preview", size: size) {
            SyncDevcontainersSheet(project: PreviewSamples.project, model: previewing).environment(harness.model)
        }

        // A legacy CLI that reports a failed worktree and still exits 0.
        let text = """
            🔄 Syncing devcontainer configuration to 3 feature worktree(s)

              prine ... ✓ synced 3 files (copy)
              remotion ... ✗ failed: IO error: Permission denied (os error 13)
              coding-agents ... ✓ synced 3 files (copy)

            ⚠️  1 error(s) occurred:
            """
        let runner = ScriptedProcessRunner([.exit(["branchbox", "devcontainer", "sync"], stdout: text)])
        let identity = RenderHarness.cliIdentity(contract: false)
        let backend = CLIBackend(executable: URL(fileURLWithPath: "/opt/homebrew/bin/branchbox"), identity: identity, runner: runner,
                                 environment: StaticEnvironment(), gitExecutable: URL(fileURLWithPath: "/usr/bin/git"))
        let legacy = RenderHarness(bootstrapper: RenderBootstrapper(backend: backend, identity: identity))
        await legacy.model.start()
        let applied = SyncSheetModel()
        applied.apply(project: PreviewSamples.project, using: legacy.model)
        try await RenderHarness.finish(applied.run)
        try SnapshotRenderer.render("sync-result-with-failure", size: size) {
            // The sample project's registry supplies the failed feature's folder (the legacy CLI doesn't print it).
            SyncDevcontainersSheet(project: PreviewSamples.project, model: applied).environment(harness.model)
        }
        await harness.tearDown()
        await legacy.tearDown()
    }

    // MARK: Project settings

    @Test func projectSettings() async throws {
        let size = CGSize(width: 620, height: 600)
        let harness = try await RenderHarness.started(.contract, addSample: true)
        let document = RenderHarness.contractConfig
        for tab in ConfigForm.Tab.allCases {
            let sheet = ProjectSettingsModel(project: PreviewSamples.project, document: document)
            sheet.tab = tab
            if tab == .sharing { sheet.apiToken = "cf-token-1234567890" }
            try SnapshotRenderer.render("settings-project-\(tab.rawValue)", size: tab == .sharing ? CGSize(width: 620, height: 1100) : size) {
                ProjectSettingsSheet(model: sheet).environment(harness.model)
            }
        }

        let editing = ProjectSettingsModel(project: PreviewSamples.project, document: document)
        editing.form?.set("feature.branch_prefix", .string("spike"))
        editing.form?.set("tunnel.enabled", .bool(false))
        await editing.review(using: harness.model)
        try SnapshotRenderer.render("settings-project-review", size: size) {
            ProjectSettingsSheet(model: editing).environment(harness.model)
        }

        let refusal = BackendError.refused(Refusal(
            cause: .configInvalid(key: "feature.branch_prefix", detail: "“feat..ure” is not a valid branch prefix (git check-ref-format)"),
            message: "Invalid value for feature.branch_prefix", diagnostics: Diagnostics(summary: "config_invalid")))
        await harness.backend?.script(.applyConfig, .fail(refusal))
        let invalid = ProjectSettingsModel(project: PreviewSamples.project, document: document)
        invalid.form?.set("feature.branch_prefix", .string("feat-ure"))
        await invalid.review(using: harness.model)
        try SnapshotRenderer.render("settings-project-key-error", size: size) {
            ProjectSettingsSheet(model: invalid).environment(harness.model)
        }

        let legacy = try await RenderHarness.started(.legacy0134, addSample: true)
        let readOnly = ProjectSettingsModel(project: PreviewSamples.project)
        await readOnly.load(using: legacy.model)
        try SnapshotRenderer.render("settings-project-legacy", size: size) {
            ProjectSettingsSheet(model: readOnly).environment(legacy.model)
        }
        await harness.tearDown()
        await legacy.tearDown()
    }

    // MARK: App settings

    @Test func appSettingsTabs() async throws {
        let harness = try await RenderHarness.started(RenderHarness.cliScenario(.contract))
        // One long name and value, so the render shows they wrap rather than clip.
        harness.model.settings.extraEnvironment = ["RUST_LOG": "info", "BRANCHBOX_PROJECTS_DIR": "~/projects",
                                                   "BRANCHBOX_EXTRA_COMPOSE_PROFILE_OVERRIDES_FOR_CI": "ci,integration-tests,metrics"]
        let tabs: [(String, AnyView)] = [
            ("general", AnyView(GeneralTab())),
            ("tools", AnyView(ToolsTab())),
            ("editors", AnyView(EditorsTerminalTab())),
            ("agent", AnyView(CodingAgentTab())),
            ("notifications", AnyView(NotificationsTab())),
            ("refresh", AnyView(RefreshTab())),
            ("advanced", AnyView(AdvancedTab())),
        ]
        for (name, view) in tabs {
            try SnapshotRenderer.render("settings-app-\(name)", size: CGSize(width: 620, height: name == "tools" ? 860 : 480)) {
                view.environment(harness.model)
            }
        }
        harness.model.settings.agentChoice = .custom(command: "aider --model sonnet")
        harness.model.settings.preferredTerminal = .custom(template: "open -a Ghostty")
        try SnapshotRenderer.render("settings-app-agent-custom", size: Self.settingsTab) {
            CodingAgentTab().environment(harness.model)
        }
        try SnapshotRenderer.render("settings-app-editors-custom", size: Self.settingsTab) {
            EditorsTerminalTab().environment(harness.model)
        }

        let missing = try await RenderHarness.started(.cliMissing)
        try SnapshotRenderer.render("settings-app-tools-cli-missing", size: CGSize(width: 620, height: 640)) {
            ToolsTab().environment(missing.model)
        }
        await harness.tearDown()
        await missing.tearDown()
    }

    // MARK: Diagnostics

    @Test func diagnostics() async throws {
        let size = CGSize(width: 640, height: 1400)
        let harness = try await RenderHarness.started(RenderHarness.cliScenario(.interruptedSetup), addSample: true)
        await harness.model.environment.runDoctor(for: nil)
        let syncPreview = harness.model.actions.dispatch(.syncDevcontainers(SyncRequest(project: PreviewSamples.project, dryRun: true)))
        if case .started(let record) = syncPreview { try await RenderHarness.finish(record) }
        await harness.backend?.script(.initProject, .fail(.commandFailed(Diagnostics(summary: "Not a git repository: /tmp/scratch"))))
        let failedCheck = harness.model.actions.dispatch(.initProject(InitRequest(folder: URL(fileURLWithPath: "/tmp/scratch"), mode: .validate)))
        if case .started(let record) = failedCheck { try await RenderHarness.finish(record) }
        try SnapshotRenderer.render("diagnostics-contract", size: size) {
            DiagnosticsWindow().environment(harness.model)
        }

        let missing = try await RenderHarness.started(.cliMissing)
        try SnapshotRenderer.render("diagnostics-cli-missing", size: CGSize(width: 640, height: 900)) {
            DiagnosticsWindow().environment(missing.model)
        }

        let legacy = try await RenderHarness.started(.legacy0134, addSample: true)
        await legacy.model.environment.runDoctor(for: nil)
        try SnapshotRenderer.render("diagnostics-legacy", size: CGSize(width: 640, height: 900)) {
            DiagnosticsWindow().environment(legacy.model)
        }
        for harness in [harness, missing, legacy] { await harness.tearDown() }
    }
}

// MARK: - Harness

/// An AppModel on `PreviewBackend` (or any bootstrapper), stored in a throwaway folder.
@MainActor private final class RenderHarness {
    let directory: URL
    let defaultsName: String
    let model: AppModel
    let backend: PreviewBackend?

    convenience init(_ scenario: PreviewScenario) {
        let backend = PreviewBackend(scenario: scenario)
        let bootstrapper: any BackendBootstrapping = switch scenario.identity {
        case .success(let identity): RenderBootstrapper(backend: backend, identity: identity)
        case .failure: PreviewBootstrapper(backend: backend)
        }
        self.init(bootstrapper: bootstrapper, backend: backend)
    }

    init(bootstrapper: any BackendBootstrapping, backend: PreviewBackend? = nil) {
        let base = FileManager.default.temporaryDirectory.appendingPathComponent("branchbox-tests", isDirectory: true)
        directory = base.appendingPathComponent("renders-\(UUID().uuidString)", isDirectory: true)
        defaultsName = base.appendingPathComponent("render-settings-\(UUID().uuidString)").path
        model = AppModel(settings: AppSettings(defaults: UserDefaults(suiteName: defaultsName)!), bootstrapper: bootstrapper,
                         notifier: NoopNotifier(), configuration: .isolated(in: directory))
        self.backend = backend
    }

    static func started(_ scenario: PreviewScenario, addSample: Bool = false) async throws -> RenderHarness {
        let harness = RenderHarness(scenario)
        await harness.model.start()
        if addSample { _ = await harness.model.projects.add(folder: PreviewSamples.project.root) }
        return harness
    }

    var store: ProjectStore? { model.projects.project(PreviewSamples.project) }

    /// Refreshes the sample project and loads its config and detect report, as the detail view would.
    func loadSample() async throws {
        let store = try #require(store)
        await store.refresh(.manual)
        await store.reloadConfig()
        await store.reloadDetect()
    }

    func waitForLoad() async throws {
        let deadline = ContinuousClock.now + .seconds(30)
        while true {
            guard let store else { return }
            if store.loadState != .idle, store.loadState != .loading, !store.isRefreshing { return }
            try #require(ContinuousClock.now < deadline, "the project did not load")
            try await Task.sleep(for: .milliseconds(5))
        }
    }

    static func finish(_ record: OperationRecord?) async throws {
        let record = try #require(record)
        let deadline = ContinuousClock.now + .seconds(30)
        while record.isCancellable {
            try #require(ContinuousClock.now < deadline, "\(record.title) did not finish")
            try await Task.sleep(for: .milliseconds(5))
        }
        try await Task.sleep(for: .milliseconds(150))             // the record's batched log flush
    }

    func tearDown() async {
        await model.prepareForTermination()
        UserDefaults(suiteName: defaultsName)?.removePersistentDomain(forName: defaultsName)
        try? FileManager.default.removeItem(atPath: defaultsName + ".plist")
        try? FileManager.default.removeItem(at: directory)
    }

    // MARK: Data

    static func cliIdentity(contract: Bool) -> BackendIdentity {
        BackendIdentity(kind: .cli(CLIResolution(path: "/opt/homebrew/bin/branchbox", source: .loginShellPath,
                                                 rejected: [RejectedCandidate(path: "/usr/local/bin/branchbox",
                                                                              reason: "version 0.12.0 is older than 0.13.4")])),
                        version: contract ? SemVer(0, 14, 0) : SemVer(0, 13, 4), contractVersion: contract ? 1 : nil,
                        capabilities: contract ? PreviewSamples.allCapabilities : [])
    }

    /// The same world reported by a CLI identity: real paths (none exist here, so folders read as missing) and a
    /// CLI location for Settings and Diagnostics.
    static func cliScenario(_ scenario: PreviewScenario) -> PreviewScenario {
        let contract = (try? scenario.identity.get())?.contractVersion != nil
        return scenario.with(identity: .success(cliIdentity(contract: contract)), name: scenario.name + "CLI")
    }

    static func line(_ message: String) -> LogLine {
        LogLine(timestamp: .now, level: message.contains("[DRY RUN]") ? .info : .output, source: .stdout, target: nil,
                message: message)
    }

    /// The interrupted-setup world plus features with long names, ports and a tunnel.
    static var busyListing: FeatureListing {
        let now = Date.now
        let long = FeatureRecord(
            workFeature: "payments-reconciliation-with-a-very-long-descriptive-name",
            branchName: "feature/payments-reconciliation-with-a-very-long-descriptive-name",
            worktreePath: "/Users/dev/projects/branchbox-suite/branchbox/payments-reconciliation-with-a-very-long-descriptive-name",
            status: .active, createdAt: now.addingTimeInterval(-7200), updatedAt: now.addingTimeInterval(-300),
            tunnel: TunnelState(provider: "cloudflared", hostname: "payments.example.dev", status: .active), color: "#16a085",
            startMode: "minimal",
            runtime: RuntimeInfo(provider: .sbx, runtimeID: "sbx-7f3a",
                                 publishedPorts: [PublishedPort(host: 49_152, runtime: 3000)]))
        return FeatureListing(features: [long] + PreviewSamples.features + [PreviewSamples.interruptedFeature],
                              strays: PreviewSamples.strays)
    }

    /// A contract `config get` document with the full key table and a few values set.
    static var contractConfig: ProjectConfigDocument {
        func key(_ name: String, _ type: String, allowed: [String] = [], _ value: JSONValue?, default defaultValue: JSONValue? = nil,
                 _ description: String) -> ConfigKeyDescriptor {
            ConfigKeyDescriptor(key: name, type: type, allowed: allowed, defaultValue: defaultValue ?? value, value: value,
                                source: "file", description: description)
        }
        let keys = [
            key("runtime.provider", "enum", allowed: ["container", "sbx", "local-vm", "in-guest"], .string("container"),
                "Runtime used when `feature start` gets no `--runtime`."),
            key("runtime.sbx.run_services", "string_list", .array([.string("web"), .string("worker")]),
                "Compose services a devcontainer starts inside Docker Sandboxes."),
            key("feature.branch_prefix", "string", .string("feature"), "Prefix of the branch a new feature gets: feature `eta` starts on `<prefix>/eta`."),
            key("feature.teardown.delete_branch_by_default", "bool", .bool(true), "Delete the feature branch when the feature is torn down."),
            key("feature.teardown.force_delete_unmerged_by_default", "bool", .bool(false),
                "Delete a feature branch that has unmerged commits (`git branch -D`) without asking."),
            key("feature.teardown.prompt_force_delete_unmerged", "bool", .bool(true),
                "In an interactive terminal, ask before force-deleting a branch with unmerged commits."),
            key("tunnel.enabled", "bool", .bool(true), "Provision a public tunnel for each feature."),
            key("tunnel.default_provider", "enum", allowed: ["cloudflared"], .string("cloudflared"), "Tunnel provider for new tunnels."),
            key("tunnel.providers.cloudflared.account_id", "string", .string("8c1f2e3d4b5a69788796a5b4c3d2e1f0"),
                "Cloudflare account that owns the tunnels."),
            key("tunnel.providers.cloudflared.tunnel_name_prefix", "string", .string("branchbox"),
                "Prefix of tunnel names and, with `dns_zone`, of feature hostnames."),
            key("tunnel.providers.cloudflared.dns_zone", "string", .string("example.dev"),
                "Cloudflare DNS zone in which tunnel hostnames are created."),
            key("tunnel.providers.cloudflared.service_url", "string", nil, "Service the tunnel forwards to (for example `http://app:5001`)."),
            key("tunnel.providers.cloudflared.manual_instructions", "bool", .bool(false),
                "Print manual tunnel setup steps instead of provisioning through the Cloudflare API."),
            key("tunnel.providers.cloudflared.api_token_path", "string", .string(".branchbox/secure/cloudflared.env"),
                "File holding `CLOUDFLARE_API_TOKEN`."),
            key("editor.default_agent", "string", .string("claude"), "Coding agent the editor integration prefers."),
            key("editor.auto_launch_agent_terminal", "bool", .bool(false),
                "Open a terminal running the default agent when the editor attaches to a feature."),
            key("editor.preferred_sidebar_view", "string", nil, "Editor view to focus on attach (for example `workbench.view.scm`)."),
            key("editor.hide_secondary_sidebar", "bool", .bool(false), "Hide the editor's secondary (right) sidebar on attach."),
        ]
        return ProjectConfigDocument(path: "\(PreviewSamples.project.path)/.branchbox/config.json", exists: true,
                                     effective: .defaults, keys: keys, editable: true)
    }
}

/// Bootstraps a fixed backend under a given identity, with a realistic login-shell capture.
private struct RenderBootstrapper: BackendBootstrapping {
    let backend: any BranchBoxBackend
    let identity: BackendIdentity

    func bootstrap(_ settings: BackendSettings) async -> BackendBootstrap { .ready(backend, identity) }

    func environmentSummary() async -> EnvironmentSummary? {
        EnvironmentSummary(source: .interactiveLogin, shell: "/bin/zsh", captureDuration: .milliseconds(412),
                           pathEntries: ["/Users/dev/.cargo/bin", "/opt/homebrew/bin", "/opt/homebrew/sbin", "/usr/local/bin",
                                         "/usr/bin", "/bin", "/usr/sbin", "/sbin", "/Applications/Docker.app/Contents/Resources/bin"],
                           capturedAt: .now, isProvisional: false)
    }

    func recaptureEnvironment() async {}
    func terminateAllProcesses() async {}
}
