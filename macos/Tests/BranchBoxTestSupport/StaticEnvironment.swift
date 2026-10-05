import BranchBoxKit
import Foundation

/// An `EnvironmentProviding` that hands out one fixed environment, for backend tests that must not run the user's
/// login shell. It records the purposes asked for, so a test can check that mutations wait for the capture.
public final class StaticEnvironment: EnvironmentProviding, @unchecked Sendable {
    public let variables: [String: String]
    private let lock = NSLock()
    private var requested: [EnvironmentPurpose] = []

    /// `PATH` defaults to launchd's (`/usr/bin:/bin:/usr/sbin:/sbin`), what a Finder-launched app gets.
    public init(_ variables: [String: String] = [:]) {
        var variables = variables
        if variables["PATH"] == nil { variables["PATH"] = "/usr/bin:/bin:/usr/sbin:/sbin" }
        if variables["HOME"] == nil { variables["HOME"] = NSHomeDirectory() }
        self.variables = variables
    }

    /// Every purpose asked for, in order.
    public var purposes: [EnvironmentPurpose] { lock.withLock { requested } }

    public func childEnvironment(for purpose: EnvironmentPurpose, settings: BackendSettings) async -> [String: String] {
        lock.withLock { requested.append(purpose) }
        return variables.merging(settings.extraEnvironment) { _, extra in extra }
    }

    public func summary() async -> EnvironmentSummary {
        EnvironmentSummary(source: .processEnvironment, shell: nil, captureDuration: nil,
                           pathEntries: (variables["PATH"] ?? "").split(separator: ":").map(String.init),
                           capturedAt: nil, isProvisional: false)
    }

    public func recapture() async {}
}
