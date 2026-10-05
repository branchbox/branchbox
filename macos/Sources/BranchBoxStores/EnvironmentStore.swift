import BranchBoxKit
import Foundation
import Observation

public enum BackendState: Sendable, Hashable { case resolving, ready(BackendIdentity), unavailable(BackendError) }

/// Owns the backend: bootstraps it through the injected `BackendBootstrapping`, and exposes its identity,
/// the captured environment and the last doctor report.
@MainActor @Observable public final class EnvironmentStore {
    public private(set) var backendState: BackendState = .resolving
    public private(set) var identity: BackendIdentity?
    public private(set) var summary: EnvironmentSummary?
    public private(set) var doctor: DoctorReport?

    // Additive (SW-2).
    /// Where the CLI was found (or the candidates that were rejected) for Diagnostics; nil for other backends.
    public private(set) var resolution: CLIResolution?
    /// A bootstrap is running; the previous state stays visible meanwhile (no flicker on Settings changes).
    public private(set) var isBootstrapping = false
    /// When the last bootstrap finished.
    public private(set) var lastBootstrapAt: Date?
    public private(set) var isRunningDoctor = false

    /// The backend of the last successful bootstrap; nil while resolving or unavailable.
    @ObservationIgnored private(set) var backend: (any BranchBoxBackend)?
    let bootstrapper: any BackendBootstrapping
    private let settings: AppSettings
    private let clock: any StoreClock
    @ObservationIgnored private var generation = 0
    /// Called after a bootstrap that changed the backend's identity or availability (AppModel refreshes then).
    @ObservationIgnored var onBackendChange: (() -> Void)?
    /// Set first by `AppModel.prepareForTermination()`: no refresh or operation starts any more.
    @ObservationIgnored var isTerminating = false

    init(bootstrapper: any BackendBootstrapping, settings: AppSettings, clock: any StoreClock = SystemClock()) {
        self.bootstrapper = bootstrapper
        self.settings = settings
        self.clock = clock
    }

    /// Bootstraps again with the current settings (after Settings changes); no relaunch needed. When calls
    /// overlap, the last one started decides the state.
    public func rebootstrap() async {
        generation += 1
        let generation = self.generation
        isBootstrapping = true
        let bootstrap = await bootstrapper.bootstrap(settings.backendSettings)
        let summary = await bootstrapper.environmentSummary()
        guard generation == self.generation else { return }
        isBootstrapping = false
        lastBootstrapAt = clock.now()
        let previous = (identity: identity, available: backend != nil)
        switch bootstrap {
        case .ready(let backend, let identity):
            self.backend = backend
            self.identity = identity
            if case .cli(let resolution) = identity.kind { self.resolution = resolution } else { resolution = nil }
            backendState = .ready(identity)
        case .unavailable(let error, let resolution):
            backend = nil
            identity = nil
            self.resolution = resolution
            backendState = .unavailable(error)
        }
        self.summary = summary
        if previous.identity != identity || previous.available != (backend != nil) { onBackendChange?() }
    }

    /// Re-checks the CLI (a `brew upgrade` may have replaced it) when the last bootstrap is older than `staleAfter`
    /// or failed. The CLI bootstrapper caches its probe by path, inode, mtime and size, so this is cheap.
    func recheckIfStale(staleAfter: Duration) async {
        guard !isBootstrapping else { return }
        if backend != nil, let last = lastBootstrapAt, clock.now().timeIntervalSince(last) <= staleAfter.seconds { return }
        await rebootstrap()
    }

    public func recaptureEnvironment() async {
        await bootstrapper.recaptureEnvironment()
        summary = await bootstrapper.environmentSummary()
    }

    public func runDoctor(for project: ProjectRef?) async {
        guard let backend else { return }
        isRunningDoctor = true
        defer { isRunningDoctor = false }
        doctor = await backend.doctor(project)
    }

    public func supports(_ capability: Capability) -> Bool {
        identity?.supports(capability) ?? false
    }

    /// The ready backend, or the error that explains why there is none.
    func currentBackend() throws -> any BranchBoxBackend {
        if let backend { return backend }
        if case .unavailable(let error) = backendState { throw error }
        throw BackendError.cliUnusable(path: "", reason: "BranchBox is still locating the CLI; try again in a moment")
    }

    /// Whether a project or worktree folder exists. The preview backend's sample paths are fictional, so under it
    /// every folder counts as present (no "Folder missing" for every sample feature).
    func directoryExists(_ path: String) -> Bool {
        if case .preview? = identity?.kind { return true }
        var isDirectory: ObjCBool = false
        return FileManager.default.fileExists(atPath: path, isDirectory: &isDirectory) && isDirectory.boolValue
    }
}
