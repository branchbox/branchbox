import Foundation

/// Plans the Prune sheet: an app-orchestrated loop of ordinary teardowns, never `branchbox prune` (D-12).
///
/// Rows are the project's features that are not removed. Only the safe set is preselected; every other row says
/// why it was left out. Selected rows become plain `TeardownRequest`s: no row ever gets discard consent here (a
/// dirty row is only torn down after its own confirmation, passed in as `consents`).
public struct PrunePlanner: Sendable {
    public struct Row: Sendable, Hashable { public let feature: FeatureRecord; public let plan: TeardownPlanDocument?
        public var selected: Bool; public let defaultReason: String?
        public init(feature: FeatureRecord, plan: TeardownPlanDocument?, selected: Bool, defaultReason: String?) {
            self.feature = feature
            self.plan = plan
            self.selected = selected
            self.defaultReason = defaultReason
        }
    }

    /// Rows with the safe set preselected for the default batch policy, Keep.
    public static func rows(features: [FeatureRecord], plans: [String: TeardownPlanDocument]) -> [Row] {  // safe set preselected
        rows(features: features, plans: plans, policy: .keep)
    }

    /// Rows with the safe set for `policy` preselected. A row is safe when tearing it down loses nothing:
    /// - its worktree has no uncommitted changes of the user's (or is already gone), as read by a complete,
    ///   readable plan, and it is neither locked nor still being set up;
    /// - and its branch survives or holds nothing unmerged: the policy is Keep, the branch is merged or missing,
    ///   or the feature is failed_retained or orphaned (a cleanup, whose unmerged branch Delete-if-merged keeps).
    /// Force-delete never preselects a row with unmerged commits.
    public static func rows(features: [FeatureRecord], plans: [String: TeardownPlanDocument], policy: BranchPolicy) -> [Row] {
        features.filter { $0.status != .removed }.map { feature in
            let plan = plans[feature.workFeature]
            let reason = exclusionReason(feature: feature, plan: plan, policy: policy)
            return Row(feature: feature, plan: plan, selected: reason == nil, defaultReason: reason)
        }
    }

    public static func selection(project: ProjectRef, rows: [Row], policy: BranchPolicy, completeSpec: Bool) -> PruneSelection {
        selection(project: project, rows: rows, policy: policy, completeSpec: completeSpec, consents: [:])
    }

    /// The teardowns to run, in row order. `consents` holds the per-row discard consent the sheet's popover
    /// collected for dirty rows, keyed by feature name; it is only attached to selected rows.
    public static func selection(project: ProjectRef, rows: [Row], policy: BranchPolicy, completeSpec: Bool,
                                 consents: [String: DiscardConsent]) -> PruneSelection {
        let requests = rows.filter(\.selected).map { row in
            let name = row.feature.workFeature
            var request = TeardownRequest(feature: FeatureRef(project: project, name: name),
                                          recordedBranch: row.feature.branchName.isEmpty ? nil : row.feature.branchName,
                                          branch: branchPolicy(for: row, batchPolicy: policy))
            request.completeSpec = completeSpec
            request.discard = consents[name]
            // A worktree that is already gone has nothing to discard; removal is forced so the registry entry is
            // cleaned up (§6.5 forceRemoval guard: `worktree.exists == false`).
            request.forceRemoval = row.plan?.worktree.exists == false
            return request
        }
        return PruneSelection(project: project, rows: requests)
    }

    /// The policy a row is torn down with under the batch policy: Delete-if-merged keeps an unmerged branch
    /// instead of having the row refused.
    public static func branchPolicy(for row: Row, batchPolicy: BranchPolicy) -> BranchPolicy {
        guard batchPolicy == .deleteIfMerged, let plan = row.plan, TeardownDraft.branchIsUnmerged(plan) else {
            return batchPolicy
        }
        return .keep
    }

    /// Why a row is not preselected, or nil when it is safe.
    static func exclusionReason(feature: FeatureRecord, plan: TeardownPlanDocument?, policy: BranchPolicy) -> String? {
        if feature.setup?.state == .inProgress { return "Still being set up" }
        guard let plan else { return "Not checked for unsaved work yet" }
        if plan.droppedBlockers > 0 || plan.blockers.contains(where: { !TeardownDraft.knownBlockerKinds.contains($0.kind) }) {
            return "BranchBox reported a problem this app version can't read"
        }
        if plan.worktree.exists {
            if plan.worktree.locked {
                return plan.worktree.lockReason.map { "The worktree is locked: \($0)" } ?? "The worktree is locked"
            }
            if !plan.changes.statusAvailable {
                let cause = plan.blockers.first { $0.kind == "status_unavailable" }?.cause
                return cause.map { "Couldn't check for unsaved work: \($0)" } ?? "Couldn't check for unsaved work"
            }
            let changes = plan.changes.user.count
            if changes > 0 {
                let counted = changes == 1 ? "1 uncommitted change" : "\(changes) uncommitted changes"
                return plan.changes.truncated ? "More than \(counted)" : counted
            }
        }
        guard TeardownDraft.branchIsUnmerged(plan), let branch = plan.branch else { return nil }
        switch policy {
        case .keep:
            return nil
        case .forceDelete:
            return "\(TeardownDraft.unmergedSummary(branch)); Force-delete would lose them"
        case .deleteIfMerged:
            let isCleanup = feature.status == .failedRetained || feature.status == .orphaned
            return isCleanup ? nil : "\(TeardownDraft.unmergedSummary(branch)); its branch would be kept"
        }
    }
}
