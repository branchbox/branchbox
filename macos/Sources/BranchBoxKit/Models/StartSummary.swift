import Foundation

/// `feature start --json`.
public struct StartSummary: Decodable, Sendable, Hashable { public let workFeature: String; public let branchName: String
    public let worktreePath: String?; public let mode: String?; public let promptSeed: String?; public let featureURL: String?
    public let composeProjectName: String?; public let runtime: RuntimeInfo?; public let envPath: String?; public let color: String?
    public let moduleOutcomes: [ModuleOutcome]; public let skippedModules: [SkippedModule]; public let warnings: [String]
    public let adapter: AdapterInfo?; public let tunnel: TunnelState?; public let promptBridgeEnabled: Bool?
    public let generatedAt: Date?; public let defaultAgent: DefaultAgentPlan?
    public var preambleWarning: String?              // set by CLIBackend from CLIJSON preamble

    public var urls: FeatureURLs {
        FeatureURLs(featureURL: featureURL, tunnel: tunnel, runtime: runtime, adapter: adapter)
    }

    public init(workFeature: String, branchName: String = "", worktreePath: String? = nil, mode: String? = nil,
                promptSeed: String? = nil, featureURL: String? = nil, composeProjectName: String? = nil,
                runtime: RuntimeInfo? = nil, envPath: String? = nil, color: String? = nil,
                moduleOutcomes: [ModuleOutcome] = [], skippedModules: [SkippedModule] = [], warnings: [String] = [],
                adapter: AdapterInfo? = nil, tunnel: TunnelState? = nil, promptBridgeEnabled: Bool? = nil,
                generatedAt: Date? = nil, defaultAgent: DefaultAgentPlan? = nil, preambleWarning: String? = nil) {
        self.workFeature = workFeature
        self.branchName = branchName
        self.worktreePath = worktreePath
        self.mode = mode
        self.promptSeed = promptSeed
        self.featureURL = featureURL
        self.composeProjectName = composeProjectName
        self.runtime = runtime
        self.envPath = envPath
        self.color = color
        self.moduleOutcomes = moduleOutcomes
        self.skippedModules = skippedModules
        self.warnings = warnings
        self.adapter = adapter
        self.tunnel = tunnel
        self.promptBridgeEnabled = promptBridgeEnabled
        self.generatedAt = generatedAt
        self.defaultAgent = defaultAgent
        self.preambleWarning = preambleWarning
    }

    private enum CodingKeys: String, CodingKey {
        case workFeature = "work_feature", branchName = "branch_name", worktreePath = "worktree_path", mode
        case promptSeed = "prompt_seed", featureURL = "feature_url", composeProjectName = "compose_project_name"
        case runtime, envPath = "env_path", color, moduleOutcomes = "module_outcomes"
        case skippedModules = "skipped_modules", warnings, adapter, tunnel
        case promptBridgeEnabled = "prompt_bridge_enabled", generatedAt = "generated_at", defaultAgent = "default_agent"
    }

    public init(from decoder: any Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        workFeature = try c.decode(String.self, forKey: .workFeature)
        branchName = c.lenient(String.self, forKey: .branchName) ?? ""
        worktreePath = c.lenient(String.self, forKey: .worktreePath)
        mode = c.lenient(String.self, forKey: .mode)
        promptSeed = c.lenient(String.self, forKey: .promptSeed)
        featureURL = c.lenient(String.self, forKey: .featureURL)
        composeProjectName = c.lenient(String.self, forKey: .composeProjectName)
        runtime = c.lenient(RuntimeInfo.self, forKey: .runtime)
        envPath = c.lenient(String.self, forKey: .envPath)
        color = c.lenient(String.self, forKey: .color)
        moduleOutcomes = c.lossyArray(ModuleOutcome.self, forKey: .moduleOutcomes)
        skippedModules = c.lossyArray(SkippedModule.self, forKey: .skippedModules)
        warnings = c.lossyArray(String.self, forKey: .warnings)
        adapter = c.lenient(AdapterInfo.self, forKey: .adapter)
        tunnel = c.lenient(TunnelState.self, forKey: .tunnel)
        promptBridgeEnabled = c.lenient(Bool.self, forKey: .promptBridgeEnabled)
        generatedAt = c.rfc3339IfPresent(forKey: .generatedAt)
        defaultAgent = c.lenient(DefaultAgentPlan.self, forKey: .defaultAgent)
        preambleWarning = nil
    }
}

public struct SkippedModule: Codable, Sendable, Hashable {
    public let module: String; public let reason: String?
    public init(module: String, reason: String? = nil) { self.module = module; self.reason = reason }

    private enum CodingKeys: String, CodingKey { case module, reason }

    public init(from decoder: any Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        module = try c.decode(String.self, forKey: .module)
        reason = c.lenient(String.self, forKey: .reason)
    }
}
