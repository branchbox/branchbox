import BranchBoxKit
import BranchBoxPreview
import BranchBoxStores
import SwiftUI

/// A running (or finished) operation in full: title, elapsed time, phase, step progress, warnings and the live
/// log, with [Run in Background] and [Stop]. Stop asks first; on a CLI without `registry-lock` and
/// `write-ahead-start` the question warns about a partial worktree and registry corruption (D-18).
struct OperationProgressView: View {
    private enum Source {
        case record(OperationRecord)
        case snapshot(OperationSummary, lines: [LogLine], archiveURL: URL?)
    }

    private let source: Source
    private let capabilities: Set<Capability>
    private let onRunInBackground: (() -> Void)?
    private let onStop: (() -> Void)?
    @State private var confirmingStop = false

    /// `capabilities` are the backend's (`BackendIdentity.capabilities`); they choose the Stop warning.
    init(record: OperationRecord, capabilities: Set<Capability>, onRunInBackground: (() -> Void)? = nil,
         onStop: (() -> Void)? = nil) {
        self.source = .record(record)
        self.capabilities = capabilities
        self.onRunInBackground = onRunInBackground
        self.onStop = onStop
    }

    init(summary: OperationSummary, lines: [LogLine], archiveURL: URL? = nil, capabilities: Set<Capability>,
         onRunInBackground: (() -> Void)? = nil, onStop: (() -> Void)? = nil) {
        self.source = .snapshot(summary, lines: lines, archiveURL: archiveURL)
        self.capabilities = capabilities
        self.onRunInBackground = onRunInBackground
        self.onStop = onStop
    }

    var body: some View {
        let (summary, lines, archiveURL) = resolved
        let confirmation = CancelConfirmation(kind: summary.kind, title: summary.title, capabilities: capabilities)
        VStack(alignment: .leading, spacing: 12) {
            header(summary)
            if let step = summary.stepProgress, step.total > 0 {
                ProgressView(value: Double(min(step.completed, step.total)), total: Double(step.total)) {
                    Text("\(step.completed) of \(step.total) done")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
                .tint(.accentColor)
            }
            if !summary.warnings.isEmpty {
                VStack(alignment: .leading, spacing: 2) {
                    ForEach(Array(summary.warnings.enumerated()), id: \.offset) { _, warning in
                        Label(warning, systemImage: "exclamationmark.triangle")
                            .foregroundStyle(.orange)
                            .font(.callout)
                    }
                }
            }
            LogView(lines: lines, archiveURL: archiveURL, firstIndex: logPosition.firstIndex, revision: logPosition.revision)
                .frame(minHeight: 160)
                .background(.background, in: RoundedRectangle(cornerRadius: 6))
                .overlay(RoundedRectangle(cornerRadius: 6).strokeBorder(.separator))
            if summary.isRunning, onRunInBackground != nil || onStop != nil {
                HStack {
                    Spacer()
                    if let onRunInBackground {
                        Button("Run in Background", action: onRunInBackground)
                            .keyboardShortcut(.cancelAction)
                    }
                    if onStop != nil {
                        Button("Stop", role: .destructive) { confirmingStop = true }
                            .keyboardShortcut(".", modifiers: .command)
                    }
                }
            }
        }
        .confirmationDialog(confirmation.title, isPresented: $confirmingStop, titleVisibility: .visible) {
            Button(confirmation.stopLabel, role: .destructive) { onStop?() }
            Button(confirmation.keepLabel, role: .cancel) {}
        } message: {
            Text(confirmation.message)
        }
    }

    /// Where the record's lines sit in its whole log, so the log view follows a full ring buffer too.
    private var logPosition: (firstIndex: Int, revision: Int?) {
        switch source {
        case .record(let record): (record.log.droppedLines, record.log.revision)
        case .snapshot: (0, nil)
        }
    }

    private var resolved: (OperationSummary, [LogLine], URL?) {
        switch source {
        case .record(let record): (OperationSummary(record), record.log.lines, record.log.archiveURL)
        case .snapshot(let summary, let lines, let archiveURL): (summary, lines, archiveURL)
        }
    }

    private func header(_ summary: OperationSummary) -> some View {
        HStack(alignment: .firstTextBaseline, spacing: 10) {
            Image(systemName: summary.kind.symbol)
                .font(.title3)
                .foregroundStyle(.secondary)
                .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: 2) {
                Text(summary.title).font(.title3.weight(.semibold))
                if summary.isRunning {
                    TimelineView(.periodic(from: .now, by: 1)) { context in
                        statusLine(summary, now: context.date)
                    }
                } else {
                    statusLine(summary, now: summary.finishedAt ?? .now)
                }
            }
            Spacer()
            if case .queued = summary.state {
                Image(systemName: summary.state.symbol)
                    .foregroundStyle(.secondary)
                    .accessibilityLabel(summary.state.label)
            } else if summary.isRunning {
                ProgressView().controlSize(.small)
            } else {
                Image(systemName: summary.state.symbol)
                    .foregroundStyle(summary.state.tint.color)
                    .accessibilityLabel(summary.state.label)
            }
        }
        .accessibilityElement(children: .combine)
    }

    private func statusLine(_ summary: OperationSummary, now: Date) -> some View {
        Text(OperationPresentation.statusLine(summary, now: now))
            .font(.callout)
            .foregroundStyle(.secondary)
            .monospacedDigit()
    }
}

#Preview("Running on a legacy CLI") {
    OperationProgressView(summary: OperationPreviewData.running,
                          lines: (0..<12).map { LogLine(timestamp: .now, level: $0 == 7 ? .warn : .info, source: .stderr,
                                                        target: "worktree_core::modules::compose", message: "step \($0)") },
                          capabilities: [], onRunInBackground: {}, onStop: {})
        .frame(width: 560, height: 420)
        .padding()
}

#Preview("Prune on a contract CLI") {
    OperationProgressView(summary: OperationPreviewData.pruning, lines: [], capabilities: PreviewSamples.allCapabilities,
                          onRunInBackground: {}, onStop: {})
        .frame(width: 560, height: 360)
        .padding()
}

#Preview("Finished with warnings") {
    OperationProgressView(summary: OperationPreviewData.finished[1],
                          lines: [LogLine(timestamp: nil, level: .output, source: .stdout, target: nil, message: "done")],
                          capabilities: PreviewSamples.allCapabilities)
        .frame(width: 560, height: 320)
        .padding()
}
