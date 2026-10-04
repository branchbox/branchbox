import Foundation

// RuntimeProvider is declared in Models (§4.8) and used here.

public enum BranchPolicy: String, Sendable, Hashable, Codable { case keep, deleteIfMerged, forceDelete }

public enum DevcontainerReusePolicy: String, Sendable, Hashable, Codable { case fail, preserve, overwrite, inspect }

public struct StartFeatureRequest: Sendable, Hashable, Codable {
    public enum Mode: String, Sendable, Hashable, Codable { case full, minimal }
    public enum Reuse: Sendable, Hashable, Codable { case none, existingWorktree(DevcontainerReusePolicy), retainedRuntime }
    public var project: ProjectRef
    public var name: String                          // resolved slug; `--title` is NEVER sent
    public var base: String?                         // nil = current HEAD (omit --base)
    public var branchPrefix: String?
    public var runtime: RuntimeProvider              // always explicit (--runtime)
    public var mode: Mode
    public var prompt: String?                       // ≤ 2000 chars (validated by StartDraft)
    public var useDefaultPrompt: Bool                // --default-prompt (minimal only)
    public var skipModules: [String]                 // compose | database | tunnel | specs
    public var reuse: Reuse                          // .retainedRuntime → --reuse-runtime
    public var keepRuntimeOnFailure: Bool            // sbx only
    public var verbose: Bool                         // RUST_LOG=debug + --telemetry
    public init(project: ProjectRef, name: String, runtime: RuntimeProvider) {
        self.project = project
        self.name = name
        self.base = nil
        self.branchPrefix = nil
        self.runtime = runtime
        self.mode = .full
        self.prompt = nil
        self.useDefaultPrompt = false
        self.skipModules = []
        self.reuse = .none
        self.keepRuntimeOnFailure = false
        self.verbose = false
    }
}

/// The user's explicit, per-presentation consent to discard changes. Built ONLY by RecoveryPlanner
/// from a refusal. Never defaulted, never persisted.
public struct DiscardConsent: Sendable, Hashable, Codable {
    public let userFiles: [String]                   // exact paths confirmed; empty = BranchBox-generated files only
    public let confirmedAt: Date
    public init(userFiles: [String], confirmedAt: Date = .now) {
        self.userFiles = userFiles
        self.confirmedAt = confirmedAt
    }
}

public struct TeardownRequest: Sendable, Hashable, Codable {
    public var feature: FeatureRef
    public var recordedBranch: String?               // FeatureRecord.branchName
    public var branch: BranchPolicy                  // ALWAYS explicit
    public var discard: DiscardConsent?              // nil on every first attempt
    public var forceRemoval: Bool                    // only from locked / status-unavailable / worktree-missing recoveries
    public var completeSpec: Bool
    public init(feature: FeatureRef, recordedBranch: String?, branch: BranchPolicy) {
        self.feature = feature
        self.recordedBranch = recordedBranch
        self.branch = branch
        self.discard = nil
        self.forceRemoval = false
        self.completeSpec = false
    }
}

public struct ExecRequest: Sendable, Hashable, Codable {
    public enum Target: Sendable, Hashable, Codable { case featureRuntime, devcontainer }
    public var feature: FeatureRef
    public var command: [String]                     // argv; "Run through shell" = ["/bin/sh","-lc",text]
    public var target: Target
    public var timeout: Duration?                    // nil = cancellable only
    public init(feature: FeatureRef, command: [String], target: Target = .featureRuntime, timeout: Duration? = nil) {
        self.feature = feature
        self.command = command
        self.target = target
        self.timeout = timeout
    }
}

public enum DevcontainerAction: Sendable, Hashable, Codable {
    case up(removeExisting: Bool, buildNoCache: Bool)   // Rebuild = .up(removeExisting: true, buildNoCache: true)
    case down(removeVolumes: Bool)
    case build(noCache: Bool)
}

public enum SyncStrategy: String, Sendable, Hashable, Codable { case copy, symlink }

public struct SyncRequest: Sendable, Hashable, Codable {
    public var project: ProjectRef
    public var strategy: SyncStrategy?
    public var dryRun: Bool
    public var features: [String]                    // requires .devcontainerSyncJSON; empty = all
    public init(project: ProjectRef, strategy: SyncStrategy? = nil, dryRun: Bool = false, features: [String] = []) {
        self.project = project
        self.strategy = strategy
        self.dryRun = dryRun
        self.features = features
    }
}

public struct InitRequest: Sendable, Hashable, Codable {
    public enum OnePassword: Sendable, Hashable, Codable { case unchanged, skip, configure(githubRef: String, signingKeyRef: String?, verify: Bool) }
    public var folder: URL
    public var stack: String?                        // rails | nodejs | rust | generic; nil = auto
    public var skipDevcontainer: Bool
    public var skipEnv: Bool
    public var codingAgents: Bool                    // false → --no-coding-agents
    public var reorganize: Bool                      // explicit opt-in only (moves the repo)
    public var dryRun: Bool
    public var mode: Mode
    public enum Mode: String, Sendable, Hashable, Codable { case initialize, update, validate }
    public var onePassword: OnePassword              // requires .initJSON; ignored (hidden) on legacy
    public var tunnelsEnabled: Bool?                 // applied after init via config apply (requires .config)
    /// Defaults match `branchbox init -y`: auto-detected stack, every module, coding agents on, no reorganize.
    public init(folder: URL, stack: String? = nil, skipDevcontainer: Bool = false, skipEnv: Bool = false,
                codingAgents: Bool = true, reorganize: Bool = false, dryRun: Bool = false, mode: Mode = .initialize,
                onePassword: OnePassword = .unchanged, tunnelsEnabled: Bool? = nil) {
        self.folder = folder
        self.stack = stack
        self.skipDevcontainer = skipDevcontainer
        self.skipEnv = skipEnv
        self.codingAgents = codingAgents
        self.reorganize = reorganize
        self.dryRun = dryRun
        self.mode = mode
        self.onePassword = onePassword
        self.tunnelsEnabled = tunnelsEnabled
    }
}

public enum JSONValue: Sendable, Hashable, Codable {
    case null, bool(Bool), number(Double), string(String), array([JSONValue]), object([String: JSONValue])

    public init(from decoder: any Decoder) throws {
        let container = try decoder.singleValueContainer()
        if container.decodeNil() {
            self = .null
        } else if let value = try? container.decode(Bool.self) {
            // Bool before Double: JSONDecoder refuses to read `true` as a number, but not every
            // decoder does.
            self = .bool(value)
        } else if let value = try? container.decode(Double.self) {
            self = .number(value)
        } else if let value = try? container.decode(String.self) {
            self = .string(value)
        } else if let value = try? container.decode([JSONValue].self) {
            self = .array(value)
        } else if let value = try? container.decode([String: JSONValue].self) {
            self = .object(value)
        } else {
            throw DecodingError.dataCorruptedError(in: container, debugDescription: "Not a JSON value")
        }
    }

    public func encode(to encoder: any Encoder) throws {
        var container = encoder.singleValueContainer()
        switch self {
        case .null: try container.encodeNil()
        case .bool(let value): try container.encode(value)
        case .number(let value): try container.encode(value)
        case .string(let value): try container.encode(value)
        case .array(let value): try container.encode(value)
        case .object(let value): try container.encode(value)
        }
    }
}

public struct ConfigChange: Sendable, Hashable, Codable {
    public let key: String                           // dotted key from the key registry, e.g. "feature.branch_prefix"
    public let value: JSONValue?                     // nil = unset
    public init(key: String, value: JSONValue?) { self.key = key; self.value = value }
}
public struct ConfigPatch: Sendable, Hashable, Codable {
    public var changes: [ConfigChange]
    public init(changes: [ConfigChange]) { self.changes = changes }
}

/// Token wrapper whose description is always redacted.
public struct SecretString: Sendable, Hashable, CustomStringConvertible, CustomDebugStringConvertible {
    public let value: String
    public init(_ value: String) { self.value = value }
    public var description: String { "••••" }
    public var debugDescription: String { "••••" }
}

extension SecretString: CustomReflectable {
    /// `dump(_:)` and debugger summaries walk children, which would print `value`; expose none.
    public var customMirror: Mirror { Mirror(self, children: [], displayStyle: .struct) }
}

public struct TunnelCredentialsRequest: Sendable, Hashable {
    public var accountID: String
    public var apiToken: SecretString?               // sent on stdin; never argv/log
    public var clear: Bool
    public init(accountID: String, apiToken: SecretString?, clear: Bool = false) {
        self.accountID = accountID
        self.apiToken = apiToken
        self.clear = clear
    }
}

public struct StrayWorktree: Sendable, Hashable, Codable {
    public let path: String
    public let branch: String?                       // refs/heads/ stripped
    public let head: String?
    public let locked: Bool
    public let prunable: Bool
    public init(path: String, branch: String?, head: String?, locked: Bool = false, prunable: Bool = false) {
        self.path = path
        self.branch = branch
        self.head = head
        self.locked = locked
        self.prunable = prunable
    }
}

public struct PruneSelection: Sendable, Hashable {
    public var project: ProjectRef; public var rows: [TeardownRequest]
    public init(project: ProjectRef, rows: [TeardownRequest]) { self.project = project; self.rows = rows }
}

/// Everything an OperationRecord can be asked to run; also the payload of `RecoveryAction.retry`.
public enum OperationRequestContext: Sendable, Hashable {
    case start(StartFeatureRequest)
    case teardown(TeardownRequest)
    case prune(PruneSelection)
    case exec(ExecRequest)
    case devcontainer(DevcontainerAction, FeatureRef)
    case syncDevcontainers(SyncRequest)
    case tunnelOpen(FeatureRef)
    case tunnelRemove(FeatureRef, force: Bool)
    case initProject(InitRequest)
    case applyConfig(ConfigPatch, ProjectRef)
    case tunnelCredentials(TunnelCredentialsRequest, ProjectRef)
    case deleteBranch(String, ProjectRef, force: Bool)
    case removeStray(StrayWorktree, ProjectRef, discardChanges: Bool)
}
