import BranchBoxKit
import Foundation
import Testing

@Suite struct RemediationTests {
    private static let feature = FeatureRef(project: Sample.project, name: "eta")
    private static let tearDown = RemediationAction.teardown(feature, preselect: .keep)
    private static let failedCompose = ModuleOutcome(module: "compose", status: .failed, notes: ["port 5432 in use"])

    private static func start(_ provider: RuntimeProvider, _ reuse: StartFeatureRequest.Reuse, mode: StartFeatureRequest.Mode = .full,
                              prompt: String? = nil) -> StartFeatureRequest {
        var request = StartFeatureRequest(project: Sample.project, name: "eta", runtime: provider)
        request.branchPrefix = "feature"
        request.reuse = reuse
        request.mode = mode
        request.prompt = prompt
        return request
    }

    private static var cleanUp: RemediationAction {
        var request = TeardownRequest(feature: feature, recordedBranch: "feature/eta", branch: .keep)
        request.forceRemoval = true
        return .cleanUpMissingFolder(request)
    }

    struct Case: Sendable, CustomTestStringConvertible {
        let name: String
        let record: FeatureRecord
        var folderExists = true
        let attention: AttentionReason?
        let callout: String?
        let actions: [RemediationAction]
        var testDescription: String { name }
    }

    /// DESIGN §9.1, row by row, across statuses × runtimes × setup states.
    static let cases: [Case] = [
        Case(name: "healthy container", record: Sample.record(), attention: nil, callout: nil, actions: []),
        Case(name: "healthy sbx", record: Sample.record(provider: .sbx, runtimeID: "bb-eta"), attention: nil, callout: nil,
             actions: []),
        Case(name: "setup interrupted", record: Sample.record(provider: .sbx, setup: .interrupted, startMode: "minimal",
                                                              prompt: "Build it"),
             attention: .interrupted, callout: "Setup of eta was interrupted.",
             actions: [.resumeSetup(start(.sbx, .existingWorktree(.fail), mode: .minimal, prompt: "Build it")), tearDown]),
        Case(name: "setup in progress", record: Sample.record(setup: .inProgress), attention: nil, callout: "Setting up…",
             actions: []),
        Case(name: "unknown setup state", record: Sample.record(setup: .unknown("paused")), attention: nil, callout: nil,
             actions: []),
        Case(name: "degraded sbx", record: Sample.record(status: .degraded, provider: .sbx, runtimeID: "bb-eta"),
             attention: .degraded, callout: "The environment for eta isn't running.",
             actions: [.retryRetainedRuntime(start(.sbx, .retainedRuntime)), tearDown]),
        Case(name: "degraded local-vm", record: Sample.record(status: .degraded, provider: .localVM),
             attention: .degraded, callout: "The environment for eta isn't running.", actions: [tearDown]),
        Case(name: "degraded in-guest", record: Sample.record(status: .degraded, provider: .inGuest),
             attention: .degraded, callout: "The environment for eta isn't running.", actions: [tearDown]),
        Case(name: "degraded container (unreachable in core)", record: Sample.record(status: .degraded),
             attention: .degraded, callout: "The environment for eta isn't running.",
             actions: [.startEnvironment(feature), tearDown]),
        Case(name: "failed_retained sbx", record: Sample.record(status: .failedRetained, provider: .sbx, runtimeID: "bb-eta",
                                                                startMode: "minimal"),
             attention: .failedRetained, callout: "Setup failed; the sandbox was kept so you can inspect it.",
             actions: [.retryRetainedRuntime(start(.sbx, .retainedRuntime, mode: .minimal)),
                       .copyCommand("sbx exec bb-eta bash", label: "Copy Inspect Command"), tearDown]),
        Case(name: "failed_retained sbx without id", record: Sample.record(status: .failedRetained, provider: .sbx),
             attention: .failedRetained, callout: "Setup failed; the sandbox was kept so you can inspect it.",
             actions: [.retryRetainedRuntime(start(.sbx, .retainedRuntime)), tearDown]),
        Case(name: "failed_retained local-vm (no --reuse-runtime in core)",
             record: Sample.record(status: .failedRetained, provider: .localVM),
             attention: .failedRetained, callout: "Setup failed; the VM was kept so you can inspect it.",
             actions: [tearDown]),
        Case(name: "failed_retained container", record: Sample.record(status: .failedRetained),
             attention: .failedRetained, callout: "Setup failed; the runtime was kept so you can inspect it.",
             actions: [tearDown]),
        Case(name: "orphaned, folder present", record: Sample.record(status: .orphaned, provider: .sbx),
             attention: .orphaned, callout: "The runtime no longer exists; your files are untouched.",
             actions: [.recreateRuntime(start(.sbx, .existingWorktree(.fail))), tearDown]),
        Case(name: "orphaned, folder missing", record: Sample.record(status: .orphaned), folderExists: false,
             attention: .folderMissing, callout: "The folder /tmp/bbx/eta is gone.", actions: [cleanUp]),
        Case(name: "active, folder missing", record: Sample.record(), folderExists: false,
             attention: .folderMissing, callout: "The folder /tmp/bbx/eta is gone.", actions: [cleanUp]),
        Case(name: "interrupted, folder missing", record: Sample.record(setup: .interrupted), folderExists: false,
             attention: .folderMissing, callout: "The folder /tmp/bbx/eta is gone.", actions: [cleanUp]),
        Case(name: "setup in progress, folder not there yet", record: Sample.record(setup: .inProgress), folderExists: false,
             attention: nil, callout: "Setting up…", actions: []),
        Case(name: "interrupted in-guest", record: Sample.record(provider: .inGuest, setup: .interrupted),
             attention: .interrupted, callout: "Setup of eta was interrupted.", actions: [tearDown]),
        Case(name: "orphaned, unknown runtime", record: Sample.record(status: .orphaned, provider: .unknown("vmware")),
             attention: .orphaned, callout: "The runtime no longer exists; your files are untouched.", actions: [tearDown]),
        Case(name: "failed modules in-guest", record: Sample.record(provider: .inGuest, modules: [failedCompose]),
             attention: .setupIncomplete(module: "compose"),
             callout: "Setup finished with problems: compose failed (port 5432 in use).", actions: [.showLog(feature)]),
        Case(name: "active with failed modules",
             record: Sample.record(modules: [ModuleOutcome(module: "devcontainer", status: .success), failedCompose,
                                             ModuleOutcome(module: "database", status: .failed)]),
             attention: .setupIncomplete(module: "compose"),
             callout: "Setup finished with problems: compose failed (port 5432 in use); database failed.",
             actions: [.rerunSetup(start(.container, .existingWorktree(.preserve))), .showLog(feature)]),
        Case(name: "degraded with failed modules", record: Sample.record(status: .degraded, provider: .localVM,
                                                                         modules: [failedCompose]),
             attention: .degraded, callout: "The environment for eta isn't running.", actions: [tearDown]),
        Case(name: "outdated devcontainer", record: Sample.record(outdated: true), attention: nil,
             callout: "This workspace's dev container config is out of date.", actions: [.syncDevcontainers(Sample.project)]),
        Case(name: "orphaned and outdated", record: Sample.record(status: .orphaned, outdated: true), attention: .orphaned,
             callout: "The runtime no longer exists; your files are untouched.",
             actions: [.recreateRuntime(start(.container, .existingWorktree(.fail))), tearDown, .syncDevcontainers(Sample.project)]),
        Case(name: "outdated but folder missing", record: Sample.record(outdated: true), folderExists: false,
             attention: .folderMissing, callout: "The folder /tmp/bbx/eta is gone.", actions: [cleanUp]),
        Case(name: "unknown status", record: Sample.record(status: .unknown("paused_by_admin")),
             attention: .unknownStatus("paused_by_admin"),
             callout: "Status “paused_by_admin” isn't recognised by this app version.", actions: [.runDoctor]),
        Case(name: "removed", record: Sample.record(status: .removed), folderExists: false, attention: nil,
             callout: "eta was torn down.", actions: [.deleteLeftoverBranch("feature/eta", Sample.project)]),
        Case(name: "removed without a branch", record: Sample.record(status: .removed, outdated: true, branch: ""),
             folderExists: false, attention: nil, callout: "eta was torn down.", actions: []),
    ]

    @Test(arguments: RemediationTests.cases)
    func table(_ testCase: Case) {
        let record = testCase.record
        #expect(Remediation.attention(for: record, folderExists: testCase.folderExists) == testCase.attention)
        #expect(Remediation.callout(for: record, folderExists: testCase.folderExists) == testCase.callout)
        #expect(Remediation.actions(for: record, project: Sample.project, identity: Sample.identity,
                                    folderExists: testCase.folderExists) == testCase.actions)
    }

    @Test func removedBranchActionFollowsBranchExistence() {
        let removed = Sample.record(status: .removed)
        #expect(Remediation.actions(for: removed, project: Sample.project, identity: Sample.identity, folderExists: false,
                                    branchExists: false).isEmpty)
        #expect(Remediation.actions(for: removed, project: Sample.project, identity: Sample.identity, folderExists: false,
                                    branchExists: true) == [.deleteLeftoverBranch("feature/eta", Sample.project)])
    }

    /// Without a backend only the actions that need none stay.
    @Test func withoutABackendOnlyLocalActionsRemain() {
        let retained = Sample.record(status: .failedRetained, provider: .sbx, runtimeID: "bb-eta")
        #expect(Remediation.actions(for: retained, project: Sample.project, identity: nil, folderExists: true)
            == [.copyCommand("sbx exec bb-eta bash", label: "Copy Inspect Command")])
        #expect(Remediation.actions(for: Sample.record(modules: [Self.failedCompose]), project: Sample.project, identity: nil,
                                    folderExists: true) == [.showLog(Self.feature)])
        #expect(Remediation.actions(for: Sample.record(status: .unknown("x")), project: Sample.project, identity: nil,
                                    folderExists: true) == [.runDoctor])
        #expect(Remediation.actions(for: Sample.record(status: .orphaned), project: Sample.project, identity: nil,
                                    folderExists: false).isEmpty)
    }

    /// The captured and synthetic 0.13.4 records.
    @Test func fixtureRecords() throws {
        let prine = try Sample.fixtureRecord("prine", in: "main_feature_list_all.json")
        #expect(Remediation.attention(for: prine, folderExists: true) == nil)
        #expect(Remediation.actions(for: prine, project: Sample.project, identity: Sample.identity, folderExists: true).isEmpty)

        let sbx = try Sample.fixtureRecord("sbx-demo", in: "synthetic_feature_list_new_statuses.json")
        #expect(Remediation.attention(for: sbx, folderExists: true) == .degraded)
        let sbxActions = Remediation.actions(for: sbx, project: Sample.project, identity: Sample.identity, folderExists: true)
        guard case .retryRetainedRuntime(let retry)? = sbxActions.first else {
            Issue.record("expected a retained-runtime retry, got \(sbxActions)")
            return
        }
        #expect(retry.name == "sbx-demo")
        #expect(retry.runtime == .sbx)
        #expect(retry.reuse == .retainedRuntime)
        #expect(retry.prompt == "do the thing")
        #expect(retry.branchPrefix == "feature")
        #expect(sbxActions.last == .syncDevcontainers(Sample.project))   // devcontainer_outdated: true

        // A retained local-vm (synthetic: core keeps runtimes only for sbx) gets no --reuse-runtime retry.
        let retained = try Sample.fixtureRecord("retained", in: "synthetic_feature_list_new_statuses.json")
        #expect(Remediation.attention(for: retained, folderExists: true) == .failedRetained)
        #expect(retained.runtime.provider == .localVM)
        #expect(!Remediation.actions(for: retained, project: Sample.project, identity: Sample.identity, folderExists: true)
            .contains { if case .retryRetainedRuntime = $0 { true } else { false } })

        let orphan = try Sample.fixtureRecord("orphan", in: "synthetic_feature_list_new_statuses.json")
        #expect(Remediation.attention(for: orphan, folderExists: true) == .orphaned)
        #expect(Remediation.attention(for: orphan, folderExists: false) == .folderMissing)
    }
}
