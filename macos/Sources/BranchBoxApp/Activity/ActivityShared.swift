import AppKit
import BranchBoxKit
import BranchBoxStores
import SwiftUI

/// One line in Activity: an operation of this session (live, observed) or one from the persisted history.
struct ActivityEntry: Identifiable {
    enum Source {
        case live(OperationRecord)
        case past(BranchBoxStores.OperationSummary)
    }

    let id: UUID
    let source: Source

    @MainActor static func live(_ record: OperationRecord) -> ActivityEntry {
        ActivityEntry(id: record.id, source: .live(record))
    }

    static func past(_ summary: BranchBoxStores.OperationSummary) -> ActivityEntry {
        ActivityEntry(id: summary.id, source: .past(summary))
    }

    /// This session's records first-hand, then earlier history entries that aren't records, newest first.
    @MainActor static func all(in operations: OperationStore) -> [ActivityEntry] {
        let live = operations.records
        let ids = Set(live.map(\.id))
        let past = operations.history.filter { !ids.contains($0.id) }
        let entries = live.map(ActivityEntry.live) + past.map(ActivityEntry.past)
        return entries.sorted { $0.startedAt > $1.startedAt }
    }

    @MainActor var startedAt: Date {
        switch source {
        case .live(let record): record.startedAt
        case .past(let summary): summary.startedAt
        }
    }

    @MainActor var projectPath: String? {
        switch source {
        case .live(let record): record.target.project?.path
        case .past(let summary): summary.projectPath
        }
    }

    /// Whether the entry is about `target`: a feature's own operations, or everything in a project.
    @MainActor func concerns(_ target: OperationTarget) -> Bool {
        switch (source, target) {
        case (.live(let record), .feature(let feature)): record.target == .feature(feature)
        case (.past(let summary), .feature(let feature)):
            summary.projectPath == feature.project.path && summary.feature == feature.name
        case (_, .project(let project)): projectPath == project.path
        case (.live(let record), .global): record.target == .global
        case (.past(let summary), .global): summary.projectPath == nil
        }
    }

    /// A value snapshot for rows and the progress view.
    @MainActor var snapshot: OperationSummary {
        switch source {
        case .live(let record): OperationSummary(record)
        case .past(let summary): OperationSummary(history: summary)
        }
    }

    @MainActor var filterState: ActivityStateFilter {
        switch snapshot.state {
        case .queued, .running: .running
        case .failed, .partial: .problems
        case .succeeded, .succeededWithWarnings: .succeeded
        case .cancelled: .stopped
        }
    }
}

/// The Activity window's state filter.
enum ActivityStateFilter: String, CaseIterable, Identifiable {
    case all, running, problems, succeeded, stopped
    var id: String { rawValue }

    var title: String {
        switch self {
        case .all: "All"
        case .running: "Running"
        case .problems: "Failed or Partial"
        case .succeeded: "Succeeded"
        case .stopped: "Stopped"
        }
    }
}

extension OperationSummary {
    /// A persisted history entry as a row snapshot. The history keeps the failure's one-line cause only.
    init(history summary: BranchBoxStores.OperationSummary) {
        let state: OperationState = switch summary.outcome {
        case .succeeded: .succeeded
        case .succeededWithWarnings: .succeededWithWarnings
        case .partial: .partial
        case .failed: .failed(.commandFailed(Diagnostics(summary: summary.detail ?? "Failed")))
        case .cancelled: .cancelled(note: summary.detail)
        }
        self.init(id: summary.id, kind: summary.kind, title: summary.title, state: state, startedAt: summary.startedAt,
                  finishedAt: summary.finishedAt)
    }
}

/// Reads an operation's log file (`LogArchive`) back into lines for a past operation's log view.
enum ArchivedLog {
    /// The last `limit` lines of the file; header and footer comments become app lines.
    static func lines(at path: String, limit: Int = 5_000) -> [LogLine] {
        guard let data = FileManager.default.contents(atPath: path) else { return [] }
        let text = String(decoding: data, as: UTF8.self)
        let rows = text.split(separator: "\n", omittingEmptySubsequences: true).suffix(limit)
        return rows.map { parse(String($0)) }
    }

    /// "2026-10-02T15:30:45Z INFO   stderr target: message" (`LogArchive.format`), or a "# comment".
    static func parse(_ row: String) -> LogLine {
        if row.hasPrefix("#") {
            return LogLine(timestamp: nil, level: .info, source: .app, target: nil,
                           message: String(row.dropFirst()).trimmingCharacters(in: .whitespaces))
        }
        let parts = row.split(separator: " ", maxSplits: 1, omittingEmptySubsequences: false)
        guard parts.count == 2, let date = RFC3339.parse(String(parts[0])) else {
            return LogLine(timestamp: nil, level: .output, source: .stdout, target: nil, message: row)
        }
        var rest = Substring(parts[1])
        let levelText = rest.prefix(6).trimmingCharacters(in: .whitespaces).lowercased()
        let level = LogLevel(rawValue: levelText) ?? .output
        rest = rest.dropFirst(min(7, rest.count))
        let sourceAndMessage = rest.split(separator: " ", maxSplits: 1, omittingEmptySubsequences: false)
        let source = LogLine.Source(rawValue: String(sourceAndMessage.first ?? "")) ?? .stdout
        let message = sourceAndMessage.count > 1 ? String(sourceAndMessage[1]) : ""
        return LogLine(timestamp: date, level: level, source: source, target: nil, message: message)
    }
}

/// One operation in full for the inspector and the Activity window: the progress view with its live log (Stop is
/// confirmed per D-18), then the outcome: the cause card with its recoveries, or the success line with warnings.
/// Viewing a failed or partial operation acknowledges it.
struct ActivityOperationDetail: View {
    let entry: ActivityEntry
    let model: AppModel
    var logHeight: CGFloat = 280
    @Environment(\.openWindow) private var openWindow
    @State private var archived: [LogLine] = []
    @State private var notice: String?

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            switch entry.source {
            case .live(let record):
                OperationProgressView(record: record, capabilities: capabilities,
                                      onStop: record.isCancellable ? { model.operations.cancel(record.id) } : nil)
                    .frame(minHeight: logHeight)
                outcome(record)
            case .past(let summary):
                OperationProgressView(summary: OperationSummary(history: summary), lines: archived,
                                      archiveURL: summary.logPath.map(URL.init(fileURLWithPath:)), capabilities: capabilities)
                    .frame(minHeight: logHeight)
                if summary.outcome == .failed || summary.outcome == .cancelled, let detail = summary.detail {
                    FlowNotice(style: summary.outcome == .failed ? .error : .info, text: detail)
                }
            }
            if let notice {
                FlowNotice(style: .error, text: notice)
            }
        }
        .onAppear(perform: acknowledge)
        .onChange(of: entry.id) { acknowledge() }
        .onChange(of: liveNeedsAttention) { _, needs in if needs { acknowledge() } }   // failed while on screen
        .task(id: entry.id) {
            if case .past(let summary) = entry.source, let path = summary.logPath {
                archived = await Task.detached { ArchivedLog.lines(at: path) }.value
            } else {
                archived = []
            }
        }
    }

    private var liveNeedsAttention: Bool {
        if case .live(let record) = entry.source { return record.needsAttention }
        return false
    }

    private var capabilities: Set<Capability> { model.environment.identity?.capabilities ?? [] }

    private var actions: FlowActions { FlowActions(model: model, openWindow: { openWindow(id: $0) }) }

    @ViewBuilder private func outcome(_ record: OperationRecord) -> some View {
        if let error = record.failure {
            ResultCard(error: error, context: record.context, operationID: nil,
                       diagnosticReport: { actions.diagnosticReport(for: record) }) { action in
                Task {
                    if case .failure(let failure)? = await actions.perform(action) { notice = failure.message }
                }
            }
        } else if record.state == .succeededWithWarnings || record.state == .partial {
            ResultCard(successTitle: OperationSummary(record).state.label, warnings: record.warnings)
        }
    }

    private func acknowledge() {
        if case .live(let record) = entry.source, record.needsAttention { record.acknowledge() }
    }
}
