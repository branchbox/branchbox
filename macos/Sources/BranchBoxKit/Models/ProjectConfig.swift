import Foundation

/// Typed read model of the effective project config (defaults applied). Decodes the nested
/// `.branchbox/config.json` shape, which is also what `config get --json` prints as `effective`.
/// Missing keys take core's defaults (`core/src/config.rs`).
public struct ProjectConfig: Decodable, Sendable, Hashable {
    public let runtimeProvider: RuntimeProvider; public let sbxRunServices: [String]
    public let branchPrefix: String; public let deleteBranchByDefault: Bool; public let forceDeleteUnmergedByDefault: Bool
    public let promptForceDeleteUnmerged: Bool; public let tunnelEnabled: Bool; public let tunnelDefaultProvider: String?
    public let cloudflared: CloudflaredConfig?; public let editorDefaultAgent: String?; public let autoLaunchAgentTerminal: Bool

    /// The config a project has with no `.branchbox/config.json`.
    public static let defaults = ProjectConfig()

    public init(runtimeProvider: RuntimeProvider = .container, sbxRunServices: [String] = [], branchPrefix: String = "feature",
                deleteBranchByDefault: Bool = true, forceDeleteUnmergedByDefault: Bool = false,
                promptForceDeleteUnmerged: Bool = true, tunnelEnabled: Bool = true,
                tunnelDefaultProvider: String? = "cloudflared", cloudflared: CloudflaredConfig? = nil,
                editorDefaultAgent: String? = nil, autoLaunchAgentTerminal: Bool = false) {
        self.runtimeProvider = runtimeProvider
        self.sbxRunServices = sbxRunServices
        self.branchPrefix = branchPrefix
        self.deleteBranchByDefault = deleteBranchByDefault
        self.forceDeleteUnmergedByDefault = forceDeleteUnmergedByDefault
        self.promptForceDeleteUnmerged = promptForceDeleteUnmerged
        self.tunnelEnabled = tunnelEnabled
        self.tunnelDefaultProvider = tunnelDefaultProvider
        self.cloudflared = cloudflared
        self.editorDefaultAgent = editorDefaultAgent
        self.autoLaunchAgentTerminal = autoLaunchAgentTerminal
    }

    private enum RootKeys: String, CodingKey { case runtime, feature, tunnel, editor }
    private enum RuntimeKeys: String, CodingKey { case provider, sbx }
    private enum SbxKeys: String, CodingKey { case runServices = "run_services" }
    private enum FeatureKeys: String, CodingKey { case branchPrefix = "branch_prefix", teardown }
    private enum TeardownKeys: String, CodingKey {
        case deleteBranchByDefault = "delete_branch_by_default"
        case forceDeleteUnmergedByDefault = "force_delete_unmerged_by_default"
        case promptForceDeleteUnmerged = "prompt_force_delete_unmerged"
    }
    private enum TunnelKeys: String, CodingKey { case enabled, defaultProvider = "default_provider", providers }
    private enum ProviderKeys: String, CodingKey { case cloudflared }
    private enum EditorKeys: String, CodingKey { case defaultAgent = "default_agent", autoLaunchAgentTerminal = "auto_launch_agent_terminal" }

    public init(from decoder: any Decoder) throws {
        let root = try decoder.container(keyedBy: RootKeys.self)
        let defaults = ProjectConfig.defaults

        let runtime = try? root.nestedContainer(keyedBy: RuntimeKeys.self, forKey: .runtime)
        runtimeProvider = runtime?.lenient(RuntimeProvider.self, forKey: .provider) ?? defaults.runtimeProvider
        let sbx = try? runtime?.nestedContainer(keyedBy: SbxKeys.self, forKey: .sbx)
        sbxRunServices = sbx?.lossyArray(String.self, forKey: .runServices) ?? defaults.sbxRunServices

        let feature = try? root.nestedContainer(keyedBy: FeatureKeys.self, forKey: .feature)
        branchPrefix = feature?.lenient(String.self, forKey: .branchPrefix) ?? defaults.branchPrefix
        let teardown = try? feature?.nestedContainer(keyedBy: TeardownKeys.self, forKey: .teardown)
        deleteBranchByDefault = teardown?.lenient(Bool.self, forKey: .deleteBranchByDefault)
            ?? defaults.deleteBranchByDefault
        forceDeleteUnmergedByDefault = teardown?.lenient(Bool.self, forKey: .forceDeleteUnmergedByDefault)
            ?? defaults.forceDeleteUnmergedByDefault
        promptForceDeleteUnmerged = teardown?.lenient(Bool.self, forKey: .promptForceDeleteUnmerged)
            ?? defaults.promptForceDeleteUnmerged

        let tunnel = try? root.nestedContainer(keyedBy: TunnelKeys.self, forKey: .tunnel)
        tunnelEnabled = tunnel?.lenient(Bool.self, forKey: .enabled) ?? defaults.tunnelEnabled
        // Core reads null and an absent key alike as None and then applies `TunnelSettings::ensure_defaults()`
        // before every tunnel use, so the effective provider is "cloudflared" in both cases.
        tunnelDefaultProvider = tunnel?.lenient(String.self, forKey: .defaultProvider) ?? defaults.tunnelDefaultProvider
        let providers = try? tunnel?.nestedContainer(keyedBy: ProviderKeys.self, forKey: .providers)
        cloudflared = providers?.lenient(CloudflaredConfig.self, forKey: .cloudflared)

        let editor = try? root.nestedContainer(keyedBy: EditorKeys.self, forKey: .editor)
        editorDefaultAgent = editor?.lenient(String.self, forKey: .defaultAgent)
        autoLaunchAgentTerminal = editor?.lenient(Bool.self, forKey: .autoLaunchAgentTerminal)
            ?? defaults.autoLaunchAgentTerminal
    }
}

public struct CloudflaredConfig: Codable, Sendable, Hashable { public let accountID: String?; public let tunnelNamePrefix: String?
    public let dnsZone: String?; public let serviceURL: String?; public let manualInstructions: Bool?; public let apiTokenPath: String?
    public init(accountID: String? = nil, tunnelNamePrefix: String? = nil, dnsZone: String? = nil, serviceURL: String? = nil,
                manualInstructions: Bool? = nil, apiTokenPath: String? = nil) {
        self.accountID = accountID
        self.tunnelNamePrefix = tunnelNamePrefix
        self.dnsZone = dnsZone
        self.serviceURL = serviceURL
        self.manualInstructions = manualInstructions
        self.apiTokenPath = apiTokenPath
    }

    private enum CodingKeys: String, CodingKey {
        case accountID = "account_id", tunnelNamePrefix = "tunnel_name_prefix", dnsZone = "dns_zone"
        case serviceURL = "service_url", manualInstructions = "manual_instructions", apiTokenPath = "api_token_path"
    }

    public init(from decoder: any Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        accountID = c.lenient(String.self, forKey: .accountID)
        tunnelNamePrefix = c.lenient(String.self, forKey: .tunnelNamePrefix)
        dnsZone = c.lenient(String.self, forKey: .dnsZone)
        serviceURL = c.lenient(String.self, forKey: .serviceURL)
        manualInstructions = c.lenient(Bool.self, forKey: .manualInstructions)
        apiTokenPath = c.lenient(String.self, forKey: .apiTokenPath)
    }
}

/// One entry of `config get --json` `keys[]` (§5.10).
public struct ConfigKeyDescriptor: Decodable, Sendable, Hashable { public let key: String; public let type: String  // bool|string|enum|string_list
    public let allowed: [String]; public let defaultValue: JSONValue?; public let value: JSONValue?; public let source: String; public let description: String
    public init(key: String, type: String, allowed: [String] = [], defaultValue: JSONValue? = nil, value: JSONValue? = nil,
                source: String = "default", description: String = "") {
        self.key = key
        self.type = type
        self.allowed = allowed
        self.defaultValue = defaultValue
        self.value = value
        self.source = source
        self.description = description
    }

    private enum CodingKeys: String, CodingKey { case key, type, allowed, defaultValue = "default", value, source, description }

    public init(from decoder: any Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        key = try c.decode(String.self, forKey: .key)
        type = c.lenient(String.self, forKey: .type) ?? "string"
        allowed = c.lossyArray(String.self, forKey: .allowed)
        defaultValue = c.lenient(JSONValue.self, forKey: .defaultValue)
        value = c.lenient(JSONValue.self, forKey: .value)
        source = c.lenient(String.self, forKey: .source) ?? ""
        description = c.lenient(String.self, forKey: .description) ?? ""
    }
}

public struct ProjectConfigDocument: Sendable, Hashable {
    public let path: String; public let exists: Bool; public let effective: ProjectConfig; public let keys: [ConfigKeyDescriptor]
    public let editable: Bool                        // false on legacy CLIs (read-only from config.json)
    public init(path: String, exists: Bool, effective: ProjectConfig, keys: [ConfigKeyDescriptor] = [], editable: Bool) {
        self.path = path
        self.exists = exists
        self.effective = effective
        self.keys = keys
        self.editable = editable
    }
}

/// `config apply --json` (§5.10).
public struct ConfigApplyResult: Decodable, Sendable, Hashable {
    public struct Change: Decodable, Sendable, Hashable {
        public let key: String; public let old: JSONValue?; public let new: JSONValue?
        public init(key: String, old: JSONValue?, new: JSONValue?) { self.key = key; self.old = old; self.new = new }

        private enum CodingKeys: String, CodingKey { case key, old, new }

        public init(from decoder: any Decoder) throws {
            let c = try decoder.container(keyedBy: CodingKeys.self)
            key = try c.decode(String.self, forKey: .key)
            old = c.lenient(JSONValue.self, forKey: .old)
            new = c.lenient(JSONValue.self, forKey: .new)
        }
    }
    public let changed: [Change]; public let effective: ProjectConfig
    public init(changed: [Change], effective: ProjectConfig) { self.changed = changed; self.effective = effective }

    private enum CodingKeys: String, CodingKey { case changed, effective }

    public init(from decoder: any Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        changed = c.lossyArray(Change.self, forKey: .changed)
        effective = try c.decode(ProjectConfig.self, forKey: .effective)
    }
}
