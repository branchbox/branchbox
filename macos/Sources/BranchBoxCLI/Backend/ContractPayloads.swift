import BranchBoxKit
import Foundation

// Decoders for the 0.14 payloads whose Kit models are not `Decodable` (DESIGN §4.8, wave-1 deviations): detect
// (§5.7), devcontainer sync (§5.8), config get (§5.10), doctor (§5.12) and init (§5.13). They follow the model
// rules: explicit snake_case keys, only identity keys required, everything else lenient, arrays lossy.

extension KeyedDecodingContainer {
    func optional<T: Decodable>(_ type: T.Type, _ key: Key) -> T? {
        try? decodeIfPresent(type, forKey: key)
    }

    func list<T: Decodable & Sendable>(_ type: T.Type, _ key: Key) -> [T] {
        (optional([Lossy<T>].self, key) ?? []).compactMap(\.value)
    }
}

/// `detect --json` (§5.7).
struct DetectPayload: Decodable {
    let report: DetectReport

    private enum CodingKeys: String, CodingKey {
        case project, gitRepository = "git_repository", initialized, stack, adapter, modules
        case hasDevcontainer = "has_devcontainer", hasEnv = "has_env", warnings
    }

    init(from decoder: any Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        report = DetectReport(project: c.optional(String.self, .project),
                              gitRepository: c.optional(Bool.self, .gitRepository) ?? false,
                              initialized: c.optional(Bool.self, .initialized) ?? false,
                              stack: c.optional(String.self, .stack), adapter: c.optional(String.self, .adapter),
                              modules: c.list(String.self, .modules),
                              hasDevcontainer: c.optional(Bool.self, .hasDevcontainer),
                              hasEnv: c.optional(Bool.self, .hasEnv), warnings: c.list(String.self, .warnings))
    }
}

/// `devcontainer sync --json` (§5.8). `results` is required: it is what makes the document a sync payload.
struct SyncPayload: Decodable {
    let report: SyncReport

    private enum CodingKeys: String, CodingKey { case dryRun = "dry_run", strategy, results }

    private struct Row: Decodable, Sendable {
        let row: SyncReport.Row

        private enum CodingKeys: String, CodingKey {
            case feature = "work_feature", worktreePath = "worktree_path", status, files
            case skipReason = "skip_reason", error
        }

        init(from decoder: any Decoder) throws {
            let c = try decoder.container(keyedBy: CodingKeys.self)
            let status = c.optional(String.self, .status).flatMap(SyncReport.Row.Status.init(rawValue:)) ?? .unknown
            row = SyncReport.Row(feature: try c.decode(String.self, forKey: .feature),
                                 worktreePath: c.optional(String.self, .worktreePath), status: status,
                                 files: c.list(String.self, .files), skipReason: c.optional(String.self, .skipReason),
                                 error: c.optional(String.self, .error))
        }
    }

    init(from decoder: any Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        let rows = try c.decode([Lossy<Row>].self, forKey: .results).compactMap(\.value).map(\.row)
        report = SyncReport(dryRun: c.optional(Bool.self, .dryRun) ?? false, strategy: c.optional(String.self, .strategy),
                            rows: rows)
    }
}

/// `config get --json` (§5.10).
struct ConfigGetPayload: Decodable {
    let document: ProjectConfigDocument

    private enum CodingKeys: String, CodingKey { case path, exists, effective, keys }

    init(from decoder: any Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        document = ProjectConfigDocument(path: c.optional(String.self, .path) ?? "",
                                         exists: c.optional(Bool.self, .exists) ?? false,
                                         effective: try c.decode(ProjectConfig.self, forKey: .effective),
                                         keys: c.list(ConfigKeyDescriptor.self, .keys), editable: true)
    }
}

/// `doctor --json` (§5.12). `checks` is required.
struct DoctorPayload: Decodable {
    let checks: [DoctorCheck]

    private enum CodingKeys: String, CodingKey { case checks }

    private struct Check: Decodable, Sendable {
        let check: DoctorCheck

        private enum CodingKeys: String, CodingKey { case id, title, required, status, path, version, detail, remediation }

        init(from decoder: any Decoder) throws {
            let c = try decoder.container(keyedBy: CodingKeys.self)
            let id = try c.decode(String.self, forKey: .id)
            // An unknown status must not read as healthy.
            let status = c.optional(String.self, .status).flatMap(DoctorCheck.Status.init(rawValue:)) ?? .warn
            check = DoctorCheck(id: id, title: c.optional(String.self, .title) ?? id,
                                required: c.optional(Bool.self, .required) ?? false, status: status,
                                path: c.optional(String.self, .path), version: c.optional(String.self, .version),
                                detail: c.optional(String.self, .detail),
                                remediation: c.optional(String.self, .remediation))
        }
    }

    init(from decoder: any Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        checks = try c.decode([Lossy<Check>].self, forKey: .checks).compactMap(\.value).map(\.check)
    }
}

/// `init --json` (§5.13). `workspace_path` is required.
struct InitPayload: Decodable {
    let workspacePath: String
    let reorganized: Bool
    let stack: String?
    let adapter: String?
    let modules: [String]
    let warnings: [String]
    let nextSteps: [String]
    let onePasswordStatus: String?

    private enum CodingKeys: String, CodingKey {
        case workspacePath = "workspace_path", reorganized, stack, adapter, modules, warnings
        case nextSteps = "next_steps", onePassword = "onepassword"
    }

    private enum OnePasswordKeys: String, CodingKey { case status }

    init(from decoder: any Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        workspacePath = try c.decode(String.self, forKey: .workspacePath)
        reorganized = c.optional(Bool.self, .reorganized) ?? false
        stack = c.optional(String.self, .stack)
        adapter = c.optional(String.self, .adapter)
        modules = c.list(String.self, .modules)
        warnings = c.list(String.self, .warnings)
        nextSteps = c.list(String.self, .nextSteps)
        let onePassword = try? c.nestedContainer(keyedBy: OnePasswordKeys.self, forKey: .onePassword)
        onePasswordStatus = onePassword?.optional(String.self, .status)
    }
}
