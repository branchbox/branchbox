import BranchBoxKit
import Foundation

/// Finds "Unregistered worktree"s (DESIGN §6.6): worktrees in BranchBox's layout that the registry does not know,
/// typically left by a start that was interrupted on a CLI without write-ahead registration. A worktree is a stray
/// only when all of these hold:
///
/// - it is not the main worktree and not bare;
/// - no registry record's `worktree_path` names it (compared with symlinks resolved);
/// - it sits beside the main worktree (`parent(path) == parent(main root)`);
/// - its branch is `<prefix>/<directory name>` for the configured prefix or a prefix a record uses, or the bare
///   directory name when the prefix is empty.
///
/// A user's own `git worktree add ../elsewhere` with an unrelated branch is therefore never offered for removal.
public enum StrayDetector {
    public static func strays(in worktrees: [WorktreeEntry], mainRoot: String, records: [FeatureRecord],
                              configPrefix: String) -> [StrayWorktree] {
        strays(in: worktrees, mainRoot: mainRoot, records: records, configPrefix: configPrefix,
               canonical: Paths.canonical)
    }

    /// `canonical` resolves symbolic links; tests pass the identity to work with paths that do not exist.
    static func strays(in worktrees: [WorktreeEntry], mainRoot: String, records: [FeatureRecord],
                       configPrefix: String, canonical: (String) -> String) -> [StrayWorktree] {
        let main = canonical(mainRoot)
        let container = (main as NSString).deletingLastPathComponent
        let registered = Set(records.filter { $0.status != .removed }.compactMap(\.worktreePath).map(canonical))
        let prefixes = Set([configPrefix] + records.compactMap(\.branchPrefix))

        return worktrees.compactMap { entry -> StrayWorktree? in
            let path = canonical(entry.path)
            guard !entry.isBare, path != main, !registered.contains(path),
                  (path as NSString).deletingLastPathComponent == container,
                  let branch = entry.branch else { return nil }
            let name = (path as NSString).lastPathComponent
            let matches = prefixes.contains { prefix in
                prefix.isEmpty ? branch == name : branch == "\(prefix)/\(name)"
            }
            return matches ? entry.stray : nil
        }
    }
}
