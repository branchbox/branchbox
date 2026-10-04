import Foundation

/// Result of `devcontainer sync`: `--json` rows (§5.8) on contract CLIs, parsed text on legacy ones.
public struct SyncReport: Sendable, Hashable {
    public struct Row: Sendable, Hashable { public enum Status: String, Sendable, Hashable { case synced, wouldSync = "would_sync", skipped, failed, unknown }
        public let feature: String; public let worktreePath: String?; public let status: Status; public let files: [String]
        public let skipReason: String?; public let error: String?
        public init(feature: String, worktreePath: String? = nil, status: Status, files: [String] = [],
                    skipReason: String? = nil, error: String? = nil) {
            self.feature = feature
            self.worktreePath = worktreePath
            self.status = status
            self.files = files
            self.skipReason = skipReason
            self.error = error
        }
    }
    public let dryRun: Bool; public let strategy: String?; public let rows: [Row]
    public var failedCount: Int { rows.lazy.filter { $0.status == .failed }.count }; public let rawText: String?
    public init(dryRun: Bool, strategy: String? = nil, rows: [Row], rawText: String? = nil) {
        self.dryRun = dryRun
        self.strategy = strategy
        self.rows = rows
        self.rawText = rawText
    }
}
