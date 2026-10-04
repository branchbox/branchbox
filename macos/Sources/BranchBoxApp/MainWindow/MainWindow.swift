import AppKit
import BranchBoxKit
import BranchBoxStores
import SwiftUI

/// The main window (DESIGN §9 "Scenes and menus"): a two-column split of the sidebar and the detail, behind the
/// environment gate, with the Activity inspector, the toolbar, one sheet driven by the window's router, the
/// Quick Open overlay and in-window toasts. The selection is persisted per window in `@SceneStorage`.
struct MainWindow: View {
    @Environment(AppModel.self) private var model
    @Environment(\.openWindow) private var openWindow
    @SceneStorage("selection") private var storedSelection = ""
    @AppStorage(GeneralTab.reopenLastSelectionKey, store: AppSettings.defaultsForCurrentProcess())
    private var reopenLastSelection = true
    @State private var router = PresentationRouter()
    @State private var showsActivity = false
    /// "DEV" or "PREVIEW · <scenario>" for non-release builds (§11).
    var buildBadge: String?

    var body: some View {
        @Bindable var router = router
        NavigationSplitView {
            Sidebar(router: router)
                .navigationSplitViewColumnWidth(min: 220, ideal: 240, max: 260)
        } detail: {
            EnvironmentGate {
                DetailColumn(selection: router.selection, router: router)
            }
            .inspector(isPresented: $router.isInspectorPresented) {
                ActivityInspector(target: router.selection?.operationTarget)
                    .inspectorColumnWidth(min: 260, ideal: 300, max: 420)
            }
        }
        .navigationTitle(title)
        .navigationSubtitle(subtitle)
        .toolbar { toolbar }
        .sheet(item: $router.sheet, onDismiss: { router.promoteNext() }) { route in
            SheetHost(route: route)
        }
        .overlay {
            if router.isQuickOpenPresented {
                QuickOpenPalette(router: router)
            }
        }
        .overlay(alignment: .bottom) {
            if let toast = ToastCenter.shared.current {
                ToastView(toast: toast) { ToastCenter.shared.dismiss() }
                    .padding(.bottom, 20)
                    .transition(.move(edge: .bottom).combined(with: .opacity))
            }
        }
        .animation(.easeOut(duration: 0.2), value: ToastCenter.shared.current)
        .focusedSceneValue(\.mainWindowRouter, router)
        .frame(minWidth: 800, minHeight: 520)
        .registersWindowOpener()
        .onAppear(perform: appear)
        .onChange(of: model.intentToken) { router.consume(from: model) }
        .onChange(of: router.selection) { _, selection in selectionChanged(selection) }
        .onChange(of: model.projects.projects.map(\.id)) { projectsChanged() }
        .onChange(of: model.hasStarted) { selectFirstProjectIfNeeded() }
    }

    // MARK: Toolbar

    @ToolbarContentBuilder private var toolbar: some ToolbarContent {
        let context = CommandContext(model: model, selection: router.selection, hasMainWindow: true)
        ToolbarItemGroup(placement: .primaryAction) {
            Button {
                model.post(.startFeature(project: context.project, prefill: nil))
            } label: {
                Label("Start Feature", systemImage: "plus")
            }
            .disabled(!context.state(.startFeature).isEnabled)
            .help(context.state(.startFeature).reason ?? "Start a feature (⌘N)")

            Button {
                refresh(context)
            } label: {
                Label("Refresh", systemImage: "arrow.clockwise")
            }
            .disabled(!context.state(.refresh).isEnabled)
            .help(context.state(.refresh).reason ?? "Refresh (⌘R)")

            ActivityToolbarButton(isPresented: $showsActivity)

            Button {
                router.isInspectorPresented.toggle()
            } label: {
                Label("Inspector", systemImage: "sidebar.trailing")
            }
            .help(router.isInspectorPresented ? "Hide the Activity inspector (⌥⌘I)" : "Show the Activity inspector (⌥⌘I)")
        }
        if let buildBadge {
            ToolbarItem(placement: .status) {
                Text(buildBadge)
                    .font(.caption2.weight(.bold))
                    .foregroundStyle(.secondary)
                    .padding(.horizontal, 6)
                    .padding(.vertical, 2)
                    .overlay(Capsule().strokeBorder(.tertiary))
                    .help("A development build: it keeps its own settings, projects and logs")
            }
        }
    }

    // MARK: Lifecycle

    private func appear() {
        router.openWindow = { id in openWindow(id: id) }
        router.operation = { [model] id in model.operations.record(id) }
        if router.selection == nil, reopenLastSelection { router.selection = PresentationRouter.decode(storedSelection) }
        selectFirstProjectIfNeeded()
        router.consume(from: model)
    }

    private func selectionChanged(_ selection: SidebarSelection?) {
        storedSelection = PresentationRouter.encode(selection)
        let project = selection?.projectRef
        // Drives the selected-project refresh timer. (Not `markOpened`: reordering the sidebar on every click
        // would move rows out from under the pointer.)
        if model.projects.selectedProject != project { model.projects.selectedProject = project }
    }

    /// The list changed: a selection in a project that was removed moves to the first project (Welcome when none
    /// is left), rather than leaving the window on "This project isn't in the list".
    private func projectsChanged() {
        if model.hasStarted, let ref = router.selection?.projectRef, model.projects.project(ref) == nil {
            router.selection = model.projects.projects.first.map { .project(path: $0.ref.path) }
        }
        selectFirstProjectIfNeeded()
    }

    /// With projects but nothing selected, select the first project rather than show an empty detail.
    private func selectFirstProjectIfNeeded() {
        guard router.selection == nil, let first = model.projects.projects.first else { return }
        router.selection = .project(path: first.ref.path)
    }

    private func refresh(_ context: CommandContext) {
        if let project = context.project, let store = model.projects.project(project) {
            store.requestRefresh(.manual)
        } else {
            model.projects.refreshAll(.manual)
        }
    }

    // MARK: Titles

    private var title: String {
        switch router.selection {
        case .feature(_, let name): name
        case .project, .stray: router.selection?.projectRef.flatMap { model.projects.project($0)?.displayName } ?? "BranchBox"
        case .welcome, nil: "BranchBox"
        }
    }

    private var subtitle: String {
        guard let ref = router.selection?.projectRef, let store = model.projects.project(ref) else { return "" }
        if case .feature = router.selection { return store.displayName }
        return store.ref.path.replacingOccurrences(of: NSHomeDirectory(), with: "~")
    }
}

/// The detail column for a selection: Welcome, a project, a feature (or that it no longer exists), or a stray.
struct DetailColumn: View {
    @Environment(AppModel.self) private var model
    let selection: SidebarSelection?
    let router: PresentationRouter

    var body: some View {
        switch selection {
        case nil where !model.hasStarted:
            // projects.json is still loading: a neutral spinner, so Welcome doesn't flash before the list arrives.
            ProgressView()
                .frame(maxWidth: .infinity, maxHeight: .infinity)
        case .welcome, nil:
            WelcomeView()
        case .project(let path):
            if let store = store(path) {
                if store.rootExists {
                    ProjectDetailView(project: store.ref)
                } else {
                    ProjectMissingView(path: store.ref.path, onLocate: { FeatureCommands.locate(store.ref, model: model) },
                                       onRemove: { FeatureCommands.confirmRemove(store.ref, model: model) })
                }
            } else {
                projectGone
            }
        case .feature(let path, let name):
            if let store = store(path) {
                featureDetail(store, name: name)
            } else {
                projectGone
            }
        case .stray(let path, let strayPath):
            if let store = store(path), let stray = store.strays.first(where: { $0.path == strayPath }) {
                StrayDetail(stray: stray) { model.post(.stray(store.ref, stray)) }
            } else {
                EmptyStateLayout(title: "This worktree is gone", systemImage: "questionmark.folder") {
                    Text("It was removed or registered since it was selected.")
                } actions: {
                    EmptyView()
                }
            }
        }
    }

    @ViewBuilder private func featureDetail(_ store: ProjectStore, name: String) -> some View {
        if store.feature(named: name) != nil {
            FeatureDetailView(feature: FeatureRef(project: store.ref, name: name))
        } else if store.lastLoadedAt == nil, store.rootExists {
            ProgressView("Loading \(name)…")
                .frame(maxWidth: .infinity, maxHeight: .infinity)
        } else {
            FeatureGoneView(name: name,
                            onShowProject: { router.selection = .project(path: store.ref.path) },
                            onShowRemoved: store.includeRemoved ? nil : { store.includeRemoved = true })
        }
    }

    private var projectGone: some View {
        EmptyStateLayout(title: "This project isn't in the list", systemImage: "folder.badge.minus") {
            Text("It was removed from BranchBox. Its files are untouched; add it again to see its features.")
        } actions: {
            Button("Add Project…") { model.post(.addProject(nil)) }
                .buttonStyle(.borderedProminent)
        }
    }

    private func store(_ path: String) -> ProjectStore? {
        model.projects.project(ProjectRef(root: URL(fileURLWithPath: path, isDirectory: true)))
    }
}

/// A selected unregistered worktree: what it is, and [Review…] to reveal, open or remove it.
struct StrayDetail: View {
    let stray: StrayWorktree
    var onReview: () -> Void

    var body: some View {
        EmptyStateLayout(title: URL(fileURLWithPath: stray.path).lastPathComponent, systemImage: "questionmark.folder",
                         tint: .orange) {
            VStack(spacing: 12) {
                Text("This worktree sits in the project's folder but isn't in BranchBox's registry, so BranchBox didn't "
                     + "start it or lost track of it. Review it to open, keep or remove it.")
                SearchedPathsList(paths: [stray.path] + (stray.branch.map { ["branch \($0)"] } ?? []), title: "Worktree:")
            }
        } actions: {
            Button("Review Worktree…", action: onReview)
                .buttonStyle(.borderedProminent)
            Button("Reveal in Finder") {
                NSWorkspace.shared.activateFileViewerSelecting([URL(fileURLWithPath: stray.path, isDirectory: true)])
            }
        }
    }
}

/// The toolbar's Activity button: a badge with the number of running operations, and a popover with what is
/// running and what finished recently.
struct ActivityToolbarButton: View {
    @Environment(AppModel.self) private var model
    @Binding var isPresented: Bool

    var body: some View {
        let running = model.operations.running.count
        Button {
            isPresented.toggle()
        } label: {
            Label("Activity", systemImage: running > 0 ? "clock.arrow.2.circlepath" : "clock.arrow.circlepath")
                .overlay(alignment: .topTrailing) {
                    if running > 0 {
                        Text("\(running)")
                            .font(.system(size: 9, weight: .bold))
                            .monospacedDigit()
                            .foregroundStyle(.white)
                            .padding(.horizontal, 4)
                            .frame(minWidth: 14, minHeight: 14)
                            .background(Color.accentColor, in: Capsule())
                            .offset(x: 8, y: -6)
                            .accessibilityHidden(true)
                    }
                }
        }
        .help(running == 0 ? "Activity" : "Activity: \(running) running")
        .accessibilityLabel(running == 0 ? "Activity" : "Activity, \(running) running")
        .popover(isPresented: $isPresented, arrowEdge: .bottom) {
            ActivityPopover { isPresented = false }
                .environment(model)
        }
    }
}
