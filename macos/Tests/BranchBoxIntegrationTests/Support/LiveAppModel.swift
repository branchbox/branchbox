import BranchBoxCLI
import BranchBoxKit
@testable import BranchBoxStores
import Foundation
import Testing

/// An `AppModel` on the real CLI (the production `CLIBackendBootstrapper`, launchd PATH), with its settings,
/// project list, history and logs in a throwaway `branchbox-it-stores-<uuid>` folder beside the TempRepos.
@MainActor final class LiveAppModel {
    let directory: URL
    let defaultsName: String
    let model: AppModel

    init() {
        directory = TempRepo.baseDirectory.appendingPathComponent("branchbox-it-stores-\(UUID().uuidString)", isDirectory: true)
        try? FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        // An absolute suite name keeps the plist out of ~/Library/Preferences (see the stores tests' Sandbox).
        defaultsName = directory.appendingPathComponent("settings").path
        let settings = AppSettings(defaults: UserDefaults(suiteName: defaultsName)!)
        model = AppModel(settings: settings, bootstrapper: LiveCLI.bootstrapper(runner: ProcessRunner()),
                         notifier: NoopNotifier(), configuration: .isolated(in: directory))
    }

    /// Starts the model, adds `repo` and waits for its first listing.
    func start(with repo: TempRepo) async throws -> ProjectStore {
        await model.start()
        #expect(model.environment.backend != nil, "the CLI was not found")
        _ = await model.projects.add(folder: repo.main)
        let store = try #require(model.projects.project(repo.project), "\(repo.main.path) was not added")
        try await waitUntil("the first listing") {
            if case .loaded = store.loadState, !store.isRefreshing { return true }
            return false
        }
        return store
    }

    /// Stops timers, watchers and processes, then deletes the folder.
    func tearDown() async {
        model.coordinator.stop()
        model.projects.setWatching(false)
        await model.operations.cancelAll()
        UserDefaults(suiteName: defaultsName)?.removePersistentDomain(forName: defaultsName)
        try? FileManager.default.removeItem(at: directory)
    }
}

/// The record a dispatch started or queued; throws otherwise.
@MainActor func operation(of result: DispatchResult) throws -> OperationRecord {
    switch result {
    case .started(let record), .queued(let record, _):
        return record
    case .rejected(let reason):
        throw TempRepoError(message: "expected a record, the dispatch was rejected: \(reason)")
    case .unavailable(let error):
        throw TempRepoError(message: "expected a record, the backend is unavailable: \(error)")
    }
}

/// Polls `condition` every 10 ms, failing after `timeout`; runs on the caller's actor.
func waitUntil(timeout: Duration = .seconds(30), _ message: String, sourceLocation: SourceLocation = #_sourceLocation,
               isolation: isolated (any Actor)? = #isolation, _ condition: () async -> Bool) async throws {
    let deadline = ContinuousClock.now + timeout
    while await !condition() {
        try #require(ContinuousClock.now < deadline, "\(message) within \(timeout)", sourceLocation: sourceLocation)
        try await Task.sleep(for: .milliseconds(10))
    }
}
