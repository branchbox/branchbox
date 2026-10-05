import BranchBoxCLI
import BranchBoxKit
import Foundation
import Testing

// Shared pieces of the VER-1 suites (DESIGN §13.2): the gate, a backend on the real CLI, request helpers and
// process checks. Every suite runs only with BRANCHBOX_IT=1, in TempRepos it removes, with the child environment
// starting from launchd's PATH (`CLISmokeTests.bootstrapper` documents why).

/// The real-CLI suites run only with `BRANCHBOX_IT=1`.
let integrationEnabled = ProcessInfo.processInfo.environment["BRANCHBOX_IT"] == "1"

/// Every VER-1 real-CLI suite is nested here, so they run one at a time: their time limits (cancel within 6 s,
/// a refresh within 2 s, a 40-feature list within 10 s) must not compete with each other for the CPU.
@Suite(.serialized) struct RealCLI {}

/// A `CLIBackend` on the CLI under test, with its own `ProcessRunner` (for `terminateAll`) and identity.
struct LiveCLI {
    let runner: ProcessRunner
    let backend: any BranchBoxBackend
    let identity: BackendIdentity
    let bootstrapper: CLIBackendBootstrapper

    /// Contract mode is the presence of `contract_version`, never a version number (wave-2 deviations).
    var isContract: Bool { identity.contractVersion != nil }
    var mode: String { isContract ? "contract" : "legacy" }

    /// The CLI's path as resolved by the locator.
    var executable: URL {
        if let backend = backend as? CLIBackend { return backend.executable }
        if case .cli(let resolution) = identity.kind { return URL(fileURLWithPath: resolution.path) }
        return URL(fileURLWithPath: "/usr/bin/false")
    }

    /// The launchd-like environment the backend's children start from, plus `extraEnvironment` (the app's
    /// Settings › Tools extra variables, which reach every child).
    static func environment() -> [String: String] {
        let process = ProcessInfo.processInfo.environment
        var environment = ["PATH": "/usr/bin:/bin:/usr/sbin:/sbin", "HOME": NSHomeDirectory(), "LANG": "en_US.UTF-8"]
        for key in ["USER", "LOGNAME", "TMPDIR", "SSH_AUTH_SOCK"] { environment[key] = process[key] }
        if let cli = process["BRANCHBOX_IT_CLI"], !cli.isEmpty { environment[CLILocator.environmentOverrideKey] = cli }
        return environment
    }

    static func bootstrapper(runner: ProcessRunner) -> CLIBackendBootstrapper {
        let configuration = EnvironmentProvider.Configuration(shell: "/usr/bin/false", processEnvironment: environment(),
                                                              home: NSHomeDirectory(), cacheDirectory: nil)
        return CLIBackendBootstrapper(runner: runner, environment: EnvironmentProvider(runner: runner, configuration: configuration),
                                      probe: CLIProbe(runner: runner, cacheDirectory: nil), bundleURL: nil)
    }

    static func make(extraEnvironment: [String: String] = [:]) async throws -> LiveCLI {
        let runner = ProcessRunner()
        let bootstrapper = bootstrapper(runner: runner)
        let bootstrap = await bootstrapper.bootstrap(BackendSettings(extraEnvironment: extraEnvironment))
        guard case .ready(let backend, let identity) = bootstrap else {
            throw TempRepoError(message: "the CLI is unavailable: \(bootstrap)")
        }
        return LiveCLI(runner: runner, backend: backend, identity: identity, bootstrapper: bootstrapper)
    }

    /// `start --minimal --skip-module tunnel` (Docker-free) on the container runtime unless told otherwise.
    static func minimalStart(_ name: String, in project: ProjectRef, runtime: RuntimeProvider = .container,
                             branchPrefix: String? = nil) -> StartFeatureRequest {
        var request = StartFeatureRequest(project: project, name: name, runtime: runtime)
        request.mode = .minimal
        request.skipModules = ["tunnel"]
        request.branchPrefix = branchPrefix
        return request
    }

    /// Starts a minimal feature and returns its summary and its record from `list`.
    @discardableResult
    func start(_ name: String, in repo: TempRepo, branchPrefix: String? = nil) async throws -> (StartSummary, FeatureRecord) {
        let summary = try await backend.startFeature(Self.minimalStart(name, in: repo.project, branchPrefix: branchPrefix),
                                                     progress: { _ in })
        let listing = try await backend.listFeatures(in: repo.project, includeRemoved: false)
        let record = try #require(listing.features.first { $0.workFeature == name }, "\(name) is not listed after start")
        return (summary, record)
    }

    /// Runs the CLI directly (as a user would in Terminal) with the same launchd-like environment.
    func run(_ arguments: [String], in folder: URL, extraEnvironment: [String: String] = [:],
             timeout: Duration = .seconds(60)) async throws -> ProcessResult {
        var spec = ProcessSpec(executable: executable, arguments: arguments,
                               environment: Self.environment().merging(extraEnvironment) { _, extra in extra },
                               workingDirectory: folder)
        spec.timeout = timeout
        return try await ProcessRunner().run(spec) { _ in }
    }
}

/// Progress events collected from a `ProgressSink`.
final class ProgressLog: @unchecked Sendable {
    private let lock = NSLock()
    private var events: [ProgressEvent] = []

    var record: ProgressSink { { [self] event in lock.withLock { events.append(event) } } }

    var phases: [OperationPhase] {
        lock.withLock { events.compactMap { if case .phase(let phase) = $0 { return phase } else { return nil } } }
    }

    var warnings: [String] {
        lock.withLock { events.compactMap { if case .warning(let text) = $0 { return text } else { return nil } } }
    }
}

extension TempRepo {
    /// `<container>/<name>`, where BranchBox puts a feature's worktree.
    func worktree(_ name: String) -> URL { container.appendingPathComponent(name, isDirectory: true) }

    /// Writes `text` to `path` inside `folder` (default: the main worktree), creating parent folders.
    func write(_ text: String, to path: String, in folder: URL? = nil) throws {
        let url = (folder ?? main).appendingPathComponent(path)
        try FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
        try Data(text.utf8).write(to: url)
    }

    /// `git add -A && git commit` in `folder` (default: main), without the user's identity or signing.
    func commitAll(in folder: URL? = nil, message: String) async throws {
        let directory = (folder ?? main).path
        try await git(["-C", directory, "add", "-A"])
        try await git(["-C", directory, "-c", "user.email=it@example.com", "-c", "user.name=BranchBox IT",
                       "-c", "commit.gpgsign=false", "commit", "-q", "-m", message])
    }

    /// Installs an executable git hook in the main repository (shared by every worktree).
    func installHook(_ name: String, script: String) throws {
        let hook = main.appendingPathComponent(".git/hooks/\(name)")
        try FileManager.default.createDirectory(at: hook.deletingLastPathComponent(), withIntermediateDirectories: true)
        try Data(script.utf8).write(to: hook)
        try FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: hook.path)
    }

    func removeHook(_ name: String) {
        try? FileManager.default.removeItem(at: main.appendingPathComponent(".git/hooks/\(name)"))
    }

    /// The registry file's bytes, or nil before the first feature.
    func registry() -> Data? {
        try? Data(contentsOf: main.appendingPathComponent(".branchbox/registry.json"))
    }
}

/// Processes whose command line mentions `needle` (normally a TempRepo's container path), from `pgrep -fl`.
func processes(mentioning needle: String) async throws -> [String] {
    var spec = ProcessSpec(executable: URL(fileURLWithPath: "/usr/bin/pgrep"), arguments: ["-fl", needle],
                           environment: ["PATH": "/usr/bin:/bin"], workingDirectory: nil)
    spec.timeout = .seconds(10)
    let result = try await ProcessRunner().run(spec) { _ in }
    // pgrep never lists itself; its own argv mentions the needle, so drop any line that is the pgrep run.
    return String(decoding: result.stdout, as: UTF8.self).split(separator: "\n").map(String.init)
        .filter { !$0.contains("pgrep") }
}

/// Seconds since `start`, for the suites' timing assertions.
func elapsed(since start: ContinuousClock.Instant) -> Double {
    let duration = ContinuousClock.now - start
    return Double(duration.components.seconds) + Double(duration.components.attoseconds) / 1e18
}
