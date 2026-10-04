import Foundation

/// Host and project readiness: `doctor --json` (§5.12), app-side probes, or both merged.
public struct DoctorCheck: Sendable, Hashable { public enum Status: String, Sendable, Hashable { case ok, warn, error, skipped }
    public let id: String; public let title: String; public let required: Bool; public let status: Status; public let path: String?
    public let version: String?; public let detail: String?; public let remediation: String?
    public init(id: String, title: String, required: Bool, status: Status, path: String? = nil, version: String? = nil,
                detail: String? = nil, remediation: String? = nil) {
        self.id = id
        self.title = title
        self.required = required
        self.status = status
        self.path = path
        self.version = version
        self.detail = detail
        self.remediation = remediation
    }
}
public struct DoctorReport: Sendable, Hashable { public enum Source: String, Sendable, Hashable { case cli, app, merged }
    public let source: Source; public let checks: [DoctorCheck]; public let generatedAt: Date
    public init(source: Source, checks: [DoctorCheck], generatedAt: Date = .now) {
        self.source = source
        self.checks = checks
        self.generatedAt = generatedAt
    }
}
