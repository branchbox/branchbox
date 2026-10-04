import Foundation

/// The Teardown sheet's choices for one feature, derived from its teardown plan (D-11, §6.5).
///
/// The draft never carries consent: `makeRequest()` always has `discard == nil` and `forceRemoval == false`.
/// Discarding changes or forcing removal only happens through a `RecoveryAction` built from a refusal.
public struct TeardownDraft: Sendable, Hashable {
    public let feature: FeatureRef
    public let recordedBranch: String?
    public let plan: TeardownPlanDocument
    public let config: ProjectConfig?
    public var branch: BranchPolicy                  // default: deleteBranchByDefault ? .deleteIfMerged : .keep; .keep if unmerged
    public var completeSpec: Bool

    /// Blocker kinds this app version knows how to explain and recover from (§5.5).
    static let knownBlockerKinds: Set<String> = [
        "uncommitted_changes", "unmerged_branch", "worktree_locked", "status_unavailable", "worktree_removal_failed",
    ]

    public init(feature: FeatureRef, recordedBranch: String?, plan: TeardownPlanDocument, config: ProjectConfig?) {
        self.feature = feature
        self.recordedBranch = recordedBranch
        self.plan = plan
        self.config = config
        self.completeSpec = false
        let deleteByDefault = config?.deleteBranchByDefault ?? plan.defaults?.deleteBranchByDefault
            ?? ProjectConfig.defaults.deleteBranchByDefault
        // Force-delete is never preselected, and an unmerged branch is never put up for deletion by default:
        // Delete-if-merged would only be refused for it.
        if Self.branchIsUnmerged(plan) || !Self.branchExists(plan) {
            self.branch = .keep
        } else {
            self.branch = deleteByDefault ? .deleteIfMerged : .keep
        }
    }

    /// Keep; Delete if merged while there is a branch to delete; Force-delete only for an existing unmerged branch.
    public var visibleBranchOptions: [BranchPolicy] {  // .forceDelete only when the branch exists and is unmerged
        guard Self.branchExists(plan) else { return [.keep] }
        return Self.branchIsUnmerged(plan) ? [.keep, .deleteIfMerged, .forceDelete] : [.keep, .deleteIfMerged]
    }

    /// Why Tear Down is disabled, or nil. Delete-if-merged on an unmerged branch is blocked here (the backend
    /// would refuse it), and so is a plan carrying a blocker this app version cannot read.
    public var blockingReason: String? {             // e.g. "feature/x has 3 commits not in main; choose Keep or Force-delete"
        if plan.droppedBlockers > 0 {
            return "BranchBox reported a problem with this teardown that this app version can't read; update BranchBox for Mac"
        }
        if let unknown = plan.blockers.first(where: { !Self.knownBlockerKinds.contains($0.kind) }) {
            return unknown.message.isEmpty ? "BranchBox reported a “\(unknown.kind)” problem with this teardown" : unknown.message
        }
        if branch == .deleteIfMerged, Self.branchIsUnmerged(plan), let branchInfo = plan.branch {
            return "\(Self.unmergedSummary(branchInfo)); choose Keep or Force-delete"
        }
        return nil
    }

    /// Set when the worktree has uncommitted changes of the user's. The first attempt is refused for them and the
    /// sheet then asks for consent to discard exactly those files.
    public var pendingDiscardWarning: String? {      // "2 uncommitted changes — you'll be asked to confirm discarding them"
        let count = plan.changes.user.count
        guard count > 0 else { return nil }
        let changes = count == 1 ? "1 uncommitted change" : "\(count) uncommitted changes"
        let lead = plan.changes.truncated ? "More than \(changes)" : changes
        return "\(lead) — you'll be asked to confirm discarding them"
    }

    /// Applies the branch policy a caller asked for (`WindowIntent.teardown(_, preselect:)`) when it is one of the
    /// visible options and is not Force-delete, which the user must always pick themselves. Delete-if-merged is not
    /// applied to an unmerged branch.
    public mutating func preselect(_ policy: BranchPolicy?) {
        guard let policy, policy != .forceDelete, visibleBranchOptions.contains(policy) else { return }
        if policy == .deleteIfMerged, Self.branchIsUnmerged(plan) { return }
        branch = policy
    }

    public func makeRequest() -> TeardownRequest {     // discard == nil, forceRemoval == false — always
        var request = TeardownRequest(feature: feature, recordedBranch: recordedBranch, branch: branch)
        request.completeSpec = completeSpec
        return request
    }

    static func branchExists(_ plan: TeardownPlanDocument) -> Bool {
        // No branch facts at all (an older plan) reads as "maybe": the options and the backend's own checks stay.
        plan.branch?.exists ?? true
    }

    static func branchIsUnmerged(_ plan: TeardownPlanDocument) -> Bool {
        guard let branch = plan.branch else { return false }
        return branch.exists && !branch.merged
    }

    /// "feature/x has 3 commits not in main".
    static func unmergedSummary(_ branch: TeardownPlanDocument.Branch) -> String {
        let reference = branch.referenceName.isEmpty ? (branch.upstream ?? "its base") : branch.referenceName
        switch branch.ahead {
        case ...0: return "\(branch.name) isn't merged into \(reference)"
        case 1: return "\(branch.name) has 1 commit not in \(reference)"
        default: return "\(branch.name) has \(branch.ahead) commits not in \(reference)"
        }
    }
}
