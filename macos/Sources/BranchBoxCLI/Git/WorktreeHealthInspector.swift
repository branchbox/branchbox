import BranchBoxKit
import Foundation

/// Read-only evidence about a registered worktree. A folder alone does not prove that Git can use it: its .git
/// pointer may refer to metadata removed by a migration, or the main repository may no longer register it.
enum WorktreeHealthInspector {
    static func issue(for record: FeatureRecord, in mainRoot: String, worktrees: [WorktreeEntry]?,
                      fileSystem: any FileSystemProbing,
                      readFile: (String) throws -> Data = { try Data(contentsOf: URL(fileURLWithPath: $0)) }) -> String? {
        guard record.status != .removed, record.setup?.state != .inProgress,
              let path = record.worktreePath, fileSystem.kind(at: path) == .directory else { return nil }
        let dotGit = Paths.join(path, ".git")
        switch fileSystem.kind(at: dotGit) {
        case .missing: return "The worktree folder exists, but its .git metadata pointer is missing at \(dotGit)."
        case .brokenSymbolicLink: return "The .git metadata link is broken at \(dotGit)."
        default: break
        }
        if case .file = fileSystem.kind(at: dotGit) {
            guard let data = try? readFile(dotGit), let text = String(data: data, encoding: .utf8),
                  text.hasPrefix("gitdir:") else { return "The .git file at \(dotGit) is unreadable or has no valid gitdir pointer." }
            let pointer = String(text.dropFirst("gitdir:".count)).trimmingCharacters(in: .whitespacesAndNewlines)
            guard !pointer.isEmpty else { return "The .git file at \(dotGit) has an empty gitdir pointer." }
            let target = resolve(pointer, relativeTo: path)
            guard fileSystem.kind(at: target) == .directory else {
                return "The .git file points to Git metadata that cannot be found at \(target)."
            }
            let commonFile = Paths.join(target, "commondir")
            // Linked worktrees have a gitdir backpointer and require commondir. A standalone repository
            // created with --separate-git-dir also uses a .git file, but needs neither of these files.
            if case .file = fileSystem.kind(at: Paths.join(target, "gitdir")),
               fileSystem.kind(at: commonFile) == .missing {
                return "Git metadata for this linked worktree is missing its common-directory pointer at \(commonFile)."
            }
            if case .file = fileSystem.kind(at: commonFile) {
                guard let data = try? readFile(commonFile), let text = String(data: data, encoding: .utf8),
                      !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else {
                    return "Git's common-directory pointer at \(commonFile) is unreadable or empty."
                }
                let common = resolve(text.trimmingCharacters(in: .whitespacesAndNewlines), relativeTo: target)
                guard fileSystem.kind(at: common) == .directory else {
                    return "Git's common metadata cannot be found at \(common)."
                }
            }
        }
        if let worktrees {
            let canonical = Paths.canonical(path)
            guard let entry = worktrees.first(where: { Paths.canonical($0.path) == canonical }) else {
                return "Git no longer registers this folder as a worktree of \(mainRoot)."
            }
            if entry.prunable { return "Git marks this worktree's registration as invalid or prunable." }
        }
        return nil
    }

    private static func resolve(_ path: String, relativeTo directory: String) -> String {
        let absolute = (path as NSString).isAbsolutePath ? path : Paths.join(directory, path)
        return URL(fileURLWithPath: absolute).standardizedFileURL.path
    }
}
