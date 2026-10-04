import Darwin
import Foundation
import os

/// Turns the read end of a child's stdout or stderr pipe into an `AsyncStream<Data>`.
///
/// The `readabilityHandler` is installed by `init`, which the runner calls before `Process.run()`, so the pipe
/// is drained from the moment the child can write and a chatty child never blocks on a full pipe buffer. The
/// stream finishes at EOF, or at `finish()` when the runner stops waiting for a grandchild that still holds
/// the write end.
///
/// The read end is never closed here: a `readabilityHandler` that is already running could otherwise read a
/// closed handle. It closes when the run releases its `Pipe`.
final class PipeReader: Sendable {
    let chunks: AsyncStream<Data>
    private let continuation: AsyncStream<Data>.Continuation
    private let handle: FileHandle
    private let finished = OSAllocatedUnfairLock(initialState: false)

    init(_ handle: FileHandle) {
        let (chunks, continuation) = AsyncStream.makeStream(of: Data.self, bufferingPolicy: .unbounded)
        self.chunks = chunks
        self.continuation = continuation
        self.handle = handle
        let finished = finished
        handle.readabilityHandler = { handle in
            let data = handle.availableData
            guard data.isEmpty else {
                continuation.yield(data)
                return
            }
            handle.readabilityHandler = nil
            if finished.withLock({ Self.markFinished(&$0) }) { continuation.finish() }
        }
    }

    /// Stops reading and ends `chunks`; data still unread in the pipe is dropped. Idempotent.
    func finish() {
        handle.readabilityHandler = nil
        if finished.withLock({ Self.markFinished(&$0) }) { continuation.finish() }
    }

    /// Sets the flag and reports whether this call was the one that set it.
    private static func markFinished(_ flag: inout Bool) -> Bool {
        defer { flag = true }
        return !flag
    }
}

/// Feeds `ProcessSpec.standardInput` to the child, then closes the pipe so the child reads EOF.
enum StandardInputWriter {
    private static let queue = DispatchQueue(label: "dev.branchbox.process.stdin", attributes: .concurrent)

    /// Writes on a GCD thread, never on the cooperative pool: a child that does not read blocks the write
    /// until it exits. `F_SETNOSIGPIPE` turns a child that exits without reading into `EPIPE` instead of a
    /// SIGPIPE that would kill the app.
    static func write(_ data: Data, to handle: FileHandle) {
        let descriptor = handle.fileDescriptor
        _ = fcntl(descriptor, F_SETNOSIGPIPE, 1)
        queue.async {
            data.withUnsafeBytes { (raw: UnsafeRawBufferPointer) in
                guard let base = raw.baseAddress else { return }
                var offset = 0
                while offset < raw.count {
                    let written = Darwin.write(descriptor, base + offset, raw.count - offset)
                    if written > 0 {
                        offset += written
                    } else if written < 0, errno == EINTR {
                        continue
                    } else {
                        break                                   // EPIPE: the child stopped reading
                    }
                }
            }
            try? handle.close()
        }
    }
}
