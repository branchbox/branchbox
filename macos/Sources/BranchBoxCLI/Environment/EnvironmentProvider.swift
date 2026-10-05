import BranchBoxKit
import Foundation
import os

/// Supplies child environments from the login-shell capture (DESIGN §7.2).
///
/// - The capture starts with `startCapture()` (or the first request) and runs in the background.
/// - `.read` never waits: until the capture finishes it gets the provisional environment, which is the process
///   environment with the PATH persisted by the last capture in front.
/// - `.mutation` waits for the capture (at most about 8 s + 5 s), so a `feature start` sees the user's real
///   PATH.
/// - When both shell attempts fail, the process environment is used; `ChildEnvironment.make` still adds the
///   well-known tool directories.
/// - Only PATH is persisted (`environment-cache.json`); everything else stays in memory.
public actor EnvironmentProvider: EnvironmentProviding {
    public struct Configuration: Sendable {
        public var shell: String
        public var processEnvironment: [String: String]
        public var home: String
        /// Where `environment-cache.json` lives; nil keeps the PATH in memory only.
        public var cacheDirectory: URL?
        public var interactiveTimeout: Duration = LoginShellEnvironment.interactiveTimeout
        public var loginTimeout: Duration = LoginShellEnvironment.loginTimeout

        public init(shell: String, processEnvironment: [String: String], home: String, cacheDirectory: URL?) {
            self.shell = shell
            self.processEnvironment = processEnvironment
            self.home = home
            self.cacheDirectory = cacheDirectory
        }

        /// The user's shell and home, this process's environment, and Application Support.
        public static func standard() -> Configuration {
            let environment = ProcessInfo.processInfo.environment
            return Configuration(shell: LoginShellEnvironment.userShell(processEnvironment: environment),
                                 processEnvironment: environment, home: NSHomeDirectory(),
                                 cacheDirectory: ApplicationSupport.directory)
        }
    }

    static let cacheFileName = "environment-cache.json"

    /// The persisted snapshot: the captured PATH and when it was captured, nothing else.
    struct CachedPath: Codable, Equatable {
        var path: String
        var capturedAt: Date

        private enum CodingKeys: String, CodingKey { case path = "PATH", capturedAt = "captured_at" }
    }

    private enum CaptureState {
        case notStarted
        case running(Task<LoginShellEnvironment?, Never>)
        case finished(LoginShellEnvironment?)
    }

    private static let logger = Logger(subsystem: "dev.branchbox.app", category: "environment")

    public let configuration: Configuration
    private let runner: any ProcessRunning
    private var state: CaptureState = .notStarted
    /// The last successful capture; a failed re-capture keeps it.
    private var captured: LoginShellEnvironment?
    private var cachedPath: CachedPath?
    private var loadedCache = false
    private var cliPath: String?

    public init(runner: any ProcessRunning = ProcessRunner(), configuration: Configuration = .standard()) {
        self.runner = runner
        self.configuration = configuration
    }

    /// Starts the login-shell capture if it has not started yet; returns immediately.
    public func startCapture() {
        _ = captureTask()
    }

    /// The unresolved CLI path; its directory leads every child PATH.
    public func setCLIPath(_ path: String?) {
        cliPath = path
    }

    public func childEnvironment(for purpose: EnvironmentPurpose,
                                 settings: BackendSettings) async -> [String: String] {
        ChildEnvironment.make(base: await baseEnvironment(for: purpose),
                              processEnvironment: configuration.processEnvironment, cliPath: cliPath,
                              settings: settings, home: configuration.home)
    }

    /// The environment `ChildEnvironment.make` starts from: the captured login environment, the provisional one
    /// (`.read` before the capture finished), or the process environment (the capture failed). `CLILocator`
    /// searches its PATH.
    public func baseEnvironment(for purpose: EnvironmentPurpose) async -> [String: String] {
        switch purpose {
        case .read:
            startCapture()
            if let captured { return captured.variables }
            if case .finished = state { return configuration.processEnvironment }
            return provisionalEnvironment()
        case .mutation:
            await finishCapture()
            return captured?.variables ?? configuration.processEnvironment
        }
    }

    public func summary() async -> EnvironmentSummary {
        let finished: Bool
        if case .finished = state { finished = true } else { finished = false }
        let base: [String: String]
        let source: EnvironmentSummary.Source
        let capturedAt: Date?
        if let captured {
            (base, source, capturedAt) = (captured.variables, captured.source, captured.capturedAt)
        } else if finished {
            (base, source, capturedAt) = (configuration.processEnvironment, .processEnvironment, nil)
        } else {
            let cache = loadCache()
            (base, source, capturedAt) = (provisionalEnvironment(), cache == nil ? .processEnvironment : .cachedPath,
                                          cache?.capturedAt)
        }
        // Without the user's extra variables: their values are never shown.
        let child = ChildEnvironment.make(base: base, processEnvironment: configuration.processEnvironment,
                                          cliPath: cliPath, settings: BackendSettings(), home: configuration.home)
        return EnvironmentSummary(source: source, shell: configuration.shell, captureDuration: captured?.duration,
                                  pathEntries: (child["PATH"] ?? "").split(separator: ":").map(String.init),
                                  capturedAt: capturedAt, isProvisional: captured == nil && !finished)
    }

    /// Runs a fresh capture (Settings › Re-capture) and waits for it. A failed re-capture keeps the previous
    /// environment.
    public func recapture() async {
        await finishCapture()
        let task = makeCaptureTask()
        state = .running(task)
        await complete(task)
    }

    // MARK: - Capture

    private func captureTask() -> Task<LoginShellEnvironment?, Never>? {
        switch state {
        case .notStarted:
            let task = makeCaptureTask()
            state = .running(task)
            Task { await self.complete(task) }
            return task
        case .running(let task):
            return task
        case .finished:
            return nil
        }
    }

    private func finishCapture() async {
        guard let task = captureTask() else { return }
        await complete(task)
    }

    private func makeCaptureTask() -> Task<LoginShellEnvironment?, Never> {
        let configuration = configuration
        let runner = runner
        return Task.detached {
            await LoginShellEnvironment.capture(shell: configuration.shell,
                                                processEnvironment: configuration.processEnvironment, runner: runner,
                                                interactiveTimeout: configuration.interactiveTimeout,
                                                loginTimeout: configuration.loginTimeout)
        }
    }

    /// Records the outcome of `task` once, whichever caller gets there first.
    private func complete(_ task: Task<LoginShellEnvironment?, Never>) async {
        let result = await task.value
        guard case .running(let current) = state, current == task else { return }
        state = .finished(result)
        guard let result else {
            Self.logger.warning("Login-shell capture with \(self.configuration.shell, privacy: .public) failed")
            return
        }
        captured = result
        if let path = result.variables["PATH"] { persist(CachedPath(path: path, capturedAt: result.capturedAt)) }
    }

    // MARK: - Provisional environment and PATH cache

    /// The process environment with the cached login PATH ahead of the process PATH.
    private func provisionalEnvironment() -> [String: String] {
        var environment = configuration.processEnvironment
        if let cache = loadCache() {
            let entries = (cache.path + ":" + (environment["PATH"] ?? "")).split(separator: ":").map(String.init)
            environment["PATH"] = ChildEnvironment.deduplicated(entries).joined(separator: ":")
        }
        return environment
    }

    private var cacheFile: URL? {
        configuration.cacheDirectory?.appendingPathComponent(Self.cacheFileName)
    }

    private func loadCache() -> CachedPath? {
        if !loadedCache {
            loadedCache = true
            if let file = cacheFile, let data = try? Data(contentsOf: file) {
                cachedPath = try? CLIJSON.decoder().decode(CachedPath.self, from: data)
            }
        }
        return cachedPath
    }

    private func persist(_ cache: CachedPath) {
        cachedPath = cache
        loadedCache = true
        guard let file = cacheFile else { return }
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.prettyPrinted, .sortedKeys, .withoutEscapingSlashes]
        encoder.dateEncodingStrategy = .custom { date, encoder in
            var container = encoder.singleValueContainer()
            try container.encode(RFC3339.format(date))
        }
        do {
            try ApplicationSupport.write(try encoder.encode(cache), to: file)
        } catch {
            Self.logger.error(
                "Could not write \(file.path, privacy: .public): \(error.localizedDescription, privacy: .public)")
        }
    }
}
