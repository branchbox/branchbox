import BranchBoxKit
import Foundation

/// Carries the 0.13 app's preferences over once (D-22, §8.1), in the same defaults domain (`dev.branchbox.app`):
/// - `branchbox.workspace` becomes a project when the backend resolves it (to its main worktree). The old
///   devcontainer default `/workspaces/milestone2` is dropped. While the CLI is unavailable the key is kept, so
///   the next start can import it.
/// - `branchbox.promptHistory` joins `AppSettings.promptHistory` (after any newer prompts).
/// - The keys the new app has no use for are deleted, notably the persisted teardown Force and Delete Branch
///   choices, which must never pre-arm a teardown again.
/// Every imported or obsolete key is removed, so running it again does nothing.
@MainActor enum LegacyDefaultsMigration {
    static let workspaceKey = "branchbox.workspace"
    static let promptHistoryKey = "branchbox.promptHistory"
    static let obsoleteKeys = [
        "branchbox.transportPreference", "branchbox.teardown.force", "branchbox.teardown.deleteBranch",
        "branchbox.teardown.completeSpec", "branchbox.devcontainerStrategy",
    ]
    /// The 0.13 app's gRPC-era default workspace; a path inside the devcontainer, never on the Mac.
    static let staleWorkspace = "/workspaces/milestone2"

    struct Report: Equatable {
        var importedProject: AddProjectOutcome?
        var importedPrompts = 0
        var removedKeys: [String] = []
        /// The workspace key was kept because no backend could resolve it yet.
        var workspaceDeferred = false
    }

    @discardableResult
    static func run(settings: AppSettings, projects: ProjectsStore, backendAvailable: Bool) async -> Report {
        let defaults = settings.defaults
        var report = Report()

        for key in obsoleteKeys where defaults.object(forKey: key) != nil {
            defaults.removeObject(forKey: key)
            report.removedKeys.append(key)
        }

        if defaults.object(forKey: promptHistoryKey) != nil {
            let legacy = defaults.stringArray(forKey: promptHistoryKey) ?? []
            let merged = AppSettings.mergedHistory(settings.promptHistory, legacy)
            report.importedPrompts = merged.count - settings.promptHistory.count
            if merged != settings.promptHistory { settings.promptHistory = merged }
            defaults.removeObject(forKey: promptHistoryKey)
            report.removedKeys.append(promptHistoryKey)
        }

        if defaults.object(forKey: workspaceKey) != nil {
            let path = defaults.string(forKey: workspaceKey)?.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
            let usable = path.hasPrefix("/") && URL(fileURLWithPath: path).standardizedFileURL.path != staleWorkspace
            if usable && !backendAvailable {
                report.workspaceDeferred = true
            } else {
                if usable {
                    let outcome = await projects.add(folder: URL(fileURLWithPath: path, isDirectory: true))
                    report.importedProject = outcome
                }
                defaults.removeObject(forKey: workspaceKey)
                report.removedKeys.append(workspaceKey)
            }
        }
        return report
    }
}
