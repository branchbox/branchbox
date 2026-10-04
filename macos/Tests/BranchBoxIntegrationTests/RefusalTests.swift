import BranchBoxCLI
import BranchBoxKit
import Foundation
import Testing

// VER-1 refusals (DESIGN §13.2): a duplicate start, and a project without BranchBox's `.gitignore` entries, whose
// generated files 0.13.x refuses to remove on its own.

extension RealCLI {
    @Suite struct RefusalTests {
        @Test(.enabled(if: integrationEnabled), .timeLimit(.minutes(2)))
        func duplicateStartIsWorktreeExists() async throws {
            let cli = try await LiveCLI.make()
            let repo = try await TempRepo.make()
            defer { repo.remove() }
            try await cli.start("dup", in: repo)

            let request = LiveCLI.minimalStart("dup", in: repo.project)
            do {
                _ = try await cli.backend.startFeature(request, progress: { _ in })
                Issue.record("a second start of the same name succeeded (\(cli.mode))")
            } catch BackendError.refused(let refusal) {
                guard case .worktreeExists(let path) = refusal.cause else {
                    Issue.record("expected worktreeExists, got \(refusal.cause): \(refusal.message)")
                    return
                }
                #expect(URL(fileURLWithPath: path).lastPathComponent == "dup", "\(path)")
                #expect(!RecoveryPlanner.recoveries(for: .refused(refusal), after: .start(request)).isEmpty)
            }
            let listing = try await cli.backend.listFeatures(in: repo.project, includeRemoved: true)
            #expect(listing.features.filter { $0.workFeature == "dup" }.count == 1)
            #expect(listing.strays.isEmpty)
        }

        /// No BranchBox `.gitignore` entries, so a fresh feature's generated env files are untracked. The app tears
        /// it down in one attempt in both modes (legacy: the generated-only `--force` of the wave-2 deviations). The
        /// refusal 0.13.x prints when it removes the worktree itself (what a change racing the app's re-plan
        /// produces) is classified as `.moduleFilesDirty`, and the generated-only recovery removes the worktree.
        @Test(.enabled(if: integrationEnabled), .timeLimit(.minutes(2)))
        func projectWithoutIgnoresTearsDownAndModuleFilesRecoveryWorks() async throws {
            let cli = try await LiveCLI.make()
            let repo = try await TempRepo.make()
            defer { repo.remove() }

            // 1. The app's own teardown: one attempt, no refusal round trip, no manual-removal fallback.
            let (firstSummary, first) = try await cli.start("plain", in: repo)
            let firstRequest = TeardownRequest(feature: FeatureRef(project: repo.project, name: "plain"),
                                               recordedBranch: first.branchName, branch: .keep)
            let plan = try await cli.backend.planTeardown(firstRequest)
            #expect(plan.changes.user.isEmpty, "generated files are not user changes: \(plan.changes.user)")
            #expect(!plan.changes.generated.isEmpty, "a project without ignores has untracked generated files")
            let outcome = try await cli.backend.teardownFeature(firstRequest, progress: { _ in })
            #expect(outcome.worktreeGone)
            let firstWorktree = try #require(firstSummary.worktreePath)
            #expect(!FileManager.default.fileExists(atPath: firstWorktree))
            #expect(!outcome.summary.warnings.contains { $0.contains("removed manually") }, "\(outcome.summary.warnings)")

            // 2. 0.13.x's own refusal and the app's recovery from it. Contract CLIs classify generated files
            //    themselves and never print this refusal.
            guard !cli.isContract else { return }
            let (summary, second) = try await cli.start("racy", in: repo)
            let worktree = try #require(summary.worktreePath)
            let raw = try await cli.run(["feature", "teardown", "racy", "--repo", repo.main.path, "--json", "--keep-branch"],
                                        in: repo.main)
            #expect(raw.termination != .exited(0), "0.13.x removed generated files without --force")
            #expect(FileManager.default.fileExists(atPath: worktree), "the refusal happens before removal")
            let error = CLIErrorClassifier.classify(
                stderr: raw.stderrTail, stdout: String(decoding: raw.stdout, as: UTF8.self),
                diagnostics: Diagnostics(summary: "feature teardown failed"),
                context: CLIErrorClassifier.Context(registryPath: repo.main.appendingPathComponent(".branchbox/registry.json").path))
            guard case .refused(let refusal) = error, case .moduleFilesDirty(let files, _) = refusal.cause else {
                Issue.record("0.13.x's dirty-module refusal was not classified as moduleFilesDirty: \(error)")
                return
            }
            #expect(!files.isEmpty)

            let request = TeardownRequest(feature: FeatureRef(project: repo.project, name: "racy"),
                                          recordedBranch: second.branchName, branch: .keep)
            let recoveries = RecoveryPlanner.recoveries(for: error, after: .teardown(request))
            let retry = try #require(recoveries.lazy.compactMap { action -> TeardownRequest? in
                if case .retry(.teardown(let retry), _, _, _) = action, retry.discard != nil { return retry }
                return nil
            }.first, "no generated-files recovery in \(recoveries)")
            #expect(retry.discard?.userFiles == [], "only BranchBox-generated files are discarded")
            let recovered = try await cli.backend.teardownFeature(retry, progress: { _ in })
            #expect(recovered.worktreeGone)
            #expect(try await repo.worktrees().count == 1)
        }
    }
}
