import Foundation

/// A project is identified by its MAIN worktree root. Paths are standardized; symlinks are NOT resolved.
public struct ProjectRef: Hashable, Sendable, Codable {
    public let root: URL
    public init(root: URL) { self.root = root.standardizedFileURL }
    public var path: String { root.path }
    public var displayName: String { root.lastPathComponent }

    /// Identity is the standardized path. Comparing the URLs would make `file:///r/main/` (a folder
    /// picked in an open panel, or built while the folder existed) differ from `file:///r/main`.
    public static func == (lhs: ProjectRef, rhs: ProjectRef) -> Bool { lhs.path == rhs.path }
    public func hash(into hasher: inout Hasher) { hasher.combine(path) }

    private enum CodingKeys: String, CodingKey { case root }

    /// Decoding goes through `init(root:)` so a persisted ref compares equal to a freshly built one.
    public init(from decoder: any Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        self.init(root: try container.decode(URL.self, forKey: .root))
    }
}

public struct FeatureRef: Hashable, Sendable, Codable {
    public let project: ProjectRef
    public let name: String                          // work_feature slug
    public init(project: ProjectRef, name: String) { self.project = project; self.name = name }
}

public struct SemVer: Hashable, Sendable, Comparable, Codable, CustomStringConvertible {
    public let major: Int, minor: Int, patch: Int
    public let prerelease: String?                   // "dev" for 0.14.0-dev+abc (build metadata dropped)

    public init(_ major: Int, _ minor: Int, _ patch: Int, prerelease: String? = nil) {
        self.major = major
        self.minor = minor
        self.patch = patch
        self.prerelease = prerelease.flatMap { $0.isEmpty ? nil : $0 }
    }

    /// Accepts "0.13.4", "branchbox 0.13.4", "0.14.0-dev+abc123"; nil otherwise.
    public init?(parsing text: String) {
        // `branchbox --version` prints "branchbox 0.13.4"; take the first token that starts with a
        // digit once an optional leading "v" is dropped.
        let tokens = text.split(whereSeparator: \.isWhitespace).map { $0.hasPrefix("v") ? $0.dropFirst() : $0 }
        guard let token = tokens.first(where: { $0.first?.isASCIIDigit == true }) else { return nil }
        // Build metadata never takes part in precedence.
        let withoutBuild = token.split(separator: "+", maxSplits: 1, omittingEmptySubsequences: false)[0]
        let parts = withoutBuild.split(separator: "-", maxSplits: 1, omittingEmptySubsequences: false)
        let core = parts[0].split(separator: ".", omittingEmptySubsequences: false)
        guard core.count == 3 else { return nil }
        var numbers: [Int] = []
        for component in core {
            guard !component.isEmpty, component.allSatisfy(\.isASCIIDigit), let value = Int(component) else {
                return nil
            }
            numbers.append(value)
        }
        var prerelease: String?
        if parts.count == 2 {
            guard !parts[1].isEmpty else { return nil }
            prerelease = String(parts[1])
        }
        self.init(numbers[0], numbers[1], numbers[2], prerelease: prerelease)
    }

    /// A prerelease sorts before the release of the same triple.
    public static func < (lhs: SemVer, rhs: SemVer) -> Bool {
        if lhs.major != rhs.major { return lhs.major < rhs.major }
        if lhs.minor != rhs.minor { return lhs.minor < rhs.minor }
        if lhs.patch != rhs.patch { return lhs.patch < rhs.patch }
        switch (lhs.prerelease, rhs.prerelease) {
        case (nil, nil), (nil, _?): return false
        case (_?, nil): return true
        case let (l?, r?): return prereleasePrecedes(l, r)
        }
    }

    public var description: String {
        let core = "\(major).\(minor).\(patch)"
        return prerelease.map { "\(core)-\($0)" } ?? core
    }

    /// SemVer 2.0 §11: dot-separated identifiers; numeric ones compare numerically and sort
    /// before alphanumeric ones; a shorter list that is a prefix of a longer one sorts first.
    private static func prereleasePrecedes(_ lhs: String, _ rhs: String) -> Bool {
        let left = lhs.split(separator: ".", omittingEmptySubsequences: false)
        let right = rhs.split(separator: ".", omittingEmptySubsequences: false)
        for (l, r) in zip(left, right) where l != r {
            switch (Int(l), Int(r)) {
            case let (ln?, rn?): return ln < rn
            case (_?, nil): return true
            case (nil, _?): return false
            case (nil, nil): return l < r
            }
        }
        return left.count < right.count
    }
}

private extension Character {
    var isASCIIDigit: Bool { isASCII && isNumber }
}

/// Capability strings exactly as printed by `branchbox version --json` (§5.3).
public struct Capability: RawRepresentable, Hashable, Sendable, Codable {
    public let rawValue: String
    public init(rawValue: String) { self.rawValue = rawValue }
    public static let jsonErrorEnvelope        = Capability(rawValue: "json-error-envelope")
    public static let registryLock             = Capability(rawValue: "registry-lock")
    public static let writeAheadStart          = Capability(rawValue: "write-ahead-start")
    public static let teardownPlan             = Capability(rawValue: "teardown-plan")
    public static let teardownDiscardChanges   = Capability(rawValue: "teardown-discard-changes")
    public static let teardownUnmergedPreflight = Capability(rawValue: "teardown-unmerged-preflight")
    public static let pruneJSON                = Capability(rawValue: "prune-json")
    public static let detectJSON               = Capability(rawValue: "detect-json")
    public static let devcontainerSyncJSON     = Capability(rawValue: "devcontainer-sync-json")
    public static let config                   = Capability(rawValue: "config")
    public static let tunnelCredentials        = Capability(rawValue: "tunnel-credentials")
    public static let doctor                   = Capability(rawValue: "doctor")
    public static let initJSON                 = Capability(rawValue: "init-json")

    /// Encoded as the bare string, matching the CLI's `capabilities` array.
    public init(from decoder: any Decoder) throws {
        self.init(rawValue: try decoder.singleValueContainer().decode(String.self))
    }

    public func encode(to encoder: any Encoder) throws {
        var container = encoder.singleValueContainer()
        try container.encode(rawValue)
    }
}

public enum CLISource: String, Sendable, Hashable, Codable {
    case environmentOverride      // BRANCHBOX_CLI_PATH
    case settingsOverride         // Settings › Tools › Locate…
    case loginShellPath           // found on the captured login-shell PATH
    case wellKnownPath            // /opt/homebrew/bin, /usr/local/bin, ~/.cargo/bin, ~/.local/bin
    case embedded                 // Contents/Helpers/branchbox (only if packaged with --embed-cli)
}

public struct RejectedCandidate: Sendable, Hashable {
    public let path: String; public let reason: String
    public init(path: String, reason: String) { self.path = path; self.reason = reason }
}

public struct CLIResolution: Sendable, Hashable {
    public let path: String                          // unresolved, e.g. /opt/homebrew/bin/branchbox
    public let source: CLISource
    public let rejected: [RejectedCandidate]         // shown in Diagnostics
    public init(path: String, source: CLISource, rejected: [RejectedCandidate] = []) {
        self.path = path
        self.source = source
        self.rejected = rejected
    }
}

public struct BackendIdentity: Sendable, Hashable {
    public enum Kind: Sendable, Hashable { case cli(CLIResolution), agent(endpoint: String), preview }
    public static let minimumCLI = SemVer(0, 13, 4)
    public let kind: Kind
    public let version: SemVer
    public let contractVersion: Int?                 // nil = legacy CLI without `version --json`
    public let capabilities: Set<Capability>         // empty for legacy 0.13.x
    public init(kind: Kind, version: SemVer, contractVersion: Int?, capabilities: Set<Capability>) {
        self.kind = kind
        self.version = version
        self.contractVersion = contractVersion
        self.capabilities = capabilities
    }
    public func supports(_ capability: Capability) -> Bool { capabilities.contains(capability) }
    public var isLegacy: Bool { contractVersion == nil }
}

public struct EnvironmentSummary: Sendable, Hashable {
    public enum Source: String, Sendable, Hashable { case interactiveLogin, login, processEnvironment, cachedPath }
    public let source: Source
    public let shell: String?
    public let captureDuration: Duration?
    public let pathEntries: [String]                 // child PATH (non-secret)
    public let capturedAt: Date?
    public let isProvisional: Bool                   // true until the login-shell capture finished
    public init(source: Source, shell: String?, captureDuration: Duration?, pathEntries: [String],
                capturedAt: Date?, isProvisional: Bool) {
        self.source = source
        self.shell = shell
        self.captureDuration = captureDuration
        self.pathEntries = pathEntries
        self.capturedAt = capturedAt
        self.isProvisional = isProvisional
    }
}
