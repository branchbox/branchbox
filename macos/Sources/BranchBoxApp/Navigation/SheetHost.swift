import BranchBoxKit
import SwiftUI

/// The content of the main window's one sheet: the screen for each `SheetRoute`. Sheets size themselves; they
/// chain to another flow by posting an intent and dismissing, never by presenting a sheet of their own.
struct SheetHost: View {
    let route: SheetRoute

    var body: some View {
        switch route {
        case .startFeature(let project, let prefill):
            StartFeatureSheet(project: project, prefill: prefill)
        case .teardown(let feature, let preselect):
            TeardownSheet(feature: feature, preselect: preselect)
        case .prune(let project):
            PruneSheet(project: project)
        case .stray(let project, let stray):
            StrayWorktreeSheet(project: project, stray: stray)
        case .addProject(let folder):
            AddProjectSheet(initialFolder: folder)
        case .initProject(let folder, let mode):
            InitProjectSheet(folder: folder, mode: mode)
        case .projectSettings(let project):
            ProjectSettingsSheet(project: project)
        case .syncDevcontainers(let project):
            SyncDevcontainersSheet(project: project)
        case .quickOpen:
            // The router shows Quick Open as an overlay and never presents this route; kept for exhaustiveness.
            EmptyView()
        }
    }
}
