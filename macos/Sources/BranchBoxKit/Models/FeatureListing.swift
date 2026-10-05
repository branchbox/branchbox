import Foundation

public struct FeatureListing: Sendable, Hashable {
    public let features: [FeatureRecord]
    public let strays: [StrayWorktree]
    public let droppedRecords: Int
    public let warnings: [String]
    public init(features: [FeatureRecord], strays: [StrayWorktree] = [], droppedRecords: Int = 0, warnings: [String] = []) {
        self.features = features
        self.strays = strays
        self.droppedRecords = droppedRecords
        self.warnings = warnings
    }

    /// Builds a listing from a lossily decoded `feature list --json` array (R-3): undecodable records are
    /// counted in `droppedRecords` and each one adds a warning naming why it was dropped.
    public init(decoding records: [Lossy<FeatureRecord>], strays: [StrayWorktree] = [], warnings: [String] = []) {
        let dropped = records.compactMap(\.error)
        self.init(features: records.compactMap(\.value), strays: strays, droppedRecords: dropped.count,
                  warnings: warnings + dropped.map { "Skipped an unreadable feature record: \($0)" })
    }
}

public struct BranchList: Sendable, Hashable {
    public let current: String?; public let local: [String]; public let remote: [String]
    public init(current: String?, local: [String], remote: [String]) {
        self.current = current
        self.local = local
        self.remote = remote
    }
}

public struct NamePreview: Sendable, Hashable { public let input: String; public let slug: String?; public let valid: Bool
    public let branchName: String?; public let worktreePath: String?; public let problem: String?
    public init(input: String, slug: String?, valid: Bool, branchName: String? = nil, worktreePath: String? = nil,
                problem: String? = nil) {
        self.input = input
        self.slug = slug
        self.valid = valid
        self.branchName = branchName
        self.worktreePath = worktreePath
        self.problem = problem
    }
}

public struct ProjectResolution: Sendable, Hashable {
    public enum Normalization: Sendable, Hashable { case none, fromFeatureWorktree, fromParentContainer }
    public let project: ProjectRef; public let requested: URL; public let normalization: Normalization; public let initialized: Bool
    public init(project: ProjectRef, requested: URL, normalization: Normalization, initialized: Bool) {
        self.project = project
        self.requested = requested
        self.normalization = normalization
        self.initialized = initialized
    }
}
