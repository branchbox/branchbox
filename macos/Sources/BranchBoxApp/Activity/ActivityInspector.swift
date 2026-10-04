import BranchBoxKit
import BranchBoxStores
import SwiftUI

/// The main window's inspector: operations for the selected feature or project, newest first. A row expands into
/// the full progress view with its live log; the running operation starts expanded.
struct ActivityInspector: View {
    let target: OperationTarget?

    @Environment(AppModel.self) private var model
    @Environment(\.openWindow) private var openWindow
    @State private var expanded: UUID?

    init(target: OperationTarget?) {
        self.target = target
    }

    var body: some View {
        let entries = self.entries
        VStack(spacing: 0) {
            header
            Divider()
            if entries.isEmpty {
                ContentUnavailableView {
                    Label("No activity yet", systemImage: "clock.arrow.circlepath")
                } description: {
                    Text(emptyText)
                }
                .frame(maxHeight: .infinity)
            } else {
                ScrollView {
                    LazyVStack(alignment: .leading, spacing: 8) {
                        ForEach(entries) { entry in
                            card(entry)
                        }
                    }
                    .padding(12)
                }
            }
            Divider()
            HStack {
                Button("Show All Activity") { openWindow(id: SceneID.activity) }
                    .buttonStyle(.link)
                Spacer()
            }
            .padding(.horizontal, 12)
            .padding(.vertical, 8)
        }
        .frame(minWidth: 280, idealWidth: 340)
        .onAppear {
            if expanded == nil { expanded = entries.first { $0.filterState == .running }?.id }
        }
    }

    private var entries: [ActivityEntry] {
        let all = ActivityEntry.all(in: model.operations)
        guard let target else { return all }
        return all.filter { $0.concerns(target) }
    }

    private var header: some View {
        HStack(spacing: 8) {
            VStack(alignment: .leading, spacing: 1) {
                Text("Activity").font(.headline)
                Text(scopeText)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
                    .truncationMode(.middle)
            }
            Spacer()
            let running = entries.filter { $0.filterState == .running }.count
            if running > 0 {
                Label("\(running) running", systemImage: "circle.dotted")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 10)
    }

    private var scopeText: String {
        switch target {
        case .feature(let feature)?: "\(feature.name) · \(projectName(feature.project))"
        case .project(let project)?: "Everything in \(projectName(project))"
        case .global?, nil: "All projects"
        }
    }

    private var emptyText: String {
        switch target {
        case .feature(let feature)?: "Starts, teardowns and commands for \(feature.name) show here."
        case .project(let project)?: "Operations in \(projectName(project)) show here."
        case .global?, nil: "Operations show here as they run."
        }
    }

    private func projectName(_ project: ProjectRef) -> String {
        model.projects.project(project)?.displayName ?? project.displayName
    }

    private func card(_ entry: ActivityEntry) -> some View {
        let isExpanded = expanded == entry.id
        return VStack(alignment: .leading, spacing: 8) {
            if isExpanded {
                Button {
                    withAnimation(.easeInOut(duration: 0.15)) { expanded = nil }
                } label: {
                    Label("Hide Log", systemImage: "chevron.down")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .frame(maxWidth: .infinity, alignment: .trailing)
                ActivityOperationDetail(entry: entry, model: model, logHeight: 240)
            } else {
                Button {
                    withAnimation(.easeInOut(duration: 0.15)) { expanded = entry.id }
                } label: {
                    HStack(spacing: 6) {
                        rowContent(entry)
                        Image(systemName: "chevron.right")
                            .font(.caption.weight(.semibold))
                            .foregroundStyle(.tertiary)
                            .accessibilityHidden(true)
                    }
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .accessibilityHint("Shows the log")
            }
        }
        .padding(10)
        .background(Color(nsColor: .controlBackgroundColor), in: RoundedRectangle(cornerRadius: 8))
        .overlay(RoundedRectangle(cornerRadius: 8)
            .strokeBorder(attention(entry) ? entry.snapshot.state.tint.color.opacity(0.6)
                                           : Color(nsColor: .separatorColor).opacity(0.5)))
    }

    @ViewBuilder private func rowContent(_ entry: ActivityEntry) -> some View {
        switch entry.source {
        case .live(let record): OperationRow(record: record)
        case .past(let summary): OperationRow(summary: OperationSummary(history: summary))
        }
    }

    private func attention(_ entry: ActivityEntry) -> Bool {
        if case .live(let record) = entry.source { return record.needsAttention }
        return false
    }
}
