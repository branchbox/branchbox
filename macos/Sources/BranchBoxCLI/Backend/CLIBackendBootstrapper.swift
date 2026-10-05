import BranchBoxKit
import Foundation

/// Builds the CLI backend (DESIGN §7): starts the login-shell capture, locates the CLI on its PATH (D-8), probes
/// its version and capabilities, and hands back a `CLIBackend` — or why there is none.
///
/// D-8 searches the login PATH before the well-known folders, so the locator needs the login PATH:
///
/// - When the provisional environment carries the PATH remembered from an earlier capture (every launch but the
///   first), it is searched at once, and the captured PATH only if nothing was found there.
/// - On the first launch there is no remembered PATH, so bootstrapping waits for the capture (typically 1–2 s).
///   Otherwise `/opt/homebrew/bin/branchbox` could shadow a `branchbox` earlier on the user's PATH.
public struct CLIBackendBootstrapper: BackendBootstrapping {
    let runner: any ProcessRunning
    let environment: EnvironmentProvider
    let locator: CLILocator
    let probe: CLIProbe
    let fileSystem: any FileSystemProbing
    let bundleURL: URL?

    /// - Parameters:
    ///   - environment: nil uses the user's login shell and Application Support caches.
    ///   - probe: nil probes with `runner` and caches in Application Support.
    ///   - bundleURL: the app bundle whose `Contents/Helpers/branchbox` is the last candidate.
    public init(runner: any ProcessRunning = ProcessRunner(), environment: EnvironmentProvider? = nil,
                locator: CLILocator = CLILocator(), probe: CLIProbe? = nil,
                fileSystem: any FileSystemProbing = LocalFileSystem(), bundleURL: URL? = Bundle.main.bundleURL) {
        self.runner = runner
        self.environment = environment ?? EnvironmentProvider(runner: runner)
        self.locator = locator
        self.probe = probe ?? CLIProbe(runner: runner)
        self.fileSystem = fileSystem
        self.bundleURL = bundleURL
    }

    public func bootstrap(_ settings: BackendSettings) async -> BackendBootstrap {
        await environment.startCapture()
        let current = await environment.summary()
        let remembered = !current.isProvisional || current.source == .cachedPath
        var outcome = await locate(settings, purpose: remembered ? .read : .mutation)
        if outcome.resolution == nil, remembered { outcome = await locate(settings, purpose: .mutation) }
        guard let resolution = outcome.resolution else {
            return .unavailable(.cliNotFound(searched: outcome.searched), nil)
        }
        await environment.setCLIPath(resolution.path)
        do {
            let identity = try await probe.identity(for: resolution,
                                                    environment: await environment.childEnvironment(for: .read,
                                                                                                    settings: settings))
            let backend = CLIBackend(executable: URL(fileURLWithPath: resolution.path), identity: identity,
                                     runner: runner, environment: environment, settings: settings,
                                     fileSystem: fileSystem, probe: probe)
            return .ready(backend, identity)
        } catch {
            return .unavailable(BackendError.normalize(error), resolution)
        }
    }

    private func locate(_ settings: BackendSettings, purpose: EnvironmentPurpose) async -> CLILocator.Outcome {
        let configuration = environment.configuration
        let base = await environment.baseEnvironment(for: purpose)
        return locator.locate(processEnvironment: configuration.processEnvironment,
                              settingsOverride: settings.cliPathOverride, searchPath: base["PATH"],
                              home: configuration.home, bundleURL: bundleURL)
    }

    public func environmentSummary() async -> EnvironmentSummary? {
        await environment.summary()
    }

    public func recaptureEnvironment() async {
        await environment.recapture()
    }

    /// App quit: SIGINT → SIGTERM → SIGKILL every live process group, bounded at 10 s.
    public func terminateAllProcesses() async {
        await runner.terminateAll()
    }
}
