import BranchBoxKit
import Foundation
import Testing

@Suite struct PrunePlannerTests {
    struct RowCase: Sendable, CustomTestStringConvertible {
        let name: String
        let record: FeatureRecord
        let plan: TeardownPlanDocument?
        let policy: BranchPolicy
        let selected: Bool
        let reason: String?
        var testDescription: String { name }
    }

    private static let unmerged = Sample.branch(merged: false, ahead: 3)

    static let rowCases: [RowCase] = [
        RowCase(name: "clean merged, keep", record: Sample.record(), plan: Sample.plan(), policy: .keep,
                selected: true, reason: nil),
        RowCase(name: "clean unmerged, keep", record: Sample.record(), plan: Sample.plan(branch: unmerged), policy: .keep,
                selected: true, reason: nil),
        RowCase(name: "clean unmerged, delete if merged", record: Sample.record(), plan: Sample.plan(branch: unmerged),
                policy: .deleteIfMerged, selected: false,
                reason: "feature/eta has 3 commits not in main; its branch would be kept"),
        RowCase(name: "clean merged, delete if merged", record: Sample.record(), plan: Sample.plan(), policy: .deleteIfMerged,
                selected: true, reason: nil),
        RowCase(name: "clean unmerged, force delete", record: Sample.record(), plan: Sample.plan(branch: unmerged),
                policy: .forceDelete, selected: false,
                reason: "feature/eta has 3 commits not in main; Force-delete would lose them"),
        RowCase(name: "clean merged, force delete", record: Sample.record(), plan: Sample.plan(), policy: .forceDelete,
                selected: true, reason: nil),
        RowCase(name: "branch gone, force delete", record: Sample.record(),
                plan: Sample.plan(branch: Sample.branch(exists: false, merged: false)), policy: .forceDelete,
                selected: true, reason: nil),
        RowCase(name: "one user change", record: Sample.record(), plan: Sample.plan(user: [Sample.changed("a.txt")]),
                policy: .keep, selected: false, reason: "1 uncommitted change"),
        RowCase(name: "truncated changes", record: Sample.record(),
                plan: Sample.plan(truncated: true, user: [Sample.changed("a"), Sample.changed("b")]), policy: .keep,
                selected: false, reason: "Too many changes to list safely; review this feature in Tear Down instead"),
        RowCase(name: "unchecked", record: Sample.record(), plan: nil, policy: .keep, selected: false,
                reason: "Not checked for unsaved work yet"),
        RowCase(name: "status unreadable", record: Sample.record(),
                plan: Sample.plan(statusAvailable: false,
                                  blockers: [.init(kind: "status_unavailable", message: "x", cause: "index corrupt")]),
                policy: .keep, selected: false, reason: "Couldn't check for unsaved work: index corrupt"),
        RowCase(name: "status unreadable, no cause", record: Sample.record(), plan: Sample.plan(statusAvailable: false),
                policy: .keep, selected: false, reason: "Couldn't check for unsaved work"),
        RowCase(name: "locked", record: Sample.record(), plan: Sample.plan(locked: true, lockReason: "agent run"),
                policy: .keep, selected: false, reason: "The worktree is locked: agent run"),
        RowCase(name: "locked, no reason", record: Sample.record(), plan: Sample.plan(locked: true), policy: .keep,
                selected: false, reason: "The worktree is locked"),
        RowCase(name: "unknown blocker", record: Sample.record(),
                plan: Sample.plan(blockers: [.init(kind: "quota_exceeded", message: "x")]), policy: .keep, selected: false,
                reason: "BranchBox reported a problem this app version can't read"),
        RowCase(name: "setup still running", record: Sample.record(setup: .inProgress), plan: Sample.plan(), policy: .keep,
                selected: false, reason: "Still being set up"),
        RowCase(name: "interrupted, clean", record: Sample.record(setup: .interrupted), plan: Sample.plan(), policy: .keep,
                selected: true, reason: nil),
        RowCase(name: "folder already gone (unreadable status)", record: Sample.record(status: .orphaned),
                plan: Sample.plan(exists: false, statusAvailable: false), policy: .keep, selected: true, reason: nil),
        RowCase(name: "failed_retained unmerged, delete if merged", record: Sample.record(status: .failedRetained, provider: .sbx),
                plan: Sample.plan(branch: unmerged), policy: .deleteIfMerged, selected: true, reason: nil),
        RowCase(name: "orphaned unmerged, delete if merged", record: Sample.record(status: .orphaned),
                plan: Sample.plan(branch: unmerged), policy: .deleteIfMerged, selected: true, reason: nil),
        RowCase(name: "orphaned unmerged, force delete", record: Sample.record(status: .orphaned),
                plan: Sample.plan(branch: unmerged), policy: .forceDelete, selected: false,
                reason: "feature/eta has 3 commits not in main; Force-delete would lose them"),
        RowCase(name: "failed_retained dirty", record: Sample.record(status: .failedRetained, provider: .sbx),
                plan: Sample.plan(user: [Sample.changed("a.txt")]), policy: .keep, selected: false,
                reason: "1 uncommitted change"),
    ]

    @Test(arguments: PrunePlannerTests.rowCases)
    func safeSetPreselection(_ testCase: RowCase) throws {
        let plans = testCase.plan.map { [testCase.record.workFeature: $0] } ?? [:]
        let rows = PrunePlanner.rows(features: [testCase.record], plans: plans, policy: testCase.policy)
        let row = try #require(rows.first)
        #expect(row.selected == testCase.selected)
        #expect(row.defaultReason == testCase.reason)
        #expect(row.plan == testCase.plan)
        if testCase.policy == .keep {
            #expect(PrunePlanner.rows(features: [testCase.record], plans: plans) == rows)
        }
    }

    @Test func rowsSkipRemovedFeaturesAndKeepOrder() throws {
        let features = try ["prine", "remotion", "coding-agents", "milestone2"].map {
            try Sample.fixtureRecord($0, in: "main_feature_list_all.json")
        }
        let plans = Dictionary(uniqueKeysWithValues: features.map { ($0.workFeature, Sample.plan(name: $0.workFeature)) })
        let rows = PrunePlanner.rows(features: features, plans: plans)
        #expect(rows.map(\.feature.workFeature) == ["prine", "remotion"])
        #expect(rows.allSatisfy { $0.selected })
    }

    @Test func selectionBuildsPlainTeardownsForSelectedRows() {
        let features = [Sample.record("alpha"), Sample.record("beta"), Sample.record("gamma"),
                        Sample.record("delta", status: .orphaned), Sample.record("bare", branch: "")]
        let plans: [String: TeardownPlanDocument] = [
            "alpha": Sample.plan(name: "alpha"),
            "beta": Sample.plan(name: "beta", user: [Sample.changed("x")]),
            "gamma": Sample.plan(name: "gamma", branch: Sample.branch("feature/gamma", merged: false, ahead: 1)),
            "delta": Sample.plan(name: "delta", exists: false, statusAvailable: false),
            "bare": Sample.plan(name: "bare"),
        ]
        var rows = PrunePlanner.rows(features: features, plans: plans)
        #expect(rows.map(\.selected) == [true, false, true, true, true])

        let selection = PrunePlanner.selection(project: Sample.project, rows: rows, policy: .deleteIfMerged, completeSpec: true)
        #expect(selection.project == Sample.project)
        #expect(selection.rows.map(\.feature.name) == ["alpha", "gamma", "delta", "bare"])
        #expect(selection.rows.allSatisfy { $0.discard == nil && $0.completeSpec })
        #expect(selection.rows.map(\.branch) == [.deleteIfMerged, .keep, .deleteIfMerged, .deleteIfMerged])
        #expect(selection.rows.map(\.forceRemoval) == [false, false, true, false])
        #expect(selection.rows.map(\.recordedBranch) == ["feature/alpha", "feature/gamma", "feature/delta", nil])

        // The user ticks the dirty row and confirms its files in the popover.
        rows[1].selected = true
        let consent = DiscardConsent(userFiles: ["x"])
        let withConsent = PrunePlanner.selection(project: Sample.project, rows: rows, policy: .keep, completeSpec: false,
                                                 consents: ["beta": consent, "omega": consent])
        #expect(withConsent.rows.map(\.feature.name) == ["alpha", "beta", "gamma", "delta", "bare"])
        #expect(withConsent.rows.map(\.discard) == [nil, consent, nil, nil, nil])
        #expect(withConsent.rows.allSatisfy { $0.branch == .keep })

        // Without consent a ticked dirty row is still a plain teardown, which the backend refuses on its own.
        let plain = PrunePlanner.selection(project: Sample.project, rows: rows, policy: .forceDelete, completeSpec: false)
        #expect(plain.rows.allSatisfy { $0.discard == nil && $0.branch == .forceDelete })
    }

    @Test func emptyProjectHasNothingToPrune() {
        #expect(PrunePlanner.rows(features: [], plans: [:]).isEmpty)
        #expect(PrunePlanner.selection(project: Sample.project, rows: [], policy: .keep, completeSpec: false).rows.isEmpty)
    }
}
