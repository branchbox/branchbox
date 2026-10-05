import BranchBoxKit
import Foundation

/// The time-based refresh triggers (§8.2):
/// - app activation refreshes every project whose data is more than 5 s old (or was never loaded);
/// - the selected project refreshes every `selectedProjectRefresh` while the app is active;
/// - every project refreshes every `otherProjectsRefresh` whether or not the app is active, which keeps the menu
///   bar fresh (D-28).
/// `.manual` turns a timer off. Interval changes in Settings restart the timers at once.
@MainActor final class RefreshCoordinator {
    private let projects: ProjectsStore
    private let settings: AppSettings
    private let clock: any StoreClock
    private let staleAfter: Duration
    private(set) var isAppActive = false
    private var selectedTimer: Task<Void, Never>?
    private var allProjectsTimer: Task<Void, Never>?

    init(projects: ProjectsStore, settings: AppSettings, clock: any StoreClock, staleAfter: Duration = .seconds(5)) {
        self.projects = projects
        self.settings = settings
        self.clock = clock
        self.staleAfter = staleAfter
    }

    var isRunning: Bool { selectedTimer != nil || allProjectsTimer != nil }

    func start() {
        restartTimers()
    }

    func stop() {
        selectedTimer?.cancel()
        allProjectsTimer?.cancel()
        selectedTimer = nil
        allProjectsTimer = nil
    }

    func restartTimers() {
        stop()
        selectedTimer = timer(every: settings.selectedProjectRefresh) { $0.selectedProjectTick() }
        allProjectsTimer = timer(every: settings.otherProjectsRefresh) { $0.allProjectsTick() }
    }

    /// Marks the app active and refreshes every stale project.
    func appDidBecomeActive() {
        setActive(true)
        for project in projects.projects where isStale(project) { project.requestRefresh(.appActivated) }
    }

    func appDidResignActive() {
        setActive(false)
    }

    func setActive(_ active: Bool) {
        isAppActive = active
    }

    func isStale(_ project: ProjectStore) -> Bool {
        guard let loaded = project.lastLoadedAt else { return true }
        return clock.now().timeIntervalSince(loaded) > staleAfter.seconds
    }

    /// A loop that sleeps `interval` on the clock and ticks; it ends when cancelled or once the coordinator is gone.
    private func timer(every interval: RefreshInterval,
                       _ tick: @escaping @MainActor (RefreshCoordinator) -> Void) -> Task<Void, Never>? {
        guard interval != .manual, interval.rawValue > 0 else { return nil }
        let clock = self.clock
        let period = Duration.seconds(interval.rawValue)
        return Task { [weak self] in
            while !Task.isCancelled {
                do { try await clock.sleep(for: period) } catch { return }
                guard !Task.isCancelled, let self else { return }
                tick(self)
            }
        }
    }

    private func selectedProjectTick() {
        guard isAppActive, let selected = projects.selectedProject, let project = projects.project(selected) else { return }
        project.requestRefresh(.timer)
    }

    private func allProjectsTick() {
        projects.refreshAll(.timer)
    }
}
