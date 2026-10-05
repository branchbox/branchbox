import BranchBoxKit
import Foundation
import Observation

/// The composition of every store; views read it through `@Environment(AppModel.self)`.
///
/// `start()` (§8.1):
/// 1. bootstraps the backend;
/// 2. loads projects.json and the operation history;
/// 3. runs `LegacyDefaultsMigration` (the 0.13 app's workspace, prompt history and obsolete keys);
/// 4. validates every project root (a missing one shows as a grey row with Locate…/Remove);
/// 5. starts the registry watchers and the refresh timers, and refreshes every project.
@MainActor @Observable public final class AppModel {
    /// Where the stores keep their files, plus the timings tests shorten.
    public struct Configuration: Sendable {
        /// Holds projects.json and operations.json.
        public var projectsDirectory: URL
        /// Holds one log file per operation (`LogArchive`).
        public var logsDirectory: URL

        var clock: any StoreClock = SystemClock()
        var terminationTimeout: Duration = .seconds(10)
        var notificationThreshold: Duration = .seconds(10)
        var listConcurrency = 2
        var watchDebounce: Duration = .milliseconds(300)
        var staleAfter: Duration = .seconds(5)
        var flushInterval: Duration = .milliseconds(100)

        public init(projectsDirectory: URL, logsDirectory: URL) {
            self.projectsDirectory = projectsDirectory
            self.logsDirectory = logsDirectory
        }

        /// The bundle identifier of the released app; only it uses the "BranchBox" folders.
        static let releasedBundleIdentifier = "dev.branchbox.app"

        /// `~/Library/Application Support/BranchBox` and `~/Library/Logs/BranchBox/operations` for the released
        /// app (D-22, bundle id `dev.branchbox.app`). Anything else — an unbundled process (`swift run`,
        /// `swift test`) or the `BranchBox Dev.app` dev bundle (`dev.branchbox.app.dev`) — uses "BranchBox Dev"
        /// folders, as it uses the dev defaults suite, so a development build never edits the installed app's
        /// project list or logs.
        public static var standard: Configuration {
            let fileManager = FileManager.default
            let library = fileManager.urls(for: .libraryDirectory, in: .userDomainMask).first
                ?? fileManager.homeDirectoryForCurrentUser.appendingPathComponent("Library", isDirectory: true)
            let folder = folderName(isBundledApp: AppBundle.isBundledApp(), bundleIdentifier: Bundle.main.bundleIdentifier)
            return Configuration(
                projectsDirectory: library.appendingPathComponent("Application Support/\(folder)", isDirectory: true),
                logsDirectory: library.appendingPathComponent("Logs/\(folder)/operations", isDirectory: true))
        }

        /// "BranchBox" for the released bundle, "BranchBox Dev" for everything else.
        static func folderName(isBundledApp: Bool, bundleIdentifier: String?) -> String {
            isBundledApp && bundleIdentifier == releasedBundleIdentifier ? "BranchBox" : "BranchBox Dev"
        }

        /// Everything under `directory`, for tests and previews that must not touch the user's files.
        public static func isolated(in directory: URL) -> Configuration {
            Configuration(projectsDirectory: directory.appendingPathComponent("Application Support", isDirectory: true),
                          logsDirectory: directory.appendingPathComponent("Logs", isDirectory: true))
        }
    }

    public let settings: AppSettings
    public let environment: EnvironmentStore
    public let projects: ProjectsStore
    public let operations: OperationStore
    public let actions: ActionDispatcher
    public let notifier: any Notifier
    public private(set) var pendingIntent: WindowIntent?
    public private(set) var intentToken: Int = 0                   // bumps on every post

    /// Additive (SW-2): whether `start()` has finished (projects loaded, watchers and timers running).
    public private(set) var hasStarted = false
    /// Additive (SW-2): the app is active, as reported by `appDidBecomeActive()` / `appDidResignActive()`.
    public private(set) var isAppActive = false

    @ObservationIgnored let configuration: Configuration
    @ObservationIgnored let coordinator: RefreshCoordinator
    @ObservationIgnored private var started = false

    public convenience init(settings: AppSettings, bootstrapper: any BackendBootstrapping, notifier: any Notifier) {
        self.init(settings: settings, bootstrapper: bootstrapper, notifier: notifier, configuration: .standard)
    }

    /// Additive (SW-2): the same with explicit storage folders (see `Configuration.isolated(in:)`).
    public init(settings: AppSettings, bootstrapper: any BackendBootstrapping, notifier: any Notifier,
                configuration: Configuration) {
        let clock = configuration.clock
        let environment = EnvironmentStore(bootstrapper: bootstrapper, settings: settings, clock: clock)
        let projects = ProjectsStore(environment: environment,
                                     repository: ProjectsRepository(directory: configuration.projectsDirectory),
                                     limiter: ListLimiter(limit: configuration.listConcurrency), clock: clock)
        projects.watchDebounce = configuration.watchDebounce
        let operations = OperationStore(
            registryLock: { [weak environment] in environment?.supports(.registryLock) ?? false },
            historyURL: configuration.projectsDirectory.appendingPathComponent("operations.json"), clock: clock)
        // `UNUserNotificationCenter` traps outside a real `.app` (xctest has a bundle id too), so an unavailable
        // notifier is never called at all.
        let notifier: any Notifier = notifier.isAvailable ? notifier : NoopNotifier()
        self.settings = settings
        self.environment = environment
        self.projects = projects
        self.operations = operations
        self.actions = ActionDispatcher(settings: settings, environment: environment, projects: projects,
                                        operations: operations, notifier: notifier, clock: clock,
                                        logsDirectory: configuration.logsDirectory,
                                        flushInterval: configuration.flushInterval,
                                        notificationThreshold: configuration.notificationThreshold)
        self.notifier = notifier
        self.configuration = configuration
        coordinator = RefreshCoordinator(projects: projects, settings: settings, clock: clock,
                                         staleAfter: configuration.staleAfter)
        environment.onBackendChange = { [weak self] in self?.backendChanged() }
        settings.onChange = { [weak self] change in self?.settingsChanged(change) }
        // Notifications are suppressed while the app is active; SW-4 may narrow this to "and the main window is
        // visible" by replacing the closure.
        actions.isAppActive = { [weak self] in self?.isAppActive ?? false }
    }

    /// Runs once; later calls do nothing (use `environment.rebootstrap()` after Settings change).
    public func start() async {
        guard !started else { return }
        started = true
        // Local files first, so the sidebar shows the user's projects while the CLI is being located (a returning
        // user never sees Welcome flash). Root checks wait for the bootstrap: they depend on the backend kind.
        projects.load()
        operations.loadHistory()
        await environment.rebootstrap()
        await LegacyDefaultsMigration.run(settings: settings, projects: projects, backendAvailable: environment.backend != nil)
        projects.validateRoots()
        projects.setWatching(settings.watchProjectFiles)
        coordinator.start()
        projects.refreshAll(.initial)
        hasStarted = true
    }

    /// Throws the unavailable `BackendError` (or a "still locating" one while resolving).
    public func backend() throws -> any BranchBoxBackend {
        try environment.currentBackend()
    }

    public func post(_ intent: WindowIntent) {
        pendingIntent = intent
        intentToken += 1
    }

    public func takePendingIntent() -> WindowIntent? {
        defer { pendingIntent = nil }
        return pendingIntent
    }

    /// Refreshes every project whose data is more than 5 s old, re-checks project roots, and re-checks the CLI
    /// binary (a `brew upgrade` may have replaced it) when the last check is that old too.
    public func appDidBecomeActive() {
        isAppActive = true
        guard hasStarted else {
            coordinator.setActive(true)                           // start() refreshes everything anyway
            return
        }
        projects.validateRoots()
        coordinator.appDidBecomeActive()
        let environment = self.environment
        let staleAfter = configuration.staleAfter
        Task { await environment.recheckIfStale(staleAfter: staleAfter) }
    }

    /// Additive (SW-2): stops the selected-project timer's refreshes until the app is active again.
    public func appDidResignActive() {
        isAppActive = false
        coordinator.appDidResignActive()
    }

    /// Stops starting anything new (no refresh, operation or project add runs any more), cancels every
    /// operation, then stops every process the backend started; returns within 10 s whatever happens. The
    /// cancellations get at most half of that, and the process teardown the rest.
    public func prepareForTermination() async {
        environment.isTerminating = true
        coordinator.stop()
        projects.setWatching(false)
        let total = configuration.terminationTimeout
        let started = ContinuousClock.now
        let operations = self.operations
        let bootstrapper = environment.bootstrapper
        await waitAtMost(total / 2) { await operations.cancelAll() }
        let remaining = total - (ContinuousClock.now - started)
        await waitAtMost(max(remaining, .milliseconds(100))) { await bootstrapper.terminateAllProcesses() }
    }

    // MARK: Reactions

    private func backendChanged() {
        guard hasStarted else { return }
        projects.refreshAll(.settingsChanged)
    }

    private func settingsChanged(_ change: AppSettings.Change) {
        guard hasStarted else { return }
        switch change {
        case .refreshIntervals: coordinator.restartTimers()
        case .watchProjectFiles: projects.setWatching(settings.watchProjectFiles)
        }
    }
}
