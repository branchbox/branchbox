import Foundation

/// Result of `init`: `init --json` (§5.13) on contract CLIs, the parsed text log on legacy ones.
public struct InitReport: Sendable, Hashable { public let workspacePath: String?; public let reorganized: Bool; public let stack: String?
    public let adapter: String?; public let modules: [String]; public let warnings: [String]; public let nextSteps: [String]
    public let onePasswordStatus: String?; public let log: [String]
    public init(workspacePath: String?, reorganized: Bool = false, stack: String? = nil, adapter: String? = nil,
                modules: [String] = [], warnings: [String] = [], nextSteps: [String] = [], onePasswordStatus: String? = nil,
                log: [String] = []) {
        self.workspacePath = workspacePath
        self.reorganized = reorganized
        self.stack = stack
        self.adapter = adapter
        self.modules = modules
        self.warnings = warnings
        self.nextSteps = nextSteps
        self.onePasswordStatus = onePasswordStatus
        self.log = log
    }
}
