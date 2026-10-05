import BranchBoxKit
import BranchBoxStores
import Foundation
import Observation

/// One main window's navigation state: the sidebar selection, the one sheet it presents, the Quick Open overlay
/// and the inspector (DESIGN §4.11).
///
/// Everything that wants the window to show something posts a `WindowIntent` (`AppModel.post(_:)`); the window
/// hands it to `handle(_:)` when `intentToken` changes and when it appears, so an intent posted while the window
/// was closed is performed once it opens. Selections apply at once. Sheets and the palette never stack: while
/// one is up, later intents wait in `queue` and `promoteNext()` (the sheet's `onDismiss`, the palette's close)
/// performs them in order.
@MainActor @Observable final class PresentationRouter {
    /// The presented sheet; the window binds its one sheet modifier to it.
    var sheet: SheetRoute?
    /// The Quick Open overlay is showing (it is an overlay, never a sheet).
    private(set) var isQuickOpenPresented = false
    var selection: SidebarSelection?
    var isInspectorPresented = false
    /// Intents that arrived while a sheet or the palette was up, oldest first.
    private(set) var queue: [WindowIntent] = []

    /// Opens another scene (Activity, Diagnostics); set by the window from its `openWindow`.
    @ObservationIgnored var openWindow: ((String) -> Void)?
    /// Looks up an operation for `.showActivity(operation:)`.
    @ObservationIgnored var operation: ((UUID) -> OperationRecord?)?

    init(selection: SidebarSelection? = nil) {
        self.selection = selection
    }

    /// The intent performed after the current sheet or palette goes away.
    var next: WindowIntent? { queue.first }

    /// A sheet or the palette is up; sheet and palette intents queue meanwhile.
    var isPresenting: Bool { sheet != nil || isQuickOpenPresented }

    /// Takes the model's pending intent, if any, and performs it.
    func consume(from model: AppModel) {
        if let intent = model.takePendingIntent() { handle(intent) }
    }

    func handle(_ intent: WindowIntent) {
        switch intent {
        case .select(let selection):
            self.selection = selection
        case .showDiagnostics:
            openWindow?(SceneID.diagnostics)
        case .showActivity(let id):
            showActivity(id)
        case .quickOpen:
            if isQuickOpenPresented { return }
            if sheet != nil { enqueue(intent) } else { isQuickOpenPresented = true }
        default:
            guard let route = SheetRoute(intent) else { return }
            if route == sheet { return }                          // already showing exactly this
            if isPresenting { enqueue(intent) } else { sheet = route }
        }
    }

    /// Call when the sheet was dismissed: performs queued intents until one presents something.
    func promoteNext() {
        while !isPresenting, !queue.isEmpty {
            handle(queue.removeFirst())
        }
    }

    /// Shows the palette (⌘K); queued behind a sheet if one is up.
    func showQuickOpen() {
        handle(.quickOpen)
    }

    /// Hides the palette, performs anything that queued behind it, then `intent` (the chosen item), so the item's
    /// sheet is presented only after the palette is gone.
    func closeQuickOpen(then intent: WindowIntent? = nil) {
        guard isQuickOpenPresented else { return }
        isQuickOpenPresented = false
        promoteNext()
        if let intent { handle(intent) }
    }

    // MARK: Private

    private func enqueue(_ intent: WindowIntent) {
        if queue.contains(intent) { return }
        queue.append(intent)
    }

    /// An operation on a feature or project selects it and opens the inspector, which lists its operations; any
    /// other (or none) opens the Activity window.
    private func showActivity(_ id: UUID?) {
        guard let id, let record = operation?(id) else {
            // A past operation (or none): the Activity window shows it from the persisted history.
            if let id { ActivitySelection.select(id) }
            openWindow?(SceneID.activity)
            return
        }
        record.acknowledge()
        switch record.target {
        case .feature(let feature):
            selection = .feature(projectPath: feature.project.path, name: feature.name)
            isInspectorPresented = true
        case .project(let project):
            selection = .project(path: project.path)
            isInspectorPresented = true
        case .global:
            ActivitySelection.select(id)
            openWindow?(SceneID.activity)
        }
    }

    // MARK: Selection persistence

    /// The selection as the JSON stored in `@SceneStorage("selection")`; "" for none.
    static func encode(_ selection: SidebarSelection?) -> String {
        guard let selection, let data = try? JSONEncoder().encode(selection) else { return "" }
        return String(decoding: data, as: UTF8.self)
    }

    static func decode(_ stored: String) -> SidebarSelection? {
        guard !stored.isEmpty else { return nil }
        return try? JSONDecoder().decode(SidebarSelection.self, from: Data(stored.utf8))
    }
}

extension SidebarSelection {
    /// The project this selection belongs to.
    var projectPath: String? {
        switch self {
        case .welcome: nil
        case .project(let path): path
        case .feature(let projectPath, _), .stray(let projectPath, _): projectPath
        }
    }

    var projectRef: ProjectRef? {
        projectPath.map { ProjectRef(root: URL(fileURLWithPath: $0, isDirectory: true)) }
    }

    var featureRef: FeatureRef? {
        guard case .feature(let projectPath, let name) = self else { return nil }
        return FeatureRef(project: ProjectRef(root: URL(fileURLWithPath: projectPath, isDirectory: true)), name: name)
    }

    /// The inspector's operations: the selected feature's or project's.
    var operationTarget: OperationTarget? {
        switch self {
        case .feature: featureRef.map { .feature($0) }
        case .project, .stray: projectRef.map { .project($0) }
        case .welcome: nil
        }
    }
}
