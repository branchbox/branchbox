import AppKit
import BranchBoxKit
import BranchBoxStores
import SwiftUI

/// One row under a project in the sidebar, in display order.
enum SidebarItem: Identifiable, Equatable {
    case feature(FeatureRecord, attention: AttentionReason?, running: String?)
    case stray(StrayWorktree)
    /// A start in flight whose feature the registry doesn't list yet.
    case provisional(name: String)
    case state(SidebarStateRow.Kind)
    case showRemoved(includeRemoved: Bool, removedCount: Int)

    var id: String {
        switch self {
        case .feature(let record, _, _): "feature:\(record.workFeature)"
        case .stray(let stray): "stray:\(stray.path)"
        case .provisional(let name): "provisional:\(name)"
        case .state: "state"
        case .showRemoved: "showRemoved"
        }
    }

    /// The selection a row stands for; state and footer rows aren't selectable.
    func selection(in project: ProjectRef) -> SidebarSelection? {
        switch self {
        case .feature(let record, _, _): .feature(projectPath: project.path, name: record.workFeature)
        case .stray(let stray): .stray(projectPath: project.path, path: stray.path)
        case .provisional, .state, .showRemoved: nil
        }
    }

    /// The rows of `project`: provisional starts, then features (attention first, as the store sorts them) and
    /// strays, then the removed-features footer. `query` filters features and strays by name or branch; the
    /// loading, empty and failed rows appear only without a query.
    @MainActor static func items(for project: ProjectStore, operations: OperationStore, query: String = "") -> [SidebarItem] {
        let needle = query.trimmingCharacters(in: .whitespaces)
        func matches(_ texts: String?...) -> Bool {
            needle.isEmpty || texts.contains { $0?.localizedCaseInsensitiveContains(needle) == true }
        }
        let reasons = Dictionary(project.attention.map { ($0.featureOrPath, $0.reason) }, uniquingKeysWith: { first, _ in first })
        var running: [String: String] = [:]
        var provisional: [String] = []
        for record in operations.running {
            guard case .feature(let feature) = record.target, feature.project == project.ref else { continue }
            if running[feature.name] == nil { running[feature.name] = record.title }
            if record.kind == .start, project.feature(named: feature.name) == nil, !provisional.contains(feature.name) {
                provisional.append(feature.name)
            }
        }
        var items: [SidebarItem] = provisional.filter { matches($0) }.map { .provisional(name: $0) }
        for record in project.features where matches(record.workFeature, record.branchName) {
            items.append(.feature(record, attention: reasons[record.workFeature], running: running[record.workFeature]))
        }
        for stray in project.strays where matches(stray.path, stray.branch) {
            items.append(.stray(stray))
        }
        if needle.isEmpty {
            switch project.loadState {
            case .failed(let error, _):
                items.append(.state(.failed(error.presentation().message)))
            case .idle, .loading:
                if project.features.isEmpty, project.rootExists { items.append(.state(.loading)) }
            case .loaded:
                if project.features.isEmpty, project.strays.isEmpty, provisional.isEmpty { items.append(.state(.empty)) }
            }
            let removed = project.features.filter { $0.status == .removed }.count
            if project.rootExists, project.lastLoadedAt != nil {
                items.append(.showRemoved(includeRemoved: project.includeRemoved, removedCount: removed))
            }
        }
        return items
    }
}

/// The main window's sidebar (DESIGN §9 Sidebar): one disclosure group per project with its features, strays
/// and states; search filters every project. Double-click opens a feature in the editor (or reviews a stray);
/// the context menu offers the item's actions.
struct Sidebar: View {
    @Environment(AppModel.self) private var model
    @Bindable var router: PresentationRouter
    @State private var query = ""

    var body: some View {
        List(selection: $router.selection) {
            ForEach(model.projects.projects) { project in
                ProjectSection(project: project, query: query)
            }
        }
        .listStyle(.sidebar)
        .searchable(text: $query, placement: .sidebar, prompt: "Filter features")
        .contextMenu(forSelectionType: SidebarSelection.self) { selections in
            if let selection = selections.first { menu(for: selection) }
        } primaryAction: { selections in
            if let selection = selections.first { primaryAction(selection) }
        }
        .overlay {
            if model.projects.projects.isEmpty, model.hasStarted {
                VStack(spacing: 8) {
                    Text("No projects").font(.headline).foregroundStyle(.secondary)
                    Button("Add Project…") { model.post(.addProject(nil)) }
                        .disabled(model.environment.identity == nil)
                }
            }
        }
    }

    // MARK: Actions

    private func primaryAction(_ selection: SidebarSelection) {
        switch selection {
        case .feature(let projectPath, let name):
            guard let ref = selection.projectRef, let store = model.projects.project(ref),
                  let record = store.feature(named: name), projectPath == store.ref.path else { return }
            FeatureCommands.openInEditor(record, project: store.ref, model: model)
        case .stray(_, let path):
            guard let ref = selection.projectRef, let stray = model.projects.project(ref)?.strays.first(where: { $0.path == path })
            else { return }
            model.post(.stray(ref, stray))
        case .project(let path):
            guard let store = model.projects.project(ProjectRef(root: URL(fileURLWithPath: path, isDirectory: true))) else { return }
            model.projects.setCollapsed(store.ref, !store.isCollapsed)
        case .welcome:
            break
        }
    }

    @ViewBuilder private func menu(for selection: SidebarSelection) -> some View {
        switch selection {
        case .feature:
            if let feature = selection.featureRef {
                FeatureActionsMenu(feature: feature, style: .contextMenu)
            }
        case .stray(_, let path):
            if let ref = selection.projectRef, let stray = model.projects.project(ref)?.strays.first(where: { $0.path == path }) {
                Button("Review Worktree…") { model.post(.stray(ref, stray)) }
                Button("Reveal in Finder") {
                    NSWorkspace.shared.activateFileViewerSelecting([URL(fileURLWithPath: path, isDirectory: true)])
                }
            }
        case .project:
            if let ref = selection.projectRef, let store = model.projects.project(ref) {
                ProjectContextMenu(project: store)
            }
        case .welcome:
            EmptyView()
        }
    }
}

/// One project's disclosure group in the sidebar.
private struct ProjectSection: View {
    @Environment(AppModel.self) private var model
    let project: ProjectStore
    let query: String

    var body: some View {
        DisclosureGroup(isExpanded: expanded) {
            ForEach(SidebarItem.items(for: project, operations: model.operations, query: query)) { item in
                row(item)
            }
        } label: {
            ProjectRow(name: project.displayName, path: project.ref.path, attentionCount: project.attention.count,
                       isRefreshing: project.isRefreshing, isStale: isStale, rootExists: project.rootExists,
                       isPinned: project.isPinned)
                .accessibilityIdentifier("sidebar.project.\(project.ref.path)")
        }
        .tag(SidebarSelection.project(path: project.ref.path))
    }

    /// Searching expands every project so matches are visible.
    private var expanded: Binding<Bool> {
        Binding(get: { !query.isEmpty || !project.isCollapsed },
                set: { model.projects.setCollapsed(project.ref, !$0) })
    }

    private var isStale: Bool {
        if case .failed = project.loadState { return true }
        return false
    }

    @ViewBuilder private func row(_ item: SidebarItem) -> some View {
        switch item {
        case .feature(let record, let attention, let running):
            FeatureRow(record: record, attention: attention, runningOperation: running,
                       projectPrefix: project.config?.effective.branchPrefix,
                       projectRuntime: project.config?.effective.runtimeProvider)
                .tag(SidebarSelection.feature(projectPath: project.ref.path, name: record.workFeature))
                .accessibilityIdentifier("sidebar.feature.\(project.ref.path).\(record.workFeature)")
        case .stray(let stray):
            StrayRow(stray: stray)
                .tag(SidebarSelection.stray(projectPath: project.ref.path, path: stray.path))
        case .provisional(let name):
            ProvisionalFeatureRow(name: name)
                .selectionDisabled()
        case .state(let kind):
            SidebarStateRow(kind: kind, onAction: stateAction(kind))
                .selectionDisabled()
        case .showRemoved(let includeRemoved, let count):
            ShowRemovedRow(includeRemoved: includeRemoved, removedCount: count) { project.includeRemoved.toggle() }
                .selectionDisabled()
        }
    }

    private func stateAction(_ kind: SidebarStateRow.Kind) -> (() -> Void)? {
        switch kind {
        case .loading: nil
        case .empty: { model.post(.startFeature(project: project.ref, prefill: nil)) }
        case .failed: { project.requestRefresh(.manual) }
        }
    }
}

/// The project row's context menu (DESIGN §9 Sidebar).
struct ProjectContextMenu: View {
    @Environment(AppModel.self) private var model
    let project: ProjectStore

    var body: some View {
        let ref = project.ref
        if project.rootExists {
            Button("Start Feature…") { model.post(.startFeature(project: ref, prefill: nil)) }
            Divider()
            Button("Settings…") { model.post(.projectSettings(ref)) }
            Button("Prune…") { model.post(.prune(ref)) }
            Button("Update All Workspaces…") { model.post(.syncDevcontainers(ref)) }
            Divider()
            Button("Reveal in Finder") { NSWorkspace.shared.activateFileViewerSelecting([ref.root]) }
            Button("Open in Terminal") { openTerminal(ref) }
            Button("Refresh") { project.requestRefresh(.manual) }
        } else {
            Button("Locate…") { FeatureCommands.locate(ref, model: model) }
        }
        Divider()
        Button(project.isPinned ? "Unpin" : "Pin to Top") { model.projects.setPinned(ref, !project.isPinned) }
        Button("Remove from BranchBox…") { FeatureCommands.confirmRemove(ref, model: model) }
    }

    private func openTerminal(_ ref: ProjectRef) {
        let plan = HostLaunchPlan.folderTerminal(model.settings.preferredTerminal, path: ref.path, folderExists: project.rootExists)
        Task { @MainActor in
            do {
                try await HostLauncher().launch(plan)
            } catch let error as HostLaunchError {
                ToastCenter.shared.show(title: "Couldn't open Terminal", body: error.message)
            } catch {
                ToastCenter.shared.show(title: "Couldn't open Terminal", body: error.localizedDescription)
            }
        }
    }
}
