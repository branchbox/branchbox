import BranchBoxKit
import Foundation

/// The git questions the backend asks directly (DESIGN §6.2, §6.5, §6.6), each one `git -C <dir> …` with an
/// absolute git executable. Throws only `BackendError`.
public struct GitInspector: Sendable {
    public static let defaultExecutable = URL(fileURLWithPath: "/usr/bin/git")
    static let readTimeout: Duration = .seconds(15)
    static let statusTimeout: Duration = .seconds(30)

    public let executable: URL
    let invocation: ToolInvocation
    let fileSystem: any FileSystemProbing

    public init(executable: URL = GitInspector.defaultExecutable, runner: any ProcessRunning,
                environment: [String: String], fileSystem: any FileSystemProbing = LocalFileSystem()) {
        self.init(executable: executable,
                  invocation: ToolInvocation(runner: runner, environment: environment), fileSystem: fileSystem)
    }

    init(executable: URL, invocation: ToolInvocation, fileSystem: any FileSystemProbing) {
        self.executable = executable
        self.invocation = invocation
        self.fileSystem = fileSystem
    }

    // MARK: - Projects

    /// Normalizes `folder` to the project's MAIN worktree (DESIGN §6.2 resolveProject):
    ///
    /// - `rev-parse --path-format=absolute --git-common-dir --show-toplevel`; the parent of the common `.git`
    ///   directory is the main root, so a feature worktree resolves to its main worktree.
    /// - A folder that is not a repository but holds `main/.git` is a parent container (`init`'s layout).
    /// - Paths keep the spelling the user picked: git's symlink-resolved output is mapped back onto `folder`.
    public func resolveProject(at folder: URL) async throws -> ProjectResolution {
        let requested = folder.standardizedFileURL
        let path = requested.path
        guard fileSystem.kind(at: path) == .directory else {
            throw BackendError.projectInvalid(fileSystem.kind(at: path) == .missing ? .missing(path)
                                                                                     : .notGitRepository(path))
        }
        if let roots = try await repositoryRoots(of: path) {
            let normalization: ProjectResolution.Normalization =
                Paths.same(roots.main, roots.toplevel) ? .none : .fromFeatureWorktree
            return resolution(main: roots.main, requested: requested, normalization: normalization)
        }
        let container = Paths.join(path, "main")
        if fileSystem.kind(at: Paths.join(container, ".git")) != .missing,
           let roots = try await repositoryRoots(of: container) {
            return resolution(main: roots.main, requested: requested, normalization: .fromParentContainer)
        }
        throw BackendError.projectInvalid(.notGitRepository(path))
    }

    private func resolution(main: String, requested: URL, normalization: ProjectResolution.Normalization)
        -> ProjectResolution {
        let project = ProjectRef(root: URL(fileURLWithPath: main, isDirectory: true))
        let initialized = fileSystem.kind(at: Paths.join(project.path, ".branchbox")) == .directory
        return ProjectResolution(project: project, requested: requested, normalization: normalization,
                                 initialized: initialized)
    }

    /// (main root, top level) of the repository containing `path`, both spelled through `path`; nil when `path`
    /// is not inside a work tree.
    func repositoryRoots(of path: String) async throws -> (main: String, toplevel: String)? {
        let result = try await git(["-C", path, "rev-parse", "--path-format=absolute", "--git-common-dir",
                                    "--show-toplevel"], operation: "git rev-parse")
        guard result.termination == .exited(0) else { return nil }
        let lines = Self.lines(result.stdout)
        guard lines.count >= 2 else { return nil }
        let commonDirectory = lines[0], toplevel = lines[1]
        // A bare or separate git directory has no main worktree beside it; fall back to the top level.
        let main = (commonDirectory as NSString).lastPathComponent == ".git" ? Paths.parent(commonDirectory) : toplevel
        return (Paths.unresolved(main, relativeTo: path), Paths.unresolved(toplevel, relativeTo: path))
    }

    /// The main root for `project`, re-resolved only when the ref names a feature worktree (its `.git` is a file):
    /// CLI 0.13.4's `feature list --repo <feature worktree>` answers `[]`.
    public func mainRoot(of project: ProjectRef) async throws -> ProjectRef {
        guard case .file = fileSystem.kind(at: Paths.join(project.path, ".git")) else { return project }
        guard let roots = try await repositoryRoots(of: project.path) else { return project }
        return ProjectRef(root: URL(fileURLWithPath: roots.main, isDirectory: true))
    }

    // MARK: - Branches

    public func branches(in project: ProjectRef) async throws -> BranchList {
        let refs = try await checked(["-C", project.path, "for-each-ref", "--format=%(refname)", "refs/heads",
                                      "refs/remotes"], operation: "git for-each-ref")
        var local: [String] = [], remote: [String] = []
        for ref in Self.lines(refs.stdout) {
            if ref.hasPrefix("refs/heads/") {
                local.append(String(ref.dropFirst("refs/heads/".count)))
            } else if ref.hasPrefix("refs/remotes/"), !ref.hasSuffix("/HEAD") {
                remote.append(String(ref.dropFirst("refs/remotes/".count)))
            }
        }
        return BranchList(current: try await currentBranch(in: project.path), local: local, remote: remote)
    }

    /// The main worktree's checked-out branch; nil when `HEAD` is detached.
    func currentBranch(in root: String) async throws -> String? {
        let result = try await git(["-C", root, "symbolic-ref", "--short", "-q", "HEAD"], operation: "git symbolic-ref")
        guard result.termination == .exited(0) else { return nil }
        return Self.lines(result.stdout).first
    }

    public func branchExists(_ branch: String, in root: String) async throws -> Bool {
        let result = try await git(["-C", root, "show-ref", "--verify", "--quiet", "refs/heads/\(branch)"],
                                   operation: "git show-ref")
        switch result.termination {
        case .exited(0): return true
        case .exited(1): return false
        default: throw failure(result, ["-C", root, "show-ref", "--verify", "--quiet", "refs/heads/\(branch)"])
        }
    }

    /// See `BranchMergeState`.
    public func mergeState(of branch: String, in root: String) async throws -> BranchMergeState {
        let headName = try await currentBranch(in: root) ?? "HEAD"
        guard try await branchExists(branch, in: root) else { return .missing(branch, headName: headName) }
        let ref = "refs/heads/\(branch)"

        var upstream: String?
        var referenceSHA: String
        let headSHA = try await revParse("HEAD", in: root)
        referenceSHA = headSHA
        // `<ref>@{upstream}` only works with a short name, which a same-named tag would shadow; ask for-each-ref
        // instead, then make sure the upstream still resolves (a deleted remote branch falls back to HEAD).
        let configured = try await checked(["-C", root, "for-each-ref", "--format=%(upstream) %(upstream:short)", ref],
                                           operation: "git for-each-ref")
        let names = Self.lines(configured.stdout).first?.split(separator: " ").map(String.init) ?? []
        if names.count == 2 {
            let resolved = try await git(["-C", root, "rev-parse", "--verify", "--quiet", "\(names[0])^{commit}"],
                                         operation: "git rev-parse")
            if resolved.termination == .exited(0), let sha = Self.lines(resolved.stdout).first {
                upstream = names[1]
                referenceSHA = sha
            }
        }

        let merged = try await isAncestor(ref, of: referenceSHA, in: root)
        let mergedIntoHead: Bool
        if referenceSHA == headSHA {
            mergedIntoHead = merged
        } else {
            mergedIntoHead = try await isAncestor(ref, of: headSHA, in: root)
        }
        let count = try await checked(["-C", root, "rev-list", "--count", "\(referenceSHA)..\(ref)"],
                                      operation: "git rev-list")
        let ahead = Int(Self.lines(count.stdout).first ?? "") ?? 0
        return BranchMergeState(branch: branch, exists: true, upstream: upstream, reference: upstream ?? "HEAD",
                                referenceName: upstream ?? headName, merged: merged, mergedIntoHead: mergedIntoHead,
                                ahead: ahead)
    }

    private func revParse(_ revision: String, in root: String) async throws -> String {
        let result = try await checked(["-C", root, "rev-parse", "--verify", revision], operation: "git rev-parse")
        return Self.lines(result.stdout).first ?? ""
    }

    private func isAncestor(_ ancestor: String, of descendant: String, in root: String) async throws -> Bool {
        let arguments = ["-C", root, "merge-base", "--is-ancestor", ancestor, descendant]
        let result = try await git(arguments, operation: "git merge-base")
        switch result.termination {
        case .exited(0): return true
        case .exited(1): return false
        default: throw failure(result, arguments)
        }
    }

    /// `git branch -d|-D`. An unmerged branch refused by `-d` is `.refused(.unmergedBranch)`; anything else that
    /// fails is `.commandFailed` naming git's own message.
    public func deleteBranch(_ branch: String, in root: String, force: Bool) async throws {
        let arguments = ["-C", root, "branch", force ? "-D" : "-d", branch]
        let result = try await git(arguments, operation: "git branch")
        guard result.termination != .exited(0) else { return }
        let message = Self.gitMessage(result) ?? "git branch \(force ? "-D" : "-d") \(branch) failed"
        if message.contains("not fully merged") {
            let refusal = Refusal(cause: .unmergedBranch(branch: branch, ahead: nil),
                                  message: "\(branch) has commits that are not merged; keep it or force-delete it",
                                  diagnostics: diagnostics(result, arguments, summary: message))
            throw BackendError.refused(refusal)
        }
        throw BackendError.commandFailed(diagnostics(result, arguments, summary: message))
    }

    // MARK: - Worktrees

    public func worktrees(in root: String) async throws -> [WorktreeEntry] {
        let result = try await checked(["-C", root, "worktree", "list", "--porcelain"], operation: "git worktree list")
        return WorktreeListParser.parse(String(decoding: result.stdout, as: UTF8.self))
    }

    /// Every change in `worktree`, ignored files excepted (`status --porcelain=v1 -z --untracked-files=all
    /// --no-renames --ignore-submodules=none`). Throws `.commandFailed` naming git's message when status fails.
    ///
    /// `worktree` must be its own work tree: when its `.git` link is missing and the folder sits inside another
    /// repository, git would report that repository's status (often empty), so a teardown would see no changes.
    public func status(of worktree: String) async throws -> [GitStatusEntry] {
        let toplevel = try await checked(["-C", worktree, "rev-parse", "--show-toplevel"], operation: "git rev-parse")
        let top = String(decoding: toplevel.stdout, as: UTF8.self).trimmingCharacters(in: .whitespacesAndNewlines)
        guard Paths.same(top, worktree) else {
            let message = "\(worktree) is not its own git work tree: git reads the enclosing repository \(top) "
                + "(the worktree's .git link is missing or broken)"
            throw BackendError.commandFailed(Diagnostics(summary: message,
                                                         invocation: invocation.invocation(executable, ["-C", worktree, "rev-parse", "--show-toplevel"])))
        }
        let result = try await checked(["-C", worktree, "status", "--porcelain=v1", "-z", "--untracked-files=all",
                                        "--no-renames", "--ignore-submodules=none"],
                                       operation: "git status", timeout: Self.statusTimeout)
        return GitStatusParser.parse(result.stdout)
    }

    /// `git show <revision>:<path>` from `worktree`; nil when the path is not in that revision.
    public func contents(of path: String, at revision: String = "HEAD", in worktree: String) async throws -> Data? {
        let result = try await git(["-C", worktree, "show", "\(revision):\(path)"], operation: "git show")
        return result.termination == .exited(0) ? result.stdout : nil
    }

    /// `git worktree remove [--force] <path>` from the main root.
    public func removeWorktree(_ path: String, in root: String, force: Bool) async throws {
        var arguments = ["-C", root, "worktree", "remove"]
        if force { arguments.append("--force") }
        arguments.append(path)
        let result = try await git(arguments, operation: "git worktree remove", timeout: .seconds(30))
        guard result.termination != .exited(0) else { return }
        let message = Self.gitMessage(result) ?? "git worktree remove failed"
        if message.contains("locked working tree") {
            let reason = message.range(of: "lock reason: ").map { String(message[$0.upperBound...]) }
            throw BackendError.refused(Refusal(cause: .worktreeLocked(reason: reason),
                                               message: "\(path) is locked; unlock it with `git worktree unlock` first",
                                               diagnostics: diagnostics(result, arguments, summary: message)))
        }
        throw BackendError.commandFailed(diagnostics(result, arguments, summary: message))
    }

    // MARK: - Running git

    func git(_ arguments: [String], operation: String, timeout: Duration = GitInspector.readTimeout) async throws
        -> ProcessResult {
        try await invocation.run(executable, arguments, ToolInvocation.Options(operation: operation, timeout: timeout))
    }

    /// `git`, throwing `.commandFailed` with git's message unless it exits 0.
    func checked(_ arguments: [String], operation: String, timeout: Duration = GitInspector.readTimeout) async throws
        -> ProcessResult {
        let result = try await git(arguments, operation: operation, timeout: timeout)
        guard result.termination == .exited(0) else { throw failure(result, arguments) }
        return result
    }

    func failure(_ result: ProcessResult, _ arguments: [String]) -> BackendError {
        let summary = Self.gitMessage(result) ?? "`git \(arguments.dropFirst(2).joined(separator: " "))` failed"
        return .commandFailed(diagnostics(result, arguments, summary: summary))
    }

    func diagnostics(_ result: ProcessResult, _ arguments: [String], summary: String) -> Diagnostics {
        Diagnostics(summary: summary, exitCode: ToolInvocation.exitCode(of: result.termination),
                    signal: ToolInvocation.signal(of: result.termination), logTail: result.stderrTail,
                    invocation: invocation.invocation(executable, arguments))
    }

    /// git's own cause line: the last `fatal:` or `error:` line, else the last non-empty stderr line.
    static func gitMessage(_ result: ProcessResult) -> String? {
        let lines = result.stderrTail.map { $0.trimmingCharacters(in: .whitespaces) }.filter { !$0.isEmpty }
        return lines.last { $0.hasPrefix("fatal:") || $0.hasPrefix("error:") } ?? lines.last
    }

    static func lines(_ data: Data) -> [String] {
        String(decoding: data, as: UTF8.self).split(whereSeparator: \.isNewline).map(String.init)
    }
}
