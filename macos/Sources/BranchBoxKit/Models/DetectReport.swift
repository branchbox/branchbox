import Foundation

/// What `detect` found for a folder: `detect --json` (§5.7) on contract CLIs, the parsed text report
/// (kept verbatim in `rawText`) on legacy ones.
public struct DetectReport: Sendable, Hashable { public let project: String?; public let gitRepository: Bool; public let initialized: Bool
    public let stack: String?; public let adapter: String?; public let modules: [String]; public let hasDevcontainer: Bool?
    public let hasEnv: Bool?; public let warnings: [String]; public let rawText: String?   // rawText on legacy
    public init(project: String?, gitRepository: Bool, initialized: Bool, stack: String? = nil, adapter: String? = nil,
                modules: [String] = [], hasDevcontainer: Bool? = nil, hasEnv: Bool? = nil, warnings: [String] = [],
                rawText: String? = nil) {
        self.project = project
        self.gitRepository = gitRepository
        self.initialized = initialized
        self.stack = stack
        self.adapter = adapter
        self.modules = modules
        self.hasDevcontainer = hasDevcontainer
        self.hasEnv = hasEnv
        self.warnings = warnings
        self.rawText = rawText
    }
}
