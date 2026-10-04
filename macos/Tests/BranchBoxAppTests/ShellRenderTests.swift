import AppKit
@testable import BranchBoxApp
import BranchBoxKit
import BranchBoxPreview
import BranchBoxStores
import SwiftUI
import Testing

/// Renders the app shell (SW-4) for visual review: sidebar states, the detail column behind the environment
/// gate, Quick Open, the menu bar icon and menu, the Activity popover, toasts and the fixed component kit.
/// Run with BRANCHBOX_RENDER_DIR=<dir>. Sidebars render as plain stacks: source lists render blank offscreen.
@MainActor
@Suite(.enabled(if: SnapshotRenderer.isEnabled), .serialized)
struct ShellRenderTests {
    static let pane = CGSize(width: 820, height: 640)
    static let sidebar = CGSize(width: 260, height: 720)

    // MARK: Sidebar

    @Test func sidebarShowcase() async throws {
        let harness = AppTestModel(.showcase)
        let other = ProjectRef(root: URL(fileURLWithPath: "/Users/dev/projects/acme-storefront/main", isDirectory: true))
        await harness.backend.setListing(FeatureListing(features: Array(PreviewSamples.features.prefix(1))), for: other)
        try await harness.start(projects: [PreviewSamples.project, other])
        await harness.backend.script(.startFeature, .suspendUntilResumed)
        var start = StartFeatureRequest(project: PreviewSamples.project, name: "billing-webhooks", runtime: .container)
        start.mode = .full
        _ = harness.model.actions.dispatch(.start(start))
        let teardown = TeardownRequest(feature: FeatureRef(project: PreviewSamples.project, name: "agent-sandbox"),
                                       recordedBranch: "feature/agent-sandbox", branch: .keep)
        await harness.backend.script(.teardownFeature, .suspendUntilResumed)
        _ = harness.model.actions.dispatch(.teardown(teardown))
        _ = await harness.backend.waitUntilSuspended(.startFeature)
        let model = harness.model
        let selected = SidebarSelection.feature(projectPath: PreviewSamples.project.path, name: "checkout-redesign")
        try render("shell-sidebar-showcase", size: Self.sidebar) {
            SidebarStack(model: model, selection: selected) {
                ProjectRow(name: "remotion", attentionCount: 0, rootExists: false)
                ProjectRow(name: "prine-app", isStale: true)
                SidebarStateRow(kind: .failed("feature list timed out"), onAction: {})
                    .padding(.leading, 22)
            }
            .environment(model)
        }
        await harness.remove()
    }

    @Test func sidebarFirstRunStates() async throws {
        let empty = AppTestModel(.emptyProject)
        try await empty.start()
        let model = empty.model
        try render("shell-sidebar-states", size: CGSize(width: 260, height: 360)) {
            SidebarStack(model: model, selection: .project(path: PreviewSamples.project.path)) {
                ProjectRow(name: "new-service", isRefreshing: true)
                SidebarStateRow(kind: .loading)
                    .padding(.leading, 22)
                ProjectRow(name: "pinned-repo", attentionCount: 2, isPinned: true)
                ShowRemovedRow(includeRemoved: true, removedCount: 6) {}
                    .padding(.leading, 22)
            }
            .environment(model)
        }
        await empty.remove()
    }

    // MARK: Detail column

    @Test func gateBlockingStates() throws {
        try render("shell-gate-cli-missing", size: Self.pane) {
            BlockingEnvironmentView(error: .cliNotFound(searched: PreviewSamples.searchedPaths), onLocate: {},
                                    onRedetect: {}, onDiagnostics: {})
        }
        try render("shell-gate-cli-too-old", size: Self.pane) {
            BlockingEnvironmentView(error: .cliTooOld(found: SemVer(0, 12, 9), minimum: BackendIdentity.minimumCLI,
                                                      path: "/usr/local/Cellar/branchbox/0.12.9/bin/branchbox"),
                                    onLocate: {}, onRedetect: {}, onDiagnostics: {})
        }
        try render("shell-gate-cli-unusable", size: Self.pane) {
            BlockingEnvironmentView(error: .cliUnusable(path: "/Users/dev/.cargo/bin/branchbox",
                                                        reason: "branchbox --version exited with signal 9 (killed)"),
                                    isRechecking: true, onLocate: {}, onRedetect: {}, onDiagnostics: {})
        }
    }

    @Test func gateLegacyBannerOverContent() async throws {
        let harness = AppTestModel(.legacy0134)
        try await harness.start()
        let model = harness.model
        let router = PresentationRouter(selection: .project(path: PreviewSamples.project.path))
        try render("shell-detail-legacy-banner", size: Self.pane) {
            EnvironmentGate {
                DetailColumn(selection: router.selection, router: router)
            }
            .environment(model)
        }
        await harness.remove()
    }

    @Test func gateCLIMissingThroughTheModel() async throws {
        let harness = AppTestModel(.cliMissing)
        await harness.model.start()
        let model = harness.model
        try render("shell-detail-cli-missing-live", size: Self.pane) {
            EnvironmentGate { Text("content") }
                .environment(model)
        }
        await harness.remove()
    }

    @Test func detailFallbacks() async throws {
        let harness = AppTestModel(.showcase)
        try await harness.start()
        let model = harness.model
        let project = PreviewSamples.project
        let gone = PresentationRouter(selection: .feature(projectPath: project.path, name: "payments-v2"))
        try render("shell-detail-feature-gone", size: Self.pane) {
            DetailColumn(selection: gone.selection, router: gone)
                .environment(model)
        }
        let stray = PresentationRouter(selection: .stray(projectPath: project.path, path: PreviewSamples.strays[1].path))
        try render("shell-detail-stray", size: Self.pane) {
            DetailColumn(selection: stray.selection, router: stray)
                .environment(model)
        }
        let removedProject = PresentationRouter(selection: .project(path: "/Users/dev/projects/old/main"))
        try render("shell-detail-project-gone", size: Self.pane) {
            DetailColumn(selection: removedProject.selection, router: removedProject)
                .environment(model)
        }
        try render("shell-detail-project-missing", size: Self.pane) {
            ProjectMissingView(path: "/Volumes/External/work/remotion/main", onLocate: {}, onRemove: {})
        }
        await harness.remove()
    }

    @Test func mainWindowWhole() async throws {
        let harness = AppTestModel(.showcase)
        try await harness.start()
        let model = harness.model
        try render("shell-main-window", size: CGSize(width: 1100, height: 720)) {
            MainWindow(buildBadge: "PREVIEW · showcase")
                .environment(model)
        }
        await harness.remove()
    }

    // MARK: Quick Open

    @Test func quickOpen() async throws {
        let harness = AppTestModel(.showcase)
        try await harness.start()
        let items = QuickOpenIndex.items(model: harness.model)
        let all = QuickOpenIndex.grouped(items)
        try render("shell-quick-open-empty-query", size: CGSize(width: 640, height: 480)) {
            QuickOpenPanelHost(query: "", results: all, highlighted: all.first?.id)
        }
        let filtered = QuickOpenIndex.grouped(QuickOpenIndex.filter(items, query: "check"))
        try render("shell-quick-open-filtered", size: CGSize(width: 640, height: 360)) {
            QuickOpenPanelHost(query: "check", results: filtered, highlighted: filtered.dropFirst().first?.id)
        }
        try render("shell-quick-open-no-match", size: CGSize(width: 640, height: 200)) {
            QuickOpenPanelHost(query: "kubernetes", results: [], highlighted: nil)
        }
        await harness.remove()
    }

    // MARK: Menu bar

    @Test func menuBarIcons() throws {
        let states: [(String, MenuBarStatus)] = [
            ("Idle", .make(blocked: false, attention: 0, running: 0)),
            ("Working", .make(blocked: false, attention: 0, running: 2)),
            ("Attention", .make(blocked: false, attention: 3, running: 0)),
            ("Blocked", .make(blocked: true, attention: 0, running: 0)),
        ]
        try render("shell-menubar-icons", size: CGSize(width: 520, height: 120)) {
            HStack(spacing: 28) {
                ForEach(states, id: \.0) { name, status in
                    VStack(spacing: 10) {
                        MenuBarIcon(status: status)
                            .padding(.horizontal, 8)
                            .padding(.vertical, 3)
                            .background(.quaternary, in: RoundedRectangle(cornerRadius: 4))
                            .scaleEffect(2)
                            .frame(height: 44)
                        Text(name).font(.caption).foregroundStyle(.secondary)
                    }
                }
            }
            .padding(24)
        }
    }

    @Test func menuBarMenu() async throws {
        let harness = AppTestModel(.showcase)
        try await harness.start()
        let model = harness.model
        let failing = TeardownRequest(feature: FeatureRef(project: PreviewSamples.project, name: "prine"),
                                      recordedBranch: "feature/prine", branch: .keep)
        if case .started(let record) = model.actions.dispatch(.teardown(failing)) {
            try await waitFor { !record.isCancellable }
        }
        try render("shell-menubar-menu", size: CGSize(width: 340, height: 720)) {
            MenuMock {
                MenuBarContent()
            }
            .environment(model)
        }
        await harness.remove()
    }

    // MARK: Activity, toasts and banners

    @Test func activityPopover() async throws {
        let harness = AppTestModel(.showcase)
        try await harness.start()
        let model = harness.model
        let project = PreviewSamples.project
        let teardown = TeardownRequest(feature: FeatureRef(project: project, name: "prine"), recordedBranch: "feature/prine",
                                       branch: .keep)
        if case .started(let record) = model.actions.dispatch(.teardown(teardown)) {     // refused: dirty worktree
            try await waitFor { !record.isCancellable }
        }
        await harness.backend.script(.startFeature, .suspendUntilResumed)
        _ = model.actions.dispatch(.start(StartFeatureRequest(project: project, name: "billing-webhooks", runtime: .container)))
        _ = await harness.backend.waitUntilSuspended(.startFeature)
        try render("shell-activity-popover", size: CGSize(width: 360, height: 300)) {
            ActivityPopover()
                .environment(model)
        }
        let empty = AppTestModel(.contract)
        let emptyModel = empty.model
        try render("shell-activity-popover-empty", size: CGSize(width: 360, height: 160)) {
            ActivityPopover()
                .environment(emptyModel)
        }
        await empty.remove()
        await harness.remove()
    }

    @Test func toastAndLegacyBanner() throws {
        try render("shell-toast-and-banner", size: CGSize(width: 820, height: 220)) {
            VStack(spacing: 24) {
                LegacyCLIBanner(version: SemVer(0, 13, 4)) {}
                ToastView(toast: .init(title: "checkout-redesign is ready", body: "Started in 1:42 with 4 modules")) {}
            }
        }
    }

    @Test func fixedComponentKit() throws {
        let project = PreviewSamples.project
        let feature = FeatureRef(project: project, name: "prine")
        let request = TeardownRequest(feature: feature, recordedBranch: "feature/prine", branch: .deleteIfMerged)
        let refusal = BackendError.refused(Refusal(cause: .uncommittedChanges(files: PreviewSamples.dirtyFiles),
                                                   message: "Worktree 'prine' has 2 uncommitted changes",
                                                   diagnostics: Diagnostics(summary: "teardown refused")))
        try render("shell-kit-error-banners", size: CGSize(width: 760, height: 420)) {
            VStack(alignment: .leading, spacing: 16) {
                ErrorBanner(error: refusal, context: .teardown(request), onRetry: {}, onDetails: {}, onRecovery: { _ in })
                ErrorBanner(error: .commandFailed(Diagnostics(summary: "feature list timed out after 30 s")), isStale: true,
                            onRetry: {})
                ErrorBanner(error: .cliNotFound(searched: PreviewSamples.searchedPaths), context: nil)
            }
            .padding(20)
        }
        try render("shell-kit-feature-gone", size: CGSize(width: 620, height: 360)) {
            FeatureGoneView(name: "payments-v2", onShowProject: {}, onShowRemoved: {})
        }
        try render("shell-kit-narrow-cli-missing", size: CGSize(width: 420, height: 520)) {
            CLINotFoundView(searched: PreviewSamples.searchedPaths, onLocate: {}, onRedetect: {})
        }
    }

    /// Renders as the key window draws: offscreen windows are never key, which greys prominent buttons.
    private func render<V: View>(_ name: String, size: CGSize, @ViewBuilder _ view: () -> V) throws {
        let content = view()
        try SnapshotRenderer.render(name, size: size) {
            content.environment(\.controlActiveState, .key)
        }
    }

    private func waitFor(_ condition: () -> Bool) async throws {
        let deadline = ContinuousClock.now + .seconds(30)    // shares the main actor with the other render suites
        while !condition() {
            try #require(ContinuousClock.now < deadline, "timed out")
            try await Task.sleep(for: .milliseconds(5))
        }
    }
}

// MARK: Render scaffolding

/// The sidebar's rows as a plain stack, styled like the source list: the real store-driven projects first, then
/// `extra` rows for states that need a hand-built project.
private struct SidebarStack<Extra: View>: View {
    let model: AppModel
    var selection: SidebarSelection?
    @ViewBuilder var extra: Extra

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            ForEach(model.projects.projects) { project in
                ProjectRow(name: project.displayName, path: project.ref.path, attentionCount: project.attention.count,
                           isRefreshing: project.isRefreshing, rootExists: project.rootExists, isPinned: project.isPinned)
                    .padding(.horizontal, 8)
                    .padding(.top, 6)
                ForEach(SidebarItem.items(for: project, operations: model.operations)) { item in
                    row(item, project: project)
                        .padding(.horizontal, 8)
                        .padding(.vertical, 2)
                        .background(isSelected(item, project) ? Color.accentColor.opacity(0.85) : .clear,
                                    in: RoundedRectangle(cornerRadius: 5))
                        .environment(\.colorScheme, isSelected(item, project) ? .dark : colorScheme)
                        .padding(.leading, 14)
                }
            }
            VStack(alignment: .leading, spacing: 6) { extra }
                .padding(.horizontal, 8)
                .padding(.top, 6)
            Spacer()
        }
        .padding(.horizontal, 8)
        .padding(.vertical, 12)
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .background(.background.secondary)
    }

    @Environment(\.colorScheme) private var colorScheme

    private func isSelected(_ item: SidebarItem, _ project: ProjectStore) -> Bool {
        item.selection(in: project.ref) == selection
    }

    @ViewBuilder private func row(_ item: SidebarItem, project: ProjectStore) -> some View {
        switch item {
        case .feature(let record, let attention, let running):
            FeatureRow(record: record, attention: attention, runningOperation: running,
                       projectPrefix: project.config?.effective.branchPrefix,
                       projectRuntime: project.config?.effective.runtimeProvider)
        case .stray(let stray):
            StrayRow(stray: stray)
        case .provisional(let name):
            ProvisionalFeatureRow(name: name)
        case .state(let kind):
            SidebarStateRow(kind: kind, onAction: {})
        case .showRemoved(let include, let count):
            ShowRemovedRow(includeRemoved: include, removedCount: count) {}
        }
    }
}

/// The palette panel centred over a dimmed window, as it appears.
private struct QuickOpenPanelHost: View {
    @State var query: String
    let results: [QuickOpenItem]
    let highlighted: QuickOpenItem.ID?
    @FocusState private var focused: Bool

    var body: some View {
        ZStack(alignment: .top) {
            Color.black.opacity(0.12)
            QuickOpenPanel(query: $query, results: results, highlighted: highlighted, fieldFocused: $focused)
                .padding(.top, 24)
        }
    }
}

/// Approximates the `.menu` extra: plain buttons and menus in a column on a menu-like background.
private struct MenuMock<Content: View>: View {
    @ViewBuilder var content: Content

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            content
        }
        .buttonStyle(.plain)
        .menuStyle(.borderlessButton)
        .labelStyle(.titleAndIcon)
        .padding(12)
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .background(.regularMaterial)
    }
}
