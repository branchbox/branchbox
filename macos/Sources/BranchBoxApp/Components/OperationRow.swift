import BranchBoxKit
import BranchBoxPreview
import BranchBoxStores
import SwiftUI

/// One operation in Activity lists and popovers: its kind, title, state, phase and elapsed time, with a spinner
/// while it runs. Built from a live `OperationRecord` (observed) or a value snapshot.
struct OperationRow: View {
    private enum Source {
        case record(OperationRecord)
        case summary(OperationSummary)
    }

    private let source: Source

    init(record: OperationRecord) {
        source = .record(record)
    }

    init(summary: OperationSummary) {
        source = .summary(summary)
    }

    var body: some View {
        let summary = switch source {
        case .record(let record): OperationSummary(record)
        case .summary(let summary): summary
        }
        Group {
            if summary.isRunning {
                TimelineView(.periodic(from: .now, by: 1)) { context in
                    content(summary, now: context.date)
                }
            } else {
                content(summary, now: summary.finishedAt ?? .now)
            }
        }
        .accessibilityElement(children: .combine)
    }

    private func content(_ summary: OperationSummary, now: Date) -> some View {
        HStack(spacing: 8) {
            Image(systemName: summary.kind.symbol)
                .foregroundStyle(.secondary)
                .frame(width: 18)
                .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: 1) {
                Text(summary.title)
                    .lineLimit(1)
                    .truncationMode(.middle)
                Text(OperationPresentation.statusLine(summary, now: now))
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .monospacedDigit()
                    .lineLimit(1)
            }
            Spacer(minLength: 4)
            if case .queued = summary.state {
                // Waiting is not working: a static clock, never a spinner (the subtitle says what it waits for).
                Image(systemName: summary.state.symbol)
                    .foregroundStyle(.secondary)
                    .accessibilityLabel(summary.state.label)
            } else if summary.isRunning {
                if let step = summary.stepProgress, step.total > 0 {
                    StepProgressBar(step: step)
                } else {
                    ProgressView().controlSize(.small)
                }
            } else {
                Image(systemName: summary.state.symbol)
                    .foregroundStyle(summary.state.tint.color)
                    .accessibilityLabel(summary.state.label)
            }
        }
    }
}

/// "n of m" beside an accent-tinted bar at least 120 pt wide, for operations that run in counted steps (prune).
struct StepProgressBar: View {
    let step: StepProgress

    var body: some View {
        HStack(spacing: 8) {
            Text("\(step.completed) of \(step.total)")
                .font(.caption)
                .monospacedDigit()
                .foregroundStyle(.secondary)
            ProgressView(value: Double(min(step.completed, step.total)), total: Double(step.total))
                .progressViewStyle(.linear)
                .tint(.accentColor)
                .frame(minWidth: 120, maxWidth: 160)
        }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel("\(step.completed) of \(step.total) done")
    }
}

enum OperationPreviewData {
    static let feature = FeatureRef(project: PreviewSamples.project, name: PreviewSamples.features[0].workFeature)

    static let running = OperationSummary(kind: .start, title: "Starting \(feature.name)", state: .running,
                                          startedAt: .now.addingTimeInterval(-42), phase: .module("compose"))
    static let queued = OperationSummary(kind: .teardown, title: "Tearing down remotion",
                                         state: .queued(behind: "Starting \(feature.name)"), startedAt: .now)
    static let pruning = OperationSummary(kind: .prune, title: "Pruning 4 features", state: .running,
                                          startedAt: .now.addingTimeInterval(-75),
                                          phase: .item(index: 2, of: 4, name: "remotion"),
                                          stepProgress: StepProgress(completed: 1, total: 4))
    static let finished: [OperationSummary] = [
        OperationSummary(kind: .teardown, title: "Tearing down milestone2", state: .succeeded,
                         startedAt: .now.addingTimeInterval(-600), finishedAt: .now.addingTimeInterval(-590)),
        OperationSummary(kind: .start, title: "Starting sbx-demo", state: .succeededWithWarnings,
                         startedAt: .now.addingTimeInterval(-3_700), finishedAt: .now.addingTimeInterval(-3_520),
                         warnings: ["Adapter: no service URL detected"]),
        OperationSummary(kind: .teardown, title: "Tearing down retained", state: .partial,
                         startedAt: .now.addingTimeInterval(-300), finishedAt: .now.addingTimeInterval(-280)),
        OperationSummary(kind: .devcontainerUp, title: "Starting the dev container for prine",
                         state: .failed(.commandFailed(Diagnostics(summary: "Docker is not available"))),
                         startedAt: .now.addingTimeInterval(-120), finishedAt: .now.addingTimeInterval(-118)),
        OperationSummary(kind: .exec, title: "Running make test in prine", state: .cancelled(note: nil),
                         startedAt: .now.addingTimeInterval(-60), finishedAt: .now.addingTimeInterval(-30)),
    ]
}

#Preview("Operation rows") {
    List {
        OperationRow(summary: OperationPreviewData.running)
        OperationRow(summary: OperationPreviewData.queued)
        OperationRow(summary: OperationPreviewData.pruning)
        ForEach(OperationPreviewData.finished) { OperationRow(summary: $0) }
    }
    .frame(width: 380, height: 360)
}
