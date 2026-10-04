import BranchBoxKit
import BranchBoxStores
import Foundation

/// The sheet a main window presents. Exactly one is presented per main window; to chain flows, a sheet posts
/// a `WindowIntent` and dismisses, and the window's router (SW-4) presents the next route after `onDismiss`.
enum SheetRoute: Identifiable, Hashable {
    case startFeature(project: ProjectRef?, prefill: StartFeatureRequest?)
    case teardown(FeatureRef, preselect: BranchPolicy?)
    case prune(ProjectRef)
    case stray(ProjectRef, StrayWorktree)
    case addProject(URL?)
    case initProject(URL, mode: InitSheetMode)
    case projectSettings(ProjectRef)
    case syncDevcontainers(ProjectRef)
    case quickOpen

    /// Distinct for every route a window can tell apart, and stable for the same route.
    var id: String {
        switch self {
        case .startFeature(let project, let prefill):
            "startFeature:\(project?.path ?? ""):\(prefill?.name ?? "")"
        case .teardown(let feature, let preselect):
            "teardown:\(feature.project.path):\(feature.name):\(preselect?.rawValue ?? "")"
        case .prune(let project):
            "prune:\(project.path)"
        case .stray(let project, let stray):
            "stray:\(project.path):\(stray.path)"
        case .addProject(let folder):
            "addProject:\(folder?.standardizedFileURL.path ?? "")"
        case .initProject(let folder, let mode):
            "initProject:\(folder.standardizedFileURL.path):\(mode.rawValue)"
        case .projectSettings(let project):
            "projectSettings:\(project.path)"
        case .syncDevcontainers(let project):
            "syncDevcontainers:\(project.path)"
        case .quickOpen:
            "quickOpen"
        }
    }

    /// The route that performs `intent`; nil for intents that are not sheets (selection, windows).
    init?(_ intent: WindowIntent) {
        switch intent {
        case .startFeature(let project, let prefill): self = .startFeature(project: project, prefill: prefill)
        case .teardown(let feature, let preselect): self = .teardown(feature, preselect: preselect)
        case .prune(let project): self = .prune(project)
        case .stray(let project, let stray): self = .stray(project, stray)
        case .addProject(let folder): self = .addProject(folder)
        case .initProject(let folder, let mode): self = .initProject(folder, mode: mode)
        case .projectSettings(let project): self = .projectSettings(project)
        case .syncDevcontainers(let project): self = .syncDevcontainers(project)
        case .quickOpen: self = .quickOpen
        case .select, .showActivity, .showDiagnostics: return nil
        }
    }
}
