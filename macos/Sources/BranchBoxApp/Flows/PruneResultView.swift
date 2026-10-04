import BranchBoxKit
import BranchBoxStores
import SwiftUI

/// A finished (or stopped) prune: "5 torn down · 1 partial · 0 failed", then one expandable row per feature with
/// what happened to it. A refused row shows its cause and the recoveries for that feature's own teardown.
struct PruneResultView: View {
    let flow: PruneFlow
    let result: PruneResult
    let record: OperationRecord
    let actions: FlowActions
    let onDone: () -> Void
    @State private var expanded: Set<String> = []
    @State private var retried: [String: OperationRecord] = [:]
    @State private var retryError: String?

    var body: some View {
        let counts = Self.Counts(result)
        FlowSheetLayout(title: title(counts), subtitle: Self.summaryLine(counts), systemImage: counts.symbol,
                        tint: counts.tint) {
            if case .cancelled(let note) = record.state, let note {
                FlowNotice(style: .warning, text: note)
            }
            VStack(spacing: 0) {
                ForEach(Array(result.rows.enumerated()), id: \.element.feature) { index, row in
                    rowView(row)
                    if index < result.rows.count - 1 { Divider() }
                }
            }
            .background(Color(nsColor: .controlBackgroundColor), in: RoundedRectangle(cornerRadius: 8))
            .overlay(RoundedRectangle(cornerRadius: 8).strokeBorder(Color(nsColor: .separatorColor).opacity(0.6)))
            if let retryError {
                FlowNotice(style: .error, text: retryError)
            }
        } footer: {
            Button("Show Log") { Task { _ = await actions.perform(.showLog(operation: record.id)) } }
            Spacer()
            Button("Done", action: onDone)
                .keyboardShortcut(.defaultAction)
                .buttonStyle(.borderedProminent)
        }
        .onAppear {
            // Rows that need a look start expanded.
            expanded = Set(result.rows.filter { Self.needsLook($0.outcome) }.map(\.feature))
        }
    }

    private func title(_ counts: Counts) -> String {
        if case .cancelled = record.state { return "Prune stopped" }
        return counts.problems == 0 ? "Pruned \(flow.projectName)" : "Pruned \(flow.projectName), with problems"
    }

    private func rowView(_ row: PruneRow) -> some View {
        DisclosureGroup(isExpanded: Binding(
            get: { expanded.contains(row.feature) },
            set: { if $0 { expanded.insert(row.feature) } else { expanded.remove(row.feature) } }
        )) {
            VStack(alignment: .leading, spacing: 8) {
                detail(row)
            }
            .padding(.vertical, 6)
            .padding(.leading, 4)
        } label: {
            HStack(spacing: 8) {
                Image(systemName: Self.symbol(for: row.outcome))
                    .foregroundStyle(Self.tint(for: row.outcome))
                    .frame(width: 16)
                    .accessibilityHidden(true)
                Text(row.feature).fontWeight(.medium)
                Spacer()
                Text(Self.label(for: row.outcome))
                    .font(.callout)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
            }
            .accessibilityElement(children: .combine)
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 8)
    }

    @ViewBuilder private func detail(_ row: PruneRow) -> some View {
        switch row.outcome {
        case .removed(let outcome):
            FlowFactRow(systemImage: outcome.worktreeGone ? "checkmark.circle.fill" : "xmark.octagon.fill",
                        tint: outcome.worktreeGone ? .green : .red,
                        text: outcome.worktreeGone ? "Worktree removed" : "The folder is still there")
            FlowFactRow(systemImage: "arrow.triangle.branch", text: Self.branchText(outcome.branch))
            if !outcome.summary.discardedChanges.isEmpty {
                FlowFactRow(systemImage: "trash", tint: .orange,
                            text: "Discarded \(Pluralized.count(outcome.summary.discardedChanges.count, "change"))",
                            detail: outcome.summary.discardedChanges.map(\.path).joined(separator: ", "))
            }
            ForEach(outcome.summary.warnings, id: \.self) { warning in
                FlowFactRow(systemImage: "exclamationmark.triangle.fill", tint: .orange, text: warning)
            }
        case .refused(let error), .failed(let error):
            if let retry = retried[row.feature] {
                OperationRow(record: retry)
                if let failure = retry.failure {
                    // Refused again (say, a new file appeared): show why, with its own recoveries.
                    ResultCard(error: failure, context: retry.context, operationID: retry.id) { action in
                        Task {
                            switch await actions.perform(action) {
                            case .success(let record)?: retried[row.feature] = record
                            case .failure(let failure)?: retryError = failure.message
                            case nil: break
                            }
                        }
                    }
                }
            } else {
                ResultCard(error: error, context: flow.request(for: row.feature).map(OperationRequestContext.teardown)) { action in
                    Task {
                        switch await actions.perform(action) {
                        case .success(let record)?: retried[row.feature] = record
                        case .failure(let failure)?: retryError = failure.message
                        case nil: break
                        }
                    }
                }
            }
        case .skipped(let reason):
            FlowFactRow(systemImage: "forward", text: reason)
        case .cancelled:
            FlowFactRow(systemImage: "stop.circle", text: "Stopped while tearing down; it may be partly removed",
                        detail: "Check it in the sidebar.")
        }
    }

    // MARK: Counting and wording

    /// Rows by outcome. A removed row whose branch couldn't be deleted, or whose folder is still there, is partial.
    struct Counts: Equatable {
        var tornDown = 0, partial = 0, refused = 0, failed = 0, notStarted = 0

        init(_ result: PruneResult) {
            for row in result.rows {
                switch row.outcome {
                case .removed(let outcome):
                    if PruneResultView.isPartial(outcome) { partial += 1 } else { tornDown += 1 }
                case .refused: refused += 1
                case .failed: failed += 1
                case .skipped, .cancelled: notStarted += 1
                }
            }
        }

        var problems: Int { partial + refused + failed + notStarted }

        var symbol: String {
            if failed > 0 { return "xmark.octagon.fill" }
            return problems == 0 ? "checkmark.circle.fill" : "exclamationmark.circle.fill"
        }

        var tint: Color {
            if failed > 0 { return .red }
            return problems == 0 ? .green : .orange
        }
    }

    /// "5 torn down · 1 partial · 0 failed", plus refused and not-started counts when there are any.
    nonisolated static func summaryLine(_ counts: Counts) -> String {
        var parts = ["\(counts.tornDown) torn down", "\(counts.partial) partial"]
        if counts.refused > 0 { parts.append("\(counts.refused) refused") }
        parts.append("\(counts.failed) failed")
        if counts.notStarted > 0 { parts.append("\(counts.notStarted) not started") }
        return parts.joined(separator: " · ")
    }

    nonisolated static func isPartial(_ outcome: TeardownOutcome) -> Bool {
        if !outcome.worktreeGone { return true }
        if case .deleteFailed = outcome.branch { return true }
        return false
    }

    nonisolated static func needsLook(_ outcome: PruneRowOutcome) -> Bool {
        switch outcome {
        case .removed(let outcome): isPartial(outcome)
        case .refused, .failed, .cancelled: true
        case .skipped: false
        }
    }

    nonisolated static func label(for outcome: PruneRowOutcome) -> String {
        switch outcome {
        case .removed(let outcome): isPartial(outcome) ? "Partly torn down" : "Torn down"
        case .refused(let error): refusalLabel(error)
        case .failed(let error): error.presentation().title
        case .skipped: "Not started"
        case .cancelled: "Stopped"
        }
    }

    nonisolated private static func refusalLabel(_ error: BackendError) -> String {
        guard case .refused(let refusal) = error else { return error.presentation().title }
        switch refusal.cause {
        case .uncommittedChanges(let files): return "Skipped: \(Pluralized.count(files.count, "uncommitted change"))"
        case .unmergedBranch: return "Skipped: unmerged branch"
        case .worktreeLocked: return "Skipped: worktree locked"
        default: return "Skipped: " + error.presentation().title
        }
    }

    nonisolated static func symbol(for outcome: PruneRowOutcome) -> String {
        switch outcome {
        case .removed(let outcome): isPartial(outcome) ? "exclamationmark.circle.fill" : "checkmark.circle.fill"
        case .refused: "hand.raised.fill"
        case .failed: "xmark.octagon.fill"
        case .skipped: "forward"
        case .cancelled: "stop.circle"
        }
    }

    nonisolated static func tint(for outcome: PruneRowOutcome) -> Color {
        switch outcome {
        case .removed(let outcome): isPartial(outcome) ? .orange : .green
        case .refused: .orange
        case .failed: .red
        case .skipped, .cancelled: .secondary
        }
    }

    nonisolated static func branchText(_ branch: BranchOutcome) -> String {
        switch branch {
        case .kept(let name): "\(name ?? "Branch") kept"
        case .deleted(let name, let deleter): "\(name) deleted" + (deleter == .app ? " by BranchBox for Mac" : "")
        case .deleteFailed(let name, let reason): "\(name) couldn't be deleted: \(reason)"
        case .notFound(let name): "\(name) was already gone"
        }
    }
}
