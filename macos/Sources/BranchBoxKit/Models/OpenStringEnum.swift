import Foundation

/// A string enum that never fails to decode (R-4): unknown raw values land in `.unknown(String)`
/// and encode back unchanged, so a newer CLI's vocabulary survives a round trip.
public protocol OpenStringEnum: Codable, Hashable, Sendable { init(raw: String); var raw: String { get } }

extension OpenStringEnum {
    public init(from decoder: any Decoder) throws {
        self.init(raw: try decoder.singleValueContainer().decode(String.self))
    }

    public func encode(to encoder: any Encoder) throws {
        var container = encoder.singleValueContainer()
        try container.encode(raw)
    }
}

public enum FeatureStatus: OpenStringEnum {   // "failed_retained"
    case active, degraded, failedRetained, orphaned, removed, unknown(String)
    public init(raw: String) {
        switch raw {
        case "active": self = .active
        case "degraded": self = .degraded
        case "failed_retained": self = .failedRetained
        case "orphaned": self = .orphaned
        case "removed": self = .removed
        default: self = .unknown(raw)
        }
    }
    public var raw: String {
        switch self {
        case .active: "active"
        case .degraded: "degraded"
        case .failedRetained: "failed_retained"
        case .orphaned: "orphaned"
        case .removed: "removed"
        case .unknown(let raw): raw
        }
    }
}

public enum ModuleStatus: OpenStringEnum {   // "ok" decodes as .success
    case success, skipped, failed, unknown(String)
    public init(raw: String) {
        switch raw {
        case "success", "ok": self = .success
        case "skipped": self = .skipped
        case "failed": self = .failed
        default: self = .unknown(raw)
        }
    }
    public var raw: String {
        switch self {
        case .success: "success"
        case .skipped: "skipped"
        case .failed: "failed"
        case .unknown(let raw): raw
        }
    }
}

public enum RuntimeProvider: OpenStringEnum {   // "local-vm","in-guest"
    case container, sbx, localVM, inGuest, unknown(String)
    public init(raw: String) {
        switch raw {
        case "container": self = .container
        case "sbx": self = .sbx
        case "local-vm": self = .localVM
        case "in-guest": self = .inGuest
        default: self = .unknown(raw)
        }
    }
    public var raw: String {
        switch self {
        case .container: "container"
        case .sbx: "sbx"
        case .localVM: "local-vm"
        case .inGuest: "in-guest"
        case .unknown(let raw): raw
        }
    }
}

public enum TunnelStatus: OpenStringEnum {
    case pending, active, manual, disabled, unknown(String)
    public init(raw: String) {
        switch raw {
        case "pending": self = .pending
        case "active": self = .active
        case "manual": self = .manual
        case "disabled": self = .disabled
        default: self = .unknown(raw)
        }
    }
    public var raw: String {
        switch self {
        case .pending: "pending"
        case .active: "active"
        case .manual: "manual"
        case .disabled: "disabled"
        case .unknown(let raw): raw
        }
    }
}

public enum AgentPlanStatus: OpenStringEnum {
    case ready, waiting, blocked, disabled, unknown(String)
    public init(raw: String) {
        switch raw {
        case "ready": self = .ready
        case "waiting": self = .waiting
        case "blocked": self = .blocked
        case "disabled": self = .disabled
        default: self = .unknown(raw)
        }
    }
    public var raw: String {
        switch self {
        case .ready: "ready"
        case .waiting: "waiting"
        case .blocked: "blocked"
        case .disabled: "disabled"
        case .unknown(let raw): raw
        }
    }
}

public enum SetupState: OpenStringEnum {   // "in_progress"
    case inProgress, interrupted, unknown(String)
    public init(raw: String) {
        switch raw {
        case "in_progress": self = .inProgress
        case "interrupted": self = .interrupted
        default: self = .unknown(raw)
        }
    }
    public var raw: String {
        switch self {
        case .inProgress: "in_progress"
        case .interrupted: "interrupted"
        case .unknown(let raw): raw
        }
    }
}
