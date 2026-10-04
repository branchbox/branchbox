import Foundation

public struct BackendSettings: Sendable, Hashable {
    public var cliPathOverride: String?
    public var extraEnvironment: [String: String]    // never logged; redacted in diagnostics
    public var verboseLogs: Bool                     // RUST_LOG=debug
    public var agentCommand: String?                 // → BRANCHBOX_DEFAULT_AGENT_CMD
    public var agentName: String?                    // → BRANCHBOX_DEFAULT_AGENT_NAME
    public init(cliPathOverride: String? = nil, extraEnvironment: [String: String] = [:], verboseLogs: Bool = false,
                agentCommand: String? = nil, agentName: String? = nil) {
        self.cliPathOverride = cliPathOverride
        self.extraEnvironment = extraEnvironment
        self.verboseLogs = verboseLogs
        self.agentCommand = agentCommand
        self.agentName = agentName
    }
}

public enum BackendBootstrap: Sendable {
    case ready(any BranchBoxBackend, BackendIdentity)
    case unavailable(BackendError, CLIResolution?)
}

/// Implemented by CLIBackendBootstrapper (BranchBoxCLI) and PreviewBootstrapper (BranchBoxPreview).
public protocol BackendBootstrapping: Sendable {
    func bootstrap(_ settings: BackendSettings) async -> BackendBootstrap
    func environmentSummary() async -> EnvironmentSummary?
    func recaptureEnvironment() async
    func terminateAllProcesses() async               // app quit
}
