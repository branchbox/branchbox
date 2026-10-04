import BranchBoxCLI
import BranchBoxKit
import BranchBoxTestSupport
import Foundation

/// A `FileSystemProbing` whose answers can change while a test runs (a teardown removes the worktree).
final class MutableFileSystem: FileSystemProbing, @unchecked Sendable {
    private let lock = NSLock()
    private var kinds: [String: FileKind]

    init(_ kinds: [String: FileKind] = [:]) { self.kinds = kinds }

    func set(_ path: String, _ kind: FileKind?) { lock.withLock { kinds[path] = kind } }
    func kind(at path: String) -> FileKind { lock.withLock { kinds[path] ?? .missing } }
    func identity(at path: String) -> FileIdentity? { nil }
}

/// Wraps a `ScriptedProcessRunner` and calls `afterRun` once each run returns, so a test can apply a command's side
/// effects (the worktree disappearing after a successful `feature teardown`).
struct ObservingRunner: ProcessRunning {
    let inner: ScriptedProcessRunner
    let afterRun: @Sendable (ProcessSpec, ProcessResult) -> Void

    func run(_ spec: ProcessSpec, onLine: @escaping @Sendable (OutputLine) -> Void) async throws -> ProcessResult {
        let result = try await inner.run(spec, onLine: onLine)
        afterRun(spec, result)
        return result
    }

    func terminateAll() async { await inner.terminateAll() }
}

/// The project and feature every scripted backend test uses: main at `/r/main`, feature `eta` at `/r/eta` on
/// `feature/eta`. Nothing under `/r` exists on disk.
enum Scripted {
    static let cli = "/opt/homebrew/bin/branchbox"
    static let project = ProjectRef(root: URL(fileURLWithPath: "/r/main"))
    static let eta = FeatureRef(project: project, name: "eta")
    static let any = ScriptedProcessRunner.anyArgument

    static func identity(contract: Bool = false, _ capabilities: Set<Capability> = []) -> BackendIdentity {
        BackendIdentity(kind: .cli(CLIResolution(path: cli, source: .wellKnownPath)), version: SemVer(0, 13, 4),
                        contractVersion: contract ? 1 : nil, capabilities: capabilities)
    }

    /// Every capability a fully migrated 0.14 CLI advertises.
    static let everything: Set<Capability> = [
        .jsonErrorEnvelope, .registryLock, .writeAheadStart, .teardownPlan, .teardownDiscardChanges,
        .teardownUnmergedPreflight, .pruneJSON, .detectJSON, .devcontainerSyncJSON, .config, .tunnelCredentials,
        .doctor, .initJSON, .hostContainerTeardownVerified,
    ]

    static func backend(_ runner: any ProcessRunning, identity: BackendIdentity = identity(),
                        fileSystem: any FileSystemProbing = MutableFileSystem(["/r/eta": .directory, "/r/eta/.git": .directory]),
                        environment: StaticEnvironment = StaticEnvironment(),
                        settings: BackendSettings = BackendSettings()) -> CLIBackend {
        CLIBackend(executable: URL(fileURLWithPath: cli), identity: identity, runner: runner, environment: environment,
                   settings: settings, fileSystem: fileSystem, gitExecutable: URL(fileURLWithPath: "/usr/bin/git"))
    }

    static let etaRecord = #"[{"work_feature":"eta","branch_name":"feature/eta","worktree_path":"/r/eta","status":"active","runtime":{"provider":"container"}}]"#

    static let teardownSummary = """
        {"work_feature":"eta","branch_name":"feature/eta","worktree_removed":true,"branch_deleted":false,
         "adapter_cleanup_warnings":[],"module_reports":[{"name":"specs","teardown_ok":true,"errors":[]}],
         "runtime_teardown":{"provider":"container","verified":true,"residue_free":true,"residue":[]},
         "warnings":["Tunnel descriptor missing; skipping provider teardown"]}
        """

    /// Everything the app preflight asks: the registry record, the worktree list, `git status` of `/r/eta` (NUL
    /// separated), and a `feature/eta` merge state against `main`.
    static func preflight(status: String = "", branchExists: Bool = true, merged: Bool = true, ahead: Int = 0,
                          locked: Bool = false, statusFails: Bool = false) -> [ScriptedProcessRunner.Rule] {
        let lock = locked ? "locked on a USB disk\n" : ""
        return [
            .exit(["branchbox", "feature", "list"], stdout: etaRecord),
            .exit(["git", "-C", "/r/main", "worktree", "list", "--porcelain"],
                  stdout: "worktree /r/main\nHEAD aaa\nbranch refs/heads/main\n\nworktree /r/eta\nHEAD bbb\nbranch refs/heads/feature/eta\n\(lock)\n"),
            .exit(["git", "-C", "/r/eta", "rev-parse", "--show-toplevel"], stdout: "/r/eta\n"),
            statusFails
                ? .exit(["git", "-C", "/r/eta", "status"], 128, stderr: ["fatal: index file corrupt"])
                : .exit(["git", "-C", "/r/eta", "status"], stdout: status),
            .exit(["git", "-C", "/r/main", "symbolic-ref"], stdout: "main\n"),
            .exit(["git", "-C", "/r/main", "show-ref", "--verify", "--quiet", "refs/heads/feature/eta"], branchExists ? 0 : 1),
            .exit(["git", "-C", "/r/main", "rev-parse", "--verify", "HEAD"], stdout: "aaa\n"),
            .exit(["git", "-C", "/r/main", "for-each-ref"], stdout: " \n"),
            .exit(["git", "-C", "/r/main", "merge-base", "--is-ancestor"], merged ? 0 : 1),
            .exit(["git", "-C", "/r/main", "rev-list", "--count"], stdout: "\(ahead)\n"),
        ]
    }

    static func arguments(_ runner: ScriptedProcessRunner) -> [[String]] {
        runner.invocations.map { [($0[0] as NSString).lastPathComponent] + $0.dropFirst() }
    }

    static func ran(_ runner: ScriptedProcessRunner, _ prefix: [String]) -> Bool {
        arguments(runner).contains { Array($0.prefix(prefix.count)) == prefix }
    }
}
