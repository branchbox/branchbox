import BranchBoxKit
import Foundation
import Observation

public enum LoadState: Sendable, Hashable { case idle, loading, loaded(Date), failed(BackendError, lastGood: Date?) }
public enum RefreshReason: Sendable, Hashable { case initial, registryChanged, appActivated, timer, afterOperation, manual, settingsChanged }

/// One project's features, strays, config and detect report.
///
/// Refresh (§8.2, D-17) is single-flight: a refresh requested while one is in flight marks a re-run, and the
/// active loop runs exactly one more pass however many requests arrived. It never cancels and restarts. Each pass
/// takes a generation number; a result whose generation is no longer current (`includeRemoved` changed, or the
/// project was removed) is dropped. A failed pass keeps the last good data. Every `feature list` goes through the
/// shared `ListLimiter` (at most 2 at once across projects). Triggers: the registry watcher, app activation, the
/// timers, every finished operation and ⌘R.
@MainActor @Observable public final class ProjectStore: Identifiable {
    public let ref: ProjectRef
    public nonisolated var id: String { ref.path }
    public private(set) var features: [FeatureRecord] = []        // attention first, then updatedAt desc (then name)
    public private(set) var strays: [StrayWorktree] = []
    public private(set) var droppedRecords: Int = 0
    public private(set) var listWarnings: [String] = []
    public private(set) var loadState: LoadState = .idle           // failed keeps last good features
    public private(set) var rootExists: Bool                      // false → grey row with Locate…/Remove
    public var includeRemoved: Bool = false {                     // toggling triggers a refresh with --all
        didSet {
            guard includeRemoved != oldValue else { return }
            generation += 1                                       // a pass already in flight lists the old way
            requestRefresh(.manual)
        }
    }
    public private(set) var config: ProjectConfigDocument?
    public private(set) var detect: DetectReport?

    // Additive (SW-2): what the sidebar and Diagnostics show besides the contract.
    /// The name shown for the project ("branchbox" for `…/branchbox/main`); persisted in projects.json.
    public internal(set) var displayName: String
    public internal(set) var isPinned: Bool
    /// Whether the sidebar's disclosure group is collapsed; set through `ProjectsStore.setCollapsed(_:_:)`.
    public internal(set) var isCollapsed: Bool
    public internal(set) var addedAt: Date
    public internal(set) var lastOpenedAt: Date?
    /// A refresh pass is running (the sidebar's spinner; stale data stays visible meanwhile).
    public private(set) var isRefreshing = false
    /// Why the last `reloadConfig()` / `reloadDetect()` failed; nil after a success.
    public private(set) var configError: BackendError?
    public private(set) var detectError: BackendError?

    @ObservationIgnored private let environment: EnvironmentStore
    @ObservationIgnored private let limiter: ListLimiter
    @ObservationIgnored private let clock: any StoreClock
    @ObservationIgnored private var loop: Task<Void, Never>?
    @ObservationIgnored private var rerun = false
    @ObservationIgnored private(set) var generation = 0
    @ObservationIgnored private var missingFolders: Set<String> = []
    @ObservationIgnored private var watcher: RegistryWatcher?
    /// Watching is on for this project (Settings › Refresh › Watch project files). A watcher that could not start
    /// yet (no `.branchbox`, a volume not mounted) is retried on every refresh pass and root check.
    @ObservationIgnored private(set) var wantsWatching = false
    @ObservationIgnored private var watchDebounce: Duration = .milliseconds(300)
    @ObservationIgnored private(set) var passes = 0               // finished passes, for tests

    init(entry: ProjectEntry, environment: EnvironmentStore, limiter: ListLimiter, clock: any StoreClock) {
        ref = entry.ref
        displayName = entry.displayName
        isPinned = entry.pinned
        isCollapsed = entry.collapsed
        addedAt = entry.addedAt
        lastOpenedAt = entry.lastOpenedAt
        self.environment = environment
        self.limiter = limiter
        self.clock = clock
        rootExists = environment.directoryExists(entry.root)
    }

    var entry: ProjectEntry {
        ProjectEntry(root: ref.path, displayName: displayName, addedAt: addedAt, lastOpenedAt: lastOpenedAt,
                     pinned: isPinned, collapsed: isCollapsed)
    }

    /// When the shown data was listed: the last success, also while a later failure is shown.
    public var lastLoadedAt: Date? {
        switch loadState {
        case .loaded(let date): date
        case .failed(_, let date): date
        case .idle, .loading: nil
        }
    }

    public func feature(named name: String) -> FeatureRecord? {
        features.first { $0.workFeature == name }
    }

    public func folderExists(for record: FeatureRecord) -> Bool {
        guard let path = record.worktreePath, !path.isEmpty else { return false }
        return environment.directoryExists(path)
    }

    // MARK: Refresh

    /// Non-blocking and coalesced: starts the refresh loop, or marks a re-run of the one in flight.
    public func requestRefresh(_ reason: RefreshReason) {
        guard !environment.isTerminating else { return }
        if loop != nil {
            rerun = true
        } else {
            startLoop()
        }
    }

    /// Single-flight + one coalesced re-run + generation guard. Returns once the loop that covers this request
    /// (the one in flight plus its re-run) has finished.
    public func refresh(_ reason: RefreshReason) async {
        guard !environment.isTerminating else { return }
        let active: Task<Void, Never>
        if let loop {
            rerun = true
            active = loop
        } else {
            active = startLoop()
        }
        await active.value
    }

    @discardableResult private func startLoop() -> Task<Void, Never> {
        let task = Task { [weak self] in
            guard let self else { return }
            await self.runPasses()
        }
        loop = task
        return task
    }

    private func runPasses() async {
        repeat {
            rerun = false
            await pass()
        } while rerun
        loop = nil                                                // same turn as the last `rerun` check
    }

    private func pass() async {
        let generation = self.generation
        let includeRemoved = self.includeRemoved
        revalidateRoot()
        if lastLoadedAt == nil { loadState = .loading }
        isRefreshing = true
        defer {
            isRefreshing = false
            passes += 1
        }
        let result: Result<FeatureListing, BackendError>
        do {
            let backend = try environment.currentBackend()
            let ref = self.ref
            let listing = try await limiter.run { try await backend.listFeatures(in: ref, includeRemoved: includeRemoved) }
            result = .success(listing)
        } catch {
            result = .failure(BackendError.normalize(error))
        }
        guard generation == self.generation else { return }      // stale: a newer pass (or none) owns the data
        switch result {
        case .success(let listing):
            apply(listing)
            retryWatcherIfWanted()
        case .failure(let error):
            // The last good features stay visible; the state says how stale they are.
            loadState = .failed(error, lastGood: lastLoadedAt)
        }
    }

    private func apply(_ listing: FeatureListing) {
        missingFolders = Set(listing.features.compactMap { record -> String? in
            guard record.status != .removed, let path = record.worktreePath, !path.isEmpty,
                  !environment.directoryExists(path) else { return nil }
            return record.workFeature
        })
        let missing = missingFolders
        let sorted = listing.features.sorted { lhs, rhs in
            let lhsAttention = Self.attentionReason(for: lhs, folderExists: !missing.contains(lhs.workFeature)) != nil
            let rhsAttention = Self.attentionReason(for: rhs, folderExists: !missing.contains(rhs.workFeature)) != nil
            if lhsAttention != rhsAttention { return lhsAttention }
            let lhsUpdated = lhs.updatedAt ?? .distantPast, rhsUpdated = rhs.updatedAt ?? .distantPast
            if lhsUpdated != rhsUpdated { return lhsUpdated > rhsUpdated }
            return lhs.workFeature < rhs.workFeature
        }
        // Assign only what changed: Observation notifies on every set, and an unchanged listing (most timer and
        // watcher refreshes) should not re-render the sidebar, the menu bar and the detail.
        if features != sorted { features = sorted }
        if strays != listing.strays { strays = listing.strays }
        if droppedRecords != listing.droppedRecords { droppedRecords = listing.droppedRecords }
        if listWarnings != listing.warnings { listWarnings = listing.warnings }
        loadState = .loaded(clock.now())
    }

    /// Drops whatever a pass in flight returns, e.g. once the project is removed or relocated.
    func invalidate() {
        generation += 1
        stopWatching()
    }

    /// Re-checks whether the root folder exists (start, app activation, each refresh), and starts the registry
    /// watcher if it is wanted but could not start before (the folder or its `.branchbox` appeared since).
    func revalidateRoot() {
        let exists = environment.directoryExists(ref.path)
        if rootExists != exists { rootExists = exists }
        retryWatcherIfWanted()
    }

    // MARK: Config and detect

    public func reloadConfig() async {
        do {
            config = try await environment.currentBackend().readConfig(ref)
            configError = nil
        } catch {
            configError = BackendError.normalize(error)
        }
    }

    public func reloadDetect() async {
        do {
            detect = try await environment.currentBackend().detect(ref.root)
            detectError = nil
        } catch {
            detectError = BackendError.normalize(error)
        }
    }

    // MARK: Attention

    /// Status-derived attention, then derived "Folder missing", "Interrupted" and "Setup incomplete", then one
    /// row per stray (DESIGN §9.1).
    public var attention: [AttentionItem] {
        let features = features.compactMap { record in
            Self.attentionReason(for: record, folderExists: !missingFolders.contains(record.workFeature)).map {
                AttentionItem(id: "feature:\(record.workFeature)", featureOrPath: record.workFeature, reason: $0)
            }
        }
        let strays = strays.map { AttentionItem(id: "stray:\($0.path)", featureOrPath: $0.path, reason: .unregisteredWorktree) }
        return features + strays
    }

    /// The §9.1 attention table. SW-3's `Remediation.attention(for:folderExists:)` is the shared version; this
    /// local copy keeps the store independent of Planning.
    static func attentionReason(for record: FeatureRecord, folderExists: Bool) -> AttentionReason? {
        if record.status == .removed { return nil }
        if record.setup?.state == .interrupted { return .interrupted }
        switch record.status {
        case .degraded: return .degraded
        case .failedRetained: return .failedRetained
        case .orphaned: return .orphaned
        case .unknown(let raw): return .unknownStatus(raw)
        case .removed: return nil
        case .active:
            if !folderExists { return .folderMissing }
            return record.moduleOutcomes.first { $0.status == .failed }.map { .setupIncomplete(module: $0.module) }
        }
    }

    // MARK: Registry watcher

    /// Watches `<root>/.branchbox` when it exists. Without it (before `init`, or a folder that is missing for now)
    /// there is no watcher yet; it starts on the next refresh pass or root check that finds `.branchbox`, however
    /// it appeared (an in-app init, `branchbox init` in Terminal, a volume mounted, Locate…).
    func startWatching(debounce: Duration) {
        wantsWatching = true
        watchDebounce = debounce
        guard watcher == nil else { return }
        let directory = ref.root.appendingPathComponent(".branchbox", isDirectory: true)
        let watcher = RegistryWatcher(directory: directory, debounce: debounce) { [weak self] in
            guard let self else { return }
            Task { @MainActor in self.requestRefresh(.registryChanged) }
        }
        if watcher.start() { self.watcher = watcher }
    }

    func stopWatching() {
        wantsWatching = false
        watcher?.stop()
        watcher = nil
    }

    private func retryWatcherIfWanted() {
        if wantsWatching, watcher == nil { startWatching(debounce: watchDebounce) }
    }

    /// Whether `<root>/.branchbox` is being watched.
    var isWatchingRegistry: Bool { watcher != nil }
}
