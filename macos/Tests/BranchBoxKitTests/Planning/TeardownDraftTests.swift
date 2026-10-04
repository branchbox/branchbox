import BranchBoxKit
import Foundation
import Testing

@Suite struct TeardownDraftTests {
    struct DefaultCase: Sendable, CustomTestStringConvertible {
        let name: String
        let deleteByDefault: Bool?                   // nil: no project config loaded
        let planDefaultDelete: Bool?                 // the plan's own `defaults`, used without a config
        let branch: TeardownPlanDocument.Branch?
        let expected: BranchPolicy
        let options: [BranchPolicy]
        var testDescription: String { name }
    }

    static let defaultCases: [DefaultCase] = [
        DefaultCase(name: "merged, config deletes", deleteByDefault: true, planDefaultDelete: nil,
                    branch: Sample.branch(merged: true), expected: .deleteIfMerged, options: [.keep, .deleteIfMerged]),
        DefaultCase(name: "merged, config keeps", deleteByDefault: false, planDefaultDelete: nil,
                    branch: Sample.branch(merged: true), expected: .keep, options: [.keep, .deleteIfMerged]),
        DefaultCase(name: "unmerged, config deletes", deleteByDefault: true, planDefaultDelete: nil,
                    branch: Sample.branch(merged: false, ahead: 3), expected: .keep,
                    options: [.keep, .deleteIfMerged, .forceDelete]),
        DefaultCase(name: "unmerged, config keeps", deleteByDefault: false, planDefaultDelete: nil,
                    branch: Sample.branch(merged: false, ahead: 1), expected: .keep,
                    options: [.keep, .deleteIfMerged, .forceDelete]),
        DefaultCase(name: "branch already gone", deleteByDefault: true, planDefaultDelete: nil,
                    branch: Sample.branch(exists: false, merged: false), expected: .keep, options: [.keep]),
        DefaultCase(name: "no branch facts, config deletes", deleteByDefault: true, planDefaultDelete: nil,
                    branch: nil, expected: .deleteIfMerged, options: [.keep, .deleteIfMerged]),
        DefaultCase(name: "no config, plan defaults keep", deleteByDefault: nil, planDefaultDelete: false,
                    branch: Sample.branch(merged: true), expected: .keep, options: [.keep, .deleteIfMerged]),
        DefaultCase(name: "no config, plan defaults delete", deleteByDefault: nil, planDefaultDelete: true,
                    branch: Sample.branch(merged: true), expected: .deleteIfMerged, options: [.keep, .deleteIfMerged]),
        DefaultCase(name: "no config, no plan defaults (core default deletes)", deleteByDefault: nil, planDefaultDelete: nil,
                    branch: Sample.branch(merged: true), expected: .deleteIfMerged, options: [.keep, .deleteIfMerged]),
    ]

    private static func draft(_ testCase: DefaultCase, user: [ChangedFile] = []) -> TeardownDraft {
        let config = testCase.deleteByDefault.map { ProjectConfig(deleteBranchByDefault: $0) }
        let defaults = testCase.planDefaultDelete.map {
            TeardownPlanDocument.Defaults(deleteBranchByDefault: $0, forceDeleteUnmergedByDefault: true)
        }
        return TeardownDraft(feature: Sample.feature, recordedBranch: "feature/eta",
                             plan: Sample.plan(user: user, branch: testCase.branch, defaults: defaults), config: config)
    }

    @Test(arguments: TeardownDraftTests.defaultCases)
    func defaultBranchPolicyAndOptions(_ testCase: DefaultCase) {
        let draft = Self.draft(testCase)
        #expect(draft.branch == testCase.expected)
        #expect(draft.visibleBranchOptions == testCase.options)
        #expect(draft.branch != .forceDelete)
        #expect(draft.blockingReason == nil)
    }

    /// Even a config that force-deletes unmerged branches by default never preselects Force-delete.
    @Test func forceDeleteIsNeverPreselected() {
        let config = ProjectConfig(deleteBranchByDefault: true, forceDeleteUnmergedByDefault: true)
        let plan = Sample.plan(branch: Sample.branch(merged: false, ahead: 2),
                               defaults: .init(deleteBranchByDefault: true, forceDeleteUnmergedByDefault: true))
        var draft = TeardownDraft(feature: Sample.feature, recordedBranch: "feature/eta", plan: plan, config: config)
        #expect(draft.branch == .keep)
        draft.preselect(.forceDelete)
        #expect(draft.branch == .keep)
        draft.preselect(.deleteIfMerged)                 // refused for an unmerged branch too
        #expect(draft.branch == .keep)
    }

    @Test func preselectAppliesVisibleSafeChoices() {
        var draft = Self.draft(Self.defaultCases[0])
        draft.preselect(.keep)
        #expect(draft.branch == .keep)
        draft.preselect(nil)
        #expect(draft.branch == .keep)
        draft.preselect(.deleteIfMerged)
        #expect(draft.branch == .deleteIfMerged)
        draft.preselect(.forceDelete)                    // not even visible for a merged branch
        #expect(draft.branch == .deleteIfMerged)
    }

    @Test func deleteIfMergedOnUnmergedBranchBlocks() {
        var draft = Self.draft(Self.defaultCases[2])
        #expect(draft.blockingReason == nil)
        draft.branch = .deleteIfMerged
        #expect(draft.blockingReason == "feature/eta has 3 commits not in main; choose Keep or Force-delete")
        draft.branch = .forceDelete
        #expect(draft.blockingReason == nil)
        draft.branch = .keep
        #expect(draft.blockingReason == nil)
    }

    @Test(arguments: [(0, "feature/eta isn't merged into main; choose Keep or Force-delete"),
                      (1, "feature/eta has 1 commit not in main; choose Keep or Force-delete")])
    func blockingReasonCountsCommits(ahead: Int, expected: String) {
        let plan = Sample.plan(branch: Sample.branch(merged: false, ahead: ahead))
        var draft = TeardownDraft(feature: Sample.feature, recordedBranch: "feature/eta", plan: plan, config: nil)
        draft.branch = .deleteIfMerged
        #expect(draft.blockingReason == expected)
    }

    @Test func unreadableBlockersBlock() throws {
        let dropped = try Sample.decodePlan(#"""
            {"work_feature":"eta","worktree":{"path":"/r/eta","exists":true},
             "changes":{"status_available":true,"user":[]},"blockers":[{"message":"no kind"}]}
            """#)
        #expect(dropped.droppedBlockers == 1)
        let draft = TeardownDraft(feature: Sample.feature, recordedBranch: nil, plan: dropped, config: nil)
        #expect(draft.blockingReason?.contains("can't read") == true)

        let unknownKind = Sample.plan(blockers: [.init(kind: "quota_exceeded", message: "Disk quota exceeded")])
        #expect(TeardownDraft(feature: Sample.feature, recordedBranch: nil, plan: unknownKind, config: nil).blockingReason
            == "Disk quota exceeded")
        let unknownSilent = Sample.plan(blockers: [.init(kind: "quota_exceeded", message: "")])
        #expect(TeardownDraft(feature: Sample.feature, recordedBranch: nil, plan: unknownSilent, config: nil).blockingReason
            == "BranchBox reported a “quota_exceeded” problem with this teardown")
    }

    @Test(arguments: [
        ([Sample.changed("a.txt")], false, "1 uncommitted change — you'll be asked to confirm discarding them"),
        ([Sample.changed("a.txt"), Sample.changed("b.txt", "untracked")], false,
         "2 uncommitted changes — you'll be asked to confirm discarding them"),
        ([Sample.changed("a.txt"), Sample.changed("b.txt")], true,
         "More than 2 uncommitted changes — you'll be asked to confirm discarding them"),
    ])
    func pendingDiscardWarning(user: [ChangedFile], truncated: Bool, expected: String) {
        let plan = Sample.plan(truncated: truncated, user: user)
        let draft = TeardownDraft(feature: Sample.feature, recordedBranch: "feature/eta", plan: plan, config: nil)
        #expect(draft.pendingDiscardWarning == expected)
        #expect(draft.blockingReason == nil)             // dirt is handled by the refusal flow, not by blocking
    }

    @Test func cleanPlanHasNoDiscardWarning() {
        #expect(Self.draft(Self.defaultCases[0]).pendingDiscardWarning == nil)
    }

    /// The first attempt never carries consent or forced removal, whatever the plan and the user chose.
    @Test(arguments: TeardownDraftTests.defaultCases)
    func makeRequestNeverCarriesConsent(_ testCase: DefaultCase) {
        let dirty = [Sample.changed("README.md"), Sample.changed("notes.txt", "untracked")]
        for user in [[], dirty] {
            for completeSpec in [false, true] {
                var draft = Self.draft(testCase, user: user)
                draft.completeSpec = completeSpec
                for policy in draft.visibleBranchOptions + [.forceDelete] {
                    draft.branch = policy
                    let request = draft.makeRequest()
                    #expect(request.discard == nil)
                    #expect(request.forceRemoval == false)
                    #expect(request.branch == policy)
                    #expect(request.completeSpec == completeSpec)
                    #expect(request.feature == Sample.feature)
                    #expect(request.recordedBranch == "feature/eta")
                }
            }
        }
    }

    @Test func lockedAndUnreadableWorktreesStillMakePlainRequests() {
        for plan in [Sample.plan(locked: true, lockReason: "in use"), Sample.plan(statusAvailable: false),
                     Sample.plan(exists: false)] {
            let request = TeardownDraft(feature: Sample.feature, recordedBranch: nil, plan: plan, config: nil).makeRequest()
            #expect(request.discard == nil)
            #expect(request.forceRemoval == false)
        }
    }

    @Test func designPlanExample() throws {
        let plan = try Sample.decodePlan(Sample.designPlanJSON)
        let draft = TeardownDraft(feature: Sample.feature, recordedBranch: "feature/eta", plan: plan, config: .defaults)
        #expect(draft.branch == .keep)
        #expect(draft.visibleBranchOptions == [.keep, .deleteIfMerged, .forceDelete])
        #expect(draft.pendingDiscardWarning == "2 uncommitted changes — you'll be asked to confirm discarding them")
        #expect(draft.blockingReason == nil)
        #expect(draft.makeRequest().discard == nil)
    }
}
