import BranchBoxCLI
import BranchBoxKit
import Foundation
import Testing

// VER-1 lifecycle (DESIGN §13.2) against the real CLI, legacy (0.13.x) or contract: exec, the dirty-teardown
// refusal and its discard recovery, each branch policy, and a custom branch prefix. Each test detects the mode from
// the CLI's identity and asserts what that mode must do.
//
//   BRANCHBOX_IT=1 BRANCHBOX_IT_CLI=/opt/homebrew/bin/branchbox swift test --filter CLIBackendLifecycleTests

extension RealCLI {
    @Suite struct CLIBackendLifecycleTests {
        @Test(.enabled(if: integrationEnabled), .timeLimit(.minutes(2)))
        func execReturnsOutputAndTheInnerExitCodeAsData() async throws {
            let cli = try await LiveCLI.make()
            let repo = try await TempRepo.make()
            defer { repo.remove() }
            try await cli.start("execs", in: repo)
            let feature = FeatureRef(project: repo.project, name: "execs")

            let ok = try await cli.backend.exec(ExecRequest(feature: feature, command: ["echo", "hi"], timeout: .seconds(60)),
                                                progress: { _ in })
            #expect(ok.exitCode == 0 && ok.stdout == "hi\n")
            let three = try await cli.backend.exec(
                ExecRequest(feature: feature, command: ["sh", "-c", "echo out; echo err >&2; exit 3"], timeout: .seconds(60)),
                progress: { _ in })
            #expect(three.exitCode == 3, "a failing command is data, not an error (\(cli.mode))")
            #expect(three.stdout == "out\n")
            #expect(three.stderr.contains("err"))
        }

        /// The DESIGN §6.5 refusal: legacy CLIs are stopped by the app preflight before anything is spawned (the
        /// registry is untouched); contract CLIs refuse with an envelope. The discard retry then removes the
        /// worktree, and Keep keeps the branch.
        @Test(.enabled(if: integrationEnabled), .timeLimit(.minutes(2)))
        func untrackedFileIsRefusedThenDiscardedWithConsentAndKeepKeepsTheBranch() async throws {
            let cli = try await LiveCLI.make()
            let repo = try await TempRepo.make(ignoring: TempRepo.branchBoxIgnores)
            defer { repo.remove() }
            let (summary, record) = try await cli.start("dirty", in: repo)
            let worktree = URL(fileURLWithPath: try #require(summary.worktreePath))
            try repo.write("my notes\n", to: "notes.txt", in: worktree)

            let request = TeardownRequest(feature: FeatureRef(project: repo.project, name: "dirty"),
                                          recordedBranch: record.branchName, branch: .keep)
            let plan = try await cli.backend.planTeardown(request)
            #expect(plan.changes.user.map(\.path) == ["notes.txt"], "\(plan.changes.user)")

            let registryBefore = repo.registry()
            let refusal: Refusal
            do {
                _ = try await cli.backend.teardownFeature(request, progress: { _ in })
                Issue.record("a worktree with an untracked file was torn down without consent (\(cli.mode))")
                return
            } catch BackendError.refused(let refused) {
                refusal = refused
            }
            guard case .uncommittedChanges(let files) = refusal.cause else {
                Issue.record("expected uncommittedChanges, got \(refusal.cause)")
                return
            }
            #expect(files.map(\.path).contains("notes.txt"))
            #expect(FileManager.default.fileExists(atPath: worktree.appendingPathComponent("notes.txt").path))
            #expect(repo.registry() == registryBefore, "a refusal must not change the registry")
            // Both modes refuse from the re-plan taken just before spawning (§6.5 steps 1-2), so no teardown runs.
            #expect(refusal.diagnostics.exitCode == nil, "the refusal must come before any teardown spawn")
            if cli.isContract {
                #expect(refusal.plan?.source == .cli, "contract re-plans come from the CLI's dry run")
                // The CLI enforces the same rule itself (what a change racing the re-plan meets): an envelope.
                let raw = try await cli.run(["feature", "teardown", "dirty", "--repo", repo.main.path, "--json",
                                             "--keep-branch", "--branch-prefix", "feature"], in: repo.main)
                #expect(raw.termination == .exited(1))
                let envelope = try CLIJSON.decode(ErrorEnvelope.self, from: raw.stdout).value
                #expect(envelope.error.code == "teardown_refused")
                let error = CLIErrorClassifier.classify(envelope: envelope, diagnostics: Diagnostics(summary: ""),
                                                        context: CLIErrorClassifier.Context())
                if case .refused(let cliRefusal) = error, case .uncommittedChanges(let cliFiles) = cliRefusal.cause {
                    #expect(cliFiles.map(\.path) == ["notes.txt"])
                } else {
                    Issue.record("the CLI's refusal classified as \(error)")
                }
            } else {
                #expect(refusal.plan?.source == .appPreflight && refusal.diagnostics.invocation == nil,
                        "legacy refusals come from the app preflight: \(refusal.diagnostics)")
            }
            #expect(FileManager.default.fileExists(atPath: worktree.appendingPathComponent("notes.txt").path))
            #expect(repo.registry() == registryBefore, "a refusal must not change the registry")

            // The recovery the result card offers: discard exactly notes.txt, destructive, with a confirmation.
            let recoveries = RecoveryPlanner.recoveries(for: .refused(refusal), after: .teardown(request))
            let retry = try #require(recoveries.lazy.compactMap { action -> TeardownRequest? in
                if case .retry(.teardown(let retry), _, true, .some) = action { return retry }
                return nil
            }.first, "no destructive discard retry in \(recoveries)")
            #expect(retry.discard?.userFiles == ["notes.txt"])
            #expect(retry.branch == .keep)

            let outcome = try await cli.backend.teardownFeature(retry, progress: { _ in })
            #expect(outcome.worktreeGone)
            #expect(!FileManager.default.fileExists(atPath: worktree.path))
            if case .kept = outcome.branch {} else { Issue.record("Keep did not keep the branch: \(outcome.branch)") }
            #expect(try await repo.branches().contains(record.branchName))
            #expect(try await repo.worktrees().count == 1)
        }

        /// Delete-if-merged is blocked in the sheet for an unmerged branch (and refused by contract CLIs); Force-delete
        /// deletes it.
        @Test(.enabled(if: integrationEnabled), .timeLimit(.minutes(2)))
        func unmergedBranchBlocksDeleteIfMergedAndForceDeleteDeletesIt() async throws {
            let cli = try await LiveCLI.make()
            let repo = try await TempRepo.make(ignoring: TempRepo.branchBoxIgnores)
            defer { repo.remove() }
            let (summary, record) = try await cli.start("ahead", in: repo)
            let worktree = URL(fileURLWithPath: try #require(summary.worktreePath))
            try repo.write("work\n", to: "work.txt", in: worktree)
            try await repo.commitAll(in: worktree, message: "work")

            let feature = FeatureRef(project: repo.project, name: "ahead")
            let deleteIfMerged = TeardownRequest(feature: feature, recordedBranch: record.branchName, branch: .deleteIfMerged)
            let plan = try await cli.backend.planTeardown(deleteIfMerged)
            #expect(plan.branch?.merged == false && plan.branch?.ahead == 1, "\(String(describing: plan.branch))")
            var draft = TeardownDraft(feature: feature, recordedBranch: record.branchName, plan: plan, config: nil)
            #expect(draft.branch == .keep, "an unmerged branch defaults to Keep")
            #expect(draft.visibleBranchOptions.contains(.forceDelete))
            draft.branch = .deleteIfMerged
            #expect(draft.blockingReason != nil, "Delete-if-merged must be blocked for an unmerged branch")

            if cli.supports(.teardownUnmergedPreflight) {
                do {
                    _ = try await cli.backend.teardownFeature(deleteIfMerged, progress: { _ in })
                    Issue.record("the CLI tore down an unmerged branch under Delete-if-merged")
                } catch BackendError.refused(let refusal) {
                    if case .unmergedBranch(let branch, _) = refusal.cause {
                        #expect(branch == record.branchName)
                    } else {
                        Issue.record("expected unmergedBranch, got \(refusal.cause)")
                    }
                }
                #expect(FileManager.default.fileExists(atPath: worktree.path), "the refusal happens before removal")
            }

            draft.branch = .forceDelete
            #expect(draft.blockingReason == nil)
            let outcome = try await cli.backend.teardownFeature(draft.makeRequest(), progress: { _ in })
            #expect(outcome.worktreeGone)
            if case .deleted(let branch, _) = outcome.branch {
                #expect(branch == record.branchName)
            } else {
                Issue.record("Force-delete did not delete the branch: \(outcome.branch)")
            }
            #expect(try await repo.branches() == ["main"])
        }

        /// DRIFT-07: a feature started with `--branch-prefix spike` lives on `spike/<name>`, and teardown deletes that
        /// branch (not `feature/<name>`).
        @Test(.enabled(if: integrationEnabled), .timeLimit(.minutes(2)))
        func customBranchPrefixIsDeleted() async throws {
            let cli = try await LiveCLI.make()
            let repo = try await TempRepo.make(ignoring: TempRepo.branchBoxIgnores)
            defer { repo.remove() }
            let (_, record) = try await cli.start("zeta", in: repo, branchPrefix: "spike")
            #expect(record.branchName == "spike/zeta")
            #expect(try await repo.branches().contains("spike/zeta"))

            let request = TeardownRequest(feature: FeatureRef(project: repo.project, name: "zeta"),
                                          recordedBranch: record.branchName, branch: .deleteIfMerged)
            let outcome = try await cli.backend.teardownFeature(request, progress: { _ in })
            #expect(outcome.worktreeGone)
            if case .deleted(let branch, _) = outcome.branch {
                #expect(branch == "spike/zeta")
            } else {
                Issue.record("spike/zeta was not deleted: \(outcome.branch)")
            }
            #expect(try await repo.branches() == ["main"])
        }

        @Test(.enabled(if: integrationEnabled), .timeLimit(.minutes(2)))
        func resolveProjectNormalizesAFeatureWorktreeToMain() async throws {
            let cli = try await LiveCLI.make()
            let repo = try await TempRepo.make()
            defer { repo.remove() }
            let (summary, _) = try await cli.start("resolve", in: repo)
            let worktree = URL(fileURLWithPath: try #require(summary.worktreePath))

            let fromMain = try await cli.backend.resolveProject(at: repo.main)
            #expect(fromMain.project == repo.project && fromMain.normalization == .none)
            let fromFeature = try await cli.backend.resolveProject(at: worktree)
            #expect(fromFeature.project == repo.project && fromFeature.normalization == .fromFeatureWorktree)
            let fromSubfolder = worktree.appendingPathComponent("docs", isDirectory: true)
            try FileManager.default.createDirectory(at: fromSubfolder, withIntermediateDirectories: true)
            #expect(try await cli.backend.resolveProject(at: fromSubfolder).project == repo.project)
        }

        /// detect: legacy CLIs are parsed from text, contract CLIs from `detect --json`; both see a git repository.
        @Test(.enabled(if: integrationEnabled), .timeLimit(.minutes(2)))
        func detectReportsTheRepository() async throws {
            let cli = try await LiveCLI.make()
            let repo = try await TempRepo.make()
            defer { repo.remove() }
            let fresh = try await cli.backend.detect(repo.main)
            #expect(fresh.gitRepository)
            #expect(!fresh.initialized)
            #expect(fresh.stack != nil || fresh.adapter != nil, "\(fresh)")
            if cli.supports(.detectJSON) {
                #expect(fresh.project.map { URL(fileURLWithPath: $0).standardizedFileURL.path } == repo.main.standardizedFileURL.path)
            }
        }
    }
}

extension LiveCLI {
    func supports(_ capability: Capability) -> Bool { identity.supports(capability) }
}
