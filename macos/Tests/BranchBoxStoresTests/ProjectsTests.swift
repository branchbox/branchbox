import BranchBoxKit
import BranchBoxPreview
@testable import BranchBoxStores
import Foundation
import Testing

// Projects (§8.5, D-22): adding through resolveProject, persistence in projects.json, ordering, relocation and
// removal; the legacy-defaults migration; AppModel start, intents and the environment; settings.

@MainActor @Suite(.timeLimit(.minutes(1))) struct ProjectsTests {
    // MARK: Adding

    @Test func addingAFeatureWorktreeAddsItsMainWorktree() async throws {
        let harness = Harness()
        defer { harness.tearDown() }
        await harness.model.start()
        let prineFolder = URL(fileURLWithPath: "/Users/dev/projects/branchbox-suite/branchbox/prine")

        let outcome = await harness.model.projects.add(folder: prineFolder)
        #expect(outcome == .added(sampleProject, note: "This is a feature worktree of \(sampleProject.path); "
                                  + "adding \(sampleProject.path) instead"))
        #expect(await harness.model.projects.add(folder: sampleProject.root) == .alreadyPresent(sampleProject))
        #expect(await harness.model.projects.add(folder: URL(fileURLWithPath: sampleProject.path + "/")) == .alreadyPresent(sampleProject))
        let store = try #require(harness.model.projects.project(sampleProject))
        #expect(store.displayName == "branchbox")
        try await waitUntilLoaded(store)
        #expect(store.features.count == 5)
    }

    @Test func addingNeedsInitOrIsRefused() async throws {
        let harness = Harness()
        defer { harness.tearDown() }
        await harness.model.start()
        let fresh = URL(fileURLWithPath: "/tmp/bbx/fresh")
        await harness.backend.setResolution(ProjectResolution(project: ProjectRef(root: fresh), requested: fresh,
                                                              normalization: .none, initialized: false), for: fresh)
        #expect(await harness.model.projects.add(folder: fresh) == .needsInit(ProjectRef(root: fresh)))
        #expect(harness.model.projects.projects.isEmpty)

        let notGit = BackendError.refused(Refusal(cause: .notGitRepository("/tmp"), message: "Not a git repository: /tmp",
                                                  diagnostics: Diagnostics(summary: "Validation error: Not a git repository: /tmp")))
        await harness.backend.script(.resolveProject, .fail(notGit))
        #expect(await harness.model.projects.add(folder: URL(fileURLWithPath: "/tmp")) == .refused(notGit))

        let container = URL(fileURLWithPath: "/tmp/bbx/app")
        let main = ProjectRef(root: container.appendingPathComponent("main"))
        await harness.backend.setResolution(ProjectResolution(project: main, requested: container, normalization: .fromParentContainer,
                                                              initialized: true), for: container)
        guard case .added(main, let note?) = await harness.model.projects.add(folder: container) else {
            throw Failure("expected the container's main worktree")
        }
        #expect(note == "/tmp/bbx/app holds BranchBox worktrees; adding its main worktree /tmp/bbx/app/main")
    }

    @Test func addingBeforeTheCLIIsLocatedIsRefused() async {
        let harness = Harness()
        defer { harness.tearDown() }
        guard case .refused(.cliUnusable) = await harness.model.projects.add(folder: sampleProject.root) else {
            Issue.record("expected a still-locating refusal")
            return
        }
        #expect(throws: BackendError.self) { _ = try harness.model.backend() }
    }

    @Test func duplicatesAreFoundByCanonicalPath() async throws {
        let harness = Harness()
        defer { harness.tearDown() }
        await harness.model.start()
        let real = harness.sandbox.folder("real/main")
        let link = harness.sandbox.directory.appendingPathComponent("link")
        try FileManager.default.createSymbolicLink(at: link, withDestinationURL: real.deletingLastPathComponent())
        guard case .added = await harness.model.projects.add(folder: real) else { throw Failure("expected .added") }
        #expect(await harness.model.projects.add(folder: link.appendingPathComponent("main")) == .alreadyPresent(ProjectRef(root: real)))
        #expect(harness.model.projects.projects.count == 1)
    }

    // MARK: Persistence and ordering

    @Test func projectsRoundTripThroughProjectsJSONInOrder() async throws {
        let clock = ManualClock()
        let harness = Harness { $0.clock = clock }
        defer { harness.tearDown() }
        await harness.model.start()
        let projects = harness.model.projects
        let roots = ["alpha", "beta", "gamma"].map { URL(fileURLWithPath: "/tmp/bbx-order/\($0)") }
        for root in roots {
            _ = await projects.add(folder: root)
            clock.advance(by: .seconds(1))
        }
        #expect(projects.projects.map(\.displayName) == ["gamma", "beta", "alpha"])     // most recently opened first

        projects.setPinned(ProjectRef(root: roots[0]), true)
        projects.markOpened(ProjectRef(root: roots[1]))
        projects.setCollapsed(ProjectRef(root: roots[2]), true)
        projects.rename(ProjectRef(root: roots[2]), to: "  Gamma App ")
        #expect(projects.projects.map(\.displayName) == ["alpha", "beta", "Gamma App"])

        let file = harness.sandbox.configuration.projectsDirectory.appendingPathComponent("projects.json")
        let json = try #require(try JSONSerialization.jsonObject(with: Data(contentsOf: file)) as? [String: Any])
        #expect(json["version"] as? Int == 1)
        let entries = try #require(json["projects"] as? [[String: Any]])
        #expect(entries.map { $0["root"] as? String } == ["/tmp/bbx-order/alpha", "/tmp/bbx-order/beta", "/tmp/bbx-order/gamma"])
        #expect(Set(entries[0].keys) == ["root", "displayName", "addedAt", "lastOpenedAt", "pinned", "collapsed"])
        #expect(entries[0]["pinned"] as? Bool == true)
        #expect((entries[0]["addedAt"] as? String).flatMap(RFC3339.parse) == Date(timeIntervalSince1970: 1_790_000_000))

        let reloaded = ProjectsStore(environment: harness.model.environment,
                                     repository: ProjectsRepository(directory: harness.sandbox.configuration.projectsDirectory),
                                     limiter: ListLimiter())
        reloaded.load()
        #expect(reloaded.projects.map(\.ref) == projects.projects.map(\.ref))
        #expect(reloaded.projects.map(\.entry) == projects.projects.map(\.entry))
        #expect(reloaded.projects[2].isCollapsed)

        projects.rename(ProjectRef(root: roots[2]), to: "")
        #expect(projects.project(ProjectRef(root: roots[2]))?.displayName == "gamma")
        projects.setPinned(ProjectRef(root: roots[0]), false)
        #expect(projects.projects.map(\.displayName) == ["beta", "gamma", "alpha"])
    }

    @Test func aBadEntryDoesNotHideTheRestAndAnUnreadableFileIsKept() throws {
        let sandbox = Sandbox()
        defer { sandbox.remove() }
        let repository = ProjectsRepository(directory: sandbox.directory)
        let json = #"""
        {"version": 1, "projects": [
          {"root": "/r/main", "displayName": "", "addedAt": "2026-10-01T10:00:00Z", "pinned": true, "extra": 1},
          {"root": "relative/path"},
          {"displayName": "no root"},
          {"root": "/r/main"},
          {"root": "/s/app/", "addedAt": "garbage", "lastOpenedAt": "2026-10-02T10:00:00.5+02:00"}
        ]}
        """#
        try Data(json.utf8).write(to: repository.fileURL)
        let entries = repository.load()
        #expect(entries.map(\.root) == ["/r/main", "/s/app"])
        #expect(entries[0].displayName == "r")
        #expect(entries[0].pinned && !entries[0].collapsed)
        #expect(entries[0].addedAt == RFC3339.parse("2026-10-01T10:00:00Z"))
        #expect(entries[1].lastOpenedAt == RFC3339.parse("2026-10-02T08:00:00.5Z"))

        try Data("not json".utf8).write(to: repository.fileURL)
        #expect(repository.load().isEmpty)
        let aside = sandbox.directory.appendingPathComponent("projects.json.unreadable")
        #expect(try String(contentsOf: aside, encoding: .utf8) == "not json")
        #expect(ProjectsRepository(directory: sandbox.directory.appendingPathComponent("missing")).load().isEmpty)
    }

    @Test func startLoadsSavedProjectsAndValidatesRoots() async throws {
        let sandbox = Sandbox()
        defer { sandbox.remove() }
        let present = sandbox.folder("present/main")
        let entries = [ProjectEntry(root: present.path, displayName: "present", addedAt: Date(timeIntervalSince1970: 1)),
                       ProjectEntry(root: "/tmp/bbx-gone-\(UUID().uuidString)/main", displayName: "gone",
                                    addedAt: Date(timeIntervalSince1970: 2))]
        try ProjectsRepository(directory: sandbox.configuration.projectsDirectory).save(entries)
        let scenario = PreviewScenario.emptyProject.with(identity: .success(cliIdentity))
        let backend = PreviewBackend(scenario: scenario)
        let model = AppModel(settings: AppSettings(defaults: sandbox.defaults), bootstrapper: PreviewBootstrapper(backend: backend),
                             notifier: NoopNotifier(), configuration: sandbox.configuration)
        defer { model.coordinator.stop() }

        await model.start()
        #expect(model.hasStarted)
        #expect(model.projects.projects.map(\.displayName) == ["gone", "present"])
        #expect(model.projects.projects.map(\.rootExists) == [false, true])
        for store in model.projects.projects { try await waitUntilLoaded(store) }   // every project refreshes at start
        #expect(await backend.calls(to: .listFeatures).count == 2)
    }

    @Test func removingAProjectNeverDeletesFiles() async throws {
        let harness = Harness()
        defer { harness.tearDown() }
        await harness.model.start()
        let root = harness.sandbox.folder("keep/main")
        try Data("hello".utf8).write(to: root.appendingPathComponent("README.md"))
        _ = await harness.model.projects.add(folder: root)
        harness.model.projects.selectedProject = ProjectRef(root: root)

        harness.model.projects.remove(ProjectRef(root: root))
        #expect(harness.model.projects.project(ProjectRef(root: root)) == nil)
        #expect(harness.model.projects.selectedProject == nil)
        #expect(try String(contentsOf: root.appendingPathComponent("README.md"), encoding: .utf8) == "hello")
        harness.model.projects.remove(ProjectRef(root: root))            // removing twice is harmless
    }

    @Test func relocateKeepsTheProjectsSettings() async throws {
        let harness = Harness()
        defer { harness.tearDown() }
        await harness.model.start()
        let projects = harness.model.projects
        let old = ProjectRef(root: URL(fileURLWithPath: "/tmp/bbx-move/old/main"))
        _ = await projects.add(folder: old.root)
        projects.setPinned(old, true)
        projects.rename(old, to: "My App")
        projects.selectedProject = old

        let new = URL(fileURLWithPath: "/tmp/bbx-move/new/main")
        #expect(await projects.relocate(old, to: new) == .added(ProjectRef(root: new), note: nil))
        #expect(projects.project(old) == nil)
        let moved = try #require(projects.project(ProjectRef(root: new)))
        #expect(moved.isPinned && moved.displayName == "My App")
        #expect(projects.selectedProject == ProjectRef(root: new))
        #expect(await projects.relocate(ProjectRef(root: new), to: new) == .alreadyPresent(ProjectRef(root: new)))

        let failure = BackendError.commandFailed(Diagnostics(summary: "boom"))
        await harness.backend.script(.resolveProject, .fail(failure))
        #expect(await projects.relocate(ProjectRef(root: new), to: URL(fileURLWithPath: "/elsewhere")) == .refused(failure))
        #expect(projects.project(ProjectRef(root: new)) != nil)
    }

    @Test func initCompletionAddsTheProjectAndStartsItsWatcher() async throws {
        let harness = Harness()
        defer { harness.tearDown() }
        await harness.model.start()
        let root = harness.sandbox.folder("fresh/main")
        let request = InitRequest(folder: root)
        let record = try operation(of: harness.model.actions.dispatch(.initProject(request)))
        #expect(record.kind == .initProject && record.target == .project(ProjectRef(root: root)))
        try await waitUntilFinished(record)
        try await waitUntil { harness.model.projects.project(ProjectRef(root: root)) != nil }
        let store = try #require(harness.model.projects.project(ProjectRef(root: root)))
        #expect(!store.isWatchingRegistry)                        // the preview CLI wrote no .branchbox

        try FileManager.default.createDirectory(at: root.appendingPathComponent(".branchbox"), withIntermediateDirectories: true)
        let repair = try operation(of: harness.model.actions.dispatch(.initProject(InitRequest(folder: root, mode: .update))))
        try await waitUntilFinished(repair)
        try await waitUntil { store.isWatchingRegistry }
    }

    // MARK: Legacy defaults

    @Test func legacyDefaultsAreMigratedOnce() async throws {
        let harness = Harness()
        defer { harness.tearDown() }
        let defaults = harness.sandbox.defaults
        let workspace = harness.sandbox.folder("legacy/main")
        defaults.set(workspace.path, forKey: "branchbox.workspace")
        defaults.set(["fix the login bug", "add oauth", "  "], forKey: "branchbox.promptHistory")
        defaults.set("grpc", forKey: "branchbox.transportPreference")
        defaults.set(true, forKey: "branchbox.teardown.force")
        defaults.set(true, forKey: "branchbox.teardown.deleteBranch")
        defaults.set(true, forKey: "branchbox.teardown.completeSpec")
        defaults.set("symlink", forKey: "branchbox.devcontainerStrategy")
        harness.settings.promptHistory = ["add oauth", "newer prompt"]

        await harness.model.start()

        #expect(harness.model.projects.project(ProjectRef(root: workspace)) != nil)
        #expect(harness.settings.promptHistory == ["add oauth", "newer prompt", "fix the login bug"])
        for key in ["branchbox.workspace", "branchbox.promptHistory"] + LegacyDefaultsMigration.obsoleteKeys {
            #expect(defaults.object(forKey: key) == nil, "\(key) should be gone")
        }
        #expect(AppSettings(defaults: defaults).promptHistory == ["add oauth", "newer prompt", "fix the login bug"])

        let again = await LegacyDefaultsMigration.run(settings: harness.settings, projects: harness.model.projects,
                                                      backendAvailable: true)
        #expect(again == LegacyDefaultsMigration.Report())
    }

    @Test func theStaleDevcontainerWorkspaceIsDropped() async throws {
        let harness = Harness()
        defer { harness.tearDown() }
        harness.sandbox.defaults.set("/workspaces/milestone2", forKey: "branchbox.workspace")
        await harness.model.start()
        #expect(harness.model.projects.projects.isEmpty)
        #expect(harness.sandbox.defaults.object(forKey: "branchbox.workspace") == nil)
        #expect(await harness.backend.calls(to: .resolveProject).isEmpty)
    }

    @Test func theWorkspaceWaitsForABackend() async throws {
        let harness = Harness(.cliMissing)
        defer { harness.tearDown() }
        harness.sandbox.defaults.set("/Users/dev/app", forKey: "branchbox.workspace")
        harness.sandbox.defaults.set(true, forKey: "branchbox.teardown.force")
        await harness.model.start()
        #expect(harness.sandbox.defaults.string(forKey: "branchbox.workspace") == "/Users/dev/app")
        #expect(harness.sandbox.defaults.object(forKey: "branchbox.teardown.force") == nil)

        let report = await LegacyDefaultsMigration.run(settings: harness.settings, projects: harness.model.projects,
                                                       backendAvailable: false)
        #expect(report.workspaceDeferred)
    }

    @Test func anUnresolvableWorkspaceIsDropped() async throws {
        let harness = Harness()
        defer { harness.tearDown() }
        let failure = BackendError.refused(Refusal(cause: .notGitRepository("/gone"), message: "Not a git repository: /gone",
                                                   diagnostics: Diagnostics(summary: "x")))
        await harness.backend.script(.resolveProject, .fail(failure))
        harness.sandbox.defaults.set("/gone", forKey: "branchbox.workspace")
        await harness.model.start()
        #expect(harness.model.projects.projects.isEmpty)
        #expect(harness.sandbox.defaults.object(forKey: "branchbox.workspace") == nil)
    }

    // MARK: AppModel

    @Test func postBumpsTheIntentToken() {
        let harness = Harness()
        defer { harness.tearDown() }
        let model = harness.model
        #expect(model.intentToken == 0)
        model.post(.showDiagnostics)
        model.post(.prune(sampleProject))
        #expect(model.intentToken == 2)
        #expect(model.pendingIntent == .prune(sampleProject))
        #expect(model.takePendingIntent() == .prune(sampleProject))
        #expect(model.takePendingIntent() == nil)
        #expect(model.pendingIntent == nil)
        model.post(.prune(sampleProject))
        #expect(model.intentToken == 3)                             // the same intent again still bumps
    }

    @Test func startBootstrapsThroughTheInjectedBootstrapper() async throws {
        let harness = Harness()
        defer { harness.tearDown() }
        let model = harness.model
        #expect(model.environment.backendState == .resolving)
        await model.start()
        let identity = try #require(model.environment.identity)
        #expect(model.environment.backendState == .ready(identity))
        #expect(model.environment.supports(.config))
        #expect(model.environment.summary?.isProvisional == false)
        #expect(model.environment.lastBootstrapAt != nil)
        #expect(model.environment.resolution == nil)                 // the preview backend has no CLI path
        _ = try model.backend()
        await model.start()                                          // runs once
        #expect(await harness.backend.calls.isEmpty)

        await model.environment.runDoctor(for: sampleProject)
        #expect(model.environment.doctor?.checks.contains { $0.id == "repo.git" } == true)
        await model.environment.recaptureEnvironment()
        #expect(model.environment.summary != nil)
    }

    @Test func missingCLIIsStoredAsUnavailable() async {
        let harness = Harness(.cliMissing)
        defer { harness.tearDown() }
        let model = harness.model
        await model.start()
        #expect(model.environment.backendState == .unavailable(.cliNotFound(searched: PreviewSamples.searchedPaths)))
        #expect(model.environment.identity == nil)
        #expect(!model.environment.supports(.config))
        #expect(throws: BackendError.cliNotFound(searched: PreviewSamples.searchedPaths)) { _ = try model.backend() }
        await model.environment.runDoctor(for: nil)
        #expect(model.environment.doctor == nil)
    }

    @Test func aChangedBackendRefreshesEveryProject() async throws {
        let bootstrapper = SwitchingBootstrapper()
        let sandbox = Sandbox()
        defer { sandbox.remove() }
        let model = AppModel(settings: AppSettings(defaults: sandbox.defaults), bootstrapper: bootstrapper, notifier: NoopNotifier(),
                             configuration: sandbox.configuration)
        defer { model.coordinator.stop() }
        await model.start()
        _ = await model.projects.add(folder: sampleProject.root)
        let store = try #require(model.projects.project(sampleProject))
        try await waitUntilLoaded(store)
        await bootstrapper.contract.clearCalls()

        bootstrapper.useContract()
        await model.environment.rebootstrap()
        #expect(model.environment.supports(.registryLock))
        try await waitUntil { await bootstrapper.contract.calls(to: .listFeatures).count == 1 }

        await model.environment.rebootstrap()                         // the same identity again: no refresh
        try await Task.sleep(for: .milliseconds(30))
        #expect(await bootstrapper.contract.calls(to: .listFeatures).count == 1)
    }

    @Test func notificationsAreUnavailableUnderTests() async throws {
        #expect(!AppBundle.isBundledApp())
        #expect(!NoopNotifier().isAvailable)
        #expect(await NoopNotifier().requestAuthorizationIfNeeded() == false)
        #expect(AppSettings.defaultsForCurrentProcess() !== UserDefaults.standard)
        #expect(AppModel.Configuration.standard.projectsDirectory.path.hasSuffix("Application Support/BranchBox Dev"))

        let spy = SpyNotifier(isAvailable: false)
        let harness = Harness(notifier: spy)
        defer { harness.tearDown() }
        #expect(harness.model.notifier is NoopNotifier)
        _ = try await harness.startWithSampleProject()
        let failure = BackendError.commandFailed(Diagnostics(summary: "boom"))
        await harness.backend.script(.openTunnel, .fail(failure))
        try await waitUntilFinished(try operation(of: harness.model.actions.dispatch(.tunnelOpen(feature("prine")))))
        try await Task.sleep(for: .milliseconds(30))
        #expect(spy.posted.isEmpty && spy.authorizationCount == 0)
    }

    // MARK: Settings

    @Test func settingsPersistAndFeedTheBackend() throws {
        let sandbox = Sandbox()
        defer { sandbox.remove() }
        let defaults = sandbox.defaults

        let settings = AppSettings(defaults: defaults)
        #expect(settings.selectedProjectRefresh == .m1)
        #expect(settings.otherProjectsRefresh == .m5)
        #expect(settings.logRetention == 100)
        #expect(settings.notificationsEnabled && settings.watchProjectFiles && settings.passPromptToAgent)
        #expect(settings.backendSettings == BackendSettings(agentCommand: "claude", agentName: "Claude Code"))
        settings.cliPathOverride = "/tmp/bbx/branchbox"
        settings.extraEnvironment = ["DOCKER_HOST": "unix:///tmp/docker.sock"]
        settings.agentChoice = .custom(command: "aider --yes")
        settings.agentDisplayName = "Aider"
        settings.preferredEditor = .custom(appPath: "/Applications/Zed.app")
        settings.otherProjectsRefresh = .manual
        settings.notificationsEnabled = false
        settings.quickCommands = [sampleProject.path: ["bin/rails test"]]

        let reloaded = AppSettings(defaults: defaults)
        #expect(reloaded.cliPathOverride == "/tmp/bbx/branchbox")
        #expect(reloaded.preferredEditor == .custom(appPath: "/Applications/Zed.app"))
        #expect(reloaded.otherProjectsRefresh == .manual)
        #expect(!reloaded.notificationsEnabled)
        #expect(reloaded.quickCommands == [sampleProject.path: ["bin/rails test"]])
        #expect(reloaded.backendSettings == BackendSettings(cliPathOverride: "/tmp/bbx/branchbox",
                                                            extraEnvironment: ["DOCKER_HOST": "unix:///tmp/docker.sock"],
                                                            agentCommand: "aider --yes", agentName: "Aider"))
        reloaded.cliPathOverride = "  "
        reloaded.agentChoice = .codex
        reloaded.agentDisplayName = " "
        #expect(reloaded.backendSettings.cliPathOverride == nil)
        #expect(reloaded.backendSettings.agentCommand == "codex" && reloaded.backendSettings.agentName == "Codex")
        reloaded.agentChoice = .custom(command: " ")
        #expect(reloaded.backendSettings.agentCommand == nil && reloaded.backendSettings.agentName == nil)
        reloaded.cliPathOverride = nil
        #expect(AppSettings(defaults: defaults).cliPathOverride == nil)
    }

    @Test func promptHistoryKeepsTheNewestTen() {
        let sandbox = Sandbox()
        defer { sandbox.remove() }
        let settings = AppSettings(defaults: sandbox.defaults)
        for index in 0..<12 { settings.recordPrompt("prompt \(index)") }
        settings.recordPrompt("prompt 5")
        settings.recordPrompt("   ")
        #expect(settings.promptHistory.count == 10)
        #expect(settings.promptHistory.first == "prompt 5")
        #expect(settings.promptHistory.filter { $0 == "prompt 5" }.count == 1)
        #expect(!settings.promptHistory.contains("prompt 0"))
    }

    @Test func operationKindFlags() {
        let registryWriters: Set<OperationKind> = [.start, .teardown, .prune, .tunnelOpen, .tunnelRemove, .syncDevcontainers,
                                                   .initProject, .applyConfig, .tunnelCredentials]
        let projectWide: Set<OperationKind> = [.prune, .syncDevcontainers, .initProject, .applyConfig, .tunnelCredentials]
        let all: [OperationKind] = [.start, .teardown, .prune, .exec, .devcontainerUp, .devcontainerDown, .devcontainerRebuild,
                                    .devcontainerBuild, .syncDevcontainers, .tunnelOpen, .tunnelRemove, .initProject,
                                    .applyConfig, .tunnelCredentials, .deleteBranch, .removeStray]
        for kind in all {
            #expect(kind.writesRegistry == registryWriters.contains(kind), "\(kind)")
            #expect(kind.isProjectWide == projectWide.contains(kind), "\(kind)")
            #expect(kind.isMutating == (kind != .exec), "\(kind)")
        }
        #expect(OperationRequestContext.deleteBranch("b", sampleProject, force: false).operationTarget == .project(sampleProject))
        #expect(OperationRequestContext.removeStray(PreviewSamples.stray, sampleProject, discardChanges: true).operationKind
            == .removeStray)
        #expect(OperationTarget.global.project == nil)
    }

    @Test func navigationValuesRoundTripThroughJSON() throws {
        let selections: [SidebarSelection] = [
            .welcome, .project(path: sampleProject.path), .feature(projectPath: sampleProject.path, name: "prine"),
            .stray(projectPath: sampleProject.path, path: PreviewSamples.stray.path),
        ]
        let data = try JSONEncoder().encode(selections)
        #expect(try JSONDecoder().decode([SidebarSelection].self, from: data) == selections)
        #expect(InitSheetMode(rawValue: "repair") == .repair)
    }
}

/// Bootstraps the legacy preview backend until told to switch to a contract one (a CLI upgraded underneath).
private final class SwitchingBootstrapper: BackendBootstrapping, @unchecked Sendable {
    let legacy = PreviewBackend(scenario: .legacy0134)
    let contract = PreviewBackend(scenario: .contract)
    private let lock = NSLock()
    private var upgraded = false

    func useContract() { lock.withLock { upgraded = true } }

    func bootstrap(_ settings: BackendSettings) async -> BackendBootstrap {
        let backend = lock.withLock { upgraded } ? contract : legacy
        guard case .success(let identity) = backend.scenario.identity else { return .unavailable(.cliNotFound(searched: []), nil) }
        return .ready(backend, identity)
    }

    func environmentSummary() async -> EnvironmentSummary? { nil }
    func recaptureEnvironment() async {}
    func terminateAllProcesses() async {}
}
