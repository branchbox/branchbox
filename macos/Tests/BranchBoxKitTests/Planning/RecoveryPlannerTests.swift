import BranchBoxKit
import Foundation
import Testing

@Suite struct RecoveryPlannerTests {
    private static let dirtyFiles = [Sample.changed("README.md"), Sample.changed("notes.txt", "untracked")]
    private static let stray = StrayWorktree(path: "/tmp/bbx/spike", branch: "spike/x", head: "abc1234")
    private static let diagnostics = Diagnostics(summary: "Error: git failed", exitCode: 1)

    private func retries(_ actions: [RecoveryAction]) -> [(context: OperationRequestContext, label: String, destructive: Bool,
                                                           confirmation: String?)] {
        actions.compactMap {
            guard case .retry(let context, let label, let destructive, let confirmation) = $0 else { return nil }
            return (context, label, destructive, confirmation)
        }
    }

    private func teardownRequest(_ action: RecoveryAction?) -> TeardownRequest? {
        guard case .retry(.teardown(let request), _, _, _)? = action else { return nil }
        return request
    }

    // MARK: Dirty worktrees

    @Test func dirtyTeardownGetsDestructiveRetryWithExactlyTheRefusedFiles() throws {
        let plan = Sample.plan(user: Self.dirtyFiles)
        let actions = RecoveryPlanner.recoveries(for: Sample.refused(.uncommittedChanges(files: Self.dirtyFiles), plan: plan),
                                                 after: .teardown(Sample.teardown(.keep)))
        let retry = try #require(retries(actions).first)
        #expect(retry.label == "Discard 2 changes and tear down…")
        #expect(retry.destructive)
        let confirmation = try #require(retry.confirmation)
        #expect(confirmation.contains("README.md (modified)"))
        #expect(confirmation.contains("notes.txt (untracked)"))
        let request = try #require(teardownRequest(actions.first))
        #expect(request.discard?.userFiles == ["README.md", "notes.txt"])
        #expect(request.branch == .keep)                 // nothing else changes
        #expect(request.forceRemoval == false)
        #expect(request.feature == Sample.feature)
        #expect(actions.last == .revealInFinder(path: "/tmp/bbx/eta"))
        #expect(retries(actions).count == 1)
    }

    @Test func aTruncatedChangeListSaysMoreIsDeleted() throws {
        let files = (0..<25).map { Sample.changed("f\($0).txt", "untracked") }
        let plan = Sample.plan(truncated: true, user: files)
        let actions = RecoveryPlanner.recoveries(for: Sample.refused(.uncommittedChanges(files: files), plan: plan),
                                                 after: .teardown(Sample.teardown(.keep)))
        let retry = try #require(retries(actions).first)
        #expect(retry.label == "Discard more than 25 changes and tear down…")
        #expect(try #require(retry.confirmation).contains("The list is truncated: other changes in this folder are deleted too."))
    }

    @Test func singleDirtyFileIsSingular() throws {
        let actions = RecoveryPlanner.recoveries(for: Sample.refused(.uncommittedChanges(files: [Sample.changed("a.txt")])),
                                                 after: .teardown(Sample.teardown()))
        #expect(retries(actions).first?.label == "Discard 1 change and tear down…")
        #expect(actions.count == 1)                      // no plan, so nothing to reveal
    }

    @Test func longFileListsAreSummarizedButConsentCoversEveryFile() throws {
        let files = (1...25).map { Sample.changed("file\($0).txt") }
        let actions = RecoveryPlanner.recoveries(for: Sample.refused(.uncommittedChanges(files: files)),
                                                 after: .teardown(Sample.teardown()))
        let retry = try #require(retries(actions).first)
        #expect(retry.confirmation?.contains("…and 5 more") == true)
        #expect(retry.confirmation?.contains("file21.txt") == false)
        #expect(teardownRequest(actions.first)?.discard?.userFiles == files.map(\.path))
    }

    /// New changes appearing after the user confirmed (the backend's race check) are added to that consent, never
    /// swapped for it, and the confirmation lists only what is new.
    @Test func raceAddsNewFilesToTheEarlierConsent() throws {
        var discarding = Sample.teardown()
        discarding.discard = DiscardConsent(userFiles: ["README.md"])
        let error = Sample.refused(.uncommittedChanges(files: [Sample.changed("late.txt", "untracked"), Sample.changed("README.md")]))
        let actions = RecoveryPlanner.recoveries(for: error, after: .teardown(discarding))
        #expect(teardownRequest(actions.first)?.discard?.userFiles == ["README.md", "late.txt"])
        let retry = try #require(retries(actions).first)
        #expect(retry.label == "Discard 2 changes and tear down…")
        #expect(retry.confirmation?.hasSuffix("…along with the change you confirmed before.") == true)

        discarding.discard = DiscardConsent(userFiles: ["a", "b"])
        let generated = RecoveryPlanner.recoveries(for: Sample.refused(.moduleFilesDirty(files: [".devcontainer/"], userChanges: [])),
                                                   after: .teardown(discarding))
        #expect(teardownRequest(generated.first)?.discard?.userFiles == ["a", "b"])
        let more = RecoveryPlanner.recoveries(for: Sample.refused(.uncommittedChanges(files: [Sample.changed("c")])),
                                              after: .teardown(discarding))
        #expect(retries(more).first?.confirmation?.hasSuffix("…along with the 2 changes you confirmed before.") == true)
    }

    @Test func moduleFilesDirtyWithoutUserChangesDiscardsGeneratedFilesOnly() throws {
        let error = Sample.refused(.moduleFilesDirty(files: [".devcontainer/"], userChanges: []))
        let actions = RecoveryPlanner.recoveries(for: error, after: .teardown(Sample.teardown()))
        #expect(actions.count == 1)
        let retry = try #require(retries(actions).first)
        #expect(retry.label == "Discard BranchBox-generated files and tear down")
        #expect(retry.destructive)
        #expect(retry.confirmation?.contains("• .devcontainer/") == true)
        #expect(teardownRequest(actions.first)?.discard?.userFiles == [])
    }

    @Test func moduleFilesDirtyWithUserChangesAsksAboutThoseChanges() throws {
        let error = Sample.refused(.moduleFilesDirty(files: [".devcontainer/"], userChanges: [Sample.changed("app.rb")]))
        let actions = RecoveryPlanner.recoveries(for: error, after: .teardown(Sample.teardown()))
        #expect(retries(actions).map(\.label) == ["Discard 1 change and tear down…"])
        #expect(teardownRequest(actions.first)?.discard?.userFiles == ["app.rb"])
    }

    @Test func dirtyStrayRemovalAsksToDiscard() throws {
        let error = Sample.refused(.uncommittedChanges(files: Self.dirtyFiles))
        let actions = RecoveryPlanner.recoveries(for: error, after: .removeStray(Self.stray, Sample.project, discard: nil))
        let retry = try #require(retries(actions).first)
        guard case .removeStray(_, _, let consent) = retry.context else {
            Issue.record("expected stray removal")
            return
        }
        #expect(consent?.userFiles == Self.dirtyFiles.map(\.path))
        #expect(retry.label == "Discard 2 changes and remove the worktree…")
        #expect(retry.destructive)
        #expect(actions.last == .revealInFinder(path: Self.stray.path))
    }

    @Test func newStrayChangesRequireAnotherConfirmationAndKeepEarlierConsent() throws {
        let earlier = DiscardConsent(userFiles: ["old.txt"])
        let error = Sample.refused(.uncommittedChanges(files: [Sample.changed("new.txt")]))
        let actions = RecoveryPlanner.recoveries(for: error,
            after: .removeStray(Self.stray, Sample.project, discard: earlier))
        let retry = try #require(retries(actions).first)
        guard case .removeStray(_, _, let consent) = retry.context else {
            Issue.record("expected a retry for newly refused stray changes")
            return
        }
        #expect(consent?.userFiles == ["old.txt", "new.txt"])
        #expect(retry.confirmation?.contains("new.txt") == true)
        #expect(retry.confirmation?.contains("you confirmed before") == true)
    }

    // MARK: Branches

    @Test func unmergedBranchOffersKeepAndForceDelete() throws {
        let error = Sample.refused(.unmergedBranch(branch: "feature/eta", ahead: 3))
        let actions = RecoveryPlanner.recoveries(for: error, after: .teardown(Sample.teardown(.deleteIfMerged)))
        let found = retries(actions)
        #expect(found.count == 2)
        #expect(found[0].label == "Keep feature/eta and tear down")
        #expect(found[0].destructive == false)
        #expect(found[0].confirmation == nil)
        #expect(teardownRequest(actions[0])?.branch == .keep)
        #expect(found[1].label == "Force-delete feature/eta and tear down…")
        #expect(found[1].destructive)
        #expect(found[1].confirmation?.contains("feature/eta has 3 commits that aren't merged") == true)
        #expect(teardownRequest(actions[1])?.branch == .forceDelete)
        #expect(teardownRequest(actions[1])?.discard == nil)
    }

    @Test(arguments: [(Int?.none, "commits that aren't merged"), (1, "1 commit that isn't merged")])
    func unmergedConfirmationCountsCommits(ahead: Int?, phrase: String) throws {
        let actions = RecoveryPlanner.recoveries(for: Sample.refused(.unmergedBranch(branch: "feature/eta", ahead: ahead)),
                                                 after: .teardown(Sample.teardown()))
        #expect(retries(actions).last?.confirmation?.contains(phrase) == true)
    }

    @Test func partialUnmergedBranchOffersForcedBranchDeletion() throws {
        let remaining = Refusal(cause: .unmergedBranch(branch: "feature/beta", ahead: 1),
                                message: "Branch 'feature/beta' could not be deleted without force", diagnostics: Self.diagnostics)
        let error = BackendError.partial(PartialFailure(completed: ["Worktree removed"], remaining: remaining))
        let actions = RecoveryPlanner.recoveries(for: error, after: .teardown(Sample.teardown()))
        let retry = try #require(retries(actions).first)
        #expect(retry.context == .deleteBranch("feature/beta", Sample.project, force: true))
        #expect(retry.destructive)
        #expect(retry.label == "Force-delete feature/beta…")
        #expect(retry.confirmation?.contains("1 commit") == true)
        #expect(actions.count == 1)

        #expect(RecoveryPlanner.recoveries(for: error, after: nil).isEmpty)
        let otherRemaining = BackendError.partial(PartialFailure(
            completed: ["Worktree removed"],
            remaining: Refusal(cause: .worktreeRemovalFailed(cause: "busy"), message: "x", diagnostics: Self.diagnostics)))
        #expect(RecoveryPlanner.recoveries(for: otherRemaining, after: .teardown(Sample.teardown())).isEmpty)
        #expect(RecoveryPlanner.recoveries(for: otherRemaining, after: nil, operation: Sample.operation)
            == [.showLog(operation: Sample.operation)])
    }

    @Test func refusedBranchDeletionOffersForce() {
        let actions = RecoveryPlanner.recoveries(for: Sample.refused(.unmergedBranch(branch: "feature/eta", ahead: 2)),
                                                 after: .deleteBranch("feature/eta", Sample.project, force: false))
        #expect(retries(actions).map(\.context) == [.deleteBranch("feature/eta", Sample.project, force: true)])
    }

    // MARK: Locked, unreadable and missing worktrees

    struct ForcedCase: Sendable, CustomTestStringConvertible {
        let cause: RefusalCause
        let label: String
        let destructive: Bool
        var testDescription: String { label }
    }

    static let forcedCases: [ForcedCase] = [
        ForcedCase(cause: .worktreeLocked(reason: "in use by another agent"), label: "Remove the locked worktree…", destructive: true),
        ForcedCase(cause: .worktreeLocked(reason: nil), label: "Remove the locked worktree…", destructive: true),
        ForcedCase(cause: .statusUnavailable(cause: "fatal: index file corrupt"), label: "Remove without checking for changes…",
                   destructive: true),
        ForcedCase(cause: .worktreeNotFound("eta"), label: "Clean up the missing worktree", destructive: false),
    ]

    @Test(arguments: RecoveryPlannerTests.forcedCases)
    func forcedRemovalAlwaysKeepsTheBranch(_ testCase: ForcedCase) throws {
        let actions = RecoveryPlanner.recoveries(for: Sample.refused(testCase.cause, plan: Sample.plan()),
                                                 after: .teardown(Sample.teardown(.forceDelete)))
        let retry = try #require(retries(actions).first)
        #expect(retry.label == testCase.label)
        #expect(retry.destructive == testCase.destructive)
        #expect((retry.confirmation != nil) == testCase.destructive)
        let request = try #require(teardownRequest(actions.first))
        #expect(request.forceRemoval)
        #expect(request.branch == .keep)
        #expect(request.discard == nil)
        if testCase.destructive {
            #expect(retry.confirmation?.contains("/tmp/bbx/eta") == true)
            #expect(retry.confirmation?.contains("feature/eta is kept.") == true)
        }
    }

    // MARK: Start

    @Test func existingWorktreeOffersReuse() throws {
        let start = StartFeatureRequest(project: Sample.project, name: "eta", runtime: .container)
        let actions = RecoveryPlanner.recoveries(for: Sample.refused(.worktreeExists(path: "/tmp/bbx/eta")), after: .start(start))
        var expected = start
        expected.reuse = .existingWorktree(.fail)
        #expect(actions == [.retry(.start(expected), label: "Start in the existing folder", destructive: false, confirmation: nil),
                            .revealInFinder(path: "/tmp/bbx/eta")])
    }

    @Test func sandboxSignInRunsInTerminal() {
        let sbx = Sample.refused(.runtimePrerequisite(provider: "sbx", detail: "Sign in with: sbx login"))
        #expect(RecoveryPlanner.recoveries(for: sbx, after: nil)
            == [.runInTerminal(command: ["sbx", "login"], workingDirectory: nil, label: "Sign in to Docker Sandboxes"), .openDoctor])
        let vm = Sample.refused(.runtimePrerequisite(provider: "local-vm", detail: "local-vm requires a Linux host"))
        #expect(RecoveryPlanner.recoveries(for: vm, after: nil) == [.openDoctor])
    }

    // MARK: Environment and failures

    @Test func missingOrOldCLI() {
        #expect(RecoveryPlanner.recoveries(for: .cliNotFound(searched: ["/opt/homebrew/bin/branchbox"]), after: nil)
            == [.locateCLI, .copyCommand("brew install branchbox/tap/branchbox", label: "Copy Install Command")])
        let old = BackendError.cliTooOld(found: SemVer(0, 13, 3), minimum: SemVer(0, 13, 4), path: "/usr/local/bin/branchbox")
        #expect(RecoveryPlanner.recoveries(for: old, after: nil)
            == [.copyCommand("brew upgrade branchbox", label: "Copy Upgrade Command"), .locateCLI])
        #expect(RecoveryPlanner.recoveries(for: .cliUnusable(path: "/x", reason: "not executable"), after: nil)
            == [.locateCLI, .openDoctor])
    }

    static let failures: [BackendError] = [
        .commandFailed(diagnostics),
        .decodeFailed(what: "feature list", detail: "unexpected token", diagnostics: diagnostics),
        .registryCorrupted(path: "/tmp/bbx/main/.branchbox/registry.json", diagnostics: diagnostics),
    ]

    @Test(arguments: RecoveryPlannerTests.failures)
    func failuresOnlyOfferTheLogAndTheDoctor(_ error: BackendError) {
        let contexts: [OperationRequestContext?] = [
            nil, .teardown(Sample.teardown()), .start(StartFeatureRequest(project: Sample.project, name: "eta", runtime: .sbx)),
            .tunnelRemove(Sample.feature, force: true), .deleteBranch("feature/eta", Sample.project, force: false),
        ]
        for context in contexts {
            #expect(RecoveryPlanner.recoveries(for: error, after: context, operation: Sample.operation)
                == [.showLog(operation: Sample.operation), .openDoctor])
            #expect(RecoveryPlanner.recoveries(for: error, after: context) == [.openDoctor])
        }
    }

    @Test func tunnelRemovalFailureOffersRemoveAnywayWithConfirmation() throws {
        let actions = RecoveryPlanner.recoveries(for: .commandFailed(Self.diagnostics),
                                                 after: .tunnelRemove(Sample.feature, force: false), operation: Sample.operation)
        let retry = try #require(retries(actions).first)
        #expect(retry.context == .tunnelRemove(Sample.feature, force: true))
        #expect(retry.label == "Remove Anyway…")
        #expect(retry.destructive)
        #expect(retry.confirmation?.contains("DNS record") == true)
        #expect(Array(actions.dropFirst()) == [.showLog(operation: Sample.operation), .openDoctor])

        let refusedByProvider = Sample.refused(.other(code: "command_failed"))
        #expect(retries(RecoveryPlanner.recoveries(for: refusedByProvider, after: .tunnelRemove(Sample.feature, force: false)))
            .map(\.label) == ["Remove Anyway…"])
    }

    @Test func timeoutsAndLocksRetryOnlyNonDestructiveRequests() {
        let exec = OperationRequestContext.exec(ExecRequest(feature: Sample.feature, command: ["make", "test"]))
        let timedOut = BackendError.timedOut(operation: "command", after: .seconds(30), diagnostics: Self.diagnostics)
        #expect(RecoveryPlanner.recoveries(for: timedOut, after: exec, operation: Sample.operation)
            == [.retry(exec, label: "Try Again", destructive: false, confirmation: nil), .showLog(operation: Sample.operation)])
        #expect(RecoveryPlanner.recoveries(for: timedOut, after: nil).isEmpty)

        let locked = Sample.refused(.registryLocked(path: "/tmp/bbx/main/.branchbox"))
        #expect(RecoveryPlanner.recoveries(for: locked, after: .teardown(Sample.teardown()))
            == [.retry(.teardown(Sample.teardown()), label: "Try Again", destructive: false, confirmation: nil)])

        var discarding = Sample.teardown()
        discarding.discard = DiscardConsent(userFiles: ["a.txt"])
        var forced = Sample.teardown()
        forced.forceRemoval = true
        let destructive: [OperationRequestContext] = [
            .teardown(discarding), .teardown(forced), .teardown(Sample.teardown(.forceDelete)),
            .prune(PruneSelection(project: Sample.project, rows: [Sample.teardown(), discarding])),
            .devcontainer(.down(removeVolumes: true), Sample.feature), .tunnelRemove(Sample.feature, force: true),
            .deleteBranch("feature/eta", Sample.project, force: true),
            .removeStray(Self.stray, Sample.project, discard: DiscardConsent(userFiles: [])),
            .initProject(InitRequest(folder: Sample.project.root, reorganize: true)),
        ]
        for context in destructive {
            #expect(RecoveryPlanner.recoveries(for: locked, after: context).isEmpty)
            #expect(RecoveryPlanner.recoveries(for: timedOut, after: context).isEmpty)
        }
    }

    @Test func missingFeatureAndFolderRefreshTheProject() {
        #expect(RecoveryPlanner.recoveries(for: Sample.refused(.featureNotFound("eta")), after: .tunnelOpen(Sample.feature))
            == [.refresh(Sample.project)])
        #expect(RecoveryPlanner.recoveries(for: .projectInvalid(.workingDirectoryMissing("/tmp/bbx/eta")),
                                           after: .exec(ExecRequest(feature: Sample.feature, command: ["ls"])))
            == [.refresh(Sample.project)])
        #expect(RecoveryPlanner.recoveries(for: Sample.refused(.featureNotFound("eta")), after: nil).isEmpty)
    }

    /// Recoveries appear only for the refusal they answer: the wrong context, or no context, gives none.
    struct MismatchCase: Sendable, CustomTestStringConvertible {
        let name: String
        let error: BackendError
        let context: OperationRequestContext?
        var testDescription: String { name }
    }

    static let mismatches: [MismatchCase] = [
        MismatchCase(name: "dirty without context", error: Sample.refused(.uncommittedChanges(files: dirtyFiles)), context: nil),
        MismatchCase(name: "dirty after start", error: Sample.refused(.uncommittedChanges(files: dirtyFiles)),
                     context: .start(StartFeatureRequest(project: Sample.project, name: "eta", runtime: .container))),
        MismatchCase(name: "unmerged after start", error: Sample.refused(.unmergedBranch(branch: "feature/eta", ahead: 1)),
                     context: .start(StartFeatureRequest(project: Sample.project, name: "eta", runtime: .container))),
        MismatchCase(name: "unmerged forced deletion", error: Sample.refused(.unmergedBranch(branch: "feature/eta", ahead: 1)),
                     context: .deleteBranch("feature/eta", Sample.project, force: true)),
        MismatchCase(name: "locked after exec", error: Sample.refused(.worktreeLocked(reason: nil)),
                     context: .exec(ExecRequest(feature: Sample.feature, command: ["ls"]))),
        MismatchCase(name: "exists after teardown", error: Sample.refused(.worktreeExists(path: "/tmp/bbx/eta")),
                     context: .teardown(Sample.teardown())),
        MismatchCase(name: "generated files without context",
                     error: Sample.refused(.moduleFilesDirty(files: [".devcontainer/"], userChanges: [])), context: nil),
        MismatchCase(name: "branch exists", error: Sample.refused(.branchExists("feature/eta")),
                     context: .start(StartFeatureRequest(project: Sample.project, name: "eta", runtime: .container))),
        MismatchCase(name: "invalid name", error: Sample.refused(.invalidName("Bad Name")), context: nil),
        MismatchCase(name: "config invalid", error: Sample.refused(.configInvalid(key: "feature.branch_prefix", detail: "x")),
                     context: .applyConfig(ConfigPatch(changes: []), Sample.project)),
        MismatchCase(name: "tunnel forced already", error: Sample.refused(.other(code: "command_failed")),
                     context: .tunnelRemove(Sample.feature, force: true)),
        MismatchCase(name: "unsupported", error: .unsupported(.config, minimumCLI: "0.14.0"), context: nil),
        MismatchCase(name: "cancelled", error: .cancelled(note: "may have left a partial worktree"),
                     context: .teardown(Sample.teardown())),
        MismatchCase(name: "project missing", error: .projectInvalid(.missing("/tmp/bbx/main")), context: nil),
    ]

    @Test(arguments: RecoveryPlannerTests.mismatches)
    func noRecoveryForAMismatchedRefusal(_ testCase: MismatchCase) {
        #expect(RecoveryPlanner.recoveries(for: testCase.error, after: testCase.context).isEmpty)
    }

    @Test func backendFailureRefusalsAlsoOfferTheLog() {
        let removal = Sample.refused(.worktreeRemovalFailed(cause: "Directory not empty"))
        #expect(RecoveryPlanner.recoveries(for: removal, after: .teardown(Sample.teardown()), operation: Sample.operation)
            == [.showLog(operation: Sample.operation)])
        #expect(RecoveryPlanner.recoveries(for: .launchFailed(executable: "/usr/bin/git", reason: "ENOENT"), after: nil,
                                           operation: Sample.operation) == [.showLog(operation: Sample.operation), .openDoctor])
    }

    /// Recovery ids are stable and distinct within one list, so SwiftUI can diff the buttons.
    @Test func recoveryIDsAreDistinct() {
        let actions = RecoveryPlanner.recoveries(for: Sample.refused(.unmergedBranch(branch: "feature/eta", ahead: 3)),
                                                 after: .teardown(Sample.teardown()))
        #expect(Set(actions.map(\.id)).count == actions.count)
    }
}
