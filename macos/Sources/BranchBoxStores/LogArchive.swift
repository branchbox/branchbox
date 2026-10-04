import BranchBoxKit
import Foundation
import os

/// Streams one operation's full log to `<directory>/<ISO>-<kind>-<subject>-<id>.log`, so nothing is lost when the
/// in-memory `LogBuffer` drops old lines. Writes happen on a private serial queue; every line is redacted first
/// (Settings' extra environment values and the tunnel token never reach the disk). Creating an archive prunes the
/// directory to the newest `retention` logs.
final class LogArchive: @unchecked Sendable {
    let url: URL
    private let secrets: [String]
    private let queue = DispatchQueue(label: "dev.branchbox.log-archive")
    private var handle: FileHandle?                               // touched only on `queue`

    static let redactedMarker = "<redacted>"
    private static let logger = Logger(subsystem: "dev.branchbox.app", category: "log-archive")

    /// Creates the file (mode 0600) and writes `header`; nil when the directory or file cannot be created.
    init?(directory: URL, fileName: String, header: [String], secrets: [String], retention: Int) {
        let fileManager = FileManager.default
        let url = directory.appendingPathComponent(fileName)
        do {
            try fileManager.createDirectory(at: directory, withIntermediateDirectories: true)
            guard fileManager.createFile(atPath: url.path, contents: nil, attributes: [.posixPermissions: 0o600]) else {
                Self.logger.error("Could not create the operation log \(url.path, privacy: .public)")
                return nil
            }
            handle = try FileHandle(forWritingTo: url)
        } catch {
            Self.logger.error("Could not open the operation log \(url.path, privacy: .public): \(error, privacy: .public)")
            return nil
        }
        self.url = url
        // Longest first, so a secret that contains another is replaced whole. Very short values would mangle
        // ordinary words, so they are not treated as secrets.
        self.secrets = Set(secrets.filter { $0.count >= 4 }).sorted { $0.count > $1.count }
        write(header)
        queue.async { Self.prune(directory, keeping: max(retention, 1)) }
    }

    deinit {
        try? handle?.close()
    }

    /// Appends `lines`, each as one text line.
    func append(_ lines: [LogLine]) {
        guard !lines.isEmpty else { return }
        let arrival = Date()
        write(lines.map { Self.format($0, arrival: arrival) })
    }

    /// Writes `footer` and closes the file; later appends are dropped.
    func close(footer: [String]) {
        write(footer)
        queue.async { [self] in
            try? handle?.close()
            handle = nil
        }
    }

    /// Waits until every queued write has reached the file.
    func drain() {
        queue.sync {}
    }

    private func write(_ texts: [String]) {
        guard !texts.isEmpty else { return }
        let text = texts.map { Self.redact($0, secrets: secrets) }.joined(separator: "\n") + "\n"
        queue.async { [self] in
            guard let handle else { return }
            do {
                try handle.write(contentsOf: Data(text.utf8))
            } catch {
                Self.logger.error("Writing the operation log failed: \(error, privacy: .public)")
                try? handle.close()
                self.handle = nil
            }
        }
    }

    // MARK: Naming, formatting, redaction and retention

    /// `20261002T153045Z-start-oauth-<uuid>.log`: sortable by start time, and safe in Finder (no colons).
    static func fileName(startedAt: Date, kind: OperationKind, subject: String, id: UUID) -> String {
        let formatter = DateFormatter()
        formatter.locale = Locale(identifier: "en_US_POSIX")
        formatter.timeZone = TimeZone(identifier: "UTC")
        formatter.dateFormat = "yyyyMMdd'T'HHmmss'Z'"
        return "\(formatter.string(from: startedAt))-\(kind.rawValue)-\(sanitized(subject))-\(id.uuidString.lowercased()).log"
    }

    static func format(_ line: LogLine, arrival: Date) -> String {
        let timestamp = RFC3339.format(line.timestamp ?? arrival)
        let level = line.level.rawValue.uppercased().padding(toLength: 6, withPad: " ", startingAt: 0)
        let target = line.target.map { "\($0): " } ?? ""
        return "\(timestamp) \(level) \(line.source.rawValue) \(target)\(line.message)"
    }

    static func redact(_ text: String, secrets: [String]) -> String {
        secrets.reduce(text) { $0.replacingOccurrences(of: $1, with: redactedMarker) }
    }

    /// Letters, digits, ".", "_" and "-"; every other run of characters becomes one "-".
    static func sanitized(_ subject: String) -> String {
        var result = ""
        for scalar in subject.unicodeScalars {
            let allowed = scalar.isASCII && (CharacterSet.alphanumerics.contains(scalar) || "._-".unicodeScalars.contains(scalar))
            if allowed {
                result.unicodeScalars.append(scalar)
            } else if result.last != "-" {
                result.append("-")
            }
        }
        let trimmed = result.trimmingCharacters(in: CharacterSet(charactersIn: "-"))
        return trimmed.isEmpty ? "operation" : String(trimmed.prefix(64))
    }

    /// Keeps the newest `count` `.log` files in `directory`, by creation time (names start with the start
    /// second, which several operations can share).
    static func prune(_ directory: URL, keeping count: Int) {
        let fileManager = FileManager.default
        let keys: [URLResourceKey] = [.creationDateKey]
        guard let urls = try? fileManager.contentsOfDirectory(at: directory, includingPropertiesForKeys: keys) else { return }
        func created(_ url: URL) -> Date {
            (try? url.resourceValues(forKeys: Set(keys)).creationDate) ?? .distantPast
        }
        let logs: [(url: URL, created: Date)] = urls.filter { $0.pathExtension == "log" }.map { ($0, created($0)) }
            .sorted { lhs, rhs in
                if lhs.created != rhs.created { return lhs.created < rhs.created }
                return lhs.url.lastPathComponent < rhs.url.lastPathComponent
            }
        guard logs.count > count else { return }
        for log in logs.prefix(logs.count - count) {
            try? fileManager.removeItem(at: log.url)
        }
    }
}
