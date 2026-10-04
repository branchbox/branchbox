import Foundation

/// One element of `feature list --json` (core `FeatureMetadata` plus the CLI-added `default_agent`).
public struct FeatureRecord: Decodable, Sendable, Hashable, Identifiable {
    public var id: String { workFeature }
    public let workFeature: String                   // work_feature (required)
    public let branchName: String                    // branch_name ("" if absent)
    public let worktreePath: String?                 // worktree_path
    public let baseBranch: String?                   // base_branch
    public let featureURL: String?                   // feature_url (scheme-less)
    public let composeProjectName: String?           // compose_project_name
    public let envPath: String?                      // env_path
    public let status: FeatureStatus
    public let createdAt: Date?, updatedAt: Date?, removedAt: Date?, lastSyncAt: Date?
    public let tunnel: TunnelState?                  // NESTED object
    public let color: String?                        // "#e67e22"
    public let lastCommit: String?
    public let prNumber: Int?
    public let devcontainerOutdated: Bool
    public let syncStrategy: String?
    public let startMode: String?                    // "full" | "minimal"
    public let promptSeed: String?
    public let moduleOutcomes: [ModuleOutcome]
    public let adapter: AdapterInfo?
    public let runtime: RuntimeInfo                  // default .container if absent
    public let defaultAgent: DefaultAgentPlan?       // default_agent
    public let setup: SetupInfo?                     // NEW (write-ahead; §5.4). nil on legacy CLIs / completed starts
    /// Local read-only Git evidence, added by the CLI backend after listing. Never read from or written to the
    /// feature registry, whose status may remain active even after its Git metadata has disappeared.
    public var worktreeIssue: String?

    /// The prefix to hand back to `feature teardown --branch-prefix` (core rebuilds the branch as
    /// `<prefix>/<name>`): `branch_name` minus "/<work_feature>"; "" when the branch is the bare name;
    /// nil when the branch does not end in the name, so no prefix can be derived.
    public var branchPrefix: String? {
        guard !branchName.isEmpty else { return nil }
        if branchName == workFeature { return "" }
        let suffix = "/" + workFeature
        guard branchName.count > suffix.count, branchName.hasSuffix(suffix) else { return nil }
        return String(branchName.dropLast(suffix.count))
    }

    public var urls: FeatureURLs {
        FeatureURLs(featureURL: featureURL, tunnel: tunnel, runtime: runtime, adapter: adapter)
    }

    public init(workFeature: String, branchName: String = "", worktreePath: String? = nil, baseBranch: String? = nil,
                featureURL: String? = nil, composeProjectName: String? = nil, envPath: String? = nil,
                status: FeatureStatus = .active, createdAt: Date? = nil, updatedAt: Date? = nil,
                removedAt: Date? = nil, lastSyncAt: Date? = nil, tunnel: TunnelState? = nil, color: String? = nil,
                lastCommit: String? = nil, prNumber: Int? = nil, devcontainerOutdated: Bool = false,
                syncStrategy: String? = nil, startMode: String? = nil, promptSeed: String? = nil,
                moduleOutcomes: [ModuleOutcome] = [], adapter: AdapterInfo? = nil,
                runtime: RuntimeInfo = .containerDefault, defaultAgent: DefaultAgentPlan? = nil,
                setup: SetupInfo? = nil, worktreeIssue: String? = nil) {
        self.workFeature = workFeature
        self.branchName = branchName
        self.worktreePath = worktreePath
        self.baseBranch = baseBranch
        self.featureURL = featureURL
        self.composeProjectName = composeProjectName
        self.envPath = envPath
        self.status = status
        self.createdAt = createdAt
        self.updatedAt = updatedAt
        self.removedAt = removedAt
        self.lastSyncAt = lastSyncAt
        self.tunnel = tunnel
        self.color = color
        self.lastCommit = lastCommit
        self.prNumber = prNumber
        self.devcontainerOutdated = devcontainerOutdated
        self.syncStrategy = syncStrategy
        self.startMode = startMode
        self.promptSeed = promptSeed
        self.moduleOutcomes = moduleOutcomes
        self.adapter = adapter
        self.runtime = runtime
        self.defaultAgent = defaultAgent
        self.setup = setup
        self.worktreeIssue = worktreeIssue
    }

    private enum CodingKeys: String, CodingKey {
        case workFeature = "work_feature", branchName = "branch_name", worktreePath = "worktree_path"
        case baseBranch = "base_branch", featureURL = "feature_url", composeProjectName = "compose_project_name"
        case envPath = "env_path", status
        case createdAt = "created_at", updatedAt = "updated_at", removedAt = "removed_at", lastSyncAt = "last_sync_at"
        case tunnel, color, lastCommit = "last_commit", prNumber = "pr_number"
        case devcontainerOutdated = "devcontainer_outdated", syncStrategy = "sync_strategy"
        case startMode = "start_mode", promptSeed = "prompt_seed", moduleOutcomes = "module_outcomes"
        case adapter, runtime, defaultAgent = "default_agent", setup
    }

    public init(from decoder: any Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        workFeature = try c.decode(String.self, forKey: .workFeature)
        branchName = c.lenient(String.self, forKey: .branchName) ?? ""
        worktreePath = c.lenient(String.self, forKey: .worktreePath)
        baseBranch = c.lenient(String.self, forKey: .baseBranch)
        featureURL = c.lenient(String.self, forKey: .featureURL)
        composeProjectName = c.lenient(String.self, forKey: .composeProjectName)
        envPath = c.lenient(String.self, forKey: .envPath)
        status = c.lenient(FeatureStatus.self, forKey: .status) ?? .unknown("")
        createdAt = c.rfc3339IfPresent(forKey: .createdAt)
        updatedAt = c.rfc3339IfPresent(forKey: .updatedAt)
        removedAt = c.rfc3339IfPresent(forKey: .removedAt)
        lastSyncAt = c.rfc3339IfPresent(forKey: .lastSyncAt)
        tunnel = c.lenient(TunnelState.self, forKey: .tunnel)
        color = c.lenient(String.self, forKey: .color)
        lastCommit = c.lenient(String.self, forKey: .lastCommit)
        prNumber = c.lenient(Int.self, forKey: .prNumber)
        devcontainerOutdated = c.lenient(Bool.self, forKey: .devcontainerOutdated) ?? false
        syncStrategy = c.lenient(String.self, forKey: .syncStrategy)
        startMode = c.lenient(String.self, forKey: .startMode)
        promptSeed = c.lenient(String.self, forKey: .promptSeed)
        moduleOutcomes = c.lossyArray(ModuleOutcome.self, forKey: .moduleOutcomes)
        adapter = c.lenient(AdapterInfo.self, forKey: .adapter)
        runtime = c.lenient(RuntimeInfo.self, forKey: .runtime) ?? .containerDefault
        defaultAgent = c.lenient(DefaultAgentPlan.self, forKey: .defaultAgent)
        setup = c.lenient(SetupInfo.self, forKey: .setup)
        worktreeIssue = nil
    }
}

public struct SetupInfo: Codable, Sendable, Hashable {
    public let state: SetupState; public let pid: Int?; public let startedAt: Date?
    public init(state: SetupState, pid: Int? = nil, startedAt: Date? = nil) {
        self.state = state
        self.pid = pid
        self.startedAt = startedAt
    }

    private enum CodingKeys: String, CodingKey { case state, pid, startedAt = "started_at" }

    /// `state` is what makes this a setup marker, so it is required; a marker without one is dropped.
    public init(from decoder: any Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        state = try c.decode(SetupState.self, forKey: .state)
        pid = c.lenient(Int.self, forKey: .pid)
        startedAt = c.rfc3339IfPresent(forKey: .startedAt)
    }

    public func encode(to encoder: any Encoder) throws {
        var c = encoder.container(keyedBy: CodingKeys.self)
        try c.encode(state, forKey: .state)
        try c.encodeIfPresent(pid, forKey: .pid)
        try c.encodeRFC3339IfPresent(startedAt, forKey: .startedAt)
    }
}

public struct ModuleOutcome: Codable, Sendable, Hashable { public let module: String; public let status: ModuleStatus
    public let durationMs: Int?; public let notes: [String]; public let forced: Bool; public let recordedAt: Date?
    public init(module: String, status: ModuleStatus, durationMs: Int? = nil, notes: [String] = [],
                forced: Bool = false, recordedAt: Date? = nil) {
        self.module = module
        self.status = status
        self.durationMs = durationMs
        self.notes = notes
        self.forced = forced
        self.recordedAt = recordedAt
    }

    private enum CodingKeys: String, CodingKey {
        case module, status, durationMs = "duration_ms", notes, forced, recordedAt = "recorded_at"
    }

    public init(from decoder: any Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        module = try c.decode(String.self, forKey: .module)
        status = c.lenient(ModuleStatus.self, forKey: .status) ?? .unknown("")
        durationMs = c.lenient(Int.self, forKey: .durationMs)
        notes = c.lossyArray(String.self, forKey: .notes)
        forced = c.lenient(Bool.self, forKey: .forced) ?? false
        recordedAt = c.rfc3339IfPresent(forKey: .recordedAt)
    }

    public func encode(to encoder: any Encoder) throws {
        var c = encoder.container(keyedBy: CodingKeys.self)
        try c.encode(module, forKey: .module)
        try c.encode(status, forKey: .status)
        try c.encodeIfPresent(durationMs, forKey: .durationMs)
        try c.encode(notes, forKey: .notes)
        try c.encode(forced, forKey: .forced)
        try c.encodeRFC3339IfPresent(recordedAt, forKey: .recordedAt)
    }
}

public struct TunnelState: Codable, Sendable, Hashable { public let provider: String?; public let hostname: String?
    public let serviceURL: String?; public let status: TunnelStatus; public let instructions: [String]; public let notes: String?
    public let lastUpdated: Date?
    public init(provider: String? = nil, hostname: String? = nil, serviceURL: String? = nil, status: TunnelStatus,
                instructions: [String] = [], notes: String? = nil, lastUpdated: Date? = nil) {
        self.provider = provider
        self.hostname = hostname
        self.serviceURL = serviceURL
        self.status = status
        self.instructions = instructions
        self.notes = notes
        self.lastUpdated = lastUpdated
    }

    private enum CodingKeys: String, CodingKey {
        case provider, hostname, serviceURL = "service_url", status, instructions, notes, lastUpdated = "last_updated"
    }

    public init(from decoder: any Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        provider = c.lenient(String.self, forKey: .provider)
        hostname = c.lenient(String.self, forKey: .hostname)
        serviceURL = c.lenient(String.self, forKey: .serviceURL)
        status = c.lenient(TunnelStatus.self, forKey: .status) ?? .unknown("")
        instructions = c.lossyArray(String.self, forKey: .instructions)
        notes = c.lenient(String.self, forKey: .notes)
        lastUpdated = c.rfc3339IfPresent(forKey: .lastUpdated)
    }

    public func encode(to encoder: any Encoder) throws {
        var c = encoder.container(keyedBy: CodingKeys.self)
        try c.encodeIfPresent(provider, forKey: .provider)
        try c.encodeIfPresent(hostname, forKey: .hostname)
        try c.encodeIfPresent(serviceURL, forKey: .serviceURL)
        try c.encode(status, forKey: .status)
        try c.encode(instructions, forKey: .instructions)
        try c.encodeIfPresent(notes, forKey: .notes)
        try c.encodeRFC3339IfPresent(lastUpdated, forKey: .lastUpdated)
    }
}

public struct AdapterInfo: Codable, Sendable, Hashable {
    public let name: String?; public let serviceURL: String?; public let warnings: [String]
    public init(name: String? = nil, serviceURL: String? = nil, warnings: [String] = []) {
        self.name = name
        self.serviceURL = serviceURL
        self.warnings = warnings
    }

    private enum CodingKeys: String, CodingKey { case name, serviceURL = "service_url", warnings }

    public init(from decoder: any Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        name = c.lenient(String.self, forKey: .name)
        serviceURL = c.lenient(String.self, forKey: .serviceURL)
        warnings = c.lossyArray(String.self, forKey: .warnings)
    }
}

public struct PublishedPort: Codable, Sendable, Hashable {
    public let host: Int; public let runtime: Int
    public init(host: Int, runtime: Int) { self.host = host; self.runtime = runtime }
    private enum CodingKeys: String, CodingKey { case host, runtime }
}

public struct RuntimeInfo: Codable, Sendable, Hashable { public let provider: RuntimeProvider; public let runtimeID: String?
    public let publishedPorts: [PublishedPort]; public let containerID: String?; public let workspaceFolder: String?
    public let containerUser: String?; public let configPath: String?
    /// What the CLI assumes for records written before runtimes existed.
    public static let containerDefault: RuntimeInfo = RuntimeInfo(provider: .container)
    public init(provider: RuntimeProvider, runtimeID: String? = nil, publishedPorts: [PublishedPort] = [],
                containerID: String? = nil, workspaceFolder: String? = nil, containerUser: String? = nil,
                configPath: String? = nil) {
        self.provider = provider
        self.runtimeID = runtimeID
        self.publishedPorts = publishedPorts
        self.containerID = containerID
        self.workspaceFolder = workspaceFolder
        self.containerUser = containerUser
        self.configPath = configPath
    }

    private enum CodingKeys: String, CodingKey {
        case provider, runtimeID = "runtime_id", publishedPorts = "published_ports", containerID = "container_id"
        case workspaceFolder = "workspace_folder", containerUser = "container_user", configPath = "config_path"
    }

    public init(from decoder: any Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        provider = c.lenient(RuntimeProvider.self, forKey: .provider) ?? .container
        runtimeID = c.lenient(String.self, forKey: .runtimeID)
        publishedPorts = c.lossyArray(PublishedPort.self, forKey: .publishedPorts)
        containerID = c.lenient(String.self, forKey: .containerID)
        workspaceFolder = c.lenient(String.self, forKey: .workspaceFolder)
        containerUser = c.lenient(String.self, forKey: .containerUser)
        configPath = c.lenient(String.self, forKey: .configPath)
    }
}

public struct DefaultAgentPlan: Codable, Sendable, Hashable { public let status: AgentPlanStatus; public let label: String?
    public let command: String?; public let detail: String?; public let followup: String?
    public init(status: AgentPlanStatus, label: String? = nil, command: String? = nil, detail: String? = nil,
                followup: String? = nil) {
        self.status = status
        self.label = label
        self.command = command
        self.detail = detail
        self.followup = followup
    }

    private enum CodingKeys: String, CodingKey { case status, label, command, detail, followup }

    public init(from decoder: any Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        status = c.lenient(AgentPlanStatus.self, forKey: .status) ?? .unknown("")
        label = c.lenient(String.self, forKey: .label)
        command = c.lenient(String.self, forKey: .command)
        detail = c.lenient(String.self, forKey: .detail)
        followup = c.lenient(String.self, forKey: .followup)
    }
}
