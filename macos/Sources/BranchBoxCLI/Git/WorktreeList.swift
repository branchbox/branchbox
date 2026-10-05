import BranchBoxKit
import Foundation

/// One entry of `git worktree list --porcelain`.
public struct WorktreeEntry: Sendable, Hashable {
    public let path: String
    public let head: String?
    /// `refs/heads/` stripped; nil when detached or bare.
    public let branch: String?
    public let isBare: Bool
    public let isDetached: Bool
    public let locked: Bool
    public let lockReason: String?
    public let prunable: Bool

    public init(path: String, head: String? = nil, branch: String? = nil, isBare: Bool = false,
                isDetached: Bool = false, locked: Bool = false, lockReason: String? = nil, prunable: Bool = false) {
        self.path = path
        self.head = head
        self.branch = branch
        self.isBare = isBare
        self.isDetached = isDetached
        self.locked = locked
        self.lockReason = lockReason
        self.prunable = prunable
    }

    public var stray: StrayWorktree {
        StrayWorktree(path: path, branch: branch, head: head, locked: locked, prunable: prunable)
    }
}

/// Parses `git worktree list --porcelain`: blank-line-separated records of `worktree <path>`, `HEAD <sha>`,
/// `branch <ref>`, `detached`, `bare`, `locked [<reason>]` and `prunable [<reason>]` lines. The first record is the
/// main worktree.
public enum WorktreeListParser {
    public static func parse(_ text: String) -> [WorktreeEntry] {
        var entries: [WorktreeEntry] = []
        var current: [String] = []
        func flush() {
            if let entry = record(current) { entries.append(entry) }
            current = []
        }
        for line in text.split(separator: "\n", omittingEmptySubsequences: false) {
            if line.isEmpty { flush() } else { current.append(String(line)) }
        }
        flush()
        return entries
    }

    private static func record(_ lines: [String]) -> WorktreeEntry? {
        guard let first = lines.first, first.hasPrefix("worktree ") else { return nil }
        var head: String?
        var branch: String?
        var bare = false, detached = false, locked = false, prunable = false
        var lockReason: String?
        for line in lines.dropFirst() {
            let (key, value) = split(line)
            switch key {
            case "HEAD": head = value
            case "branch":
                branch = value.map { $0.hasPrefix("refs/heads/") ? String($0.dropFirst("refs/heads/".count)) : $0 }
            case "bare": bare = true
            case "detached": detached = true
            case "locked":
                locked = true
                lockReason = value
            case "prunable": prunable = true
            default: break
            }
        }
        return WorktreeEntry(path: String(first.dropFirst("worktree ".count)), head: head, branch: branch,
                             isBare: bare, isDetached: detached, locked: locked, lockReason: lockReason,
                             prunable: prunable)
    }

    private static func split(_ line: String) -> (String, String?) {
        guard let space = line.firstIndex(of: " ") else { return (line, nil) }
        let value = String(line[line.index(after: space)...])
        return (String(line[..<space]), value.isEmpty ? nil : value)
    }
}
