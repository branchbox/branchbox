import BranchBoxKit
import BranchBoxPreview
@testable import BranchBoxStores
import Foundation
import Testing

// RegistryWatcher (FSEvents on <root>/.branchbox) in a temporary directory: both ways the CLI writes the
// registry (0.13.x truncates in place, 0.14 renames a temp file over it) must refresh the project within 2 s.

/// Counts debounced callbacks from the FSEvents queue.
private final class Counter: @unchecked Sendable {
    private let lock = NSLock()
    private var count = 0
    var value: Int { lock.withLock { count } }
    func increment() { lock.withLock { count += 1 } }
}

@Suite(.serialized, .timeLimit(.minutes(1))) struct WatcherTests {
    private func makeRegistry(in sandbox: Sandbox) throws -> (directory: URL, registry: URL) {
        let directory = sandbox.folder("repo/main/.branchbox")
        let registry = directory.appendingPathComponent("registry.json")
        try Data("[]".utf8).write(to: registry)
        return (directory, registry)
    }

    /// Core 0.13.x `write_text_file`: open with O_TRUNC and write, keeping the inode.
    private func truncateAndWrite(_ registry: URL, _ text: String) throws {
        let handle = try FileHandle(forWritingTo: registry)
        try handle.truncate(atOffset: 0)
        try handle.write(contentsOf: Data(text.utf8))
        try handle.close()
    }

    /// Core 0.14 `write_atomic`: write `.registry.<random>.tmp`, then rename it over the registry.
    private func atomicReplace(_ registry: URL, _ text: String) throws {
        let temp = registry.deletingLastPathComponent().appendingPathComponent(".registry.\(UUID().uuidString.prefix(6)).tmp")
        try Data(text.utf8).write(to: temp)
        guard rename(temp.path, registry.path) == 0 else { throw Failure("rename failed: \(errno)") }
    }

    @Test func watcherSeesInPlaceWritesAndAtomicRenamesWithinTwoSeconds() async throws {
        let sandbox = Sandbox()
        defer { sandbox.remove() }
        let (directory, registry) = try makeRegistry(in: sandbox)
        let counter = Counter()
        let watcher = RegistryWatcher(directory: directory) { counter.increment() }
        #expect(watcher.start())
        #expect(watcher.start())                                       // starting twice keeps one stream
        defer { watcher.stop() }
        try await Task.sleep(for: .milliseconds(200))

        try truncateAndWrite(registry, #"[{"work_feature":"a"}]"#)
        try await waitUntil(timeout: .seconds(2), "in-place write not seen") { counter.value >= 1 }
        try await Task.sleep(for: .milliseconds(600))                  // let the debounce settle
        let afterInPlace = counter.value

        try atomicReplace(registry, #"[{"work_feature":"b"}]"#)
        try await waitUntil(timeout: .seconds(2), "atomic rename not seen") { counter.value > afterInPlace }
        try await Task.sleep(for: .milliseconds(600))

        // A burst of writes folds into a few callbacks.
        let beforeBurst = counter.value
        for index in 0..<20 { try truncateAndWrite(registry, "[\(index)]") }
        try await Task.sleep(for: .milliseconds(900))
        #expect(counter.value - beforeBurst >= 1)
        #expect(counter.value - beforeBurst <= 3)

        watcher.stop()
        #expect(!watcher.isWatching)
        let afterStop = counter.value
        try truncateAndWrite(registry, "[]")
        try await Task.sleep(for: .milliseconds(700))
        #expect(counter.value == afterStop)
    }

    @Test func noRegistryDirectoryMeansNoWatcher() {
        let sandbox = Sandbox()
        defer { sandbox.remove() }
        let watcher = RegistryWatcher(directory: sandbox.directory.appendingPathComponent("missing/.branchbox")) {}
        #expect(!watcher.start())
        #expect(!watcher.isWatching)
    }

    @Test @MainActor func aRegistryWriteRefreshesTheProject() async throws {
        let harness = Harness()
        defer { harness.tearDown() }
        let (directory, registry) = try makeRegistry(in: harness.sandbox)
        let root = directory.deletingLastPathComponent()
        await harness.model.start()
        _ = await harness.model.projects.add(folder: root)
        let store = try #require(harness.model.projects.project(ProjectRef(root: root)))
        #expect(store.isWatchingRegistry)
        try await waitUntilLoaded(store)
        try await Task.sleep(for: .milliseconds(200))
        await harness.backend.clearCalls()

        try truncateAndWrite(registry, #"[{"work_feature":"a"}]"#)
        try await waitUntil(timeout: .seconds(2), "in-place write did not refresh") {
            await harness.backend.calls(to: .listFeatures).count == 1
        }
        try await waitUntilLoaded(store)
        try await Task.sleep(for: .milliseconds(600))

        try atomicReplace(registry, #"[{"work_feature":"b"}]"#)
        try await waitUntil(timeout: .seconds(2), "atomic rename did not refresh") {
            await harness.backend.calls(to: .listFeatures).count == 2
        }

        // Settings › Watch project files off stops the watcher; on starts it again.
        harness.settings.watchProjectFiles = false
        #expect(!store.isWatchingRegistry)
        harness.settings.watchProjectFiles = true
        #expect(store.isWatchingRegistry)
        harness.model.projects.remove(store.ref)
        #expect(!store.isWatchingRegistry)
    }

    /// `.branchbox` created after the project was added (`branchbox init` in Terminal, a volume mounted later):
    /// the next root check starts the watcher, and registry writes then refresh the project.
    @Test @MainActor func aRegistryThatAppearsLaterIsWatchedFromTheNextCheck() async throws {
        let harness = Harness()
        defer { harness.tearDown() }
        let root = harness.sandbox.folder("later/main")
        await harness.model.start()
        _ = await harness.model.projects.add(folder: root)
        let store = try #require(harness.model.projects.project(ProjectRef(root: root)))
        try await waitUntilLoaded(store)
        #expect(!store.isWatchingRegistry)

        let directory = root.appendingPathComponent(".branchbox", isDirectory: true)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        let registry = directory.appendingPathComponent("registry.json")
        try Data("[]".utf8).write(to: registry)
        harness.model.appDidBecomeActive()
        #expect(store.isWatchingRegistry)
        try await waitUntilLoaded(store)
        try await Task.sleep(for: .milliseconds(200))
        await harness.backend.clearCalls()

        try truncateAndWrite(registry, #"[{"work_feature":"a"}]"#)
        try await waitUntil(timeout: .seconds(2), "the late watcher did not refresh") {
            await harness.backend.calls(to: .listFeatures).count >= 1
        }
    }
}
