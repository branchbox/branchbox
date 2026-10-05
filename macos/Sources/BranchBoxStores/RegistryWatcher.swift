import CoreServices
import Foundation
import os

/// Watches `<root>/.branchbox` with FSEvents and calls `onChange` once per burst of changes (D-17).
///
/// File-level events (`kFSEventStreamCreateFlagFileEvents`) catch both ways the CLI writes the registry: 0.13.x
/// truncates and rewrites `registry.json` in place, 0.14 writes `.registry.*.tmp` and renames it over. The stream
/// uses 0.2 s latency with `NoDefer` (the first event of a quiet period is delivered at once); a 300 ms trailing
/// debounce then folds the burst into one call. FSEvents keeps working while the app is inactive, which keeps the
/// menu bar fresh. Ported from the design spike `fsspike/main.swift`.
final class RegistryWatcher: @unchecked Sendable {
    let directory: URL
    private let latency: CFTimeInterval
    private let debounce: Duration
    private let onChange: @Sendable () -> Void
    private let queue = DispatchQueue(label: "dev.branchbox.registry-watcher")
    private let lock = NSLock()
    private var stream: FSEventStreamRef?                         // guarded by `lock`
    private var pending: DispatchWorkItem?                        // guarded by `lock`

    private static let logger = Logger(subsystem: "dev.branchbox.app", category: "registry-watcher")

    init(directory: URL, latency: CFTimeInterval = 0.2, debounce: Duration = .milliseconds(300),
         onChange: @escaping @Sendable () -> Void) {
        self.directory = directory
        self.latency = latency
        self.debounce = debounce
        self.onChange = onChange
    }

    deinit {
        stop()
    }

    /// Starts watching; false when the directory does not exist (a project before `init`) or FSEvents refused.
    @discardableResult func start() -> Bool {
        var isDirectory: ObjCBool = false
        guard FileManager.default.fileExists(atPath: directory.path, isDirectory: &isDirectory), isDirectory.boolValue else {
            return false
        }
        // FSEvents reports real paths; watch the resolved one so /tmp → /private/tmp style links still match.
        let path = directory.resolvingSymlinksInPath().path
        return lock.withLock { () -> Bool in
            guard stream == nil else { return true }
            // The stream owns a retained box (released through the context's release callback), so a callback
            // never sees a freed watcher.
            let box = Unmanaged.passRetained(CallbackBox { [weak self] in self?.eventsArrived() })
            var context = FSEventStreamContext(version: 0, info: box.toOpaque(), retain: nil,
                                               release: { info in
                                                   guard let info else { return }
                                                   Unmanaged<CallbackBox>.fromOpaque(info).release()
                                               },
                                               copyDescription: nil)
            let callback: FSEventStreamCallback = { _, info, _, _, _, _ in
                guard let info else { return }
                Unmanaged<CallbackBox>.fromOpaque(info).takeUnretainedValue().handler()
            }
            let flags = FSEventStreamCreateFlags(kFSEventStreamCreateFlagFileEvents | kFSEventStreamCreateFlagNoDefer
                                                 | kFSEventStreamCreateFlagUseCFTypes)
            guard let created = FSEventStreamCreate(kCFAllocatorDefault, callback, &context, [path] as CFArray,
                                                    FSEventStreamEventId(kFSEventStreamEventIdSinceNow), latency, flags)
            else {
                box.release()
                Self.logger.error("FSEvents refused to watch \(path, privacy: .public)")
                return false
            }
            FSEventStreamSetDispatchQueue(created, queue)
            guard FSEventStreamStart(created) else {
                FSEventStreamInvalidate(created)
                FSEventStreamRelease(created)
                Self.logger.error("FSEvents could not start watching \(path, privacy: .public)")
                return false
            }
            stream = created
            return true
        }
    }

    /// Lock-based rather than a `queue.sync`, so it is safe from any thread, including the watcher's own queue
    /// (where `deinit` can run if a callback held the last reference).
    func stop() {
        let stopped: FSEventStreamRef? = lock.withLock {
            pending?.cancel()
            pending = nil
            defer { stream = nil }
            return stream
        }
        guard let stopped else { return }
        FSEventStreamStop(stopped)
        FSEventStreamInvalidate(stopped)
        FSEventStreamRelease(stopped)
    }

    var isWatching: Bool {
        lock.withLock { stream != nil }
    }

    /// Runs on `queue`: restarts the trailing debounce.
    private func eventsArrived() {
        let item = DispatchWorkItem { [onChange] in onChange() }
        let watching: Bool = lock.withLock {
            guard stream != nil else { return false }
            pending?.cancel()
            pending = item
            return true
        }
        guard watching else { return }
        queue.asyncAfter(deadline: .now() + debounce.seconds, execute: item)
    }
}

/// What the FSEvents context points at.
private final class CallbackBox {
    let handler: () -> Void
    init(_ handler: @escaping () -> Void) { self.handler = handler }
}
