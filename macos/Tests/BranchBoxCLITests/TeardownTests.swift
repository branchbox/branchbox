@testable import BranchBoxCLI
import BranchBoxKit
import BranchBoxTestSupport
import Foundation
import Testing

private func request(_ policy: BranchPolicy = .keep, discard: [String]? = nil, forceRemoval: Bool = false,
                     branch: String? = "feature/eta") -> TeardownRequest {
    var request = TeardownRequest(feature: Scripted.eta, recordedBranch: branch, branch: policy)
    request.discard = discard.map { DiscardConsent(userFiles: $0) }
    request.forceRemoval = forceRemoval
    return request
}

private let teardownCommand = ["branchbox", "feature", "teardown"]

/// A backend whose file system loses `/r/eta` once `feature teardown` has succeeded.
private func backend(_ scripted: ScriptedProcessRunner, identity: BackendIdentity = Scripted.identity(),
                     worktreeExists: Bool = true) -> CLIBackend {
    let fileSystem = MutableFileSystem(worktreeExists ? ["/r/eta": .directory] : [:])
    let runner = ObservingRunner(inner: scripted) { spec, result in
        if spec.arguments.starts(with: ["feature", "teardown"]), !spec.arguments.contains("--dry-run"),
           result.termination == .exited(0) {
            fileSystem.set("/r/eta", nil)
        }
    }
    return Scripted.backend(runner, identity: identity, fileSystem: fileSystem)
}

@Suite struct TeardownTests {
    // MARK: - Refusals before any spawn (§6.5 steps 1–4)

    @Test func legacyTeardownWithAnUntrackedUserFileRefusesWithoutSpawningTheCLI() async throws {
        let runner = ScriptedProcessRunner(Scripted.preflight(status: "?? notes.txt\0"))
        let error = await backendError { try await backend(runner).teardownFeature(request(), progress: { _ in }) }

        let refusal = try #require(error?.refusal)
        #expect(refusal.cause == .uncommittedChanges(files: [ChangedFile(path: "notes.txt", kind: "untracked", area: "other")]))
        #expect(refusal.message.contains("notes.txt") && refusal.message.contains("nothing was removed"))
        #expect(refusal.plan?.source == .appPreflight)
        #expect(refusal.plan?.blockers.first?.kind == "uncommitted_changes")
        #expect(!Scripted.ran(runner, teardownCommand), "\(Scripted.arguments(runner))")
        // Step 1: the plan was built right before deciding (status read once, on this attempt).
        #expect(Scripted.ran(runner, ["git", "-C", "/r/eta", "status", "--porcelain=v1", "-z", "--untracked-files=all",
                                      "--no-renames", "--ignore-submodules=none"]))
    }

    @Test func consentThatDoesNotCoverTheCurrentFilesRefusesNamingTheNewOnes() async throws {
        let runner = ScriptedProcessRunner(Scripted.preflight(status: "?? notes.txt\0 M README.md\0"))
        let error = await backendError {
            try await backend(runner).teardownFeature(request(discard: ["notes.txt"]), progress: { _ in })
        }
        #expect(error?.refusal?.cause == .uncommittedChanges(files: [ChangedFile(path: "README.md", kind: "modified", area: "other")]))
        #expect(error?.refusal?.message.contains("appeared after you confirmed") == true)
        #expect(!Scripted.ran(runner, teardownCommand))
    }

    @Test func deleteIfMergedOfAnUnmergedBranchRefusesWithoutSpawning() async throws {
        let runner = ScriptedProcessRunner(Scripted.preflight(merged: false, ahead: 2))
        let error = await backendError { try await backend(runner).teardownFeature(request(.deleteIfMerged), progress: { _ in }) }
        #expect(error?.refusal?.cause == .unmergedBranch(branch: "feature/eta", ahead: 2))
        #expect(error?.refusal?.message.contains("2 commits not in main") == true)
        #expect(!Scripted.ran(runner, teardownCommand))
    }

    @Test func lockedWorktreeOrUnreadableStatusRefusesUnlessForcedRemoval() async throws {
        let locked = ScriptedProcessRunner(Scripted.preflight(locked: true))
        let lockedError = await backendError { try await backend(locked).teardownFeature(request(), progress: { _ in }) }
        #expect(lockedError?.refusal?.cause == .worktreeLocked(reason: "on a USB disk"))
        #expect(!Scripted.ran(locked, teardownCommand))

        let unreadable = ScriptedProcessRunner(Scripted.preflight(statusFails: true))
        let statusError = await backendError { try await backend(unreadable).teardownFeature(request(), progress: { _ in }) }
        #expect(statusError?.refusal?.cause == .statusUnavailable(cause: "fatal: index file corrupt"))
        #expect(statusError?.refusal?.plan?.changes.statusAvailable == false)

        // forceRemoval is honoured for both.
        for preflight in [Scripted.preflight(locked: true), Scripted.preflight(statusFails: true)] {
            let runner = ScriptedProcessRunner(preflight + [.exit(teardownCommand, stdout: Scripted.teardownSummary)])
            let outcome = try await backend(runner).teardownFeature(request(forceRemoval: true), progress: { _ in })
            #expect(outcome.worktreeGone)
            let teardown = try #require(Scripted.arguments(runner).first { $0.starts(with: teardownCommand) })
            #expect(teardown.suffix(2) == ["--keep-branch", "--force"])
        }
    }

    @Test func forcedRemovalNeverBypassesConsentForUserFiles() async throws {
        let runner = ScriptedProcessRunner(Scripted.preflight(status: "?? notes.txt\0"))
        let error = await backendError { try await backend(runner).teardownFeature(request(forceRemoval: true), progress: { _ in }) }
        #expect(error?.refusal?.cause == .uncommittedChanges(files: [ChangedFile(path: "notes.txt", kind: "untracked", area: "other")]))
        #expect(error?.refusal?.message.contains("force the removal") == true)
        #expect(!Scripted.ran(runner, teardownCommand))

        // A worktree whose folder is gone has nothing to lose: forced removal goes ahead.
        let missing = ScriptedProcessRunner(Scripted.preflight() + [.exit(teardownCommand, stdout: Scripted.teardownSummary)])
        _ = try await backend(missing, worktreeExists: false).teardownFeature(request(forceRemoval: true), progress: { _ in })
        #expect(!Scripted.ran(missing, ["git", "-C", "/r/eta", "status"]))
        #expect(Scripted.ran(missing, teardownCommand))
    }

    @Test func refusalDecisionsInIsolation() {
        let user = [ChangedFile(path: "notes.txt", kind: "untracked", area: "other")]
        func plan(user: [ChangedFile] = [], exists: Bool = true, blockers: [TeardownPlanDocument.Blocker] = [],
                  merged: Bool = true) -> TeardownPlanDocument {
            TeardownPlanDocument(workFeature: "eta", registered: true, worktree: .init(path: "/r/eta", exists: exists),
                                 changes: .init(statusAvailable: true, user: user),
                                 branch: .init(name: "feature/eta", source: "registry", exists: true, reference: "HEAD",
                                               referenceName: "main", merged: merged, mergedIntoHead: merged,
                                               ahead: merged ? 0 : 1, action: "delete"),
                                 blockers: blockers)
        }
        #expect(LegacyTeardown.refusal(for: request(), plan: plan(), cliVersion: nil) == nil)
        #expect(LegacyTeardown.refusal(for: request(discard: ["notes.txt"]), plan: plan(user: user), cliVersion: nil) == nil)
        #expect(LegacyTeardown.refusal(for: request(discard: ["notes.txt"], forceRemoval: true), plan: plan(user: user),
                                       cliVersion: nil) == nil)
        #expect(LegacyTeardown.refusal(for: request(forceRemoval: true), plan: plan(user: user, exists: false),
                                       cliVersion: nil)?.cause == .uncommittedChanges(files: user))
        #expect(LegacyTeardown.refusal(for: request(.forceDelete), plan: plan(merged: false), cliVersion: nil) == nil)
        #expect(LegacyTeardown.refusal(for: request(.keep), plan: plan(merged: false), cliVersion: nil) == nil)
        #expect(LegacyTeardown.refusal(for: request(discard: []), plan: plan(), cliVersion: "0.13.4") == nil)
        let refusal = LegacyTeardown.refusal(for: request(), plan: plan(user: user), cliVersion: "0.13.4")
        #expect(refusal?.diagnostics.cliVersion == "0.13.4")
        #expect(refusal?.diagnostics.invocation == nil, "nothing was run")
        #expect(LegacyTeardown.list(["a", "b", "c", "d", "e"]) == "a, b, c and 2 more")
    }

    // MARK: - Legacy mode (§6.5 step 6)

    @Test func legacyBranchStepRunsGitBranchDashDAfterKeepBranch() async throws {
        let runner = ScriptedProcessRunner(Scripted.preflight() + [
            .exit(teardownCommand, stdout: Scripted.teardownSummary),
            .exit(["git", "-C", "/r/main", "branch", "-d", "feature/eta"]),
        ])
        let progress = ProgressCollector()
        let outcome = try await backend(runner).teardownFeature(request(.deleteIfMerged), progress: progress.sink)

        let arguments = Scripted.arguments(runner)
        let teardown = try #require(arguments.firstIndex { $0.starts(with: teardownCommand) })
        let branchStep = try #require(arguments.firstIndex { $0 == ["git", "-C", "/r/main", "branch", "-d", "feature/eta"] })
        #expect(teardown < branchStep)
        #expect(arguments[teardown] == ["branchbox", "feature", "teardown", "eta", "--repo", "/r/main", "--json",
                                        "--branch-prefix", "feature", "--keep-branch"])
        #expect(outcome.branch == .deleted("feature/eta", by: .app))
        #expect(outcome.worktreeGone)
        #expect(outcome.summary.worktreeRemoved)
        #expect(progress.phases.first == .preparing && progress.phases.contains(.deletingBranch))
        #expect(runner.specs.first { $0.arguments.starts(with: ["feature", "teardown"]) }?.workingDirectory?.path == "/r/main")
    }

    @Test func legacyForceDeleteUsesDashCapitalDAndAFailureIsAnOutcomeNotAnError() async throws {
        let runner = ScriptedProcessRunner(Scripted.preflight(merged: false, ahead: 1) + [
            .exit(teardownCommand, stdout: Scripted.teardownSummary),
            .exit(["git", "-C", "/r/main", "branch", "-D", "feature/eta"], 1,
                  stderr: ["error: Cannot delete branch 'feature/eta' checked out at '/r/other'"]),
        ])
        let outcome = try await backend(runner).teardownFeature(request(.forceDelete), progress: { _ in })
        #expect(outcome.branch == .deleteFailed("feature/eta",
                                                reason: "error: Cannot delete branch 'feature/eta' checked out at '/r/other'"))
        #expect(!Scripted.arguments(runner).contains { $0.contains("--force") })
    }

    @Test func legacyKeepRunsNoBranchStepAndAMissingBranchIsNotFound() async throws {
        let keep = ScriptedProcessRunner(Scripted.preflight() + [.exit(teardownCommand, stdout: Scripted.teardownSummary)])
        let kept = try await backend(keep).teardownFeature(request(.keep), progress: { _ in })
        #expect(kept.branch == .kept("feature/eta"))
        #expect(!Scripted.ran(keep, ["git", "-C", "/r/main", "branch"]))

        let gone = ScriptedProcessRunner(Scripted.preflight(branchExists: false) + [.exit(teardownCommand, stdout: Scripted.teardownSummary)])
        let notFound = try await backend(gone).teardownFeature(request(.deleteIfMerged), progress: { _ in })
        #expect(notFound.branch == .notFound("feature/eta"))
    }

    @Test func legacyTeardownOfOnlyGeneratedFilesForcesTheCleanRemoval() async throws {
        let runner = ScriptedProcessRunner(Scripted.preflight(status: "?? .devcontainer/.branchbox.env\0") + [
            .exit(teardownCommand, stdout: Scripted.teardownSummary),
        ])
        let outcome = try await backend(runner).teardownFeature(request(), progress: { _ in })
        #expect(outcome.worktreeGone)
        let attempts = Scripted.arguments(runner).filter { $0.starts(with: teardownCommand) }
        #expect(attempts.count == 1 && attempts[0].suffix(2) == ["--keep-branch", "--force"])

        // A clean worktree needs no --force.
        let clean = ScriptedProcessRunner(Scripted.preflight() + [.exit(teardownCommand, stdout: Scripted.teardownSummary)])
        _ = try await backend(clean).teardownFeature(request(), progress: { _ in })
        #expect(!Scripted.arguments(clean).contains { $0.contains("--force") })
    }

    @Test func legacyDirtyModuleRefusalIsRepreflightedForTheRecovery() async throws {
        let banner = try Fixtures.string("cli-0.13.4/sandbox_teardown_alpha.json")
        // Clean at the preflight, so no --force; module files appeared before 0.13.x checked them.
        var clean = Scripted.preflight()
        for index in clean.indices { clean[index].times = 1 }
        let runner = ScriptedProcessRunner(clean + [
            .exit(teardownCommand, 1, stdout: banner,
                  stderr: ["Error: Devcontainer/compose changes detected; rerun this command with --force to proceed."]),
        ] + Scripted.preflight(status: "?? .devcontainer/.branchbox.env\0"))
        let error = await backendError { try await backend(runner).teardownFeature(request(), progress: { _ in }) }
        let refusal = try #require(error?.refusal)
        #expect(refusal.cause == .moduleFilesDirty(files: [".devcontainer/"], userChanges: []))
        #expect(refusal.plan?.changes.generated.map(\.path) == [".devcontainer/.branchbox.env"])
        let attempts = Scripted.arguments(runner).filter { $0.starts(with: teardownCommand) }
        #expect(attempts.count == 1 && !attempts[0].contains("--force"))

        // The generated-only recovery (consent for no user files) adds --force and succeeds.
        let retry = ScriptedProcessRunner(Scripted.preflight(status: "?? .devcontainer/.branchbox.env\0") + [
            .exit(teardownCommand, stdout: Scripted.teardownSummary),
        ])
        let outcome = try await backend(retry).teardownFeature(request(discard: []), progress: { _ in })
        #expect(outcome.worktreeGone)
        #expect(Scripted.arguments(retry).first { $0.starts(with: teardownCommand) }?.contains("--force") == true)
    }

    @Test func manualRemovalWarningIsSurfaced() async throws {
        let summary = Scripted.teardownSummary.replacingOccurrences(
            of: #""warnings":["#, with: #""warnings":["Worktree directory removed manually after git removal failed","#)
        let runner = ScriptedProcessRunner(Scripted.preflight() + [.exit(teardownCommand, stdout: summary)])
        let progress = ProgressCollector()
        _ = try await backend(runner).teardownFeature(request(), progress: progress.sink)
        #expect(progress.warnings.contains { $0.contains("removed manually after git removal failed") })
    }

    @Test func cancelledLegacyTeardownNotesAPossiblePartialWorktree() async throws {
        let runner = ScriptedProcessRunner(Scripted.preflight() + [
            ScriptedProcessRunner.Rule(teardownCommand, outcome: .hang),
        ])
        let task = Task { try await backend(runner).teardownFeature(request(), progress: { _ in }) }
        try await eventually { runner.launched.contains { $0.arguments.starts(with: ["feature", "teardown"]) } }
        task.cancel()
        let error = await backendError { try await task.value }
        #expect(error == .cancelled(note: CLIBackend.partialWorktreeNote))
    }

    // MARK: - Contract mode (§6.5 step 5)

    private var contract: BackendIdentity { Scripted.identity(contract: true, Scripted.everything) }

    private static let cliPlan = """
        {"schema_version":1,"work_feature":"eta","registered":true,"status":"active",
         "worktree":{"path":"/r/eta","exists":true,"locked":false,"lock_reason":null},
         "changes":{"status_available":true,"truncated":false,"user":[{"path":"notes.txt","kind":"untracked","area":"other"}],
           "generated":[],"preserved":[]},
         "branch":{"name":"feature/eta","source":"registry","exists":true,"upstream":null,"reference":"HEAD",
           "reference_name":"main","merged":true,"merged_into_head":true,"ahead":0,"action":"delete"},
         "blockers":[{"kind":"uncommitted_changes","count":1,"message":"…","override":"--discard-changes"}],"warnings":[]}
        """

    @Test func contractPlanIsTheCLIDryRunWithTheSamePolicyFlags() async throws {
        let runner = ScriptedProcessRunner([.exit(teardownCommand, stdout: Self.cliPlan)])
        let plan = try await Scripted.backend(runner, identity: contract).planTeardown(request(.deleteIfMerged, discard: ["notes.txt"]))
        #expect(plan.source == .cli)
        #expect(plan.changes.user.map(\.path) == ["notes.txt"])
        #expect(Scripted.arguments(runner) == [["branchbox", "feature", "teardown", "eta", "--repo", "/r/main", "--json",
                                                "--branch-prefix", "feature", "--delete-branch", "--discard-changes",
                                                "--dry-run"]])
    }

    @Test func contractDiscardSendsDiscardChangesAndTheCLIDeletesTheBranch() async throws {
        let summary = Scripted.teardownSummary.replacingOccurrences(of: #""branch_deleted":false"#,
                                                                    with: #""branch_deleted":true,"branch_action":"delete""#)
        let runner = ScriptedProcessRunner([
            .exit(teardownCommand + ["eta", "--repo", "/r/main", "--json", "--branch-prefix", "feature", "--delete-branch",
                                     "--discard-changes", "--dry-run"], stdout: Self.cliPlan),
            .exit(teardownCommand, stdout: summary),
        ])
        let outcome = try await backend(runner, identity: contract).teardownFeature(request(.deleteIfMerged, discard: ["notes.txt"]),
                                                                                  progress: { _ in })
        #expect(outcome.branch == .deleted("feature/eta", by: .cli))
        let teardown = try #require(Scripted.arguments(runner).last)
        #expect(teardown == ["branchbox", "feature", "teardown", "eta", "--repo", "/r/main", "--json", "--branch-prefix",
                             "feature", "--delete-branch", "--discard-changes"])
        #expect(!teardown.contains("--force"))
        #expect(!Scripted.ran(runner, ["git"]))
    }

    @Test func contractPartialBranchFailureIsDeleteFailed() async throws {
        let plan = Self.cliPlan.replacingOccurrences(of: #""user":[{"path":"notes.txt","kind":"untracked","area":"other"}]"#,
                                                     with: #""user":[]"#)
        let summary = Scripted.teardownSummary.replacingOccurrences(
            of: #""branch_deleted":false"#,
            with: #""branch_deleted":false,"branch_action":"delete","branch_delete_error":"error: branch not fully merged""#)
        let runner = ScriptedProcessRunner([.exit(teardownCommand + ["eta"], stdout: plan, times: 1),
                                            .exit(teardownCommand, stdout: summary)])
        let outcome = try await backend(runner, identity: contract).teardownFeature(request(.deleteIfMerged), progress: { _ in })
        #expect(outcome.branch == .deleteFailed("feature/eta", reason: "error: branch not fully merged"))
    }

    @Test func contractRefusalEnvelopeCarriesThePlan() async throws {
        let envelope = #"{"schema_version":1,"error":{"code":"teardown_refused","message":"Refusing to tear down 'eta'","causes":[],"details":{"plan":\#(Self.cliPlan),"changed_anything":false,"completed_steps":[]}}}"#
        let cleanPlan = Self.cliPlan.replacingOccurrences(of: #""user":[{"path":"notes.txt","kind":"untracked","area":"other"}]"#,
                                                          with: #""user":[]"#)
        let runner = ScriptedProcessRunner([.exit(teardownCommand + ["eta"], stdout: cleanPlan, times: 1),
                                            .exit(teardownCommand, 1, stdout: envelope,
                                                  stderr: ["Error: Refusing to tear down 'eta'"])])
        let error = await backendError {
            try await backend(runner, identity: contract).teardownFeature(request(.deleteIfMerged), progress: { _ in })
        }
        #expect(error?.refusal?.cause == .uncommittedChanges(files: [ChangedFile(path: "notes.txt", kind: "untracked", area: "other")]))
        #expect(error?.refusal?.plan?.workFeature == "eta")
    }

    /// A contract CLI without the teardown capabilities (RS-1 only) gets the legacy flags, preflight and branch step.
    @Test func contractCLIWithoutTeardownCapabilitiesUsesTheLegacyPath() async throws {
        let rs1 = Scripted.identity(contract: true, [.jsonErrorEnvelope, .registryLock, .writeAheadStart])
        let runner = ScriptedProcessRunner(Scripted.preflight() + [
            .exit(teardownCommand, stdout: Scripted.teardownSummary),
            .exit(["git", "-C", "/r/main", "branch", "-d", "feature/eta"]),
        ])
        let outcome = try await backend(runner, identity: rs1).teardownFeature(request(.deleteIfMerged), progress: { _ in })
        #expect(outcome.branch == .deleted("feature/eta", by: .app))
        #expect(Scripted.arguments(runner).first { $0.starts(with: teardownCommand) }?.contains("--keep-branch") == true)

        // RS-1's legacy refusal envelope (plan: null) is the dirty-module refusal.
        let details = #"{"plan":null,"changed_anything":false,"completed_steps":[],"worktree":"/r/eta","files":[".devcontainer/"]}"#
        let refused = ScriptedProcessRunner(Scripted.preflight() + [
            .exit(teardownCommand, 1,
                  stdout: #"{"schema_version":1,"error":{"code":"teardown_refused","message":"Devcontainer/compose changes detected; rerun this command with --force to proceed.","causes":[],"details":\#(details)}}"#),
        ])
        let error = await backendError { try await backend(refused, identity: rs1).teardownFeature(request(), progress: { _ in }) }
        #expect(error?.refusal?.cause == .moduleFilesDirty(files: [".devcontainer/"], userChanges: []))
        #expect(error?.refusal?.plan?.source == .appPreflight)
    }
}
