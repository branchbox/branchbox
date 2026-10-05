import Foundation

/// `feature teardown --dry-run --json` (§5.5), mirrored exactly. CLIBackend also builds one from its own
/// preflight on legacy CLIs (`source == .appPreflight`).
public struct TeardownPlanDocument: Decodable, Sendable, Hashable {
    public enum Source: String, Sendable, Hashable { case cli, appPreflight }
    public var source: Source                        // not in JSON; CLIBackend sets it (decoder default .cli)
    public let workFeature: String
    public let registered: Bool
    public let status: FeatureStatus?
    public let worktree: Worktree
    public let changes: Changes
    public let branch: Branch?
    public let defaults: Defaults?
    public let runtime: RuntimeRef?
    public let tunnel: TunnelRef?
    public let blockers: [Blocker]
    public let warnings: [String]
    /// Blockers that were in the JSON but could not be decoded (R-3 drops them). Planners must treat a plan with
    /// `droppedBlockers > 0` as blocked: the CLI refused something this version cannot name.
    public let droppedBlockers: Int

    public init(source: Source = .cli, workFeature: String, registered: Bool, status: FeatureStatus? = nil,
                worktree: Worktree, changes: Changes, branch: Branch? = nil, defaults: Defaults? = nil,
                runtime: RuntimeRef? = nil, tunnel: TunnelRef? = nil, blockers: [Blocker] = [], warnings: [String] = []) {
        self.source = source
        self.workFeature = workFeature
        self.registered = registered
        self.status = status
        self.worktree = worktree
        self.changes = changes
        self.branch = branch
        self.defaults = defaults
        self.runtime = runtime
        self.tunnel = tunnel
        self.blockers = blockers
        self.warnings = warnings
        self.droppedBlockers = 0
    }

    private enum CodingKeys: String, CodingKey {
        case workFeature = "work_feature", registered, status, worktree, changes, branch, defaults, runtime, tunnel
        case blockers, warnings
    }

    public init(from decoder: any Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        source = .cli
        workFeature = try c.decode(String.self, forKey: .workFeature)
        registered = c.lenient(Bool.self, forKey: .registered) ?? false
        status = c.lenient(FeatureStatus.self, forKey: .status)
        worktree = try c.decode(Worktree.self, forKey: .worktree)
        // A plan without a readable change set must not look clean.
        changes = c.lenient(Changes.self, forKey: .changes) ?? .unavailable
        branch = c.lenient(Branch.self, forKey: .branch)
        defaults = c.lenient(Defaults.self, forKey: .defaults)
        runtime = c.lenient(RuntimeRef.self, forKey: .runtime)
        tunnel = c.lenient(TunnelRef.self, forKey: .tunnel)
        let blockerList = c.lossyArrayCountingDrops(Blocker.self, forKey: .blockers)
        blockers = blockerList.values
        droppedBlockers = blockerList.dropped
        warnings = c.lossyArray(String.self, forKey: .warnings)
    }

    public struct Worktree: Codable, Sendable, Hashable {
        public let path: String; public let exists: Bool; public let locked: Bool; public let lockReason: String?
        public init(path: String, exists: Bool, locked: Bool = false, lockReason: String? = nil) {
            self.path = path
            self.exists = exists
            self.locked = locked
            self.lockReason = lockReason
        }

        private enum CodingKeys: String, CodingKey { case path, exists, locked, lockReason = "lock_reason" }

        public init(from decoder: any Decoder) throws {
            let c = try decoder.container(keyedBy: CodingKeys.self)
            path = try c.decode(String.self, forKey: .path)
            exists = c.lenient(Bool.self, forKey: .exists) ?? false
            locked = c.lenient(Bool.self, forKey: .locked) ?? false
            lockReason = c.lenient(String.self, forKey: .lockReason)
        }
    }

    public struct Changes: Codable, Sendable, Hashable { public let statusAvailable: Bool; public let truncated: Bool
        public let user: [ChangedFile]; public let generated: [GeneratedFile]; public let preserved: [PreservedFile]
        /// Entries of `user`, `generated` and `preserved` that could not be decoded. A dropped user change also
        /// forces `statusAvailable` to false, so an unreadable change set never reads as a clean worktree.
        public let droppedEntries: Int
        /// `git status` could not be read, so nothing is known about the worktree's changes.
        public static let unavailable = Changes(statusAvailable: false, truncated: false, user: [], generated: [], preserved: [])
        public init(statusAvailable: Bool, truncated: Bool = false, user: [ChangedFile] = [],
                    generated: [GeneratedFile] = [], preserved: [PreservedFile] = []) {
            self.statusAvailable = statusAvailable
            self.truncated = truncated
            self.user = user
            self.generated = generated
            self.preserved = preserved
            self.droppedEntries = 0
        }

        private enum CodingKeys: String, CodingKey {
            case statusAvailable = "status_available", truncated, user, generated, preserved
        }

        public init(from decoder: any Decoder) throws {
            let c = try decoder.container(keyedBy: CodingKeys.self)
            truncated = c.lenient(Bool.self, forKey: .truncated) ?? false
            let user = c.lossyArrayCountingDrops(ChangedFile.self, forKey: .user)
            let generated = c.lossyArrayCountingDrops(GeneratedFile.self, forKey: .generated)
            let preserved = c.lossyArrayCountingDrops(PreservedFile.self, forKey: .preserved)
            self.user = user.values
            self.generated = generated.values
            self.preserved = preserved.values
            droppedEntries = user.dropped + generated.dropped + preserved.dropped
            statusAvailable = (c.lenient(Bool.self, forKey: .statusAvailable) ?? false) && user.dropped == 0
        }
    }

    public struct GeneratedFile: Codable, Sendable, Hashable {
        public let path: String; public let rule: String
        public init(path: String, rule: String) { self.path = path; self.rule = rule }

        private enum CodingKeys: String, CodingKey { case path, rule }

        public init(from decoder: any Decoder) throws {
            let c = try decoder.container(keyedBy: CodingKeys.self)
            path = try c.decode(String.self, forKey: .path)
            rule = c.lenient(String.self, forKey: .rule) ?? ""
        }
    }

    public struct Branch: Codable, Sendable, Hashable { public let name: String; public let source: String; public let exists: Bool
        public let upstream: String?; public let reference: String; public let referenceName: String; public let merged: Bool
        public let mergedIntoHead: Bool; public let ahead: Int; public let action: String
        public init(name: String, source: String, exists: Bool, upstream: String? = nil, reference: String,
                    referenceName: String, merged: Bool, mergedIntoHead: Bool, ahead: Int, action: String) {
            self.name = name
            self.source = source
            self.exists = exists
            self.upstream = upstream
            self.reference = reference
            self.referenceName = referenceName
            self.merged = merged
            self.mergedIntoHead = mergedIntoHead
            self.ahead = ahead
            self.action = action
        }

        private enum CodingKeys: String, CodingKey {
            case name, source, exists, upstream, reference, referenceName = "reference_name", merged
            case mergedIntoHead = "merged_into_head", ahead, action
        }

        /// Absent facts read as the cautious answer: not merged, nothing known ahead, keep the branch.
        public init(from decoder: any Decoder) throws {
            let c = try decoder.container(keyedBy: CodingKeys.self)
            name = try c.decode(String.self, forKey: .name)
            source = c.lenient(String.self, forKey: .source) ?? ""
            exists = c.lenient(Bool.self, forKey: .exists) ?? false
            upstream = c.lenient(String.self, forKey: .upstream)
            reference = c.lenient(String.self, forKey: .reference) ?? ""
            referenceName = c.lenient(String.self, forKey: .referenceName) ?? ""
            merged = c.lenient(Bool.self, forKey: .merged) ?? false
            mergedIntoHead = c.lenient(Bool.self, forKey: .mergedIntoHead) ?? false
            ahead = c.lenient(Int.self, forKey: .ahead) ?? 0
            action = c.lenient(String.self, forKey: .action) ?? "keep"
        }
    }

    public struct Defaults: Codable, Sendable, Hashable {
        public let deleteBranchByDefault: Bool; public let forceDeleteUnmergedByDefault: Bool
        public init(deleteBranchByDefault: Bool, forceDeleteUnmergedByDefault: Bool) {
            self.deleteBranchByDefault = deleteBranchByDefault
            self.forceDeleteUnmergedByDefault = forceDeleteUnmergedByDefault
        }

        private enum CodingKeys: String, CodingKey {
            case deleteBranchByDefault = "delete_branch_by_default"
            case forceDeleteUnmergedByDefault = "force_delete_unmerged_by_default"
        }

        /// Missing keys take core's `FeatureTeardownSettings` defaults (delete: true, force: false).
        public init(from decoder: any Decoder) throws {
            let c = try decoder.container(keyedBy: CodingKeys.self)
            deleteBranchByDefault = c.lenient(Bool.self, forKey: .deleteBranchByDefault) ?? true
            forceDeleteUnmergedByDefault = c.lenient(Bool.self, forKey: .forceDeleteUnmergedByDefault) ?? false
        }
    }

    public struct RuntimeRef: Codable, Sendable, Hashable {
        public let provider: String?; public let runtimeID: String?
        public init(provider: String?, runtimeID: String? = nil) { self.provider = provider; self.runtimeID = runtimeID }

        private enum CodingKeys: String, CodingKey { case provider, runtimeID = "runtime_id" }

        public init(from decoder: any Decoder) throws {
            let c = try decoder.container(keyedBy: CodingKeys.self)
            provider = c.lenient(String.self, forKey: .provider)
            runtimeID = c.lenient(String.self, forKey: .runtimeID)
        }
    }

    public struct TunnelRef: Codable, Sendable, Hashable {
        public let status: TunnelStatus?
        public init(status: TunnelStatus?) { self.status = status }

        private enum CodingKeys: String, CodingKey { case status }

        public init(from decoder: any Decoder) throws {
            let c = try decoder.container(keyedBy: CodingKeys.self)
            status = c.lenient(TunnelStatus.self, forKey: .status)
        }
    }

    public struct Blocker: Codable, Sendable, Hashable { public let kind: String; public let message: String; public let override: String?
        public let count: Int?; public let branch: String?; public let ahead: Int?; public let cause: String?
        public init(kind: String, message: String, override: String? = nil, count: Int? = nil, branch: String? = nil,
                    ahead: Int? = nil, cause: String? = nil) {
            self.kind = kind
            self.message = message
            self.override = override
            self.count = count
            self.branch = branch
            self.ahead = ahead
            self.cause = cause
        }

        private enum CodingKeys: String, CodingKey { case kind, message, override, count, branch, ahead, cause }

        public init(from decoder: any Decoder) throws {
            let c = try decoder.container(keyedBy: CodingKeys.self)
            kind = try c.decode(String.self, forKey: .kind)
            message = c.lenient(String.self, forKey: .message) ?? ""
            override = c.lenient(String.self, forKey: .override)
            count = c.lenient(Int.self, forKey: .count)
            branch = c.lenient(String.self, forKey: .branch)
            ahead = c.lenient(Int.self, forKey: .ahead)
            cause = c.lenient(String.self, forKey: .cause)
        }
    }
}
