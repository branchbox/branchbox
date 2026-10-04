import BranchBoxKit
import BranchBoxStores
import Foundation
import Observation

/// The Stray sheet's state: a worktree in the BranchBox layout that the registry doesn't know (an interrupted
/// start on a legacy CLI, a crash before the write-ahead record).
///
/// Removal is first attempted without discarding anything; a dirty worktree is refused, and discarding happens
/// only through the refusal's recovery, which names the files. Deleting the branch afterwards is opt-in and
/// merged-only (`git branch -d`); an unmerged branch is refused and offers a confirmed force-delete.
@MainActor @Observable final class StrayFlow {
    let model: AppModel
    let project: ProjectRef
    let stray: StrayWorktree
    var confirmingRemove = false
    /// Also delete the stray's branch once the worktree is gone, if it is merged.
    var deleteBranchIfMerged = false
    private(set) var record: OperationRecord?
    private(set) var branchRecord: OperationRecord?
    private(set) var dispatchError: String?
    let stop = StopConfirmationState()

    init(model: AppModel, project: ProjectRef, stray: StrayWorktree) {
        self.model = model
        self.project = project
        self.stray = stray
    }

    var folderName: String { URL(fileURLWithPath: stray.path).lastPathComponent }

    var folderExists: Bool {
        if case .preview? = model.environment.identity?.kind { return !stray.prunable }
        return FileManager.default.fileExists(atPath: stray.path)
    }

    var canRemove: Bool { record == nil }

    /// The first attempt: never discards (`discardChanges: false`).
    @discardableResult func remove() -> Bool {
        guard canRemove else { return false }
        return adopt(FlowDispatch.record(of: model.actions.dispatch(.removeStray(stray, project, discardChanges: false))))
    }

    /// A recovery's retry (the confirmed "Discard N changes and remove") replaces the attempt.
    @discardableResult func adopt(_ result: Result<OperationRecord, FlowDispatchError>?) -> Bool {
        guard let result else { return false }
        switch result {
        case .success(let next):
            record = next
            dispatchError = nil
            return true
        case .failure(let error):
            dispatchError = error.message
            return false
        }
    }

    var removed: Bool { record?.state == .succeeded || record?.state == .succeededWithWarnings }

    /// After a successful removal, deletes the branch when the user asked (merged only; git refuses otherwise).
    func removalFinished() {
        guard removed, deleteBranchIfMerged, branchRecord == nil, let branch = stray.branch else { return }
        switch FlowDispatch.record(of: model.actions.dispatch(.deleteBranch(branch, project, force: false))) {
        case .success(let record): branchRecord = record
        case .failure(let error): dispatchError = error.message
        }
    }

    /// The branch deletion's recovery (a confirmed force-delete of an unmerged branch).
    func adoptBranchRetry(_ result: Result<OperationRecord, FlowDispatchError>?) {
        guard let result else { return }
        switch result {
        case .success(let record): branchRecord = record
        case .failure(let error): dispatchError = error.message
        }
    }

    var removeConfirmationMessage: String {
        var text = "BranchBox removes \(stray.path) with git. If it has uncommitted changes, nothing is removed and "
            + "you'll be asked first."
        if let branch = stray.branch {
            text += deleteBranchIfMerged ? " \(branch) is deleted afterwards if it's merged." : " \(branch) is kept."
        }
        return text
    }
}
