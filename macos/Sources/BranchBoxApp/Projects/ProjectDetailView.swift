import BranchBoxKit
import BranchBoxPreview
import BranchBoxStores
import SwiftUI

/// One row of the project's feature table, with sortable values.
struct ProjectFeatureRow: Identifiable, Hashable {
    let record: FeatureRecord
    let attention: AttentionReason?

    var id: String { record.workFeature }
    var name: String { record.workFeature }
    var statusLabel: String { attention?.label ?? record.status.label }
    /// Attention first, then active, then the rest.
    var statusRank: Int { attention != nil ? 0 : (record.status == .active ? 1 : 2) }
    var runtimeLabel: String { record.runtime.provider.label }
    var isQuick: Bool { FeaturePresentation.isQuick(record) }
    var branch: String { record.branchName }
    var updated: Date { record.updatedAt ?? record.createdAt ?? .distantPast }
}

/// What the status tiles count. Every number matches what the rows, the sidebar badge and the menu bar say:
/// Active counts rows whose badge reads Active, and Need attention is `ProjectStore.attention` (strays
/// included), the same list the sidebar badge and the menu bar summary count.
struct ProjectStatusCounts: Hashable {
    var active = 0
    var attention = 0
    var unregistered = 0
    /// Torn-down features (listed only when the sidebar shows removed features).
    var removed = 0

    @MainActor init(store: ProjectStore) {
        let attention = store.attention
        let flagged = Set(attention.filter { $0.reason != .unregisteredWorktree }.map(\.featureOrPath))
        self.init(features: store.features, flagged: flagged, attention: attention.count, unregistered: store.strays.count)
    }

    init(features: [FeatureRecord], flagged: Set<String>, attention: Int, unregistered: Int) {
        let listed = features.filter { !flagged.contains($0.workFeature) }
        active = listed.filter { $0.status == .active }.count
        removed = listed.filter { $0.status == .removed }.count
        self.attention = attention
        self.unregistered = unregistered
    }
}

/// One project: the header, a not-set-up banner, load problems, status counts and a sortable feature table whose
/// selection opens the feature. The toolbar has Start, Prune, Update All Workspaces, Project Settings and More
/// (Repair, Check Setup, Remove from Sidebar).
struct ProjectDetailView: View {
    let project: ProjectRef

    @Environment(AppModel.self) private var model
    @State private var sortOrder = [KeyPathComparator(\ProjectFeatureRow.statusRank),
                                    KeyPathComparator(\ProjectFeatureRow.updated, order: .reverse)]
    @State private var selection: ProjectFeatureRow.ID?
    @State private var confirmingRemove = false
    @State private var relocateProblem: String?

    init(project: ProjectRef) {
        self.project = project
    }

    var body: some View {
        Group {
            if let store = model.projects.project(project) {
                content(store)
                    .toolbar { ProjectToolbar(store: store, confirmingRemove: $confirmingRemove) }
                    .confirmationDialog("Remove “\(store.displayName)” from the sidebar?", isPresented: $confirmingRemove,
                                        titleVisibility: .visible) {
                        Button("Remove from Sidebar", role: .destructive) { model.projects.remove(project) }
                        Button("Cancel", role: .cancel) {}
                    } message: {
                        Text("This doesn't delete any files. The repository and its feature folders stay where they are, and you can add the project again at any time.")
                    }
                    .task(id: project) { await loadDetails(store) }
                    .onChange(of: finishedSetupOperations) { _, _ in
                        // Set Up, Repair or a config change finished: detect and config are stale (nothing else
                        // reloads them), so the "not set up" state and the chips follow.
                        Task {
                            await store.reloadDetect()
                            await store.reloadConfig()
                        }
                    }
            } else {
                ContentUnavailableView("This project isn't in the sidebar", systemImage: "folder.badge.questionmark",
                                       description: Text(project.path))
            }
        }
        .navigationTitle(model.projects.project(project)?.displayName ?? project.displayName)
    }

    @ViewBuilder private func content(_ store: ProjectStore) -> some View {
        VStack(alignment: .leading, spacing: 0) {
            ProjectHeader(store: store)
                .padding(.horizontal, 24)
                .padding(.top, 20)
                .padding(.bottom, 16)
            Divider()
            if !store.rootExists {
                ProjectMissingView(path: store.ref.path, onLocate: { locate(store) }, onRemove: { confirmingRemove = true })
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
                if let relocateProblem {
                    ProjectNotice(style: .error, title: "Couldn't use that folder", message: relocateProblem)
                        .padding(24)
                }
            } else {
                mainContent(store)
            }
        }
    }

    @ViewBuilder private func mainContent(_ store: ProjectStore) -> some View {
        let rows = rows(for: store)
        VStack(alignment: .leading, spacing: 16) {
            banners(store)
            if isNotInitialized(store) {
                ContentUnavailableView {
                    Label("BranchBox isn't set up here yet", systemImage: "sparkles")
                } description: {
                    Text("Setting up adds a .branchbox folder and a dev container to \(store.displayName), so features know how to run. You review every change before anything is written.")
                } actions: {
                    Button("Set Up BranchBox…") { model.post(.initProject(store.ref.root, mode: .setUp)) }
                        .buttonStyle(.borderedProminent)
                    Button("Remove from Sidebar…") { confirmingRemove = true }
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else if rows.isEmpty, store.lastLoadedAt == nil, store.loadState != .idle, !isFailed(store) {
                ProgressView("Loading features…")
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else if rows.isEmpty, store.strays.isEmpty, !isNotInitialized(store), !isFailed(store) {
                NoFeaturesView(projectName: store.displayName) {
                    model.post(.startFeature(project: project, prefill: nil))
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else if !rows.isEmpty || !store.strays.isEmpty {
                ProjectStatusTiles(counts: ProjectStatusCounts(store: store))
                featureTable(rows, projectPrefix: store.config?.effective.branchPrefix)
            } else {
                Spacer()
            }
        }
        .padding(.horizontal, 24)
        .padding(.vertical, 16)
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
    }

    // MARK: Banners

    @ViewBuilder private func banners(_ store: ProjectStore) -> some View {
        if isNotInitialized(store) {
            EmptyView()
        } else if case .failed(let error, let lastGood) = store.loadState {
            ErrorBanner(error: error, isStale: lastGood != nil, onRetry: { store.requestRefresh(.manual) })
        }
        if store.droppedRecords > 0 {
            ProjectNotice(style: .warning,
                          title: store.droppedRecords == 1 ? "1 feature couldn't be read"
                                                            : "\(store.droppedRecords) features couldn't be read",
                          message: "Their registry entries are damaged or from a newer version of branchbox, so they're hidden here.")
        }
        ForEach(store.listWarnings, id: \.self) { warning in
            Label(warning, systemImage: "exclamationmark.triangle")
                .font(.callout)
                .foregroundStyle(.orange)
                .textSelection(.enabled)
        }
    }

    /// Finished operations that change what `detect` and `config` report.
    private var finishedSetupOperations: Int {
        let kinds: Set<OperationKind> = [.initProject, .applyConfig, .tunnelCredentials]
        return model.operations.records(for: .project(project)).filter { kinds.contains($0.kind) && !$0.isCancellable }.count
    }

    private func isNotInitialized(_ store: ProjectStore) -> Bool {
        if store.detect?.initialized == false { return true }
        if case .failed(.projectInvalid(.notInitialized), _) = store.loadState { return true }
        return false
    }

    private func isFailed(_ store: ProjectStore) -> Bool {
        if case .failed = store.loadState { return true }
        return false
    }

    // MARK: Table

    private func rows(for store: ProjectStore) -> [ProjectFeatureRow] {
        let attention = Dictionary(store.attention.map { ($0.featureOrPath, $0.reason) }, uniquingKeysWith: { first, _ in first })
        return store.features.map { ProjectFeatureRow(record: $0, attention: attention[$0.workFeature]) }
            .sorted(using: sortOrder)
    }

    private func featureTable(_ rows: [ProjectFeatureRow], projectPrefix: String?) -> some View {
        Table(rows, selection: $selection, sortOrder: $sortOrder) {
            TableColumn("Feature", value: \.name) { row in
                HStack(spacing: 8) {
                    ColorSwatch(hex: row.record.color, size: 10)
                    Text(row.name)
                        .fontWeight(.medium)
                        .lineLimit(1)
                        .truncationMode(.middle)
                    if row.isQuick {
                        QuickCapsule()
                    }
                }
                .help(row.name)
            }
            .width(min: 140, ideal: 180)
            TableColumn("Status", value: \.statusRank) { row in
                if let attention = row.attention {
                    StatusBadge(attention: attention)
                } else {
                    StatusBadge(status: row.record.status)
                }
            }
            .width(min: 110, ideal: 130)
            TableColumn("Runtime", value: \.runtimeLabel) { row in
                RuntimeBadge(provider: row.record.runtime.provider)
            }
            .width(min: 80, ideal: 110)
            TableColumn("Branch", value: \.branch) { row in
                // A branch that is just <prefix>/<name> is dimmed so unusual ones stand out.
                let conventional = FeaturePresentation.hasConventionalBranch(row.record, projectPrefix: projectPrefix)
                Text(row.branch.isEmpty ? "—" : row.branch)
                    .font(.callout.monospaced())
                    .foregroundStyle(conventional ? .secondary : .primary)
                    .lineLimit(1)
                    .truncationMode(.middle)
                    .help(row.branch)
            }
            .width(min: 120, ideal: 170)
            TableColumn("Updated", value: \.updated) { row in
                Text(row.updated == .distantPast ? "—" : FeaturePresentation.shortRelative(row.updated))
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
                    .help(row.updated == .distantPast ? "" : FeaturePresentation.absolute(row.updated))
            }
            .width(min: 96, ideal: 110)
        }
        .tableStyle(.inset(alternatesRowBackgrounds: false))
        // The table fills the rest of the pane, so every row the tiles count is visible (it scrolls only when the
        // pane runs out of room). Stripes are off, so the space below the last row reads as plain background.
        .frame(minHeight: 160, maxHeight: .infinity)
        .onChange(of: selection) { _, name in
            guard let name else { return }
            model.post(.select(.feature(projectPath: project.path, name: name)))
            selection = nil
        }
        .accessibilityIdentifier("project.features")
    }

    // MARK: Actions

    private func loadDetails(_ store: ProjectStore) async {
        if store.config == nil { await store.reloadConfig() }
        if store.detect == nil { await store.reloadDetect() }
    }

    private func locate(_ store: ProjectStore) {
        relocateProblem = nil
        guard let folder = ProjectActions.chooseFolder(title: "Locate \(store.displayName)", prompt: "Use This Folder",
                                                       startingAt: store.ref.root.deletingLastPathComponent()) else { return }
        Task {
            switch await model.projects.relocate(project, to: folder) {
            case .added(let ref, _), .alreadyPresent(let ref):
                model.post(.select(.project(path: ref.path)))
            case .needsInit(let ref):
                model.post(.initProject(ref.root, mode: .setUp))
            case .refused(let error):
                relocateProblem = error.presentation().message
            }
        }
    }
}

/// Small tiles: active, needing attention (unregistered worktrees included, and said so), and torn down when
/// removed features are listed. The table already shows how many features there are.
struct ProjectStatusTiles: View {
    let counts: ProjectStatusCounts

    var body: some View {
        HStack(spacing: 10) {
            tile("Active", counts.active, systemImage: "circle.fill", tint: counts.active > 0 ? .green : .secondary)
            tile("Need attention", counts.attention, systemImage: "exclamationmark.triangle.fill",
                 tint: counts.attention > 0 ? .orange : .secondary, detail: Self.attentionDetail(counts))
            if counts.removed > 0 {
                tile("Torn down", counts.removed, systemImage: "archivebox", tint: .secondary)
            }
        }
    }

    /// "Includes 2 unregistered worktrees", so the number matches the sidebar badge and the menu bar.
    static func attentionDetail(_ counts: ProjectStatusCounts) -> String? {
        switch counts.unregistered {
        case 0: nil
        case 1: "Includes 1 unregistered worktree"
        default: "Includes \(counts.unregistered) unregistered worktrees"
        }
    }

    private func tile(_ title: String, _ value: Int, systemImage: String, tint: Color, detail: String? = nil) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            Label(title, systemImage: systemImage)
                .font(.caption)
                .foregroundStyle(tint == .secondary ? Color.secondary : tint)
                .labelStyle(.titleAndIcon)
            HStack(alignment: .firstTextBaseline, spacing: 8) {
                Text("\(value)")
                    .font(.title2.weight(.semibold))
                    .monospacedDigit()
                if let detail {
                    Text(detail)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                }
            }
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 8)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(Color(nsColor: .controlBackgroundColor), in: RoundedRectangle(cornerRadius: 10))
        .overlay(RoundedRectangle(cornerRadius: 10).strokeBorder(Color(nsColor: .separatorColor)))
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(detail.map { "\(title): \(value). \($0)" } ?? "\(title): \(value)")
    }
}

/// The project's window toolbar items. The main action, Start Feature, comes last (rightmost).
struct ProjectToolbar: ToolbarContent {
    let store: ProjectStore
    @Binding var confirmingRemove: Bool

    var body: some ToolbarContent {
        ToolbarItemGroup(placement: .primaryAction) {
            ProjectActionButtons(store: store, confirmingRemove: $confirmingRemove)
        }
    }
}

/// The toolbar's buttons as a view, so they can also be shown and tested outside a window toolbar.
///
/// Only the main action, Start Feature, shows its title; the secondary actions are icons whose help names them
/// (the title stays the accessibility label and the overflow-menu entry), so no label truncates at the default
/// window width.
struct ProjectActionButtons: View {
    let store: ProjectStore
    @Binding var confirmingRemove: Bool

    @Environment(AppModel.self) private var model
    @State private var checkProblem: String?

    private var canRun: Bool { store.rootExists && model.environment.identity != nil && !isNotSetUp }

    /// The project isn't set up: the main area offers only Set Up, and these actions could only fail.
    private var isNotSetUp: Bool {
        if store.detect?.initialized == false { return true }
        if case .failed(.projectInvalid(.notInitialized), _) = store.loadState { return true }
        return false
    }

    private func help(_ text: String) -> String {
        isNotSetUp ? "Set up BranchBox in this project first" : text
    }

    var body: some View {
        Group {
            Menu {
                Button("Repair Setup…") { model.post(.initProject(store.ref.root, mode: .repair)) }
                Button("Check Setup", action: checkSetup)
                Divider()
                Button("Reveal in Finder") { ProjectActions.reveal(store.ref.path) }
                Divider()
                Button("Remove from Sidebar…", role: .destructive) { confirmingRemove = true }
            } label: {
                Label("More", systemImage: "ellipsis.circle")
                    .labelStyle(.iconOnly)
            }
            .fixedSize()
            .help(checkProblem ?? "More project actions")
            Button {
                model.post(.projectSettings(store.ref))
            } label: {
                Label("Project Settings", systemImage: "gearshape")
                    .labelStyle(.iconOnly)
            }
            .help("Project Settings")
            .disabled(!store.rootExists)
            Button {
                model.post(.syncDevcontainers(store.ref))
            } label: {
                Label("Update All Workspaces", systemImage: "arrow.triangle.2.circlepath")
                    .labelStyle(.iconOnly)
            }
            .help(help("Update All Workspaces: copy main's dev container setup to every active feature"))
            .disabled(!canRun)
            Button {
                model.post(.prune(store.ref))
            } label: {
                Label("Prune", systemImage: "scissors")
                    .labelStyle(.iconOnly)
            }
            .help(help("Prune: tear down several finished features at once"))
            .disabled(!canRun || store.features.isEmpty)
            Button {
                model.post(.startFeature(project: store.ref, prefill: nil))
            } label: {
                Label("Start Feature", systemImage: "plus")
                    .labelStyle(.titleAndIcon)
            }
            .fixedSize()
            .help(help("Start a feature in \(store.displayName)"))
            .disabled(!canRun)
            .accessibilityIdentifier("project.startFeature")
        }
    }

    /// `init --validate`: changes nothing; the result shows in Activity.
    private func checkSetup() {
        checkProblem = nil
        switch model.actions.dispatch(.initProject(InitRequest(folder: store.ref.root, mode: .validate))) {
        case .started(let record), .queued(let record, _):
            model.post(.showActivity(operation: record.id))
        case .rejected(let reason):
            checkProblem = reason
        case .unavailable(let error):
            checkProblem = error.presentation().message
        }
    }
}

#Preview("Project") {
    ProjectDetailView(project: PreviewSamples.project)
        .environment(ProjectsPreviewModel.model())
        .frame(width: 820, height: 640)
}
