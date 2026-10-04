import BranchBoxKit
import Foundation

/// Bootstraps one `PreviewBackend`: ready with the scenario's identity, or unavailable with its error. Every
/// bootstrap returns the same backend, so its listings, scripts and call log survive a rebootstrap.
public struct PreviewBootstrapper: BackendBootstrapping {
    public let backend: PreviewBackend

    public init(scenario: PreviewScenario = .contract) {
        backend = PreviewBackend(scenario: scenario)
    }

    public init(backend: PreviewBackend) {
        self.backend = backend
    }

    public func bootstrap(_ settings: BackendSettings) async -> BackendBootstrap {
        switch backend.scenario.identity {
        case .success(let identity): .ready(backend, identity)
        case .failure(let error): .unavailable(error, nil)
        }
    }

    /// A finished login-shell capture with a typical Homebrew PATH.
    public func environmentSummary() async -> EnvironmentSummary? {
        EnvironmentSummary(source: .interactiveLogin, shell: "/bin/zsh", captureDuration: .milliseconds(180),
                           pathEntries: ["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin", "/bin", "/usr/sbin", "/sbin"],
                           capturedAt: .now, isProvisional: false)
    }

    public func recaptureEnvironment() async {}

    /// Ends every waiting preview call with `.cancelled`, as quitting the app stops the CLI's processes.
    public func terminateAllProcesses() async {
        await backend.terminateAll()
    }
}
