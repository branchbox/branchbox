import BranchBoxCLI
import BranchBoxKit
import Foundation
import Testing

/// A disposable repository in BranchBox's layout under `$TMPDIR/branchbox-tests`: `<container>/main` is the main
/// worktree (one commit, branch `main`) and feature worktrees go beside it. git is the real `/usr/bin/git`, run with
/// an empty `HOME` and no system config so the user's settings (signing, hooks, default branch) cannot interfere.
/// Paths go through `$TMPDIR`, which is a symbolic link (`/var` → `/private/var`), so tests also cover git's
/// resolved output being mapped back. Call `remove()` in a `defer`.
struct GitSandbox {
    static let git = URL(fileURLWithPath: "/usr/bin/git")

    let container: URL
    let home: URL

    var main: String { container.appendingPathComponent("main").path }
    var mainRef: ProjectRef { ProjectRef(root: URL(fileURLWithPath: main)) }

    /// The environment every git run gets, including the inspector's.
    var environment: [String: String] {
        ["PATH": "/usr/bin:/bin:/usr/sbin:/sbin", "HOME": home.path, "GIT_CONFIG_NOSYSTEM": "1", "LANG": "en_US.UTF-8",
         "GIT_TERMINAL_PROMPT": "0"]
    }

    init(_ label: String = #function) async throws {
        let name = label.filter { $0.isLetter || $0.isNumber }.prefix(32)
        container = FileManager.default.temporaryDirectory
            .appendingPathComponent("branchbox-tests/git-\(name)-\(UUID().uuidString.prefix(8))", isDirectory: true)
        home = container.appendingPathComponent(".home", isDirectory: true)
        try FileManager.default.createDirectory(at: home, withIntermediateDirectories: true)
        try FileManager.default.createDirectory(atPath: main, withIntermediateDirectories: true)
        try await run(["init", "-q", "-b", "main"], in: main)
        try write("README.md", "hello\n", in: main)
        try await commitAll("init", in: main)
    }

    func remove() {
        try? FileManager.default.removeItem(at: container)
    }

    func inspector(runner: any ProcessRunning = ProcessRunner()) -> GitInspector {
        GitInspector(executable: Self.git, runner: runner, environment: environment)
    }

    func path(_ name: String) -> String { container.appendingPathComponent(name).path }

    // MARK: - Building repositories

    /// `git -C <directory> <arguments>`; throws with git's stderr unless it exits 0. Returns stdout.
    @discardableResult
    func run(_ arguments: [String], in directory: String) async throws -> String {
        var spec = ProcessSpec(executable: Self.git, arguments: ["-C", directory] + arguments, environment: environment,
                               workingDirectory: nil)
        spec.timeout = .seconds(30)
        let result = try await ProcessRunner().run(spec) { _ in }
        guard result.termination == .exited(0) else {
            throw SandboxError(message: "git \(arguments.joined(separator: " ")) failed: "
                               + result.stderrTail.joined(separator: "\n"))
        }
        return String(decoding: result.stdout, as: UTF8.self)
    }

    func commitAll(_ message: String, in directory: String) async throws {
        try await run(["add", "-A"], in: directory)
        try await run(["-c", "user.name=Test", "-c", "user.email=test@example.com", "-c", "commit.gpgsign=false",
                       "commit", "-q", "--allow-empty", "-m", message], in: directory)
    }

    /// `git worktree add -b <branch> <container>/<name>` from main; returns the worktree path.
    @discardableResult
    func addWorktree(_ name: String, branch: String) async throws -> String {
        let path = path(name)
        try await run(["worktree", "add", "-q", "-b", branch, path], in: main)
        return path
    }

    func write(_ relativePath: String, _ contents: String, in directory: String) throws {
        let url = URL(fileURLWithPath: directory).appendingPathComponent(relativePath)
        try FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
        try Data(contents.utf8).write(to: url)
    }

    func symlink(_ relativePath: String, to target: String, in directory: String) throws {
        let url = URL(fileURLWithPath: directory).appendingPathComponent(relativePath)
        try FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
        try FileManager.default.createSymbolicLink(atPath: url.path, withDestinationPath: target)
    }

    func exists(_ path: String) -> Bool {
        FileManager.default.fileExists(atPath: path)
    }
}

struct SandboxError: Error, CustomStringConvertible {
    let message: String
    var description: String { message }
}
