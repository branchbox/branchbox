import BranchBoxCLI
import BranchBoxKit
import Foundation

/// A disposable git repository for the gated integration suites (DESIGN §13.2), laid out the way BranchBox expects:
/// `<container>/main` is the repository and feature worktrees are created beside it, so removing the container
/// removes every worktree, branch and registry entry a test made.
///
/// - The container lives in `$BRANCHBOX_IT_TMP` (default `$TMPDIR`) as `branchbox-it-<uuid>`.
/// - `git init -b main`, a `.gitignore` (plus `ignoring`, for example the entries `branchbox init` adds) and one
///   commit made with `-c user.email/-c user.name` (no signing), so the user's git configuration cannot break it.
/// - `remove()` is synchronous for use in `defer`; `sweep()` deletes containers older than an hour that a crashed
///   run left behind.
struct TempRepo {
    static let prefix = "branchbox-it-"
    static let git = URL(fileURLWithPath: "/usr/bin/git")

    let container: URL
    var main: URL { container.appendingPathComponent("main", isDirectory: true) }
    var project: ProjectRef { ProjectRef(root: main) }

    static var baseDirectory: URL {
        if let configured = ProcessInfo.processInfo.environment["BRANCHBOX_IT_TMP"], !configured.isEmpty {
            return URL(fileURLWithPath: configured, isDirectory: true)
        }
        return FileManager.default.temporaryDirectory
    }

    /// The `.gitignore` lines `branchbox init` adds (core's `GITIGNORE_ENTRIES`), for an initialized-project layout.
    static let branchBoxIgnores = [
        ".branchbox/registry.json", ".branchbox/secure/", ".branchbox/runtime/", ".branchbox/devcontainer-sync/",
        ".branchbox/.registry.*.tmp", ".branchbox/.lock", ".devcontainer/.branchbox.env", ".devcontainer/.cloudflared.env",
        ".devcontainer/.branchbox-sbx-compose.yaml", ".devcontainer/.devcontainer.json", ".devcontainer/.github-token.env",
        ".devcontainer/.git-signing-key", ".devcontainer/.gitconfig.env", ".branchbox.env", ".env", ".env.local",
    ]

    static func make(ignoring: [String] = []) async throws -> TempRepo {
        sweep()
        let container = baseDirectory.appendingPathComponent(prefix + UUID().uuidString.prefix(8), isDirectory: true)
        let repo = TempRepo(container: container)
        try FileManager.default.createDirectory(at: repo.main, withIntermediateDirectories: true)
        do {
            try await repo.git(["init", "-q", "-b", "main"])
            try Data("hello\n".utf8).write(to: repo.main.appendingPathComponent("README.md"))
            let ignores = ([".DS_Store"] + ignoring).joined(separator: "\n") + "\n"
            try Data(ignores.utf8).write(to: repo.main.appendingPathComponent(".gitignore"))
            try await repo.git(["add", "-A"])
            try await repo.git(["-c", "user.email=it@example.com", "-c", "user.name=BranchBox IT", "-c", "commit.gpgsign=false",
                                "commit", "-q", "-m", "init"])
        } catch {
            repo.remove()
            throw error
        }
        return repo
    }

    func remove() {
        try? FileManager.default.removeItem(at: container)
    }

    /// Removes containers from earlier runs that are more than an hour old.
    static func sweep() {
        let files = FileManager.default
        guard let entries = try? files.contentsOfDirectory(at: baseDirectory, includingPropertiesForKeys: [.creationDateKey])
        else { return }
        for entry in entries where entry.lastPathComponent.hasPrefix(prefix) {
            let created = (try? entry.resourceValues(forKeys: [.creationDateKey]))?.creationDate ?? .distantFuture
            if created < Date().addingTimeInterval(-3600) { try? files.removeItem(at: entry) }
        }
    }

    /// `git -C <main> <arguments>`; stdout, or an error naming git's message.
    @discardableResult
    func git(_ arguments: [String]) async throws -> String {
        var spec = ProcessSpec(executable: Self.git, arguments: ["-C", main.path] + arguments,
                               environment: ["PATH": "/usr/bin:/bin", "HOME": NSHomeDirectory(), "GIT_TERMINAL_PROMPT": "0"],
                               workingDirectory: nil)
        spec.timeout = .seconds(30)
        let result = try await ProcessRunner().run(spec) { _ in }
        guard result.termination == .exited(0) else {
            throw TempRepoError(message: "git \(arguments.joined(separator: " ")) failed: \(result.stderrTail.joined(separator: " "))")
        }
        return String(decoding: result.stdout, as: UTF8.self)
    }

    /// Worktree paths git knows, the main one included.
    func worktrees() async throws -> [String] {
        try await git(["worktree", "list", "--porcelain"]).split(separator: "\n")
            .filter { $0.hasPrefix("worktree ") }.map { String($0.dropFirst("worktree ".count)) }
    }

    func branches() async throws -> [String] {
        try await git(["for-each-ref", "--format=%(refname:short)", "refs/heads"]).split(separator: "\n").map(String.init)
    }
}

struct TempRepoError: Error, CustomStringConvertible {
    let message: String
    var description: String { message }
}
