import BranchBoxKit
import Foundation

/// Whether deleting a branch loses commits, with `git branch -d` semantics (DESIGN §6.5):
///
/// - The reference is the branch's upstream when one is configured and resolves, otherwise `HEAD` of the main
///   worktree — exactly what `git branch -d` checks.
/// - `merged` is `git merge-base --is-ancestor <branch> <reference>`; `ahead` is
///   `git rev-list --count <reference>..<branch>`.
/// - `git branch --merged` is never parsed: it marks branches checked out in other worktrees with `+`, which
///   naive parsers misread.
public struct BranchMergeState: Sendable, Hashable {
    public let branch: String
    public let exists: Bool
    /// `origin/feature/x` when an upstream is configured and resolves.
    public let upstream: String?
    /// What `merged` and `ahead` are measured against: the upstream's name, or `HEAD`.
    public let reference: String
    /// The upstream's name, or the main worktree's current branch (`HEAD` when detached).
    public let referenceName: String
    /// `git branch -d` would delete the branch.
    public let merged: Bool
    /// The branch is contained in the main worktree's `HEAD` (false for an upstream-pushed, unmerged branch).
    public let mergedIntoHead: Bool
    /// Commits on the branch that `reference` lacks.
    public let ahead: Int

    public init(branch: String, exists: Bool, upstream: String? = nil, reference: String, referenceName: String,
                merged: Bool, mergedIntoHead: Bool, ahead: Int) {
        self.branch = branch
        self.exists = exists
        self.upstream = upstream
        self.reference = reference
        self.referenceName = referenceName
        self.merged = merged
        self.mergedIntoHead = mergedIntoHead
        self.ahead = ahead
    }

    /// A branch that does not exist: nothing can be lost.
    static func missing(_ branch: String, headName: String) -> BranchMergeState {
        BranchMergeState(branch: branch, exists: false, reference: "HEAD", referenceName: headName, merged: true,
                         mergedIntoHead: true, ahead: 0)
    }
}
