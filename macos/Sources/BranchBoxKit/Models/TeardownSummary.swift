import Foundation

/// `feature teardown --json` (+ additive 0.14 fields, §5.6).
public struct TeardownSummary: Decodable, Sendable, Hashable { public let workFeature: String; public let branchName: String?
    public let worktreeRemoved: Bool; public let branchDeleted: Bool; public let adapterCleanupWarnings: [String]
    public let moduleReports: [ModuleReport]; public let runtimeTeardown: RuntimeTeardownReport?; public let warnings: [String]
    public let branchAction: String?                 // 0.14: keep|delete|force_delete
    public let branchDeleteError: String?            // 0.14
    public let discardedChanges: [ChangedFile]       // 0.14
    public let preserved: [PreservedFile]            // 0.14
    public let registryUpdated: Bool?                // 0.14

    public init(workFeature: String, branchName: String? = nil, worktreeRemoved: Bool = false, branchDeleted: Bool = false,
                adapterCleanupWarnings: [String] = [], moduleReports: [ModuleReport] = [],
                runtimeTeardown: RuntimeTeardownReport? = nil, warnings: [String] = [], branchAction: String? = nil,
                branchDeleteError: String? = nil, discardedChanges: [ChangedFile] = [], preserved: [PreservedFile] = [],
                registryUpdated: Bool? = nil) {
        self.workFeature = workFeature
        self.branchName = branchName
        self.worktreeRemoved = worktreeRemoved
        self.branchDeleted = branchDeleted
        self.adapterCleanupWarnings = adapterCleanupWarnings
        self.moduleReports = moduleReports
        self.runtimeTeardown = runtimeTeardown
        self.warnings = warnings
        self.branchAction = branchAction
        self.branchDeleteError = branchDeleteError
        self.discardedChanges = discardedChanges
        self.preserved = preserved
        self.registryUpdated = registryUpdated
    }

    private enum CodingKeys: String, CodingKey {
        case workFeature = "work_feature", branchName = "branch_name", worktreeRemoved = "worktree_removed"
        case branchDeleted = "branch_deleted", adapterCleanupWarnings = "adapter_cleanup_warnings"
        case moduleReports = "module_reports", runtimeTeardown = "runtime_teardown", warnings
        case branchAction = "branch_action", branchDeleteError = "branch_delete_error"
        case discardedChanges = "discarded_changes", preserved, registryUpdated = "registry_updated"
    }

    public init(from decoder: any Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        workFeature = try c.decode(String.self, forKey: .workFeature)
        branchName = c.lenient(String.self, forKey: .branchName)
        worktreeRemoved = c.lenient(Bool.self, forKey: .worktreeRemoved) ?? false
        branchDeleted = c.lenient(Bool.self, forKey: .branchDeleted) ?? false
        adapterCleanupWarnings = c.lossyArray(String.self, forKey: .adapterCleanupWarnings)
        moduleReports = c.lossyArray(ModuleReport.self, forKey: .moduleReports)
        runtimeTeardown = c.lenient(RuntimeTeardownReport.self, forKey: .runtimeTeardown)
        warnings = c.lossyArray(String.self, forKey: .warnings)
        branchAction = c.lenient(String.self, forKey: .branchAction)
        branchDeleteError = c.lenient(String.self, forKey: .branchDeleteError)
        discardedChanges = c.lossyArray(ChangedFile.self, forKey: .discardedChanges)
        preserved = c.lossyArray(PreservedFile.self, forKey: .preserved)
        registryUpdated = c.lenient(Bool.self, forKey: .registryUpdated)
    }
}

public struct ModuleReport: Codable, Sendable, Hashable {
    public let name: String; public let teardownOk: Bool; public let errors: [String]
    public init(name: String, teardownOk: Bool, errors: [String] = []) {
        self.name = name
        self.teardownOk = teardownOk
        self.errors = errors
    }

    private enum CodingKeys: String, CodingKey { case name, teardownOk = "teardown_ok", errors }

    public init(from decoder: any Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        name = try c.decode(String.self, forKey: .name)
        teardownOk = c.lenient(Bool.self, forKey: .teardownOk) ?? false
        errors = c.lossyArray(String.self, forKey: .errors)
    }
}

public struct RuntimeTeardownReport: Codable, Sendable, Hashable { public let provider: String?; public let runtimeID: String?
    public let verified: Bool; public let residueFree: Bool; public let residue: [ResidueItem]
    public init(provider: String?, runtimeID: String? = nil, verified: Bool, residueFree: Bool, residue: [ResidueItem] = []) {
        self.provider = provider
        self.runtimeID = runtimeID
        self.verified = verified
        self.residueFree = residueFree
        self.residue = residue
    }

    private enum CodingKeys: String, CodingKey {
        case provider, runtimeID = "runtime_id", verified, residueFree = "residue_free", residue
    }

    public init(from decoder: any Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        provider = c.lenient(String.self, forKey: .provider)
        runtimeID = c.lenient(String.self, forKey: .runtimeID)
        verified = c.lenient(Bool.self, forKey: .verified) ?? false
        residueFree = c.lenient(Bool.self, forKey: .residueFree) ?? false
        residue = c.lossyArray(ResidueItem.self, forKey: .residue)
    }
}

public struct ResidueItem: Codable, Sendable, Hashable {
    public let kind: String; public let identifiers: [String]
    public init(kind: String, identifiers: [String]) { self.kind = kind; self.identifiers = identifiers }

    private enum CodingKeys: String, CodingKey { case kind, identifiers }

    public init(from decoder: any Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        kind = c.lenient(String.self, forKey: .kind) ?? ""
        identifiers = c.lossyArray(String.self, forKey: .identifiers)
    }
}

public struct PreservedFile: Codable, Sendable, Hashable {
    public let path: String; public let destination: String
    public init(path: String, destination: String) { self.path = path; self.destination = destination }

    private enum CodingKeys: String, CodingKey { case path, destination }

    public init(from decoder: any Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        path = try c.decode(String.self, forKey: .path)
        destination = c.lenient(String.self, forKey: .destination) ?? ""
    }
}

public struct TeardownOutcome: Sendable, Hashable {
    public let summary: TeardownSummary
    public let branch: BranchOutcome
    public let worktreeGone: Bool                    // verified on disk by CLIBackend after return
    public init(summary: TeardownSummary, branch: BranchOutcome, worktreeGone: Bool) {
        self.summary = summary
        self.branch = branch
        self.worktreeGone = worktreeGone
    }
}

public enum BranchOutcome: Sendable, Hashable {
    public enum Deleter: String, Sendable, Hashable { case cli, app }
    case kept(String?)
    case deleted(String, by: Deleter)
    case deleteFailed(String, reason: String)        // UI offers .retry(.deleteBranch(force: true)) only if unmerged
    case notFound(String)
}
