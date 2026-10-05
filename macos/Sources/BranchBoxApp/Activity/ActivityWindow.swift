import AppKit
import BranchBoxKit
import BranchBoxStores
import SwiftUI

/// The `activity` window: every operation, master-detail, filtered by project and state. The detail is the
/// operation's progress view (live log with auto-scroll, pause and Jump to Latest) and its outcome, with
/// [Reveal Log File] and [Copy Diagnostic Report].
struct ActivityWindow: View {
    @Environment(AppModel.self) private var model
    @AppStorage(ActivitySelection.key, store: ActivitySelection.defaults) private var selection = ""
    @State private var projectFilter = ""
    @State private var stateFilter = ActivityStateFilter.all

    init() {}

    var body: some View {
        let entries = filtered
        HStack(spacing: 0) {
            VStack(spacing: 0) {
                filters
                Divider()
                if entries.isEmpty {
                    ContentUnavailableView {
                        Label(ActivityEntry.all(in: model.operations).isEmpty ? "No operations yet" : "No matching operations",
                              systemImage: "clock.arrow.circlepath")
                    } description: {
                        Text(ActivityEntry.all(in: model.operations).isEmpty
                             ? "Starts, teardowns, prunes and commands show here."
                             : "Change the filters to see more.")
                    }
                    .frame(maxHeight: .infinity)
                } else {
                    ScrollView {
                        LazyVStack(spacing: 2) {
                            ForEach(entries) { entry in
                                row(entry)
                            }
                        }
                        .padding(8)
                    }
                }
            }
            .frame(width: 320)
            Divider()
            detail(entries)
                .frame(maxWidth: .infinity, maxHeight: .infinity)
                .background(Color(nsColor: .textBackgroundColor))
        }
        .frame(minWidth: 860, minHeight: 520)
    }

    // MARK: Master

    private var filters: some View {
        HStack(spacing: 8) {
            Picker("Project", selection: $projectFilter) {
                Text("All Projects").tag("")
                Divider()
                ForEach(model.projects.projects) { project in
                    Text(project.displayName).tag(project.ref.path)
                }
            }
            .labelsHidden()
            .help("Show operations of one project")
            Picker("State", selection: $stateFilter) {
                ForEach(ActivityStateFilter.allCases) { Text($0.title).tag($0) }
            }
            .labelsHidden()
            .help("Show operations in one state")
        }
        .padding(12)
    }

    private var filtered: [ActivityEntry] {
        ActivityEntry.all(in: model.operations).filter { entry in
            (projectFilter.isEmpty || entry.projectPath == projectFilter)
                && (stateFilter == .all || entry.filterState == stateFilter)
        }
    }

    private func row(_ entry: ActivityEntry) -> some View {
        let isSelected = selectedID(in: filtered) == entry.id
        return Button {
            selection = entry.id.uuidString
        } label: {
            Group {
                switch entry.source {
                case .live(let record): OperationRow(record: record)
                case .past(let summary): OperationRow(summary: OperationSummary(history: summary))
                }
            }
            .padding(.horizontal, 8)
            .padding(.vertical, 6)
            .background(isSelected ? Color.accentColor.opacity(0.16) : Color.clear, in: RoundedRectangle(cornerRadius: 6))
            .overlay(alignment: .leading) {
                if case .live(let record) = entry.source, record.needsAttention {
                    Circle().fill(.red).frame(width: 6, height: 6).offset(x: -2).accessibilityLabel("Needs attention")
                }
            }
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityAddTraits(isSelected ? [.isSelected] : [])
    }

    /// The chosen operation if it is listed, else the newest listed one.
    private func selectedID(in entries: [ActivityEntry]) -> UUID? {
        if let id = UUID(uuidString: selection), entries.contains(where: { $0.id == id }) { return id }
        return entries.first?.id
    }

    // MARK: Detail

    @ViewBuilder private func detail(_ entries: [ActivityEntry]) -> some View {
        if let id = selectedID(in: entries), let entry = entries.first(where: { $0.id == id }) {
            VStack(spacing: 0) {
                detailHeader(entry)
                Divider()
                ScrollView {
                    ActivityOperationDetail(entry: entry, model: model, logHeight: entry.filterState == .running ? 380 : 240)
                        .padding(16)
                }
            }
        } else {
            ContentUnavailableView("Select an operation", systemImage: "sidebar.left")
        }
    }

    private func detailHeader(_ entry: ActivityEntry) -> some View {
        HStack(spacing: 8) {
            Text(targetText(entry))
                .font(.callout)
                .foregroundStyle(.secondary)
                .lineLimit(1)
                .truncationMode(.middle)
            Spacer()
            if let url = logURL(entry) {
                Button {
                    NSWorkspace.shared.activateFileViewerSelecting([url])
                } label: {
                    Label("Reveal Log File", systemImage: "doc.text.magnifyingglass")
                }
            }
            CopyButton(label: "Copy Diagnostic Report", showsTitle: true) { report(for: entry) }
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 10)
    }

    private func targetText(_ entry: ActivityEntry) -> String {
        let started = entry.startedAt.formatted(date: .abbreviated, time: .shortened)
        guard let path = entry.projectPath else { return "Started \(started)" }
        let name = model.projects.projects.first { $0.ref.path == path }?.displayName
            ?? URL(fileURLWithPath: path).lastPathComponent
        return "\(name) · started \(started)"
    }

    private func logURL(_ entry: ActivityEntry) -> URL? {
        switch entry.source {
        case .live(let record): record.log.archiveURL
        case .past(let summary): summary.logPath.map(URL.init(fileURLWithPath:))
        }
    }

    private func report(for entry: ActivityEntry) -> String {
        switch entry.source {
        case .live(let record):
            return FlowActions(model: model).diagnosticReport(for: record)
        case .past(let summary):
            let error: BackendError? = summary.outcome == .failed
                ? .commandFailed(Diagnostics(summary: summary.detail ?? "Failed")) : nil
            return DiagnosticReport(identity: model.environment.identity, environment: model.environment.summary,
                                    operationTitle: summary.title, error: error,
                                    secrets: Array(model.settings.extraEnvironment.values)).markdown()
        }
    }
}
