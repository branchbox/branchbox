import Foundation

/// Typed recoveries carry the concrete request to re-run. Built only by RecoveryPlanner (Kit/Planning).
public enum RecoveryAction: Sendable, Hashable, Identifiable {
    case retry(OperationRequestContext, label: String, destructive: Bool, confirmation: String?)
    case runInTerminal(command: [String], workingDirectory: String?, label: String)
    case revealInFinder(path: String)
    case copyCommand(String, label: String)
    case openDoctor
    case locateCLI
    case refresh(ProjectRef)
    case showLog(operation: UUID)
    /// Stable across rebuilds of the same recovery list: the case name plus its label (or, for the
    /// cases without a label, the value that tells two of them apart).
    public var id: String {
        switch self {
        case .retry(_, let label, _, _): "retry:\(label)"
        case .runInTerminal(_, _, let label): "runInTerminal:\(label)"
        case .revealInFinder(let path): "revealInFinder:\(path)"
        case .copyCommand(_, let label): "copyCommand:\(label)"
        case .openDoctor: "openDoctor"
        case .locateCLI: "locateCLI"
        case .refresh(let project): "refresh:\(project.path)"
        case .showLog(let operation): "showLog:\(operation.uuidString)"
        }
    }
}

public enum AttentionReason: Sendable, Hashable { case degraded, failedRetained, orphaned, interrupted, setupIncomplete(module: String), folderMissing, worktreeInvalid, unknownStatus(String), unregisteredWorktree }
public struct AttentionItem: Sendable, Hashable, Identifiable {
    public let id: String; public let featureOrPath: String; public let reason: AttentionReason
    public init(id: String, featureOrPath: String, reason: AttentionReason) {
        self.id = id
        self.featureOrPath = featureOrPath
        self.reason = reason
    }
}
