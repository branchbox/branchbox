import BranchBoxCLI
import BranchBoxKit
import Foundation
import Testing

// VER-1 prune planning (DESIGN §13.2, manual loop step 8): three features, one with an untracked file. Each row's
// teardown plan comes from the real CLI (dry run or app preflight); PrunePlanner leaves the dirty row unchecked,
// and running the selection removes the two clean ones and keeps the dirty one.

extension RealCLI {
    @Suite struct PrunePlanningTests {
        @Test(.enabled(if: integrationEnabled), .timeLimit(.minutes(3)))
        func planPruneUnchecksTheDirtyRow() async throws {
            let cli = try await LiveCLI.make()
            let repo = try await TempRepo.make(ignoring: TempRepo.branchBoxIgnores)
            defer { repo.remove() }
            for name in ["prune-a", "prune-b", "prune-dirty"] { try await cli.start(name, in: repo) }
            try repo.write("draft\n", to: "draft.txt", in: repo.worktree("prune-dirty"))

            let features = try await cli.backend.listFeatures(in: repo.project, includeRemoved: false).features
            #expect(features.count == 3)
            var plans: [String: TeardownPlanDocument] = [:]
            for feature in features {
                let request = TeardownRequest(feature: FeatureRef(project: repo.project, name: feature.workFeature),
                                              recordedBranch: feature.branchName, branch: .deleteIfMerged)
                plans[feature.workFeature] = try await cli.backend.planTeardown(request)
            }
            #expect(plans["prune-dirty"]?.changes.user.map(\.path) == ["draft.txt"])

            let rows = PrunePlanner.rows(features: features, plans: plans, policy: .deleteIfMerged)
            let selected = Set(rows.filter(\.selected).map(\.feature.workFeature))
            #expect(selected == ["prune-a", "prune-b"], "\(rows.map { ($0.feature.workFeature, $0.selected, $0.defaultReason) })")
            #expect(rows.first { $0.feature.workFeature == "prune-dirty" }?.defaultReason != nil)

            let selection = PrunePlanner.selection(project: repo.project, rows: rows, policy: .deleteIfMerged, completeSpec: false)
            #expect(selection.rows.allSatisfy { $0.discard == nil && !$0.forceRemoval }, "a prune never discards on its own")
            for row in selection.rows {
                let outcome = try await cli.backend.teardownFeature(row, progress: { _ in })
                #expect(outcome.worktreeGone, "\(row.feature.name)")
            }
            let after = try await cli.backend.listFeatures(in: repo.project, includeRemoved: false)
            #expect(after.features.map(\.workFeature) == ["prune-dirty"])
            #expect(FileManager.default.fileExists(atPath: repo.worktree("prune-dirty").appendingPathComponent("draft.txt").path))
            #expect(Set(try await repo.branches()) == ["main", "feature/prune-dirty"])
        }
    }
}
