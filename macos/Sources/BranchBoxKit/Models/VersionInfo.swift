import Foundation

/// `branchbox version --json` (§5.3). A pre-0.14 CLI has no `version` subcommand (clap exits 2); the
/// locator then parses `branchbox --version` with `SemVer(parsing:)` and assumes no capabilities.
public struct VersionInfo: Decodable, Sendable, Hashable {
    public let version: String; public let contractVersion: Int; public let capabilities: [String]
    public init(version: String, contractVersion: Int, capabilities: [String]) {
        self.version = version
        self.contractVersion = contractVersion
        self.capabilities = capabilities
    }

    /// `capabilities` as typed values; strings this app does not know are kept as they are.
    public var capabilitySet: Set<Capability> { Set(capabilities.map(Capability.init(rawValue:))) }

    private enum CodingKeys: String, CodingKey { case version, contractVersion = "contract_version", capabilities }

    public init(from decoder: any Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        version = try c.decode(String.self, forKey: .version)
        contractVersion = try c.decode(Int.self, forKey: .contractVersion)
        capabilities = c.lossyArray(String.self, forKey: .capabilities)
    }
}
