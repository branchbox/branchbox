@testable import BranchBoxApp
import AppKit
import BranchBoxKit
import BranchBoxPreview
import BranchBoxStores
import Foundation
import Testing

// SW-5: the feature surfaces at view-model level, on PreviewBackend. Availability per runtime and disk state,
// the requests remediation buttons dispatch (checked in the backend's call log), which URLs are links, exec
// results, and the tunnel's forced-removal recovery.

private let project = PreviewSamples.project

private func sample(_ name: String) throws -> FeatureRecord {
    try #require(PreviewSamples.features.first { $0.workFeature == name })
}

private func record(_ name: String, runtime: RuntimeProvider, status: FeatureStatus = .active,
                    modules: [ModuleOutcome] = [], runtimeID: String? = nil) -> FeatureRecord {
    FeatureRecord(workFeature: name, branchName: "feature/\(name)", worktreePath: "/Users/dev/projects/x/\(name)",
                  featureURL: "dev-\(name).localhost", status: status, startMode: "full", moduleOutcomes: modules,
                  runtime: RuntimeInfo(provider: runtime, runtimeID: runtimeID))
}

/// An `AppModel` on a `PreviewBackend` with its storage and defaults in a throwaway folder.
@MainActor private final class SurfaceHarness {
    let directory: URL
    let defaultsName: String
    let backend: PreviewBackend
    let model: AppModel

    init(_ scenario: PreviewScenario = .contract) {
        let base = FileManager.default.temporaryDirectory.appendingPathComponent("branchbox-tests", isDirectory: true)
        directory = base.appendingPathComponent("surfaces-\(UUID().uuidString)", isDirectory: true)
        try? FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        defaultsName = base.appendingPathComponent("surfaces-settings-\(UUID().uuidString)").path
        let settings = AppSettings(defaults: UserDefaults(suiteName: defaultsName)!)
        settings.watchProjectFiles = false
        backend = PreviewBackend(scenario: scenario)
        model = AppModel(settings: settings, bootstrapper: PreviewBootstrapper(backend: backend), notifier: NoopNotifier(),
                         configuration: .isolated(in: directory))
    }

    func start() async throws -> ProjectStore {
        await model.start()
        _ = await model.projects.add(folder: project.root)
        let store = try #require(model.projects.project(project))
        try await until("the project loads") {
            if case .loaded = store.loadState, !store.isRefreshing { return true }
            return false
        }
        await backend.clearCalls()
        return store
    }

    func tearDown() async {
        await model.prepareForTermination()
        UserDefaults(suiteName: defaultsName)?.removePersistentDomain(forName: defaultsName)
        try? FileManager.default.removeItem(atPath: defaultsName + ".plist")
        try? FileManager.default.removeItem(at: directory)
    }

    /// Dispatches `effect` and waits for the operation it started to finish.
    func performAndWait(_ effect: RemediationEffect) async throws {
        let pasteboard = Pasteboard(pasteboard: NSPasteboard(name: .init("bbx-surface-tests")))
        let result = RemediationPerformer.perform(effect, model: model, pasteboard: pasteboard)
        switch result {
        case .started(let record)?, .queued(let record, _)?:
            try await until("\(record.title) finishes") { !record.isCancellable }
        case .rejected(let reason)?:
            Issue.record("dispatch rejected: \(reason)")
        case .unavailable(let error)?:
            Issue.record("backend unavailable: \(error)")
        case nil:
            break
        }
    }

    func until(_ what: String, timeout: Duration = .seconds(5), _ condition: () async -> Bool) async throws {
        let deadline = ContinuousClock.now + timeout
        while await !condition() {
            try #require(ContinuousClock.now < deadline, "\(what) within \(timeout)")
            try await Task.sleep(for: .milliseconds(2))
        }
    }
}

/// SW-5's suites, grouped so `--filter FeatureSurfaceTests` runs them all.
@Suite struct FeatureSurfaceTests {}

// MARK: Availability

extension FeatureSurfaceTests {
    @MainActor @Suite struct Availability {
        @Test func volumeDeletionConfirmationCarriesExplicitConsentAndExplainsItsScope() {
            let feature = FeatureRef(project: project, name: "scope")
            let confirmation = FeatureConfirmations.stopDeletingVolumes(feature)
            #expect(confirmation.request == .devcontainer(.down(removeVolumes: true), feature))
            #expect(confirmation.message.contains("anonymous volumes are deleted"))
            #expect(confirmation.message.contains("named and shared volumes are kept"))
            #expect(confirmation.message.contains("volumes owned by the feature's project"))
            #expect(confirmation.message.contains("cannot be recovered"))
        }

        @Test(arguments: [RuntimeProvider.container, .sbx, .localVM], [true, false])
        func availabilityMatrix(runtime: RuntimeProvider, folderExists: Bool) {
            let feature = record("matrix", runtime: runtime, runtimeID: runtime == .sbx ? "branchbox-matrix" : nil)
            let availability = FeatureActionAvailability(record: feature, folderExists: folderExists, backendReady: true,
                                                         preferences: LaunchPreferences())
            guard folderExists else {
                // Everything that needs the folder says it is missing.
                for plan in [availability.primaryEditor, availability.terminal, availability.agent, availability.devContainer] {
                    #expect(!plan.isEnabled)
                    #expect(plan.disabledReason?.contains("is missing") == true)
                }
                #expect(availability.reveal.disabledReason == "The folder /Users/dev/projects/x/matrix is missing")
                #expect(availability.runCommand.disabledReason?.contains("is missing") == true)
                #expect(!availability.devcontainer.isEnabled)
                #expect(availability.teardown.isEnabled)
                return
            }
            #expect(availability.primaryEditor.isEnabled)
            #expect(availability.reveal.isEnabled)
            #expect(availability.runCommand.isEnabled)
            #expect(availability.tunnel.isEnabled)
            switch runtime {
            case .container:
                #expect(availability.terminal.isEnabled)
                #expect(availability.agent.isEnabled)
                #expect(availability.devContainer.isEnabled)
                #expect(availability.devcontainer.isEnabled)
            case .sbx:
                #expect(availability.terminal.disabledReason == HostLaunchPlan.sandboxShellUnavailable)
                #expect(availability.terminal.copyCommand == "sbx exec branchbox-matrix bash")
                #expect(availability.agent.disabledReason == HostLaunchPlan.sandboxShellUnavailable)
                #expect(!availability.devContainer.isEnabled)
                #expect(availability.devcontainer.disabledReason == "Only features on the container runtime have a dev container")
            default:
                #expect(availability.terminal.isEnabled)
                #expect(availability.agent.isEnabled)
                #expect(!availability.devContainer.isEnabled)
                #expect(!availability.devcontainer.isEnabled)
            }
        }

        @Test func runCommandIsBlockedForOrphanedRemovedOrWithoutCLI() throws {
            let orphan = record("orphan", runtime: .container, status: .orphaned)
            #expect(FeatureActionAvailability(record: orphan, folderExists: true, backendReady: true, preferences: LaunchPreferences())
                .runCommand.disabledReason?.contains("no longer exists") == true)
            let removed = try sample("coding-agents")
            let removedAvailability = FeatureActionAvailability(record: removed, folderExists: false, backendReady: true,
                                                                preferences: LaunchPreferences())
            #expect(removedAvailability.runCommand.disabledReason == "coding-agents has been torn down")
            #expect(!removedAvailability.teardown.isEnabled)
            #expect(!removedAvailability.primaryEditor.isEnabled)
            let prine = try sample("prine")
            #expect(FeatureActionAvailability(record: prine, folderExists: true, backendReady: false, preferences: LaunchPreferences())
                .runCommand.disabledReason == "The BranchBox CLI isn't available")
        }

        @Test func aRunningOperationHoldsTheOperationButtons() throws {
            let availability = FeatureActionAvailability(record: try sample("prine"), folderExists: true, backendReady: true,
                                                         preferences: LaunchPreferences(), busyWith: "Starting prine")
            #expect(availability.remediation.disabledReason == "Waiting for “Starting prine” to finish")
            #expect(!availability.devcontainer.isEnabled)
            #expect(!availability.tunnel.isEnabled)
            #expect(availability.primaryEditor.isEnabled)            // host launches don't wait
        }

        @Test func editorPreferences() throws {
            let prine = try sample("prine")
            let cursor = FeatureActionAvailability(record: prine, folderExists: true, backendReady: true,
                                                   preferences: LaunchPreferences(editor: .cursor))
            #expect(cursor.primaryEditorTitle == "Open in Cursor")
            #expect(cursor.primaryEditor.kind == .openFolder(appBundleID: HostLaunchPlan.cursorBundleID, appPath: nil,
                                                             path: prine.worktreePath ?? ""))
            let devContainer = FeatureActionAvailability(record: prine, folderExists: true, backendReady: true,
                                                         preferences: LaunchPreferences(editorMode: .devContainer))
            #expect(devContainer.primaryEditorTitle == "Open in Dev Container")
            guard case .openURL(let url) = devContainer.primaryEditor.kind else {
                Issue.record("expected a dev container link")
                return
            }
            #expect(url.absoluteString.hasPrefix("vscode://vscode-remote/dev-container+"))
            // A sandbox has no dev container: the preference falls back to the folder.
            let sbx = FeatureActionAvailability(record: try sample("sbx-demo"), folderExists: true, backendReady: true,
                                                preferences: LaunchPreferences(editorMode: .devContainer))
            #expect(sbx.primaryEditorTitle == "Open in VS Code")
            #expect(sbx.primaryEditor.isEnabled)
        }

        @Test func agentComesFromTheProjectSlugThroughThePlanner() {
            #expect(LaunchPreferences(agent: .claude, projectDefaultAgent: "codex").resolvedAgent == .codex)
            #expect(LaunchPreferences(agent: .codex, projectDefaultAgent: "rm -rf /").resolvedAgent == .codex)
            #expect(LaunchPreferences(agent: .custom(command: "aider --yes"), agentDisplayName: "Aider").agentName == "Aider")
            #expect(LaunchPreferences(agent: .claude).agentName == "Claude Code")
        }
    }
}

// MARK: Remediation

extension FeatureSurfaceTests {
    @MainActor @Suite(.serialized) struct RemediationDispatch {
        private func items(_ record: FeatureRecord, model: AppModel, folderExists: Bool = true,
                           branchExists: Bool? = nil) -> [RemediationItem] {
            RemediationPresenter.items(
                for: Remediation.actions(for: record, project: project, identity: model.environment.identity,
                                         folderExists: folderExists, branchExists: branchExists),
                record: record)
        }

        private func noDiscardConsent(_ backend: PreviewBackend) async {
            for call in await backend.calls(to: .teardownFeature) {
                #expect(call.teardownRequest?.discard == nil)
            }
        }

        @Test func interruptedSetupResumesWithReuse() async throws {
            let harness = SurfaceHarness(.interruptedSetup)
            let store = try await harness.start()
            let oauth = try #require(store.feature(named: "oauth"))
            let buttons = items(oauth, model: harness.model)
            #expect(buttons.map(\.title) == ["Resume Setup", "Tear Down…"])
            #expect(buttons.map(\.role) == [.primary, .secondary])

            try await harness.performAndWait(buttons[0].effect)
            let start = try #require(await harness.backend.calls(to: .startFeature).first?.startRequest)
            var expected = StartFeatureRequest(project: project, name: "oauth", runtime: oauth.runtime.provider)
            expected.branchPrefix = "feature"
            expected.mode = .full
            expected.reuse = .existingWorktree(.fail)
            #expect(start == expected)

            try await harness.performAndWait(buttons[1].effect)
            #expect(harness.model.pendingIntent == .teardown(FeatureRef(project: project, name: "oauth"), preselect: .keep))
            #expect(await harness.backend.calls(to: .teardownFeature).isEmpty)
            await harness.tearDown()
        }

        @Test func missingFolderCleansUpWithForcedRemovalAndKeepsTheBranch() async throws {
            let harness = SurfaceHarness()
            let store = try await harness.start()
            let prine = try #require(store.feature(named: "prine"))
            let buttons = items(prine, model: harness.model, folderExists: false)
            #expect(buttons.map(\.title) == ["Clean Up"])
            #expect(buttons.map(\.role) == [.primary])
            #expect(buttons.map(\.role) == [.primary])

            try await harness.performAndWait(buttons[0].effect)
            let request = try #require(await harness.backend.calls(to: .teardownFeature).first?.teardownRequest)
            #expect(request.forceRemoval)
            #expect(request.branch == .keep)
            #expect(request.discard == nil)
            #expect(request.recordedBranch == "feature/prine")
            await harness.tearDown()
        }

        @Test func degradedSandboxRetriesTheRetainedRuntime() async throws {
            let harness = SurfaceHarness()
            let store = try await harness.start()
            let sbx = try #require(store.feature(named: "sbx-demo"))
            let buttons = items(sbx, model: harness.model)
            #expect(buttons.map(\.title) == ["Retry Setup", "Tear Down…", "Update All Workspaces…"])

            try await harness.performAndWait(buttons[0].effect)
            let start = try #require(await harness.backend.calls(to: .startFeature).first?.startRequest)
            #expect(start.runtime == .sbx)
            #expect(start.reuse == .retainedRuntime)
            #expect(start.name == "sbx-demo")

            try await harness.performAndWait(buttons[2].effect)
            #expect(harness.model.pendingIntent == .syncDevcontainers(project))
            await harness.tearDown()
        }

        @Test func degradedContainerStartsTheDevContainer() async throws {
            let harness = SurfaceHarness()
            _ = try await harness.start()
            let degraded = record("prine", runtime: .container, status: .degraded)
            let buttons = items(degraded, model: harness.model)
            #expect(buttons.first?.title == "Start Environment")

            try await harness.performAndWait(try #require(buttons.first).effect)
            let calls = await harness.backend.calls(to: .devcontainer)
            #expect(calls == [.devcontainer(.up(removeExisting: false, buildNoCache: false), FeatureRef(project: project, name: "prine"))])
            await harness.tearDown()
        }

        @Test func failedModulesRerunSetupPreservingTheDevContainerAndShowTheLog() async throws {
            let harness = SurfaceHarness()
            _ = try await harness.start()
            let failed = record("prine", runtime: .container,
                                modules: [ModuleOutcome(module: "compose", status: .failed, notes: ["port 5432 in use"])])
            let buttons = items(failed, model: harness.model)
            #expect(buttons.map(\.title) == ["Re-run Setup…", "Show Log"])

            try await harness.performAndWait(buttons[0].effect)
            let start = try #require(await harness.backend.calls(to: .startFeature).first?.startRequest)
            #expect(start.reuse == .existingWorktree(.preserve))

            let latest = harness.model.operations.records(for: .feature(FeatureRef(project: project, name: "prine"))).first
            try await harness.performAndWait(buttons[1].effect)
            #expect(harness.model.pendingIntent == .showActivity(operation: latest?.id))
            await harness.tearDown()
        }

        @Test func failedRetainedOnASandboxOffersRetryInspectAndDiscard() async throws {
            let harness = SurfaceHarness()
            _ = try await harness.start()
            let retained = record("box", runtime: .sbx, status: .failedRetained, runtimeID: "branchbox-box")
            let buttons = items(retained, model: harness.model)
            #expect(buttons.map(\.title) == ["Retry", "Copy Inspect Command", "Discard…"])
            #expect(buttons[1].effect == .copy("sbx exec branchbox-box bash"))
            #expect(buttons[2].effect == .post(.teardown(FeatureRef(project: project, name: "box"), preselect: .keep)))
            #expect(buttons[2].isTeardown)
            await harness.tearDown()
        }

        @Test func unknownStatusRunsTheDoctorAndRemovedDeletesTheBranchAfterConfirming() async throws {
            let harness = SurfaceHarness()
            _ = try await harness.start()
            let unknown = record("odd", runtime: .container, status: .unknown("paused_by_admin"))
            let doctor = items(unknown, model: harness.model)
            #expect(doctor.map(\.title) == ["Run Doctor"])
            try await harness.performAndWait(doctor[0].effect)
            #expect(harness.model.pendingIntent == .showDiagnostics)

            let removed = try sample("coding-agents")
            #expect(items(removed, model: harness.model, branchExists: false).isEmpty)
            let delete = try #require(items(removed, model: harness.model, branchExists: true).first)
            guard case .confirmThenDispatch(let request, _, _, _) = delete.effect else {
                Issue.record("Delete Branch must confirm first")
                return
            }
            #expect(request == .deleteBranch("feature/coding-agents", project, force: false))
            try await harness.performAndWait(.dispatch(request))
            #expect(await harness.backend.calls(to: .deleteBranch) == [.deleteBranch("feature/coding-agents", project, force: false)])
            await noDiscardConsent(harness.backend)
            await harness.tearDown()
        }

        @Test func callAndCopyEffectsNeedNoBackend() {
            let copy = RemediationItem(action: .copyCommand("x", label: "Copy"), title: "Copy", effect: .copy("x"),
                                       role: .secondary, isTeardown: false)
            #expect(!copy.needsBackend)
            #expect(HealthCallout.verb("Update All Workspaces…") == "updateAllWorkspaces")
            #expect(HealthCallout.verb("Re-run Setup") == "reRunSetup")
        }
    }
}

// MARK: Links

extension FeatureSurfaceTests {
    @Suite struct Links {
        @Test func primaryURLIsHTTPSAndTheInContainerURLIsNotALink() throws {
            let prine = try sample("prine")
            let rows = FeatureLinks.rows(for: prine, service: nil)
            let primary = try #require(rows.first)
            #expect(primary.kind == .primary)
            #expect(primary.url?.absoluteString == "https://dev-prine.localhost")
            #expect(primary.isLink)
            let inside = try #require(rows.first { $0.kind == .inContainer })
            #expect(inside.value == "http://dev:3000")
            #expect(inside.url == nil)
            #expect(!inside.isLink)
            #expect(FeatureLinks.menuLinks(for: prine).map(\.url.absoluteString) == ["https://dev-prine.localhost",
                                                                                     "http://dev-prine.localhost"])
        }

        @Test func portsBecomeLocalhostLinksAndTheContainerServiceIsText() throws {
            let sbx = try sample("sbx-demo")
            let service = DevcontainerServiceInfo(serviceName: "app", port: 3000, serviceURL: "http://app:3000", containerUser: "vscode")
            let rows = FeatureLinks.rows(for: sbx, service: service)
            let port = try #require(rows.first { $0.kind == .port })
            #expect(port.url?.absoluteString == "http://localhost:49152")
            #expect(port.subtitle == "→ container :3000")
            let tunnel = try #require(rows.first { $0.kind == .tunnel })
            #expect(tunnel.url?.absoluteString == "https://sbx-demo.example.dev")
            let container = try #require(rows.first { $0.kind == .containerService })
            #expect(container.url == nil)
            #expect(container.subtitle == FeatureLinks.containerServiceNote)
            // The adapter reports the same in-container address; it is listed once.
            #expect(!rows.contains { $0.kind == .inContainer })
            #expect(rows.filter(\.isLink).allSatisfy { $0.url?.host != "app" })
        }
    }
}

// MARK: Run Command

extension FeatureSurfaceTests {
    @Suite struct RunCommand {
        @Test func aNonZeroExitIsAResultNotAnError() {
            let phase = RunCommandPhase.phase(state: .succeededWithWarnings,
                                              result: .exec(ExecResult(exitCode: 3, stdout: "checking\n", stderr: "boom\n")),
                                              runningSince: Date(timeIntervalSince1970: 100),
                                              finishedAt: Date(timeIntervalSince1970: 102.5))
            guard case .finished(let output) = phase else {
                Issue.record("expected a finished command, got \(phase)")
                return
            }
            #expect(!phase.isError)
            #expect(output.exitCode == 3)
            #expect(output.exitLabel == "Exit 3")
            #expect(!output.succeeded)
            #expect(output.tint == .red)
            #expect(output.result.stdout == "checking\n")
            #expect(output.durationLabel == "2.5 s")
            #expect(!output.isTruncated)
        }

        @Test func onlyAFailureToRunIsAnError() {
            let failed = RunCommandPhase.phase(state: .failed(.commandFailed(Diagnostics(summary: "Docker is not available"))),
                                               result: nil, runningSince: nil, finishedAt: nil)
            #expect(failed.isError)
            let ok = RunCommandPhase.phase(state: .succeeded, result: .exec(ExecResult(exitCode: 0)), runningSince: nil, finishedAt: nil)
            guard case .finished(let output) = ok else {
                Issue.record("expected a finished command")
                return
            }
            #expect(output.tint == .green)
            #expect(RunCommandPhase.phase(state: .running, result: nil, runningSince: nil, finishedAt: nil).isRunning)
        }

        @Test func theShellToggleChoosesTheArgv() {
            var draft = RunCommandDraft(text: "  sh -c 'exit 3'  ")
            #expect(draft.argv == ["/bin/sh", "-lc", "sh -c 'exit 3'"])
            draft.runThroughShell = false
            #expect(draft.argv == ["sh", "-c", "exit 3"])
            let feature = FeatureRef(project: project, name: "prine")
            draft.target = .devcontainer
            #expect(draft.makeRequest(for: feature) == ExecRequest(feature: feature, command: ["sh", "-c", "exit 3"], target: .devcontainer))
            #expect(RunCommandDraft.words(#"echo "a b" c\ d 'e"f'"#) == ["echo", "a b", "c d", "e\"f"])
            #expect(RunCommandDraft.words("echo 'open") == nil)
            #expect(RunCommandDraft(text: "   ").argv == nil)
        }

        @Test func historyIsNewestFirstWithoutDuplicates() {
            var history: [String] = []
            for command in ["make test", "ls", "make test", "  "] { history = RunCommandHistory.adding(command, to: history) }
            #expect(history == ["make test", "ls"])
            let long = (0..<30).reduce(into: [String]()) { list, index in list = RunCommandHistory.adding("cmd \(index)", to: list) }
            #expect(long.count == RunCommandHistory.limit)
            #expect(long.first == "cmd 29")
        }
    }
}

// MARK: Sharing

extension FeatureSurfaceTests {
    @MainActor @Suite(.serialized) struct Sharing {
        @Test func cardStates() {
            #expect(TunnelCardState.state(tunnel: nil, tunnelsEnabled: false) == .offInConfig)
            #expect(TunnelCardState.state(tunnel: TunnelState(status: .disabled), tunnelsEnabled: true) == .notShared)
            #expect(TunnelCardState.state(tunnel: TunnelState(status: .manual, instructions: ["a"]), tunnelsEnabled: true) == .manual)
            #expect(TunnelCardState.state(tunnel: TunnelState(status: .active), tunnelsEnabled: false) == .active)
        }

        @Test func forcedRemovalIsOfferedOnlyAfterTheProviderFails() async throws {
            let harness = SurfaceHarness()
            _ = try await harness.start()
            let feature = FeatureRef(project: project, name: "sbx-demo")
            let records = { harness.model.operations.records(for: .feature(feature)) }
            #expect(OperationFailure.latest(of: TunnelCard.tunnelKinds, in: records()) == nil)

            // A removal that works offers nothing.
            try await harness.performAndWait(.dispatch(.tunnelRemove(feature, force: false)))
            #expect(OperationFailure.latest(of: TunnelCard.tunnelKinds, in: records()) == nil)

            // The provider fails: the card's failure offers Remove Anyway (destructive, confirmed).
            await harness.backend.script(.removeTunnel, .fail(.commandFailed(Diagnostics(summary: "cloudflared: tunnel not found"))))
            try await harness.performAndWait(.dispatch(.tunnelRemove(feature, force: false)))
            let failure = try #require(OperationFailure.latest(of: TunnelCard.tunnelKinds, in: records()))
            let force = try #require(failure.recoveries.first { if case .retry = $0 { true } else { false } })
            guard case .retry(let request, _, let destructive, let confirmation) = force else { return }
            #expect(request == .tunnelRemove(feature, force: true))
            #expect(destructive)
            #expect(confirmation?.isEmpty == false)
            // Every removal so far went out without --force.
            #expect(await harness.backend.calls(to: .removeTunnel) == [.removeTunnel(feature, force: false),
                                                                        .removeTunnel(feature, force: false)])
            await harness.tearDown()
        }
    }
}
