import BranchBoxKit
import BranchBoxPreview
@testable import BranchBoxStores
import Foundation
import Testing

// Shared fixtures for the store suites: a throwaway sandbox (defaults suite plus storage folders), a manual clock,
// a notifier spy and polling helpers. Everything a test writes lives under $TMPDIR/branchbox-tests/.

let sampleProject = PreviewSamples.project

func feature(_ name: String, in project: ProjectRef = sampleProject) -> FeatureRef {
    FeatureRef(project: project, name: name)
}

func startRequest(_ name: String, in project: ProjectRef = sampleProject) -> StartFeatureRequest {
    StartFeatureRequest(project: project, name: name, runtime: .container)
}

func logLine(_ message: String, level: LogLevel = .info) -> LogLine {
    LogLine(timestamp: nil, level: level, source: .stderr, target: nil, message: message)
}

/// A backend identity whose kind is the CLI, so the stores check folders on disk (the preview kind does not).
let cliIdentity = BackendIdentity(kind: .cli(CLIResolution(path: "/opt/homebrew/bin/branchbox", source: .wellKnownPath)),
                                  version: SemVer(0, 14, 0), contractVersion: 1, capabilities: PreviewSamples.allCapabilities)

/// A folder for one test: a `UserDefaults` suite and the stores' storage, removed by `remove()`.
///
/// The suite's name is an absolute path, which CFPreferences takes as the plist's location, so test runs never
/// leave files in ~/Library/Preferences (cfprefsd rewrites a suite's plist there even after
/// `removePersistentDomain`).
struct Sandbox {
    static let base: URL = {
        let base = FileManager.default.temporaryDirectory.appendingPathComponent("branchbox-tests", isDirectory: true)
        try? FileManager.default.createDirectory(at: base, withIntermediateDirectories: true)
        return base
    }()

    let directory: URL
    let defaultsName: String
    let defaults: UserDefaults

    init() {
        directory = Sandbox.base.appendingPathComponent("stores-\(UUID().uuidString)", isDirectory: true)
        try? FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        defaultsName = Sandbox.base.appendingPathComponent("settings-\(UUID().uuidString)").path
        defaults = UserDefaults(suiteName: defaultsName)!
    }

    var configuration: AppModel.Configuration { .isolated(in: directory) }

    /// A real folder inside the sandbox, e.g. a project root.
    func folder(_ name: String) -> URL {
        let url = directory.appendingPathComponent(name, isDirectory: true)
        try? FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
        return url
    }

    func remove() {
        defaults.removePersistentDomain(forName: defaultsName)
        try? FileManager.default.removeItem(atPath: defaultsName + ".plist")
        try? FileManager.default.removeItem(at: directory)
    }
}

/// An `AppModel` on a `PreviewBackend`, storing everything in a sandbox.
@MainActor struct Harness {
    let sandbox = Sandbox()
    let backend: PreviewBackend
    let settings: AppSettings
    let model: AppModel

    init(_ scenario: PreviewScenario = .contract, notifier: any Notifier = NoopNotifier(),
         configure: (inout AppModel.Configuration) -> Void = { _ in }) {
        backend = PreviewBackend(scenario: scenario)
        settings = AppSettings(defaults: sandbox.defaults)
        var configuration = sandbox.configuration
        configure(&configuration)
        model = AppModel(settings: settings, bootstrapper: PreviewBootstrapper(backend: backend), notifier: notifier,
                         configuration: configuration)
    }

    /// Starts the model, adds the sample project and waits for its first listing.
    func startWithSampleProject() async throws -> ProjectStore {
        await model.start()
        _ = await model.projects.add(folder: sampleProject.root)
        let store = try #require(model.projects.project(sampleProject))
        try await waitUntilLoaded(store)
        return store
    }

    /// Stops timers and watchers, then deletes the sandbox.
    func tearDown() {
        model.coordinator.stop()
        model.projects.setWatching(false)
        sandbox.remove()
    }
}

/// Polls `condition` every millisecond, failing the test after `timeout`. Runs on the caller's actor, so the
/// condition may read main-actor stores.
func waitUntil(timeout: Duration = .seconds(5), _ message: String = "condition not met",
               sourceLocation: SourceLocation = #_sourceLocation, isolation: isolated (any Actor)? = #isolation,
               _ condition: () async -> Bool) async throws {
    let deadline = ContinuousClock.now + timeout
    while await !condition() {
        try #require(ContinuousClock.now < deadline, "\(message) within \(timeout)", sourceLocation: sourceLocation)
        try await Task.sleep(for: .milliseconds(1))
    }
}

@MainActor func waitUntilLoaded(_ store: ProjectStore, sourceLocation: SourceLocation = #_sourceLocation) async throws {
    try await waitUntil("\(store.ref.path) did not load", sourceLocation: sourceLocation) {
        if case .loaded = store.loadState, !store.isRefreshing { return true }
        return false
    }
}

@MainActor func waitUntilFinished(_ record: OperationRecord, sourceLocation: SourceLocation = #_sourceLocation) async throws {
    try await waitUntil("\(record.title) did not finish", sourceLocation: sourceLocation) { !record.isCancellable }
}

/// The record a dispatch started or queued; throws otherwise.
@MainActor func operation(of result: DispatchResult) throws -> OperationRecord {
    switch result {
    case .started(let record), .queued(let record, _):
        return record
    case .rejected(let reason):
        throw Failure("expected a record, the dispatch was rejected: \(reason)")
    case .unavailable(let error):
        throw Failure("expected a record, the backend is unavailable: \(error)")
    }
}

struct Failure: Error, CustomStringConvertible {
    let description: String
    init(_ description: String) { self.description = description }
}

/// A clock that only moves when told to. `sleep(for:)` returns once `advance(by:)` passes its deadline.
final class ManualClock: StoreClock, @unchecked Sendable {
    private struct Sleeper {
        let id: UUID
        let deadline: Date
        let continuation: CheckedContinuation<Void, any Error>
    }

    private let lock = NSLock()
    private var current: Date
    private var sleepers: [Sleeper] = []
    private var cancelled: Set<UUID> = []

    init(start: Date = Date(timeIntervalSince1970: 1_790_000_000)) {
        current = start
    }

    func now() -> Date { lock.withLock { current } }

    var sleeperCount: Int { lock.withLock { sleepers.count } }

    func sleep(for duration: Duration) async throws {
        let id = UUID()
        try await withTaskCancellationHandler {
            try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Void, any Error>) in
                let early: Bool = lock.withLock {
                    if cancelled.remove(id) != nil { return true }
                    sleepers.append(Sleeper(id: id, deadline: current.addingTimeInterval(duration.seconds),
                                            continuation: continuation))
                    return false
                }
                if early { continuation.resume(throwing: CancellationError()) }
            }
        } onCancel: {
            let sleeper: Sleeper? = lock.withLock {
                guard let index = sleepers.firstIndex(where: { $0.id == id }) else {
                    cancelled.insert(id)
                    return nil
                }
                return sleepers.remove(at: index)
            }
            sleeper?.continuation.resume(throwing: CancellationError())
        }
    }

    func advance(by duration: Duration) {
        let due: [Sleeper] = lock.withLock {
            current = current.addingTimeInterval(duration.seconds)
            let due = sleepers.filter { $0.deadline <= current }
            sleepers.removeAll { $0.deadline <= current }
            return due
        }
        for sleeper in due { sleeper.continuation.resume() }
    }
}

/// Records what the stores ask of a notifier.
final class SpyNotifier: Notifier, @unchecked Sendable {
    let isAvailable: Bool
    private let lock = NSLock()
    private var notes: [UserNote] = []
    private var authorizationRequests = 0

    init(isAvailable: Bool) {
        self.isAvailable = isAvailable
    }

    var posted: [UserNote] { lock.withLock { notes } }
    var authorizationCount: Int { lock.withLock { authorizationRequests } }

    func requestAuthorizationIfNeeded() async -> Bool {
        lock.withLock { authorizationRequests += 1 }
        return true
    }

    func post(_ note: UserNote) async {
        lock.withLock { notes.append(note) }
    }
}

/// Collects progress events from a backend's `@Sendable` sink.
final class EventSink: @unchecked Sendable {
    private let lock = NSLock()
    private var collected: [ProgressEvent] = []
    var events: [ProgressEvent] { lock.withLock { collected } }
    var sink: ProgressSink { { [self] event in lock.withLock { collected.append(event) } } }
}
