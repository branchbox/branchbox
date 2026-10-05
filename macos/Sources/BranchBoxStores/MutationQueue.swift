import BranchBoxKit
import Foundation

/// Orders the operations that have not finished yet and decides which may run (D-16):
/// - exec never waits (it is not a mutation), and reads never come through here;
/// - at most one mutating operation per feature; a second one is rejected rather than queued;
/// - a project-wide operation (prune, sync, init, config, credentials) waits for every mutating operation already
///   in its project, and every later mutating operation in the project waits for it;
/// - without the CLI's `registry-lock` capability, registry writers in a project run one at a time, in dispatch
///   order (start, teardown, prune, tunnel open/remove, sync, init, config apply, credentials).
/// An operation waits behind every earlier entry it conflicts with, queued ones included, so a waiting writer is
/// never overtaken by a later one (FIFO). It shows `.queued(behind:)` with the title of the last such entry.
@MainActor final class MutationQueue {
    /// Queued and running records, oldest first.
    private(set) var entries: [OperationRecord] = []

    func append(_ record: OperationRecord) {
        entries.append(record)
    }

    func remove(_ record: OperationRecord) {
        entries.removeAll { $0 === record }
    }

    /// The running or queued mutation of the same feature, which makes another mutation of it inadmissible.
    func featureConflict(kind: OperationKind, target: OperationTarget) -> OperationRecord? {
        guard kind.isMutating, case .feature = target else { return nil }
        return entries.first { $0.kind.isMutating && $0.target == target }
    }

    /// The last entry ahead of `record` that it must wait for; nil when it may run.
    func blocker(for record: OperationRecord, registryLock: Bool) -> OperationRecord? {
        guard let index = entries.firstIndex(where: { $0 === record }) else { return nil }
        return entries[..<index].last {
            Self.conflicts($0.kind, $0.target, before: record.kind, record.target, registryLock: registryLock)
        }
    }

    /// The last current entry an operation of `kind` on `target` would wait for if dispatched now.
    func blocker(kind: OperationKind, target: OperationTarget, registryLock: Bool) -> OperationRecord? {
        entries.last { Self.conflicts($0.kind, $0.target, before: kind, target, registryLock: registryLock) }
    }

    /// Whether an operation dispatched later must wait for an earlier one.
    static func conflicts(_ earlierKind: OperationKind, _ earlierTarget: OperationTarget,
                          before laterKind: OperationKind, _ laterTarget: OperationTarget, registryLock: Bool) -> Bool {
        guard earlierKind.isMutating, laterKind.isMutating,
              let earlierProject = earlierTarget.project, let laterProject = laterTarget.project,
              earlierProject == laterProject else { return false }
        if earlierKind.isProjectWide || laterKind.isProjectWide { return true }
        if case .feature = laterTarget, earlierTarget == laterTarget { return true }
        return !registryLock && earlierKind.writesRegistry && laterKind.writesRegistry
    }

    /// An unfinished entry that holds one of `keys` (the same branch or stray worktree), which makes a second
    /// request on them inadmissible: two `deleteBranch` of one branch, or a branch delete during the teardown of
    /// the feature that owns it, would fail confusingly ("branch not found") however they interleave.
    func resourceConflict(_ keys: Set<String>) -> OperationRecord? {
        guard !keys.isEmpty else { return nil }
        return entries.first { !$0.context.exclusiveKeys.isDisjoint(with: keys) }
    }
}

extension OperationRequestContext {
    /// The branches and stray worktrees this request deletes, as `branch <project> <name>` and
    /// `worktree <project> <path>`; at most one unfinished request may hold each.
    var exclusiveKeys: Set<String> {
        func branch(_ name: String?, _ project: ProjectRef) -> Set<String> {
            guard let name, !name.isEmpty else { return [] }
            return ["branch \(project.path) \(name)"]
        }
        switch self {
        case .deleteBranch(let name, let project, _):
            return branch(name, project)
        case .removeStray(let stray, let project, _):
            return ["worktree \(project.path) \(URL(fileURLWithPath: stray.path).standardizedFileURL.path)"]
        case .teardown(let request):
            return request.branch == .keep ? [] : branch(request.recordedBranch, request.feature.project)
        case .prune(let selection):
            return selection.rows.reduce(into: Set<String>()) { keys, row in
                if row.branch != .keep { keys.formUnion(branch(row.recordedBranch, row.feature.project)) }
            }
        default:
            return []
        }
    }
}
