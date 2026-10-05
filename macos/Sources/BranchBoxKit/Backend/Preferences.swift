import Foundation

// User preferences that Kit planning (HostLaunchPlan) and the stores both read.

public enum EditorChoice: Sendable, Hashable, Codable { case vscode, cursor, custom(appPath: String) }
public enum EditorOpenMode: String, Sendable, Hashable, Codable { case folder, devContainer }
public enum TerminalChoice: Sendable, Hashable, Codable { case terminal, iTerm, custom(template: String) } // {path} {command}
public enum AgentChoice: Sendable, Hashable, Codable { case claude, codex, custom(command: String) }
public enum RefreshInterval: Int, Sendable, Hashable, Codable { case s30 = 30, m1 = 60, m5 = 300, m15 = 900, manual = 0 }
