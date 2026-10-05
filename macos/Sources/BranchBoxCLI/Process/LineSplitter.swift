import BranchBoxKit
import Foundation

/// Splits a byte stream into lines on `\n`, `\r\n` and bare `\r` (progress redraws), whatever the chunking.
///
/// Bytes are buffered until a terminator arrives and only whole lines are decoded, so a multi-byte UTF-8
/// character split across chunks stays intact (`\n` and `\r` never occur inside one). A line longer than
/// `maxLineBytes` keeps its first bytes, cut at a character boundary and marked with "…"; the rest of it is
/// dropped, so memory stays bounded however long the line runs.
struct LineSplitter: Sendable {
    static let ellipsis = Array("…".utf8)

    let maxLineBytes: Int
    private var buffer: [UInt8] = []
    private var truncated = false
    /// The previous chunk ended with `\r`: a `\n` that starts the next chunk completes that `\r\n`.
    private var pendingLineFeed = false

    init(maxLineBytes: Int = LogLine.maxMessageBytes) {
        precondition(maxLineBytes > Self.ellipsis.count, "a line must have room for its truncation mark")
        self.maxLineBytes = maxLineBytes
    }

    /// Feeds one chunk and returns the lines it completed, in order and without their terminators.
    mutating func append(_ chunk: Data) -> [String] {
        var lines: [String] = []
        chunk.withUnsafeBytes { (raw: UnsafeRawBufferPointer) in
            let bytes = raw.bindMemory(to: UInt8.self)
            var start = 0
            if pendingLineFeed, let first = bytes.first {
                pendingLineFeed = false
                if first == 0x0A { start = 1 }
            }
            var index = start
            while index < bytes.count {
                let byte = bytes[index]
                guard byte == 0x0A || byte == 0x0D else {
                    index += 1
                    continue
                }
                collect(UnsafeBufferPointer(rebasing: bytes[start..<index]))
                lines.append(takeLine())
                index += 1
                if byte == 0x0D {
                    if index < bytes.count {
                        if bytes[index] == 0x0A { index += 1 }
                    } else {
                        pendingLineFeed = true
                    }
                }
                start = index
            }
            collect(UnsafeBufferPointer(rebasing: bytes[start..<bytes.count]))
        }
        return lines
    }

    /// The unterminated last line, if any, once the stream has ended.
    mutating func flush() -> String? {
        pendingLineFeed = false
        guard !buffer.isEmpty || truncated else { return nil }
        return takeLine()
    }

    private mutating func collect(_ bytes: UnsafeBufferPointer<UInt8>) {
        guard !bytes.isEmpty else { return }
        let room = maxLineBytes - buffer.count
        if bytes.count <= room {
            buffer.append(contentsOf: bytes)
        } else {
            buffer.append(contentsOf: UnsafeBufferPointer(rebasing: bytes[0..<room]))
            truncated = true
        }
    }

    private mutating func takeLine() -> String {
        defer {
            buffer.removeAll(keepingCapacity: true)
            truncated = false
        }
        guard truncated else { return String(decoding: buffer, as: UTF8.self) }
        var end = maxLineBytes - Self.ellipsis.count
        // Back off to the start of a character: continuation bytes look like 0b10xxxxxx.
        while end > 0, buffer[end] & 0xC0 == 0x80 { end -= 1 }
        return String(decoding: buffer[0..<end], as: UTF8.self) + "…"
    }
}

/// The last `limit` lines of a stream, kept in a ring so appending stays O(1).
struct LineTail: Sendable {
    let limit: Int
    private var storage: [String] = []
    private var next = 0

    init(limit: Int) {
        self.limit = max(0, limit)
    }

    mutating func append(_ line: String) {
        guard limit > 0 else { return }
        if storage.count < limit {
            storage.append(line)
        } else {
            storage[next] = line
            next = (next + 1) % limit
        }
    }

    /// Oldest first.
    var lines: [String] { Array(storage[next...] + storage[..<next]) }
}
