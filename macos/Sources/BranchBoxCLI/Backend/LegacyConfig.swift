import BranchBoxKit
import Foundation

/// The project config on CLIs without `config get` (DESIGN §6.2 readConfig, legacy): `<root>/.branchbox/config.json`
/// read strictly (it must be valid JSON) with core's defaults for missing keys, shown read-only with the built-in
/// key descriptors of the §5.10 registry.
enum LegacyConfig {
    static func path(for root: String) -> String {
        Paths.join(root, ".branchbox/config.json")
    }

    /// Throws `.decodeFailed` naming the file when it exists but is not valid JSON.
    static func read(root: String) throws -> ProjectConfigDocument {
        let path = path(for: root)
        guard FileManager.default.fileExists(atPath: path) else {
            return ProjectConfigDocument(path: path, exists: false, effective: .defaults,
                                         keys: descriptors(file: nil, effective: .defaults), editable: false)
        }
        do {
            let data = try Data(contentsOf: URL(fileURLWithPath: path))
            let effective = try CLIJSON.decoder().decode(ProjectConfig.self, from: data)
            let file = try? CLIJSON.decoder().decode(JSONValue.self, from: data)
            return ProjectConfigDocument(path: path, exists: true, effective: effective,
                                         keys: descriptors(file: file, effective: effective), editable: false)
        } catch {
            let detail = CLIOutput.describe(error)
            throw BackendError.decodeFailed(what: "project config", detail: detail,
                                            diagnostics: Diagnostics(summary: "Could not read \(path): \(detail)"))
        }
    }

    /// The effective config, or the defaults when the file is missing or unreadable; for callers that only need a
    /// value such as the branch prefix.
    static func effective(root: String) -> ProjectConfig {
        (try? read(root: root).effective) ?? .defaults
    }

    private struct Key: Sendable {
        let key: String
        let type: String
        var allowed: [String] = []
        let defaultValue: JSONValue?
        let description: String
    }

    private static let registry: [Key] = {
        let defaults = ProjectConfig.defaults
        return [
            Key(key: "runtime.provider", type: "enum", allowed: ["container", "sbx", "local-vm", "in-guest"],
                defaultValue: .string(defaults.runtimeProvider.raw), description: "Runtime new features start in"),
            Key(key: "runtime.sbx.run_services", type: "string_list", defaultValue: .array([]),
                description: "Compose services started inside Docker Sandboxes"),
            Key(key: "feature.branch_prefix", type: "string", defaultValue: .string(defaults.branchPrefix),
                description: "Prefix of feature branch names"),
            Key(key: "feature.teardown.delete_branch_by_default", type: "bool",
                defaultValue: .bool(defaults.deleteBranchByDefault), description: "Delete the branch on teardown"),
            Key(key: "feature.teardown.force_delete_unmerged_by_default", type: "bool",
                defaultValue: .bool(defaults.forceDeleteUnmergedByDefault),
                description: "Force-delete unmerged branches on teardown"),
            Key(key: "feature.teardown.prompt_force_delete_unmerged", type: "bool",
                defaultValue: .bool(defaults.promptForceDeleteUnmerged),
                description: "Ask before force-deleting an unmerged branch"),
            Key(key: "tunnel.enabled", type: "bool", defaultValue: .bool(defaults.tunnelEnabled),
                description: "Provision tunnels for new features"),
            Key(key: "tunnel.default_provider", type: "string",
                defaultValue: defaults.tunnelDefaultProvider.map(JSONValue.string), description: "Tunnel provider"),
            Key(key: "tunnel.providers.cloudflared.account_id", type: "string", defaultValue: nil,
                description: "Cloudflare account ID"),
            Key(key: "tunnel.providers.cloudflared.tunnel_name_prefix", type: "string", defaultValue: nil,
                description: "Prefix of Cloudflare tunnel names"),
            Key(key: "tunnel.providers.cloudflared.dns_zone", type: "string", defaultValue: nil,
                description: "DNS zone for feature hostnames"),
            Key(key: "tunnel.providers.cloudflared.service_url", type: "string", defaultValue: nil,
                description: "Service the tunnel points at"),
            Key(key: "tunnel.providers.cloudflared.manual_instructions", type: "bool", defaultValue: nil,
                description: "Print manual tunnel instructions instead of provisioning"),
            Key(key: "tunnel.providers.cloudflared.api_token_path", type: "string", defaultValue: nil,
                description: "File holding the Cloudflare API token"),
            Key(key: "editor.default_agent", type: "string", defaultValue: nil, description: "Default coding agent"),
            Key(key: "editor.auto_launch_agent_terminal", type: "bool",
                defaultValue: .bool(defaults.autoLaunchAgentTerminal),
                description: "Open the agent's terminal after start"),
            Key(key: "editor.preferred_sidebar_view", type: "string", defaultValue: nil,
                description: "Editor sidebar view to open"),
            Key(key: "editor.hide_secondary_sidebar", type: "bool", defaultValue: nil,
                description: "Hide the editor's secondary sidebar"),
        ]
    }()

    /// Each key's value is read from the raw file by its dotted path; `source` says whether the file set it.
    static func descriptors(file: JSONValue?, effective: ProjectConfig) -> [ConfigKeyDescriptor] {
        registry.map { key in
            let value = file.flatMap { lookup(key.key, in: $0) }
            return ConfigKeyDescriptor(key: key.key, type: key.type, allowed: key.allowed, defaultValue: key.defaultValue,
                                       value: value ?? key.defaultValue, source: value == nil ? "default" : "file",
                                       description: key.description)
        }
    }

    private static func lookup(_ dotted: String, in value: JSONValue) -> JSONValue? {
        var current: JSONValue? = value
        for component in dotted.split(separator: ".") {
            current = current?.objectValue?[String(component)]
        }
        if case .null? = current { return nil }
        return current
    }

    /// An RFC 7386 merge patch for `config apply`: dotted keys become nested objects and an unset is `null`.
    static func mergePatch(_ patch: ConfigPatch) -> JSONValue {
        var root: [String: JSONValue] = [:]
        for change in patch.changes {
            insert(change.value ?? .null, at: change.key.split(separator: ".").map(String.init)[...], into: &root)
        }
        return .object(root)
    }

    private static func insert(_ value: JSONValue, at path: ArraySlice<String>, into object: inout [String: JSONValue]) {
        guard let head = path.first else { return }
        let rest = path.dropFirst()
        if rest.isEmpty {
            object[head] = value
            return
        }
        var child = object[head]?.objectValue ?? [:]
        insert(value, at: rest, into: &child)
        object[head] = .object(child)
    }
}
