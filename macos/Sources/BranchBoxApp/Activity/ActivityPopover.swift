import BranchBoxKit
import BranchBoxStores
import SwiftUI

/// The toolbar's Activity popover: what is running, then the most recent finished operations. A row opens the
/// Activity window on that operation; [Show All] opens it as it was.
struct ActivityPopover: View {
    /// Finished operations listed under the running ones.
    static let recentLimit = 5

    @Environment(AppModel.self) private var model
    @Environment(\.openWindow) private var openWindow
    var onOpen: () -> Void = {}

    var body: some View {
        let entries = ActivityEntry.all(in: model.operations)
        let running = entries.filter { $0.filterState == .running }
        let recent = Array(entries.filter { $0.filterState != .running }.prefix(Self.recentLimit))
        VStack(alignment: .leading, spacing: 0) {
            HStack {
                Text("Activity").font(.headline)
                Spacer()
                if !running.isEmpty {
                    Text("\(running.count) running")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
            }
            .padding(.horizontal, 14)
            .padding(.top, 12)
            .padding(.bottom, 8)
            if entries.isEmpty {
                Text("Nothing has run yet.")
                    .foregroundStyle(.secondary)
                    .frame(maxWidth: .infinity, minHeight: 80)
            } else {
                if !running.isEmpty {
                    section("Running", running)
                }
                if !recent.isEmpty {
                    section("Recent", recent)
                }
            }
            Divider()
            HStack {
                Spacer()
                Button("Show All") { show(nil) }
                    .keyboardShortcut(.defaultAction)
            }
            .padding(10)
        }
        .frame(width: 360)
    }

    private func section(_ title: String, _ entries: [ActivityEntry]) -> some View {
        VStack(alignment: .leading, spacing: 2) {
            Text(title)
                .font(.caption.weight(.semibold))
                .foregroundStyle(.secondary)
                .padding(.horizontal, 14)
                .padding(.top, 4)
            ForEach(entries) { entry in
                Button {
                    show(entry.id)
                } label: {
                    Group {
                        switch entry.source {
                        case .live(let record): OperationRow(record: record)
                        case .past(let summary): OperationRow(summary: OperationSummary(history: summary))
                        }
                    }
                    .padding(.horizontal, 14)
                    .padding(.vertical, 5)
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
            }
        }
        .padding(.bottom, 8)
    }

    private func show(_ operation: UUID?) {
        if let operation { ActivitySelection.select(operation) }
        openWindow(id: SceneID.activity)
        onOpen()
    }
}
