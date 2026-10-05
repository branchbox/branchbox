import BranchBoxKit
import Foundation
import Observation
import os

public enum AddProjectOutcome: Sendable, Hashable {
    case added(ProjectRef, note: String?)                         // note e.g. "This is a feature worktree of …/main; added …/main"
    case alreadyPresent(ProjectRef)
    case needsInit(ProjectRef)
    case refused(BackendError)
}

/// The user's projects, pinned first, then most recently opened, persisted in projects.json
/// (`ProjectsRepository`, §8.5). Adding goes through `backend.resolveProject`, which normalizes a feature
/// worktree or a container folder to the main worktree; duplicates are detected by canonical path. Removing a
/// project never touches its files.
@MainActor @Observable public final class ProjectsStore {
    public private(set) var projects: [ProjectStore] = []         // pinned first, then lastOpenedAt desc

    /// The project whose detail is showing; the selected-project refresh timer follows it (SW-4 sets it from the
    /// sidebar selection). Additive (SW-2).
    public var selectedProject: ProjectRef?

    private let environment: EnvironmentStore
    private let repository: ProjectsRepository
    private let limiter: ListLimiter
    private let clock: any StoreClock
    @ObservationIgnored var watchDebounce: Duration = .milliseconds(300)
    /// Whether projects watch `<root>/.branchbox` (Settings › Refresh › Watch project files); set by AppModel.
    @ObservationIgnored private(set) var watching = false

    private static let logger = Logger(subsystem: "dev.branchbox.app", category: "projects")

    init(environment: EnvironmentStore, repository: ProjectsRepository, limiter: ListLimiter,
         clock: any StoreClock = SystemClock()) {
        self.environment = environment
        self.repository = repository
        self.limiter = limiter
        self.clock = clock
    }

    public func project(_ ref: ProjectRef) -> ProjectStore? {
        projects.first { $0.ref == ref }
    }

    /// Loads projects.json (once, at start). Projects already added in this session are kept.
    func load() {
        for entry in repository.load() where project(entry.ref) == nil {
            projects.append(makeStore(entry))
        }
        reorder()
    }

    /// Resolves `folder` to its main worktree through the backend and adds that project.
    public func add(folder: URL) async -> AddProjectOutcome {
        let resolution: ProjectResolution
        do {
            resolution = try await environment.currentBackend().resolveProject(at: folder)
        } catch {
            return .refused(BackendError.normalize(error))
        }
        let ref = resolution.project
        if let existing = existing(matching: ref) { return .alreadyPresent(existing.ref) }
        guard resolution.initialized else { return .needsInit(ref) }
        let store = insert(ref)
        store.requestRefresh(.initial)
        return .added(ref, note: Self.note(for: resolution))
    }

    /// Never deletes files.
    public func remove(_ ref: ProjectRef) {
        guard let store = project(ref) else { return }
        store.invalidate()
        projects.removeAll { $0.ref == ref }
        if selectedProject == ref { selectedProject = nil }
        save()
    }

    /// Points a project whose folder moved at its new location, keeping its name, pin and sidebar state.
    public func relocate(_ ref: ProjectRef, to folder: URL) async -> AddProjectOutcome {
        let resolution: ProjectResolution
        do {
            resolution = try await environment.currentBackend().resolveProject(at: folder)
        } catch {
            return .refused(BackendError.normalize(error))
        }
        let newRef = resolution.project
        if newRef == ref, let store = project(ref) {             // the folder is back where it was
            store.revalidateRoot()
            store.requestRefresh(.manual)
            return .alreadyPresent(ref)
        }
        if let existing = existing(matching: newRef) { return .alreadyPresent(existing.ref) }
        guard resolution.initialized else { return .needsInit(newRef) }
        let old = project(ref)
        var entry = ProjectEntry(root: newRef.path, displayName: ProjectEntry.defaultDisplayName(for: newRef.path),
                                 addedAt: clock.now(), lastOpenedAt: clock.now())
        if let old {
            entry.displayName = old.displayName
            entry.addedAt = old.addedAt
            entry.pinned = old.isPinned
            entry.collapsed = old.isCollapsed
            old.invalidate()
            projects.removeAll { $0.ref == ref }
        }
        let store = makeStore(entry)
        projects.append(store)
        if selectedProject == ref { selectedProject = newRef }
        reorder()
        save()
        store.requestRefresh(.initial)
        return .added(newRef, note: Self.note(for: resolution))
    }

    public func setPinned(_ ref: ProjectRef, _ pinned: Bool) {
        guard let store = project(ref), store.isPinned != pinned else { return }
        store.isPinned = pinned
        reorder()
        save()
    }

    public func markOpened(_ ref: ProjectRef) {
        guard let store = project(ref) else { return }
        store.lastOpenedAt = clock.now()
        reorder()
        save()
    }

    /// Additive (SW-2): remembers the sidebar disclosure state.
    public func setCollapsed(_ ref: ProjectRef, _ collapsed: Bool) {
        guard let store = project(ref), store.isCollapsed != collapsed else { return }
        store.isCollapsed = collapsed
        save()
    }

    /// Additive (SW-2): renames the project in the sidebar (files are untouched); an empty name restores the default.
    public func rename(_ ref: ProjectRef, to name: String) {
        guard let store = project(ref) else { return }
        let trimmed = name.trimmingCharacters(in: .whitespacesAndNewlines)
        store.displayName = trimmed.isEmpty ? ProjectEntry.defaultDisplayName(for: ref.path) : trimmed
        save()
    }

    public func refreshAll(_ reason: RefreshReason) {
        for project in projects { project.requestRefresh(reason) }
    }

    public var attentionCount: Int {
        projects.reduce(0) { $0 + $1.attention.count }
    }

    // MARK: Lifecycle (AppModel)

    /// Re-checks every root folder (start, app activation).
    func validateRoots() {
        for project in projects { project.revalidateRoot() }
    }

    /// Starts or stops every project's registry watcher.
    func setWatching(_ watching: Bool) {
        self.watching = watching
        for project in projects {
            if watching { project.startWatching(debounce: watchDebounce) } else { project.stopWatching() }
        }
    }

    /// After `init` finished in `folder`: adds the project if needed (init may have moved it into `<dir>/main`),
    /// starts its watcher now that `.branchbox` exists, and refreshes it.
    func didInitialize(folder: URL, workspacePath: String?) async {
        let target = workspacePath.map { URL(fileURLWithPath: $0, isDirectory: true) } ?? folder
        if let store = project(ProjectRef(root: target)) ?? project(ProjectRef(root: folder)) {
            if watching { store.startWatching(debounce: watchDebounce) }
            store.requestRefresh(.afterOperation)
            return
        }
        _ = await add(folder: target)
    }

    // MARK: Private

    /// A project with the same canonical path (symlinks resolved), e.g. `/tmp/x` and `/private/tmp/x`.
    private func existing(matching ref: ProjectRef) -> ProjectStore? {
        if let exact = project(ref) { return exact }
        let canonical = ref.root.resolvingSymlinksInPath().path
        return projects.first { $0.ref.root.resolvingSymlinksInPath().path == canonical }
    }

    private func insert(_ ref: ProjectRef) -> ProjectStore {
        let now = clock.now()
        let store = makeStore(ProjectEntry(root: ref.path, displayName: ProjectEntry.defaultDisplayName(for: ref.path),
                                           addedAt: now, lastOpenedAt: now))
        projects.append(store)
        reorder()
        save()
        return store
    }

    private func makeStore(_ entry: ProjectEntry) -> ProjectStore {
        let store = ProjectStore(entry: entry, environment: environment, limiter: limiter, clock: clock)
        if watching { store.startWatching(debounce: watchDebounce) }
        return store
    }

    private func reorder() {
        projects.sort { lhs, rhs in
            if lhs.isPinned != rhs.isPinned { return lhs.isPinned }
            let lhsOpened = lhs.lastOpenedAt ?? lhs.addedAt, rhsOpened = rhs.lastOpenedAt ?? rhs.addedAt
            if lhsOpened != rhsOpened { return lhsOpened > rhsOpened }
            return lhs.displayName.localizedStandardCompare(rhs.displayName) == .orderedAscending
        }
    }

    private func save() {
        do {
            try repository.save(projects.map(\.entry))
        } catch {
            Self.logger.error("Saving projects.json failed: \(error, privacy: .public)")
        }
    }

    private static func note(for resolution: ProjectResolution) -> String? {
        switch resolution.normalization {
        case .none:
            nil
        case .fromFeatureWorktree:
            "This is a feature worktree of \(resolution.project.path); adding \(resolution.project.path) instead"
        case .fromParentContainer:
            "\(resolution.requested.path) holds BranchBox worktrees; adding its main worktree \(resolution.project.path)"
        }
    }
}
