@testable import BranchBoxApp
import BranchBoxCLI
import BranchBoxKit
import BranchBoxPreview
import BranchBoxStores
import BranchBoxTestSupport
import Foundation
import Testing

// SW-7: projects, onboarding, settings and diagnostics. The flows run against PreviewBackend (and one scripted
// legacy CLIBackend) through a real AppModel stored in a throwaway folder.

/// An AppModel whose preferences and files live under $TMPDIR/branchbox-tests/, never in the user's Library.
@MainActor private struct ProjectsHarness {
    static let base: URL = {
        let base = FileManager.default.temporaryDirectory.appendingPathComponent("branchbox-tests", isDirectory: true)
        try? FileManager.default.createDirectory(at: base, withIntermediateDirectories: true)
        return base
    }()

    let directory: URL
    let defaultsName: String
    let model: AppModel
    let backend: PreviewBackend?

    init(_ scenario: PreviewScenario = .contract) {
        let backend = PreviewBackend(scenario: scenario)
        self.init(bootstrapper: PreviewBootstrapper(backend: backend), backend: backend)
    }

    init(bootstrapper: any BackendBootstrapping, backend: PreviewBackend? = nil) {
        directory = Self.base.appendingPathComponent("projects-\(UUID().uuidString)", isDirectory: true)
        defaultsName = Self.base.appendingPathComponent("settings-\(UUID().uuidString)").path
        let defaults = UserDefaults(suiteName: defaultsName)!
        model = AppModel(settings: AppSettings(defaults: defaults), bootstrapper: bootstrapper, notifier: NoopNotifier(),
                         configuration: .isolated(in: directory))
        self.backend = backend
    }

    /// Starts the model and adds the sample project.
    func startWithSampleProject() async throws -> ProjectStore {
        await model.start()
        _ = await model.projects.add(folder: PreviewSamples.project.root)
        return try #require(model.projects.project(PreviewSamples.project))
    }

    func tearDown() async {
        await model.prepareForTermination()
        UserDefaults(suiteName: defaultsName)?.removePersistentDomain(forName: defaultsName)
        try? FileManager.default.removeItem(atPath: defaultsName + ".plist")
        try? FileManager.default.removeItem(at: directory)
    }
}

/// Polls `condition` every few milliseconds on the main actor, failing after `timeout`.
@MainActor private func eventually(_ message: String, timeout: Duration = .seconds(5),
                                   sourceLocation: SourceLocation = #_sourceLocation,
                                   _ condition: @MainActor () -> Bool) async throws {
    let deadline = ContinuousClock.now + timeout
    while !condition() {
        try #require(ContinuousClock.now < deadline, "\(message) within \(timeout)", sourceLocation: sourceLocation)
        try await Task.sleep(for: .milliseconds(2))
    }
}

/// Bootstraps a fixed backend, e.g. a `CLIBackend` on a scripted runner.
private struct FixedBootstrapper: BackendBootstrapping {
    let backend: any BranchBoxBackend
    let identity: BackendIdentity

    func bootstrap(_ settings: BackendSettings) async -> BackendBootstrap { .ready(backend, identity) }
    func environmentSummary() async -> EnvironmentSummary? { nil }
    func recaptureEnvironment() async {}
    func terminateAllProcesses() async {}
}

private let scriptedProject = ProjectRef(root: URL(fileURLWithPath: "/r/main"))

private func legacyCLIIdentity() -> BackendIdentity {
    BackendIdentity(kind: .cli(CLIResolution(path: "/opt/homebrew/bin/branchbox", source: .wellKnownPath)),
                    version: SemVer(0, 13, 4), contractVersion: nil, capabilities: [])
}

private func scriptedBackend(_ runner: ScriptedProcessRunner, identity: BackendIdentity) -> CLIBackend {
    CLIBackend(executable: URL(fileURLWithPath: "/opt/homebrew/bin/branchbox"), identity: identity, runner: runner,
               environment: StaticEnvironment(), gitExecutable: URL(fileURLWithPath: "/usr/bin/git"))
}

private func contractDocument(for project: ProjectRef) -> ProjectConfigDocument {
    ProjectConfigDocument(path: "\(project.path)/.branchbox/config.json", exists: true, effective: .defaults,
                          keys: PreviewSamples.configKeys + [
                            ConfigKeyDescriptor(key: "tunnel.providers.cloudflared.account_id", type: "string",
                                                description: "Cloudflare account"),
                          ],
                          editable: true)
}

/// Every SW-7 suite, so `--filter ProjectsSettingsTests` runs them all.
@MainActor @Suite struct ProjectsSettingsTests {}

extension ProjectsSettingsTests {
    @MainActor @Suite struct AddProject {
        @Test func pickingAFeatureWorktreeAddsMainWithANote() async throws {
            let harness = ProjectsHarness()
            await harness.model.start()
            let worktree = try #require(PreviewSamples.features.first { $0.status == .active }?.worktreePath)

            let sheet = AddProjectModel()
            let outcome = await sheet.add(URL(fileURLWithPath: worktree, isDirectory: true), to: harness.model.projects)

            guard case .added(let ref, let note?) = outcome else {
                Issue.record("expected .added with a note, got \(outcome)")
                return
            }
            #expect(ref == PreviewSamples.project)
            #expect(note.contains("feature worktree"))
            #expect(note.contains(PreviewSamples.project.path))
            #expect(harness.model.projects.project(PreviewSamples.project) != nil)
            #expect(sheet.phase == .finished(URL(fileURLWithPath: worktree, isDirectory: true), outcome))
            #expect(AddProjectModel.followUp(for: outcome) == .select(.project(path: PreviewSamples.project.path)))
            await harness.tearDown()
        }

        @Test func anUninitializedRepositoryRoutesToSetUp() async throws {
            let harness = ProjectsHarness()
            await harness.model.start()
            let folder = URL(fileURLWithPath: "/Users/dev/projects/fresh", isDirectory: true)
            let ref = ProjectRef(root: folder)
            await harness.backend?.setResolution(ProjectResolution(project: ref, requested: folder, normalization: .none,
                                                                   initialized: false), for: folder)

            let outcome = await AddProjectModel().add(folder, to: harness.model.projects)

            #expect(outcome == .needsInit(ref))
            #expect(AddProjectModel.followUp(for: outcome) == .initProject(ref.root, mode: .setUp))
            #expect(harness.model.projects.project(ref) == nil)     // nothing is added before it is set up
            await harness.tearDown()
        }

        @Test func aRefusalNamesItsCause() {
            let folder = URL(fileURLWithPath: "/tmp/notes")
            #expect(AddProjectSheet.refusalTitle(.projectInvalid(.notGitRepository(folder.path)), folder: folder)
                    == "notes isn't a Git repository")
            #expect(AddProjectModel.followUp(for: .refused(.projectInvalid(.notGitRepository(folder.path)))) == nil)
            #expect(AddProjectSheet.name(of: PreviewSamples.project) == "branchbox")
        }
    }
}

extension ProjectsSettingsTests {
    @MainActor @Suite struct InitSheet {
        private let folder = URL(fileURLWithPath: "/Users/dev/projects/acme")

        @Test func defaultsKeepTheRepositoryInPlaceWithTunnelsOff() {
            let draft = InitDraft(folder: folder, mode: .setUp)
            #expect(draft.layout == .keepInPlace)
            #expect(!draft.tunnelsEnabled)

            let request = draft.makeRequest(dryRun: false, capabilities: PreviewSamples.allCapabilities)
            #expect(request.reorganize == false)
            #expect(request.tunnelsEnabled == false)
            #expect(request.mode == .initialize)
            #expect(request.stack == nil)
            #expect(request.onePassword == .unchanged)
            #expect(!request.skipDevcontainer && !request.skipEnv && request.codingAgents)
        }

        @Test func onlyTheConfirmedMoveReorganizes() {
            var draft = InitDraft(folder: folder, mode: .setUp)
            #expect(draft.makeRequest(dryRun: true, capabilities: []).reorganize == false)
            draft.confirmMoveIntoParent()
            #expect(draft.makeRequest(dryRun: true, capabilities: []).reorganize == true)
            #expect(draft.movedRepositoryPath == "/Users/dev/projects/acme/main")
            draft.keepInPlace()
            #expect(draft.makeRequest(dryRun: false, capabilities: []).reorganize == false)

            // Repair and Check Setup never move anything, whatever the layout says.
            var repair = InitDraft(folder: folder, mode: .repair)
            repair.confirmMoveIntoParent()
            #expect(repair.makeRequest(dryRun: false, capabilities: []).mode == .update)
            #expect(repair.makeRequest(dryRun: false, capabilities: []).reorganize == false)
            #expect(repair.makeRequest(dryRun: false, capabilities: [], mode: .validate).reorganize == false)
        }

        @Test func legacyCLIsGetNeitherTunnelsNorOnePassword() {
            var draft = InitDraft(folder: folder, mode: .setUp)
            draft.tunnelsEnabled = true
            draft.usesOnePassword = true
            draft.gitHubRef = "op://Private/GitHub/token"
            let legacy = draft.makeRequest(dryRun: false, capabilities: [])
            #expect(legacy.tunnelsEnabled == nil)
            #expect(legacy.onePassword == .unchanged)

            let contract = draft.makeRequest(dryRun: false, capabilities: PreviewSamples.allCapabilities)
            #expect(contract.tunnelsEnabled == true)
            #expect(contract.onePassword == .configure(githubRef: "op://Private/GitHub/token", signingKeyRef: nil, verify: true))

            draft.gitHubRef = "ghp_plain_token"
            #expect(draft.problems == ["The GitHub token reference must start with op://"])
        }

        @Test func initializeDispatchesTheDefaultsAndPreviewIsADryRun() async throws {
            let harness = ProjectsHarness()
            await harness.model.start()
            let sheet = InitSheetModel(folder: folder, mode: .setUp)

            sheet.startPreview(using: harness.model)
            let preview = try #require(sheet.preview)
            try await eventually("the preview finished") { !preview.isCancellable }
            #expect(sheet.showsPreview)

            sheet.initialize(using: harness.model)
            let run = try #require(sheet.run)
            try await eventually("init finished") { !run.isCancellable }

            let requests = await harness.backend?.calls(to: .initProject).compactMap { call -> InitRequest? in
                if case .initProject(let request) = call { return request }
                return nil
            } ?? []
            #expect(requests.map(\.dryRun) == [true, false])
            #expect(requests.allSatisfy { !$0.reorganize && $0.tunnelsEnabled == false })
            guard case .initProject(let report)? = run.result else {
                Issue.record("expected an init report")
                return
            }
            #expect(report.workspacePath == folder.path)
            await harness.tearDown()
        }
    }
}

extension ProjectsSettingsTests {
    @MainActor @Suite struct ProjectSettings {
        @Test func editingOneFieldBuildsExactlyThatPatch() {
            var form = ConfigForm(document: contractDocument(for: PreviewSamples.project))
            #expect(form.editable)
            #expect(!form.hasChanges)

            form.set("feature.branch_prefix", .string("spike"))

            #expect(form.patch == ConfigPatch(changes: [ConfigChange(key: "feature.branch_prefix", value: .string("spike"))]))
            // Setting it back leaves nothing to save.
            form.set("feature.branch_prefix", .string("feature"))
            #expect(form.patch.changes.isEmpty)
        }

        @Test func blankTextUnsetsAndInvalidPrefixesAreFlagged() {
            var form = ConfigForm(document: contractDocument(for: PreviewSamples.project))
            form.set("feature.branch_prefix", .string("my prefix"))
            #expect(form.problem(for: "feature.branch_prefix")?.contains("spaces") == true)
            form.set("feature.branch_prefix", .string("   "))
            #expect(form.patch == ConfigPatch(changes: [ConfigChange(key: "feature.branch_prefix", value: nil)]))
            #expect(ConfigForm.Tab.of("feature.teardown.delete_branch_by_default") == .teardown)
            #expect(ConfigForm.Tab.of("tunnel.providers.cloudflared.dns_zone") == .sharing)
            #expect(ConfigForm.unavailableReason("local-vm", key: "runtime.provider") != nil)
        }

        @Test func legacyIdentityIsReadOnlyAndNeverAppliesConfig() async throws {
            let harness = ProjectsHarness(.legacy0134)
            _ = try await harness.startWithSampleProject()
            let sheet = ProjectSettingsModel(project: PreviewSamples.project)

            await sheet.load(using: harness.model)
            let form = try #require(sheet.form)
            #expect(!form.editable)
            #expect(!form.fields.isEmpty)                            // built-in descriptors, read-only
            sheet.form?.set("feature.branch_prefix", .string("spike"))
            #expect(sheet.form?.hasChanges == false)
            #expect(!sheet.canSave)

            await sheet.review(using: harness.model)
            sheet.apply(using: harness.model)

            #expect(await harness.backend?.calls(to: .applyConfig).isEmpty == true)
            #expect(await harness.backend?.calls(to: .setTunnelCredentials).isEmpty == true)
            #expect(harness.model.operations.records.isEmpty)
            await harness.tearDown()
        }

        @Test func savingDryRunsThenApplies() async throws {
            let harness = ProjectsHarness()
            _ = try await harness.startWithSampleProject()
            let sheet = ProjectSettingsModel(project: PreviewSamples.project)
            await sheet.load(using: harness.model)
            sheet.form?.set("feature.branch_prefix", .string("spike"))

            await sheet.review(using: harness.model)
            guard case .reviewing(let changes) = sheet.phase else {
                Issue.record("expected the review, got \(sheet.phase)")
                return
            }
            #expect(changes.map(\.key) == ["feature.branch_prefix"])

            sheet.apply(using: harness.model)
            try await eventually("the save finished") { sheet.saveState == .succeeded }
            let calls = await harness.backend?.calls(to: .applyConfig) ?? []
            let patch = ConfigPatch(changes: [ConfigChange(key: "feature.branch_prefix", value: .string("spike"))])
            #expect(calls == [.applyConfig(patch, PreviewSamples.project, dryRun: true),
                              .applyConfig(patch, PreviewSamples.project, dryRun: false)])
            #expect(await sheet.saveFinished(using: harness.model))
            await harness.tearDown()
        }

        @Test func theTokenNeverAppearsInTitlesLogsOrCommandLines() async throws {
            let token = "cf-token-9f8e7d6c5b4a3210"
            let harness = ProjectsHarness()
            await harness.model.start()
            let sheet = ProjectSettingsModel(project: PreviewSamples.project, document: contractDocument(for: PreviewSamples.project))
            sheet.form?.set("tunnel.providers.cloudflared.account_id", .string("acct-123"))
            sheet.apiToken = token

            sheet.apply(using: harness.model)

            // The token goes out only after the account ID was saved, then leaves the sheet.
            try await eventually("the credentials were sent") { harness.model.operations.records.count == 2 }
            #expect(sheet.apiToken.isEmpty)                          // gone from the sheet once handed over
            let records = harness.model.operations.records
            #expect(records.map(\.kind) == [.tunnelCredentials, .applyConfig])     // newest first
            for record in records {
                try await eventually("\(record.title) finished") { !record.isCancellable }
                #expect(!record.title.contains(token))
                #expect(!record.log.lines.contains { $0.message.contains(token) })
                #expect(!record.warnings.contains { $0.contains(token) })
            }
            let credentials = try #require(records.first { $0.kind == .tunnelCredentials })
            guard case .tunnelCredentials(let request, _) = credentials.context else {
                Issue.record("expected a credentials request")
                return
            }
            #expect(request.apiToken?.value == token)                // the CLI gets it on stdin
            #expect(!String(describing: request).contains(token))

            let cli = scriptedBackend(ScriptedProcessRunner(),
                                      identity: BackendIdentity(kind: .cli(CLIResolution(path: "/opt/homebrew/bin/branchbox",
                                                                                         source: .wellKnownPath)),
                                                                version: SemVer(0, 14, 0), contractVersion: 1,
                                                                capabilities: PreviewSamples.allCapabilities))
            let commandLine = try #require(cli.previewCommandLine(credentials.context))
            #expect(commandLine.contains("tunnel credentials set"))
            #expect(!commandLine.contains(token))
            #expect(!harness.model.actions.title(for: credentials.context).contains(token))
            await harness.tearDown()
        }

        @Test func aConfigInvalidRefusalLandsOnItsField() async throws {
            let harness = ProjectsHarness()
            _ = try await harness.startWithSampleProject()
            let refusal = BackendError.refused(Refusal(cause: .configInvalid(key: "feature.branch_prefix", detail: "not a valid ref"),
                                                       message: "Invalid value for feature.branch_prefix",
                                                       diagnostics: Diagnostics(summary: "not a valid ref")))
            await harness.backend?.script(.applyConfig, .fail(refusal))
            let sheet = ProjectSettingsModel(project: PreviewSamples.project)
            await sheet.load(using: harness.model)
            sheet.tab = .codingAgent
            sheet.form?.set("feature.branch_prefix", .string("spike"))

            await sheet.review(using: harness.model)

            #expect(sheet.phase == .editing)
            #expect(sheet.keyErrors["feature.branch_prefix"] == "not a valid ref")
            #expect(sheet.tab == .features)
            await harness.tearDown()
        }
    }
}

extension ProjectsSettingsTests {
    @MainActor @Suite struct SyncSheet {
        @Test func failureRowsShowForALegacyFailureThatExitsZero() async throws {
            let text = """
                🔄 Syncing devcontainer configuration to 2 feature worktree(s)

                  alpha ... ✓ synced 3 files (copy)
                  beta ... ✗ failed: IO error: Permission denied (os error 13)

                ✓ Successfully synced 1 feature worktree(s)

                ⚠️  1 error(s) occurred:
                  - beta: IO error: Permission denied (os error 13)
                """
            let runner = ScriptedProcessRunner([
                ScriptedProcessRunner.Rule(["branchbox", "devcontainer", "sync"],
                                           lines: text.split(separator: "\n").map { OutputLine(channel: .stdout, text: String($0)) },
                                           outcome: .exit(0, stdout: text)),
            ])
            let identity = legacyCLIIdentity()
            let harness = ProjectsHarness(bootstrapper: FixedBootstrapper(backend: scriptedBackend(runner, identity: identity),
                                                                          identity: identity))
            await harness.model.start()
            let sheet = SyncSheetModel()

            sheet.apply(project: scriptedProject, using: harness.model)
            let record = try #require(sheet.run)
            try await eventually("the sync finished") { !record.isCancellable }

            #expect(record.state == .partial)
            let rows = SyncSheetModel.rows(for: record)
            #expect(rows.map(\.feature) == ["alpha", "beta"])
            let beta = try #require(rows.first { $0.feature == "beta" })
            #expect(beta.isFailure)
            #expect(beta.status == "Failed")
            #expect(beta.detail?.contains("Permission denied") == true)
            // The folder found later in the registry also goes into the message (and enables Reveal Folder).
            var located = beta
            located.worktreePath = "/Volumes/work/repo/beta"
            #expect(located.detail == "BranchBox couldn't write to /Volumes/work/repo/beta/.devcontainer (Permission denied).")
            #expect(rows.first { $0.feature == "alpha" }?.isFailure == false)
            guard case .sync(let report)? = record.result else {
                Issue.record("expected a sync report")
                return
            }
            #expect(SyncSheetModel.summary(of: report) == "1 updated · 1 failed")
            await harness.tearDown()
        }

        @Test func previewRowsSayWhatWillHappen() {
            let report = SyncReport(dryRun: true, strategy: "copy", rows: [
                SyncReport.Row(feature: "prine", status: .wouldSync, files: ["devcontainer.json"]),
                SyncReport.Row(feature: "old", status: .skipped, skipReason: "not active"),
            ])
            #expect(SyncSheetModel.summary(of: report) == "1 workspace will be updated · 1 skipped")
            #expect(SyncSheetModel.rows(from: report).map(\.status) == ["Will update", "Skipped"])
        }
    }
}

extension ProjectsSettingsTests {
    @MainActor @Suite struct DiagnosticsAndOnboarding {
        @Test func doctorFixesNameTheRightRemedy() {
            let daemon = DoctorCheck(id: "docker.daemon", title: "Docker daemon", required: true, status: .error,
                                     detail: "Cannot connect to the Docker daemon", remediation: "Start Docker Desktop")
            #expect(DoctorFix.fix(for: daemon) == .openDockerDesktop)
            let sbx = DoctorCheck(id: "runtime.sbx", title: "Docker Sandboxes", required: false, status: .warn,
                                  remediation: "Sign in with: sbx login")
            #expect(DoctorFix.fix(for: sbx) == .signInToSandboxes)
            let git = DoctorCheck(id: "git", title: "Git", required: true, status: .error)
            #expect(DoctorFix.fix(for: git) == .copyCommand("xcode-select --install", label: "Copy Install Command"))
            let devcontainer = DoctorCheck(id: "devcontainer.cli", title: "Dev Container CLI", required: false, status: .warn,
                                           remediation: "npm install -g @devcontainers/cli")
            #expect(DoctorFix.fix(for: devcontainer) == .copyCommand("npm install -g @devcontainers/cli", label: "Copy Command"))
            #expect(DoctorFix.fix(for: DoctorCheck(id: "gh", title: "GitHub CLI", required: false, status: .ok)) == nil)
            #expect(DoctorReport(source: .app, checks: [daemon, sbx]).blockingChecks == [daemon])
        }

        @Test func theReportIsRedactedAndCoversProjects() async throws {
            let harness = ProjectsHarness()
            _ = try await harness.startWithSampleProject()
            harness.model.settings.extraEnvironment = ["OPENAI_KEY_FOR_TESTS": "sk-live-abcdefghijklmnop"]
            await harness.model.environment.runDoctor(for: nil)

            let report = DiagnosticsReportBuilder(model: harness.model).markdown()

            #expect(report.contains("## Tools"))
            #expect(report.contains("Docker daemon"))
            #expect(report.contains("## Projects"))
            #expect(report.contains("branchbox"))
            #expect(!report.contains("sk-live-abcdefghijklmnop"))
            await harness.tearDown()
        }

        @Test func extraEnvironmentKeepsOnlyValidNames() {
            let rows = [EnvironmentVariableRow(name: "DEBUG", value: "1"), EnvironmentVariableRow(name: "1BAD", value: "x"),
                        EnvironmentVariableRow(name: "", value: "y"), EnvironmentVariableRow(name: " RUST_LOG ", value: "info")]
            #expect(EnvironmentVariableRow.environment(from: rows) == ["DEBUG": "1", "RUST_LOG": "info"])
        }

        @Test func updatedTextCountsUp() {
            let now = Date(timeIntervalSince1970: 1_000_000)
            #expect(ProjectHeader.updatedText(since: now, now: now) == "Updated just now")
            #expect(ProjectHeader.updatedText(since: now.addingTimeInterval(-12), now: now) == "Updated 12 s ago")
            #expect(ProjectHeader.updatedText(since: now.addingTimeInterval(-180), now: now) == "Updated 3 min ago")
        }
    }
}
