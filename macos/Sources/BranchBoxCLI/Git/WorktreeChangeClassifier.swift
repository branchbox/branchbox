import BranchBoxKit
import Darwin
import Foundation

/// Which side of a teardown a path is read from.
public enum ChangeTree: Sendable, Hashable { case worktree, main }

/// What a path names, without following symbolic links.
public enum ChangeItem: Sendable, Hashable {
    case missing
    case file(size: Int)
    case symbolicLink(target: String)
    case directory
    case other
    /// The path is absolute, has a `..` component or passes through a symbolic link; it is never read.
    case unsafe
}

/// Everything `WorktreeChangeClassifier` reads, so the rules can be tested without a repository.
public protocol ChangeContext: Sendable {
    /// The worktree's directory name, which names its devcontainer sync baseline.
    var featureName: String { get }
    func item(_ path: String, in tree: ChangeTree) -> ChangeItem
    /// The bytes of a regular file of at most `limit` bytes; nil for anything else.
    func contents(_ path: String, in tree: ChangeTree, limit: Int) -> Data?
    /// `<main>/.branchbox/devcontainer-sync/<feature>.json`: `.devcontainer`-relative path → FNV-1a-64 hex digest.
    func devcontainerBaseline() -> [String: String]?
    /// The worktree's committed (`HEAD`) version of `path`; nil when it is not tracked.
    func committedContents(_ path: String) -> Data?
}

/// Splits a worktree's `git status` into user changes, BranchBox-generated files and preserved files — a Swift port
/// of core's S4 classifier (DESIGN §6.5), used on legacy CLIs where the CLI has no teardown plan. Rules, in
/// precedence order:
///
/// - **R1** reserved names under `.devcontainer/` → generated (`reserved_name`).
/// - **R2** this feature's spec (`docs/features/{in-progress,backlog,completed}/<name>.md`) → preserved.
/// - **R3** a `.devcontainer/` file whose FNV-1a-64 digest equals its sync baseline → generated.
/// - **R4** a regular file of at most 8 MiB byte-identical to the main worktree's, or a symbolic link with the same
///   target → generated (`derived_from_main`).
/// - **R5** `.devcontainer/.env` that links to `../.env` or equals the worktree's `.env` → generated.
/// - **R6** `.env` that is main's `.env` plus only BranchBox's managed feature block → generated.
/// - **R7** `.vscode/settings.json` that differs from the committed one only in BranchBox's managed keys, and
///   `.vscode/tasks.json` holding only the "Open Feature URL" task → generated.
/// - Anything else is a user change. Past 2000 entries, the rest are user changes and the result is truncated.
public enum WorktreeChangeClassifier {
    public static let entryLimit = 2000
    public static let derivedFileLimit = 8 << 20
    /// JSON and `.env` files are read up to this size.
    static let textFileLimit = 1 << 20

    static let reservedNames: Set<String> = [
        ".devcontainer/.branchbox.env", ".devcontainer/.cloudflared.env", ".devcontainer/.devcontainer.json",
        ".devcontainer/.branchbox-sbx-compose.yaml",
    ]
    static let specStates = ["in-progress", "backlog", "completed"]
    static let envBlockMarker = "# Feature-specific configuration"
    static let envBlockKeys: Set<String> = [
        "WORK_FEATURE", "APP_URL", "COMPOSE_PROJECT_NAME", "DEVCONTAINER_NAME", "GIT_BRANCH", "DATABASE_NAME",
    ]
    /// The keys `setup_vscode_workspace` writes into `.vscode/settings.json`.
    static let managedVSCodeKeys = ["peacock.color", "peacock.remoteColor", "window.title",
                                    "workbench.colorCustomizations"]
    static let managedTaskLabels = ["Open Feature URL"]

    public struct Result: Sendable, Hashable {
        public var user: [ChangedFile] = []
        public var generated: [TeardownPlanDocument.GeneratedFile] = []
        public var preserved: [PreservedFile] = []
        public var truncated = false

        public var changes: TeardownPlanDocument.Changes {
            TeardownPlanDocument.Changes(statusAvailable: true, truncated: truncated, user: user, generated: generated,
                                         preserved: preserved)
        }
    }

    public enum Verdict: Sendable, Hashable {
        case user
        case generated(rule: String)
        case preserved(destination: String)
    }

    /// - Parameter completeSpec: a preserved spec goes to `completed/` instead of `backlog/`.
    public static func classify(_ entries: [GitStatusEntry], context: any ChangeContext,
                                completeSpec: Bool = false) -> Result {
        var result = Result()
        for (offset, entry) in entries.enumerated() {
            guard offset < entryLimit else {
                result.truncated = true
                result.user.append(entry.changedFile)
                continue
            }
            switch verdict(for: entry, context: context, completeSpec: completeSpec) {
            case .user: result.user.append(entry.changedFile)
            case .generated(let rule): result.generated.append(.init(path: entry.path, rule: rule))
            case .preserved(let destination): result.preserved.append(PreservedFile(path: entry.path,
                                                                                    destination: destination))
            }
        }
        return result
    }

    public static func verdict(for entry: GitStatusEntry, context: any ChangeContext,
                               completeSpec: Bool = false) -> Verdict {
        let path = entry.path
        if reservedNames.contains(path) { return .generated(rule: "reserved_name") }                       // R1
        if let destination = specDestination(path, feature: context.featureName, completeSpec: completeSpec) {
            return .preserved(destination: destination)                                                  // R2
        }
        // Every later rule compares what is on disk; a deleted path or one that cannot be read safely is the user's.
        guard !entry.isDeleted else { return .user }
        if isDevcontainerBaseline(path, context: context) { return .generated(rule: "devcontainer_baseline") } // R3
        if isDerivedFromMain(path, context: context) { return .generated(rule: "derived_from_main") }          // R4
        if path == ".devcontainer/.env", isDevcontainerEnvLink(context: context) {
            return .generated(rule: "devcontainer_env_link")                                              // R5
        }
        if path == ".env", isManagedEnv(context: context) { return .generated(rule: "env_feature_block") }      // R6
        if path == ".vscode/settings.json", isManagedVSCodeSettings(entry: entry, context: context) {
            return .generated(rule: "vscode_managed_keys")                                                // R7
        }
        if path == ".vscode/tasks.json", isManagedVSCodeTasks(context: context) {
            return .generated(rule: "vscode_managed_tasks")                                               // R7
        }
        return .user
    }

    // MARK: - R2

    static func specDestination(_ path: String, feature: String, completeSpec: Bool) -> String? {
        let matches = specStates.contains { path == "docs/features/\($0)/\(feature).md" }
        guard matches else { return nil }
        return "docs/features/\(completeSpec ? "completed" : "backlog")/\(feature).md"
    }

    // MARK: - R3

    static func isDevcontainerBaseline(_ path: String, context: any ChangeContext) -> Bool {
        let prefix = ".devcontainer/"
        guard path.hasPrefix(prefix), let baseline = context.devcontainerBaseline(),
              let expected = baseline[String(path.dropFirst(prefix.count))],
              let data = context.contents(path, in: .worktree, limit: derivedFileLimit) else { return false }
        return digest(data) == expected.lowercased()
    }

    /// core's `stable_content_hash` (core/src/modules/devcontainer.rs): FNV-1a 64 over the bytes.
    public static func stableContentHash<Bytes: Sequence>(_ bytes: Bytes) -> UInt64 where Bytes.Element == UInt8 {
        bytes.reduce(0xcbf2_9ce4_8422_2325 as UInt64) { hash, byte in
            (hash ^ UInt64(byte)) &* 0x0000_0100_0000_01b3
        }
    }

    /// core's `baseline_digest`: the hash as 16 lowercase hex digits.
    public static func digest(_ data: Data) -> String {
        let hex = String(stableContentHash(data), radix: 16)
        return String(repeating: "0", count: max(0, 16 - hex.count)) + hex
    }

    // MARK: - R4

    static func isDerivedFromMain(_ path: String, context: any ChangeContext) -> Bool {
        switch (context.item(path, in: .worktree), context.item(path, in: .main)) {
        case let (.file(size), .file(mainSize)):
            guard size == mainSize, size <= derivedFileLimit,
                  let ours = context.contents(path, in: .worktree, limit: derivedFileLimit),
                  let theirs = context.contents(path, in: .main, limit: derivedFileLimit) else { return false }
            return ours == theirs
        case let (.symbolicLink(target), .symbolicLink(mainTarget)):
            return target == mainTarget
        default:
            return false
        }
    }

    // MARK: - R5

    static func isDevcontainerEnvLink(context: any ChangeContext) -> Bool {
        switch context.item(".devcontainer/.env", in: .worktree) {
        case .symbolicLink(let target):
            return target == "../.env"
        case .file:
            guard let link = context.contents(".devcontainer/.env", in: .worktree, limit: textFileLimit),
                  let env = context.contents(".env", in: .worktree, limit: textFileLimit) else { return false }
            return link == env
        default:
            return false
        }
    }

    // MARK: - R6

    static func isManagedEnv(context: any ChangeContext) -> Bool {
        guard case .file = context.item(".env", in: .worktree), case .file = context.item(".env", in: .main),
              let ours = context.contents(".env", in: .worktree, limit: textFileLimit),
              let theirs = context.contents(".env", in: .main, limit: textFileLimit),
              let feature = String(data: ours, encoding: .utf8), let main = String(data: theirs, encoding: .utf8)
        else { return false }
        let (base, block) = splitFeatureSection(feature)
        guard let block, trimmingTrailingWhitespace(base) == trimmingTrailingWhitespace(splitFeatureSection(main).base)
        else { return false }
        for rawLine in block.split(whereSeparator: \.isNewline) {
            let line = rawLine.trimmingCharacters(in: .whitespaces)
            if line.isEmpty || line.hasPrefix(envBlockMarker) { continue }
            guard let equals = line.firstIndex(of: "="), envBlockKeys.contains(String(line[..<equals])) else {
                return false
            }
        }
        return true
    }

    /// core's `split_feature_section`: everything before the first feature marker, and the block from it on.
    static func splitFeatureSection(_ text: String) -> (base: String, block: String?) {
        guard let marker = text.range(of: envBlockMarker) else { return (text, nil) }
        return (String(text[..<marker.lowerBound]), String(text[marker.lowerBound...]))
    }

    private static func trimmingTrailingWhitespace(_ text: String) -> Substring {
        var end = text.endIndex
        while end > text.startIndex, text[text.index(before: end)].isWhitespace { end = text.index(before: end) }
        return text[..<end]
    }

    // MARK: - R7

    static func isManagedVSCodeSettings(entry: GitStatusEntry, context: any ChangeContext) -> Bool {
        guard let current = jsonObject(context.contents(".vscode/settings.json", in: .worktree, limit: textFileLimit))
        else { return false }
        let committed: [String: Any]
        if entry.isUntracked {
            committed = [:]
        } else {
            guard let object = jsonObject(context.committedContents(".vscode/settings.json")) else { return false }
            committed = object
        }
        return NSDictionary(dictionary: withoutManagedKeys(current)).isEqual(to: withoutManagedKeys(committed))
    }

    static func isManagedVSCodeTasks(context: any ChangeContext) -> Bool {
        guard let object = jsonObject(context.contents(".vscode/tasks.json", in: .worktree, limit: textFileLimit)),
              let tasks = object["tasks"] as? [[String: Any]] else { return false }
        return tasks.map { $0["label"] as? String ?? "" } == managedTaskLabels
    }

    private static func withoutManagedKeys(_ object: [String: Any]) -> [String: Any] {
        object.filter { !managedVSCodeKeys.contains($0.key) }
    }

    private static func jsonObject(_ data: Data?) -> [String: Any]? {
        guard let data else { return nil }
        return (try? JSONSerialization.jsonObject(with: data)) as? [String: Any]
    }
}

/// `ChangeContext` over the real worktree and main worktree. Symbolic links are never followed, so a link cannot
/// make a file outside the worktree look generated.
public struct FileSystemChangeContext: ChangeContext {
    public let worktree: String
    public let main: String
    public let featureName: String
    /// Committed versions fetched beforehand (`git show HEAD:<path>`), keyed by path.
    public let committed: [String: Data]

    public init(worktree: String, main: String, featureName: String? = nil, committed: [String: Data] = [:]) {
        self.worktree = worktree
        self.main = main
        self.featureName = featureName ?? (worktree as NSString).lastPathComponent
        self.committed = committed
    }

    public func item(_ path: String, in tree: ChangeTree) -> ChangeItem {
        let components = path.split(separator: "/", omittingEmptySubsequences: true).map(String.init)
        guard !path.hasPrefix("/"), !components.isEmpty, !components.contains(".."), !components.contains(".")
        else { return .unsafe }
        var current = tree == .worktree ? worktree : main
        for (index, component) in components.enumerated() {
            current = Paths.join(current, component)
            var info = stat()
            guard lstat(current, &info) == 0 else { return .missing }
            let type = info.st_mode & S_IFMT
            let isLast = index == components.count - 1
            if !isLast {
                // An intermediate symbolic link would be followed by every read below it.
                guard type == S_IFDIR else { return type == S_IFLNK ? .unsafe : .missing }
                continue
            }
            switch type {
            case S_IFREG: return .file(size: Int(info.st_size))
            case S_IFDIR: return .directory
            case S_IFLNK:
                guard let target = try? FileManager.default.destinationOfSymbolicLink(atPath: current) else {
                    return .other
                }
                return .symbolicLink(target: target)
            default: return .other
            }
        }
        return .missing
    }

    public func contents(_ path: String, in tree: ChangeTree, limit: Int) -> Data? {
        guard case .file(let size) = item(path, in: tree), size <= limit else { return nil }
        let full = Paths.join(tree == .worktree ? worktree : main, path)
        let descriptor = open(full, O_RDONLY | O_NOFOLLOW | O_CLOEXEC)
        guard descriptor >= 0 else { return nil }
        let handle = FileHandle(fileDescriptor: descriptor, closeOnDealloc: true)
        guard let data = try? handle.readToEnd() ?? Data(), data.count <= limit else { return nil }
        return data
    }

    public func devcontainerBaseline() -> [String: String]? {
        let file = Paths.join(main, ".branchbox/devcontainer-sync/\(featureName).json")
        guard let data = try? Data(contentsOf: URL(fileURLWithPath: file)) else { return nil }
        return try? JSONDecoder().decode([String: String].self, from: data)
    }

    public func committedContents(_ path: String) -> Data? {
        committed[path]
    }
}
