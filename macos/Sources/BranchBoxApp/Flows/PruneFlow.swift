import BranchBoxKit
import BranchBoxStores
import Foundation
import Observation

/// The Prune sheet's state for one presentation (D-12: an app-orchestrated loop of ordinary teardowns, never
/// `branchbox prune`).
///
/// Every row is a feature that is not removed. Each is checked with `planTeardown`, at most four at a time, and
/// `PrunePlanner` preselects the safe set. Checking a row with uncommitted changes asks first, and only then gets
/// that row's `DiscardConsent` for exactly the files the popover listed: the only place consent is made outside
/// `RecoveryPlanner`, always per row and explicit.
@MainActor @Observable final class PruneFlow {
    /// Where a row is while the prune runs.
    enum RowRunState: Equatable {
        case waiting
        case running
        case finished
        case outcome(PruneRowOutcome)
    }

    /// Most `planTeardown` calls in flight at once.
    static let planConcurrency = 4

    let model: AppModel
    let project: ProjectRef
    /// The rows: the project's features that are not removed, as listed when the sheet opened.
    let features: [FeatureRecord]
    private(set) var plans: [String: TeardownPlanDocument] = [:]
    private(set) var planErrors: [String: String] = [:]
    private(set) var selected: Set<String> = []
    private(set) var consents: [String: DiscardConsent] = [:]
    private(set) var policy: BranchPolicy = .keep
    /// The dirty row whose "Delete N uncommitted files?" popover is open.
    var pendingConsent: String?
    /// Force-delete was chosen; Prune asks first, listing the rows.
    var confirmingForceDelete = false
    private(set) var record: OperationRecord?
    private(set) var dispatchError: String?
    let stop = StopConfirmationState()

    /// Rows the user checked or unchecked themselves; a plan arriving later doesn't override them.
    @ObservationIgnored private var touched: Set<String> = []
    @ObservationIgnored private var isLoadingPlans = false

    init(model: AppModel, project: ProjectRef) {
        self.model = model
        self.project = project
        features = model.projects.project(project)?.features.filter { $0.status != .removed } ?? []
    }

    var projectName: String { model.projects.project(project)?.displayName ?? project.displayName }

    var isEmpty: Bool { features.isEmpty }

    /// The planner's rows with the user's selection applied.
    var rows: [PrunePlanner.Row] {
        PrunePlanner.rows(features: features, plans: plans, policy: policy).map { row in
            var row = row
            row.selected = selected.contains(row.feature.workFeature)
            return row
        }
    }

    var isChecking: Bool { plans.count + planErrors.count < features.count }

    // MARK: Checking

    /// Plans every row, at most `planConcurrency` at a time; each answer preselects its row when it is safe.
    /// Checks the rows whose check failed again (the [Check Again] button).
    func recheckFailed() async {
        guard !isLoadingPlans, !planErrors.isEmpty else { return }
        planErrors.removeAll()
        await loadPlans()
    }

    func loadPlans() async {
        guard !isLoadingPlans else { return }
        isLoadingPlans = true
        defer { isLoadingPlans = false }
        let backend: any BranchBoxBackend
        do {
            backend = try model.backend()
        } catch {
            let message = BackendError.normalize(error).oneLine
            for feature in features { planErrors[feature.workFeature] = message }
            return
        }
        let requests = features.filter { plans[$0.workFeature] == nil }.map { feature in
            TeardownRequest(feature: FeatureRef(project: project, name: feature.workFeature),
                            recordedBranch: feature.branchName.isEmpty ? nil : feature.branchName, branch: .keep)
        }
        await withTaskGroup(of: (String, Result<TeardownPlanDocument, BackendError>).self) { group in
            var pending = requests[...]
            func addNext() {
                guard let request = pending.popFirst() else { return }
                group.addTask {
                    do {
                        return (request.feature.name, .success(try await backend.planTeardown(request)))
                    } catch {
                        return (request.feature.name, .failure(BackendError.normalize(error)))
                    }
                }
            }
            for _ in 0..<Self.planConcurrency { addNext() }
            for await (name, result) in group {
                switch result {
                case .success(let plan):
                    plans[name] = plan
                    planErrors[name] = nil
                case .failure(.cancelled):
                    break                                   // the sheet went away; nothing to report
                case .failure(let error):
                    if plans[name] == nil { planErrors[name] = error.oneLine }
                }
                applyPlannerSelection(to: [name])
                addNext()
            }
        }
    }

    // MARK: Selection

    /// Whether checking `name` needs the popover's confirmation: its plan lists uncommitted changes of the user's.
    func needsConsent(_ name: String) -> Bool {
        guard let plan = plans[name], plan.worktree.exists, plan.changes.statusAvailable else { return false }
        return !plan.changes.user.isEmpty
    }

    /// Why `name` can't be checked at all, or nil.
    func unselectableReason(_ name: String) -> String? {
        guard let feature = features.first(where: { $0.workFeature == name }) else { return "Unknown feature" }
        if feature.setup?.state == .inProgress { return "Still being set up" }
        if let issue = feature.worktreeIssue { return "Git worktree needs repair: \(issue)" }
        if plans[name]?.changes.truncated == true {
            return "Too many changes to list safely; review this feature in Tear Down instead"
        }
        if plans[name] != nil { return nil }
        if let error = planErrors[name] { return "Couldn't check it: \(error)" }
        return "Checking for unsaved work…"
    }

    func toggle(_ name: String) {
        touched.insert(name)
        if selected.contains(name) {
            selected.remove(name)
            consents[name] = nil
            return
        }
        guard unselectableReason(name) == nil else { return }
        if needsConsent(name) {
            pendingConsent = name
        } else {
            selected.insert(name)
        }
    }

    /// The files the popover lists for `name`.
    func consentFiles(_ name: String) -> [ChangedFile] {
        plans[name]?.changes.user ?? []
    }

    /// The user confirmed deleting exactly the listed files of the pending row.
    func confirmConsent() {
        guard let name = pendingConsent else { return }
        pendingConsent = nil
        guard unselectableReason(name) == nil else { return }
        consents[name] = DiscardConsent(userFiles: consentFiles(name).map(\.path))
        selected.insert(name)
    }

    func cancelConsent() {
        pendingConsent = nil
    }

    func selectSafe() {
        touched = Set(features.map(\.workFeature))
        consents = [:]
        selected = Set(PrunePlanner.rows(features: features, plans: plans, policy: policy).filter(\.selected)
            .map(\.feature.workFeature))
    }

    /// Every row that can be checked without a confirmation, plus the dirty rows already confirmed.
    func selectAll() {
        touched = Set(features.map(\.workFeature))
        selected = Set(features.map(\.workFeature).filter { name in
            unselectableReason(name) == nil && (!needsConsent(name) || consents[name] != nil)
        })
    }

    func selectNone() {
        touched = Set(features.map(\.workFeature))
        selected = []
        consents = [:]
    }

    func setPolicy(_ policy: BranchPolicy) {
        self.policy = policy
        applyPlannerSelection(to: features.map(\.workFeature))
    }

    private func applyPlannerSelection(to names: [String]) {
        let safe = Set(PrunePlanner.rows(features: features, plans: plans, policy: policy).filter(\.selected)
            .map(\.feature.workFeature))
        for name in names where !touched.contains(name) {
            if safe.contains(name) { selected.insert(name) } else { selected.remove(name) }
        }
    }

    /// Selected rows whose branch has unmerged commits, with the count: what Force-delete would lose.
    var forceDeleteRows: [(name: String, branch: String, ahead: Int)] {
        rows.filter(\.selected).compactMap { row in
            guard let branch = row.plan?.branch, branch.exists, !branch.merged else { return nil }
            return (row.feature.workFeature, branch.name, branch.ahead)
        }
    }

    var forceDeleteMessage: String {
        let lines = forceDeleteRows.map { row in
            "• \(row.name): \(row.branch)" + (row.ahead > 0 ? " (\(Pluralized.count(row.ahead, "unmerged commit")))" : "")
        }
        let list = lines.isEmpty ? "None of the selected branches has unmerged commits." : lines.joined(separator: "\n")
        return "Every selected feature's branch is deleted, merged or not:\n\(list)\n"
            + "Commits that aren't pushed or on another branch are lost."
    }

    // MARK: Running

    var selection: PruneSelection {
        PrunePlanner.selection(project: project, rows: rows, policy: policy, completeSpec: false, consents: consents)
    }

    var canPrune: Bool { record == nil && !selected.isEmpty }

    /// Dispatches the prune; Force-delete asks first.
    @discardableResult func prune(confirmed: Bool = false) -> Bool {
        guard canPrune else { return false }
        if policy == .forceDelete, !confirmed {
            confirmingForceDelete = true
            return false
        }
        confirmingForceDelete = false
        switch FlowDispatch.record(of: model.actions.dispatch(.prune(selection))) {
        case .success(let record):
            self.record = record
            dispatchError = nil
            return true
        case .failure(let error):
            dispatchError = error.message
            return false
        }
    }

    var result: PruneResult? {
        if case .prune(let result)? = record?.result { return result }
        return nil
    }

    /// The requests the prune ran, by feature, for a refused row's recoveries.
    func request(for name: String) -> TeardownRequest? {
        guard case .prune(let selection)? = record?.context else { return nil }
        return selection.rows.first { $0.feature.name == name }
    }

    func runState(for name: String) -> RowRunState? {
        guard let record, case .prune(let selection) = record.context,
              let position = selection.rows.firstIndex(where: { $0.feature.name == name }) else { return nil }
        if let row = result?.rows.first(where: { $0.feature == name }) { return .outcome(row.outcome) }
        if record.isFinished, result == nil { return nil }               // stopped while queued, or failed early
        guard case .item(let index, _, _)? = record.phase else { return .waiting }
        if position + 1 < index { return .finished }
        if position + 1 == index { return record.isFinished ? .finished : .running }
        return .waiting
    }
}
