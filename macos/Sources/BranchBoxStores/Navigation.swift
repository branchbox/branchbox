import BranchBoxKit
import Foundation

/// What the main window's sidebar has selected. Persisted per window as JSON in `@SceneStorage("selection")`
/// and resolved against the stores on every render, so a selection whose feature is gone reads as nil.
public enum SidebarSelection: Hashable, Codable, Sendable {
    case welcome
    case project(path: String)
    case feature(projectPath: String, name: String)
    case stray(projectPath: String, path: String)
}

/// How the Set Up BranchBox sheet runs `init`: a first set-up, or a repair of an existing one (`--update`).
public enum InitSheetMode: String, Hashable, Codable, Sendable { case setUp, repair }

/// A request for the main window to show something. Views post one through `AppModel.post(_:)` and dismiss;
/// the window's router performs it once nothing else is presented.
public enum WindowIntent: Hashable, Sendable {
    case select(SidebarSelection)
    case startFeature(project: ProjectRef?, prefill: StartFeatureRequest?)
    case teardown(FeatureRef, preselect: BranchPolicy?)
    case prune(ProjectRef)
    case stray(ProjectRef, StrayWorktree)
    case addProject(URL?)
    case initProject(URL, mode: InitSheetMode)
    case projectSettings(ProjectRef)
    case syncDevcontainers(ProjectRef)
    case showActivity(operation: UUID?)
    case showDiagnostics
    case quickOpen
}
