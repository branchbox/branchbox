import BranchBoxKit
import Foundation
import Observation

/// An operation's most recent log lines, capped as a ring buffer. Lines arrive in batches from `EventBatcher`,
/// so `revision` bumps at most 10 times per second; the full log streams to `archiveURL` (`LogArchive`).
@MainActor @Observable public final class LogBuffer {
    public private(set) var lines: [LogLine] = []                 // ring buffer ≤ 10_000
    public private(set) var revision: Int = 0                     // bumped ≤ 10 Hz
    public private(set) var archiveURL: URL?                      // full log on disk (LogArchive)
    /// How many of the oldest lines the ring dropped; the archive still has them.
    public private(set) var droppedLines: Int = 0

    static let capacity = 10_000

    init(archiveURL: URL? = nil) {
        self.archiveURL = archiveURL
    }

    /// Appends one batch: one `revision` bump however many lines it holds.
    func append(_ newLines: [LogLine]) {
        guard !newLines.isEmpty else { return }
        lines.append(contentsOf: newLines)
        if lines.count > Self.capacity {
            let overflow = lines.count - Self.capacity
            lines.removeFirst(overflow)
            droppedLines += overflow
        }
        revision += 1
    }

    func setArchiveURL(_ url: URL?) {
        archiveURL = url
    }
}
