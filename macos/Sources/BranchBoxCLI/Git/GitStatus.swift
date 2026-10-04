import BranchBoxKit
import Foundation

/// One entry of `git status --porcelain=v1 -z`: the index (`X`) and worktree (`Y`) columns and the path,
/// relative to the worktree root.
public struct GitStatusEntry: Sendable, Hashable {
    public let index: Character
    public let worktree: Character
    public let path: String

    public init(index: Character, worktree: Character, path: String) {
        self.index = index
        self.worktree = worktree
        self.path = path
    }

    /// `"??"`, `" M"`, `"A "`, …
    public var code: String { "\(index)\(worktree)" }

    public var isUntracked: Bool { index == "?" && worktree == "?" }

    /// The path no longer exists in the worktree (deleted there, or staged as deleted).
    public var isDeleted: Bool { worktree == "D" || (index == "D" && worktree == " ") }

    /// The §5.5 change kind: `untracked`, `modified`, `added`, `deleted`, `typechange`, `conflicted` or `staged`.
    /// The worktree column wins over the index column, because it is what a discard would lose first.
    public var kind: String {
        if isUntracked { return "untracked" }
        if Self.conflictCodes.contains(code) { return "conflicted" }
        switch worktree {
        case "M": return "modified"
        case "D": return "deleted"
        case "T": return "typechange"
        case "A": return "added"                     // intent-to-add (`git add -N`)
        default: break
        }
        switch index {
        case "A": return "added"
        case "D": return "deleted"
        case "T": return "typechange"
        default: return "staged"
        }
    }

    /// Unmerged states, as listed by git-status(1).
    static let conflictCodes: Set<String> = ["DD", "AU", "UD", "UA", "DU", "AA", "UU"]

    /// The §5.5 change area: `devcontainer`, `compose`, `vscode`, `spec`, `env` or `other`. The devcontainer and
    /// compose areas are the paths core's teardown treats as module-managed.
    public var area: String { Self.area(of: path) }

    public static func area(of path: String) -> String {
        let name = (path as NSString).lastPathComponent
        if path == ".devcontainer" || path.hasPrefix(".devcontainer/") { return "devcontainer" }
        if path == "compose" || path.hasPrefix("compose/") || composeFileNames.contains(name) { return "compose" }
        if path.hasPrefix(".vscode/") { return "vscode" }
        if path.hasPrefix("docs/features/") { return "spec" }
        if name == ".env" || name.hasPrefix(".env.") { return "env" }
        return "other"
    }

    static let composeFileNames: Set<String> = ["compose.yaml", "compose.yml", "docker-compose.yml", "docker-compose.yaml"]

    /// This entry as a user change.
    public var changedFile: ChangedFile { ChangedFile(path: path, kind: kind, area: area) }
}

/// Parses `git status --porcelain=v1 -z` output.
public enum GitStatusParser {
    /// Records are NUL-terminated `XY PATH`. A rename or copy (only without `--no-renames`) is followed by one more
    /// NUL-terminated field, the original path, which is skipped. Paths are never quoted in `-z` mode.
    public static func parse(_ data: Data) -> [GitStatusEntry] {
        var entries: [GitStatusEntry] = []
        var fields = data.split(separator: 0, omittingEmptySubsequences: true).makeIterator()
        while let field = fields.next() {
            let bytes = Array(field)
            guard bytes.count > 3, bytes[2] == UInt8(ascii: " ") else { continue }
            let index = Character(Unicode.Scalar(bytes[0]))
            let worktree = Character(Unicode.Scalar(bytes[1]))
            let path = String(decoding: bytes[3...], as: UTF8.self)
            entries.append(GitStatusEntry(index: index, worktree: worktree, path: path))
            if index == "R" || index == "C" { _ = fields.next() }
        }
        return entries
    }
}
