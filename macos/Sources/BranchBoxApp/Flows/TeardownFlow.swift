import BranchBoxKit
import BranchBoxStores
import Foundation
import Observation

/// The Teardown sheet's state for one presentation: the plan ("Checking for unsaved work…"), the
/// `TeardownDraft` built from it, and the attempts.
///
/// Safety (D-11): the first attempt is `draft.makeRequest()`, which never carries discard consent or forced
/// removal. Consent only comes from a `RecoveryAction` that `RecoveryPlanner` built from a refusal, after the
/// user confirmed the files it lists. Force-delete is only chosen after its own confirmation.
@MainActor @Observable final class TeardownFlow {
    enum PlanState: Equatable {
        case loading
        case failed(BackendError)
        case ready
    }

    let model: AppModel
    let feature: FeatureRef
    let preselect: BranchPolicy?
    private(set) var planState: PlanState = .loading
    private(set) var draft: TeardownDraft?
    /// The latest attempt (first attempt or a recovery's retry).
    private(set) var record: OperationRecord?
    /// Earlier attempts in this presentation, oldest first.
    private(set) var earlierAttempts: [OperationRecord] = []
    /// A [Force-delete Branch…] after the teardown.
    private(set) var branchRecord: OperationRecord?
    private(set) var dispatchError: String?
    /// Force-delete was picked; it applies only once the confirmation is accepted.
    var confirmingForceDelete = false
    let stop = StopConfirmationState()

    init(model: AppModel, feature: FeatureRef, preselect: BranchPolicy?) {
        self.model = model
        self.feature = feature
        self.preselect = preselect
    }

    var projectStore: ProjectStore? { model.projects.project(feature.project) }
    var featureRecord: FeatureRecord? { projectStore?.feature(named: feature.name) }
    var plan: TeardownPlanDocument? { draft?.plan }

    // MARK: Plan

    /// Runs `planTeardown` (the CLI's dry run, or the app preflight on legacy CLIs) and builds the draft.
    func load() async {
        planState = .loading
        let recorded = featureRecord.flatMap { $0.branchName.isEmpty ? nil : $0.branchName }
        let probe = TeardownRequest(feature: feature, recordedBranch: recorded, branch: .keep)
        do {
            let plan = try await model.backend().planTeardown(probe)
            if projectStore?.config == nil { await projectStore?.reloadConfig() }
            var draft = TeardownDraft(feature: feature, recordedBranch: recorded, plan: plan,
                                      config: projectStore?.config?.effective)
            draft.preselect(preselect)
            self.draft = draft
            planState = .ready
        } catch {
            planState = .failed(BackendError.normalize(error))
        }
    }

    // MARK: Choices

    /// Keep and Delete-if-merged apply at once; Force-delete asks first.
    func choose(_ policy: BranchPolicy) {
        guard draft?.visibleBranchOptions.contains(policy) == true else { return }
        if policy == .forceDelete {
            confirmingForceDelete = true
        } else {
            draft?.branch = policy
        }
    }

    func confirmForceDelete() {
        confirmingForceDelete = false
        draft?.branch = .forceDelete
    }

    func setCompleteSpec(_ on: Bool) {
        draft?.completeSpec = on && hasPreservedSpec
    }

    /// "feature/x has 3 commits that aren't merged. Force-deleting it loses them…".
    var forceDeleteMessage: String {
        let branch = plan?.branch?.name ?? draft?.recordedBranch ?? "The branch"
        return RecoveryPlanner.unmergedConfirmationText(branch: branch, ahead: plan?.branch?.ahead)
    }

    var hasPreservedSpec: Bool { !(plan?.changes.preserved.isEmpty ?? true) }

    var branchIsUnmerged: Bool {
        guard let branch = plan?.branch else { return false }
        return branch.exists && !branch.merged
    }

    var canTearDown: Bool {
        record == nil && planState == .ready && draft?.blockingReason == nil
    }

    // MARK: Attempts

    /// The first attempt: `draft.makeRequest()` (no discard, no forced removal).
    @discardableResult func tearDown() -> Bool {
        guard canTearDown, let request = draft?.makeRequest() else { return false }
        return adopt(FlowDispatch.record(of: model.actions.dispatch(.teardown(request))))
    }

    /// A recovery's retry replaces the latest attempt (the earlier one stays listed); host recoveries return nil.
    @discardableResult func adopt(_ result: Result<OperationRecord, FlowDispatchError>?) -> Bool {
        guard let result else { return false }
        switch result {
        case .success(let next):
            if let record { earlierAttempts.append(record) }
            record = next
            dispatchError = nil
            return true
        case .failure(let error):
            dispatchError = error.message
            return false
        }
    }

    /// The recoveries the latest failed attempt offers (what the result card shows).
    var recoveries: [RecoveryAction] {
        guard let record, let error = record.failure else { return [] }
        return RecoveryPlanner.recoveries(for: error, after: record.context)
    }

    /// `spec_not_preserved`: the CLI names `--force`. Offered only behind a confirmation that the spec is lost,
    /// never for `not_a_worktree`.
    var specOverride: RecoveryAction? {
        guard let record, case .refused(let refusal)? = record.failure, case .other(let code) = refusal.cause,
              code == "spec_not_preserved", case .teardown(var request) = record.context else { return nil }
        request.forceRemoval = true
        request.branch = .keep
        return .retry(.teardown(request), label: "Tear Down Without Preserving the Spec…", destructive: true,
                      confirmation: "BranchBox couldn't move the spec of \(feature.name) into the main worktree. Tearing "
                          + "down anyway deletes the worktree with the spec in it; copy the spec first if you need it. "
                          + "The branch is kept.")
    }

    var outcome: TeardownOutcome? {
        if case .teardown(let outcome)? = record?.result { return outcome }
        return nil
    }

    // MARK: After the teardown

    /// [Force-delete Branch…] for a branch the teardown could not delete because it is unmerged.
    @discardableResult func forceDeleteBranch(_ branch: String) -> Bool {
        switch FlowDispatch.record(of: model.actions.dispatch(.deleteBranch(branch, feature.project, force: true))) {
        case .success(let record):
            branchRecord = record
            return true
        case .failure(let error):
            dispatchError = error.message
            return false
        }
    }

    /// Adopts a retry of the branch delete started from its result card, so the sheet shows its progress and
    /// outcome instead of dropping it.
    func adoptBranchRetry(_ result: Result<OperationRecord, FlowDispatchError>?) {
        guard let result else { return }
        switch result {
        case .success(let record):
            branchRecord = record
            dispatchError = nil
        case .failure(let error):
            dispatchError = error.message
        }
    }

    /// The branch delete failed because the branch has commits that aren't merged.
    func deleteFailedBecauseUnmerged(reason: String) -> Bool {
        branchIsUnmerged || reason.localizedCaseInsensitiveContains("not fully merged")
    }
}

extension RecoveryPlanner {
    /// The planner's own unmerged-branch confirmation text, reused for the sheet's Force-delete choice.
    static func unmergedConfirmationText(branch: String, ahead: Int?) -> String {
        let commits = switch ahead {
        case nil, 0?: "commits that aren't merged"
        case 1?: "1 commit that isn't merged"
        case let count?: "\(count) commits that aren't merged"
        }
        return "\(branch) has \(commits). Force-deleting it loses them unless they were pushed or are on another branch."
    }
}
