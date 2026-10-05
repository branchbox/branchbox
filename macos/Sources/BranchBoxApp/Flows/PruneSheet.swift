import BranchBoxKit
import BranchBoxStores
import SwiftUI

/// Tears down a selection of a project's features, one after another.
///
/// Each row is checked for unsaved work first; the safe set is preselected, and every other row says why it
/// isn't. Rows run sequentially as ordinary teardowns (D-12); a refused row is skipped and reported while the
/// others continue.
struct PruneSheet: View {
    let project: ProjectRef

    @Environment(AppModel.self) private var model
    @State private var flow: PruneFlow?

    init(project: ProjectRef) {
        self.project = project
    }

    /// A sheet around an existing flow (previews and render tests).
    init(flow: PruneFlow) {
        self.project = flow.project
        _flow = State(initialValue: flow)
    }

    var body: some View {
        Group {
            if let flow {
                PruneContent(flow: flow)
            } else {
                ProgressView().frame(maxWidth: .infinity, maxHeight: .infinity)
            }
        }
        .frame(minWidth: 720, idealWidth: 780, minHeight: 480, idealHeight: 600)
        .onAppear {
            if flow == nil { flow = PruneFlow(model: model, project: project) }
        }
    }
}

private struct PruneContent: View {
    @Bindable var flow: PruneFlow
    @Environment(\.dismiss) private var dismiss
    @Environment(\.openWindow) private var openWindow

    var body: some View {
        Group {
            if let record = flow.record {
                if let result = flow.result, record.isFinished {
                    PruneResultView(flow: flow, result: result, record: record, actions: actions, onDone: { dismiss() })
                } else if record.isFinished {
                    // Stopped while queued (D-16 queues a prune behind a running mutation), or failed before any
                    // row ran: there is no per-row result to show.
                    finishedWithoutResult(record)
                } else {
                    running(record)
                }
            } else if flow.isEmpty {
                empty
            } else {
                selecting
            }
        }
        .stopConfirmation(flow.stop, model: flow.model)
        .task {
            // Rows already planned are skipped, so this also picks up rows whose earlier check was cancelled.
            await flow.loadPlans()
        }
        .onChange(of: flow.record?.isFinished == true) { _, finished in
            if finished, flow.record?.needsAttention == true { flow.record?.acknowledge() }
        }
    }

    private var actions: FlowActions {
        FlowActions(model: flow.model, openWindow: { openWindow(id: $0) })
    }

    private var empty: some View {
        FlowSheetLayout(title: "Prune \(flow.projectName)", systemImage: "scissors") {
            ContentUnavailableView {
                Label("Nothing to prune", systemImage: "checkmark.seal")
            } description: {
                Text("Every feature in \(flow.projectName) is already torn down.")
            }
            .frame(maxWidth: .infinity, minHeight: 320)
        } footer: {
            Spacer()
            Button("Close") { dismiss() }
                .keyboardShortcut(.defaultAction)
        }
    }

    // MARK: Selecting

    private var selecting: some View {
        FlowSheetLayout(title: "Prune \(flow.projectName)",
                        subtitle: "Tear down the features you're done with, one at a time.", systemImage: "scissors") {
            HStack(spacing: 8) {
                Button("Select Safe") { flow.selectSafe() }
                    .help("Select features that pass the reported-change and branch checks")
                Button("All") { flow.selectAll() }
                Button("None") { flow.selectNone() }
                if flow.isChecking {
                    ProgressView().controlSize(.small).padding(.leading, 4)
                    Text("Checking for unsaved work…").font(.callout).foregroundStyle(.secondary)
                } else if !flow.planErrors.isEmpty {
                    Button("Check Again") { Task { await flow.recheckFailed() } }
                        .help("Check the rows that couldn't be checked again")
                }
                Spacer()
                Picker("Branches", selection: Binding(get: { flow.policy }, set: { flow.setPolicy($0) })) {
                    Text("Keep").tag(BranchPolicy.keep)
                    Text("Delete if merged").tag(BranchPolicy.deleteIfMerged)
                    Text("Force-delete").tag(BranchPolicy.forceDelete)
                }
                .fixedSize()
                .help("What happens to each feature's branch")
            }
            PruneTable(flow: flow)
            FlowNotice(style: .info, text: "Git-ignored files are not listed; they are removed with each worktree even when you keep its branch.")
            if flow.policy == .deleteIfMerged {
                FlowNotice(style: .info, text: "Unmerged branches are kept, and their rows say so.")
            } else if flow.policy == .forceDelete {
                FlowNotice(style: .warning, text: "Force-delete deletes branches even with unmerged commits. You'll be "
                    + "asked to confirm.")
            }
            if let error = flow.dispatchError {
                FlowNotice(style: .error, text: error)
            }
        } footer: {
            Text(flow.selected.count == 1 ? "1 feature selected" : "\(flow.selected.count) features selected")
                .foregroundStyle(.secondary)
                .monospacedDigit()
            Spacer()
            Button("Cancel") { dismiss() }
                .keyboardShortcut(.cancelAction)
            Button(flow.selected.count == 1 ? "Tear Down 1 Feature" : "Tear Down \(flow.selected.count) Features",
                   role: .destructive) { flow.prune() }
                .buttonStyle(.borderedProminent)
                .tint(.red)
                .disabled(!flow.canPrune)
        }
        .confirmationDialog("Force-delete the branches of \(Pluralized.count(flow.selected.count, "feature"))?",
                            isPresented: $flow.confirmingForceDelete, titleVisibility: .visible) {
            Button("Force-Delete and Tear Down", role: .destructive) { flow.prune(confirmed: true) }
            Button("Cancel", role: .cancel) {}
        } message: {
            Text(flow.forceDeleteMessage)
        }
    }

    // MARK: Finished without a result

    private func finishedWithoutResult(_ record: OperationRecord) -> some View {
        FlowSheetLayout(title: "Prune \(flow.projectName)", subtitle: record.title, systemImage: "scissors") {
            switch record.state {
            case .failed(let error):
                ResultCard(error: error, context: record.context, operationID: record.id) { recovery in
                    Task { _ = await actions.perform(recovery) }
                }
            case .cancelled(let note):
                ResultCard(error: .cancelled(note: note ?? "Prune stopped before it tore anything down."),
                           context: record.context) { _ in }
            default:
                FlowNotice(style: .info, text: "Prune finished without a per-feature result.")
            }
        } footer: {
            Spacer()
            Button("Close") { dismiss() }
                .keyboardShortcut(.defaultAction)
        }
    }

    // MARK: Running

    private func running(_ record: OperationRecord) -> some View {
        FlowSheetLayout(title: "Prune \(flow.projectName)", subtitle: record.title, systemImage: "scissors") {
            PruneTable(flow: flow)
            OperationProgressView(record: record, capabilities: flow.model.environment.identity?.capabilities ?? [])
                .frame(minHeight: 240)
        } footer: {
            Text("Rows run one after another; a refused row is skipped.")
                .font(.callout)
                .foregroundStyle(.secondary)
            Spacer()
            Button("Run in Background") { dismiss() }
                .keyboardShortcut(.cancelAction)
            Button("Stop…", role: .destructive) { flow.stop.request(record) }
                .keyboardShortcut(".", modifiers: .command)
                .disabled(record.isFinished)
                .help("Stops the feature being torn down and starts no others")
        }
    }
}

/// The rows: checkbox, feature, status, runtime, your changes, unmerged commits, updated.
private struct PruneTable: View {
    @Bindable var flow: PruneFlow

    var body: some View {
        VStack(spacing: 0) {
            header
            Divider()
            ForEach(Array(flow.rows.enumerated()), id: \.element.feature.workFeature) { index, row in
                PruneRowView(flow: flow, row: row)
                    .background(index.isMultiple(of: 2) ? Color.clear : Color(nsColor: .alternatingContentBackgroundColors[1]))
                if index < flow.rows.count - 1 { Divider().opacity(0.5) }
            }
        }
        .background(Color(nsColor: .controlBackgroundColor), in: RoundedRectangle(cornerRadius: 8))
        .overlay(RoundedRectangle(cornerRadius: 8).strokeBorder(Color(nsColor: .separatorColor).opacity(0.6)))
        .clipShape(RoundedRectangle(cornerRadius: 8))
    }

    private var header: some View {
        HStack(spacing: 10) {
            Color.clear.frame(width: 18)
            Text("Feature").frame(maxWidth: .infinity, alignment: .leading)
            Text("Status").frame(width: 110, alignment: .leading)
            Text("Your changes").frame(width: 120, alignment: .leading)
            Text("Unmerged").frame(width: 80, alignment: .leading)
            Text("Updated").frame(width: 80, alignment: .trailing)
        }
        .font(.caption.weight(.semibold))
        .foregroundStyle(.secondary)
        .padding(.horizontal, 12)
        .padding(.vertical, 6)
    }
}

private struct PruneRowView: View {
    @Bindable var flow: PruneFlow
    let row: PrunePlanner.Row

    private var name: String { row.feature.workFeature }

    var body: some View {
        HStack(alignment: .center, spacing: 10) {
            leading
                .frame(width: 18)
                .popover(isPresented: Binding(
                    get: { flow.pendingConsent == name }, set: { if !$0 { flow.cancelConsent() } }
                ), arrowEdge: .leading) {
                    ConsentPopover(flow: flow, name: name)
                }
            VStack(alignment: .leading, spacing: 2) {
                HStack(spacing: 6) {
                    ColorSwatch(hex: row.feature.color)
                    Text(name)
                        .fontWeight(.medium)
                        .lineLimit(1)
                        .truncationMode(.middle)
                    RuntimeBadge(provider: row.feature.runtime.provider, style: .glyph)
                }
                if let note {
                    Text(note)
                        .font(.caption)
                        .foregroundStyle(noteIsWarning ? Color.orange : .secondary)
                        .lineLimit(1)
                        .truncationMode(.tail)
                        .help(note)
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            StatusBadge(status: row.feature.status)
                .frame(width: 110, alignment: .leading)
            changes.frame(width: 120, alignment: .leading)
            unmerged.frame(width: 80, alignment: .leading)
            Text(row.feature.updatedAt.map { FeaturePresentation.relative($0) } ?? "—")
                .font(.caption)
                .foregroundStyle(.secondary)
                .lineLimit(1)
                .frame(width: 80, alignment: .trailing)
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 8)
        .contentShape(Rectangle())
        .accessibilityElement(children: .combine)
        .accessibilityLabel(accessibilityLabel)
        .accessibilityAddTraits(row.selected ? [.isSelected] : [])
    }

    @ViewBuilder private var leading: some View {
        if let state = flow.runState(for: name) {
            runStateIcon(state)
        } else if flow.record != nil {
            Image(systemName: "minus").foregroundStyle(.tertiary)
        } else {
            Toggle("", isOn: Binding(get: { row.selected }, set: { _ in flow.toggle(name) }))
                .toggleStyle(.checkbox)
                .labelsHidden()
                .disabled(flow.unselectableReason(name) != nil)
                .accessibilityLabel("Tear down \(name)")
        }
    }

    @ViewBuilder private func runStateIcon(_ state: PruneFlow.RowRunState) -> some View {
        switch state {
        case .waiting:
            Image(systemName: "clock").foregroundStyle(.secondary).help("Waiting")
        case .running:
            ProgressView().controlSize(.small)
        case .finished:
            Image(systemName: "checkmark").foregroundStyle(.secondary)
        case .outcome(let outcome):
            Image(systemName: PruneResultView.symbol(for: outcome))
                .foregroundStyle(PruneResultView.tint(for: outcome))
                .help(PruneResultView.label(for: outcome))
        }
    }

    /// Why an unchecked row isn't selected, a checked dirty row's consent, or a checked row's branch outcome.
    private var note: String? {
        if let reason = flow.unselectableReason(name) { return reason }
        if let consent = flow.consents[name], row.selected {
            return "Deletes \(Pluralized.count(consent.userFiles.count, "uncommitted file"))"
        }
        if row.selected, flow.policy == .deleteIfMerged, PrunePlanner.branchPolicy(for: row, batchPolicy: .deleteIfMerged) == .keep {
            return "Branch kept: it has unmerged commits"
        }
        if !row.selected, let reason = row.defaultReason { return reason }
        return nil
    }

    private var noteIsWarning: Bool {
        flow.consents[name] != nil && row.selected
    }

    @ViewBuilder private var changes: some View {
        if let plan = row.plan {
            if !plan.worktree.exists {
                Text("Folder gone").font(.callout).foregroundStyle(.secondary)
            } else if !plan.changes.statusAvailable {
                Label("Unknown", systemImage: "questionmark.circle").font(.callout).foregroundStyle(.orange)
            } else if plan.changes.user.isEmpty {
                Label("None reported", systemImage: "checkmark.circle").font(.callout).foregroundStyle(.secondary)
            } else {
                let count = plan.changes.user.count
                Label(plan.changes.truncated ? "\(count)+ files" : Pluralized.count(count, "file"),
                      systemImage: "exclamationmark.triangle.fill")
                    .font(.callout)
                    .foregroundStyle(.orange)
            }
        } else if flow.planErrors[name] != nil {
            Label("Unknown", systemImage: "questionmark.circle").font(.callout).foregroundStyle(.orange)
        } else {
            ProgressView().controlSize(.mini)
        }
    }

    @ViewBuilder private var unmerged: some View {
        if let branch = row.plan?.branch, branch.exists, !branch.merged {
            Text(branch.ahead > 0 ? Pluralized.count(branch.ahead, "commit") : "Yes")
                .font(.callout)
                .foregroundStyle(.orange)
                .monospacedDigit()
        } else if row.plan != nil {
            Text("—").foregroundStyle(.tertiary)
        } else {
            Text("")
        }
    }

    private var accessibilityLabel: String {
        var parts = [name, row.feature.status.label]
        if let note { parts.append(note) }
        if row.selected { parts.append("selected") }
        return parts.joined(separator: ", ")
    }
}

/// "Delete 2 uncommitted files in prine?", listing them; confirming gives that row its discard consent.
struct ConsentPopover: View {
    let flow: PruneFlow
    let name: String

    var body: some View {
        let files = flow.consentFiles(name)
        VStack(alignment: .leading, spacing: 10) {
            Text("Delete \(Pluralized.count(files.count, "uncommitted file")) in \(name)?")
                .font(.headline)
            Text("Tearing down \(name) permanently deletes these changes:")
                .font(.callout)
                .foregroundStyle(.secondary)
            FlowPathList(lines: files.map { $0.kind.isEmpty ? $0.path : "\($0.path)  (\($0.kind))" }, visibleLines: 8)
            if flow.plans[name]?.changes.truncated == true {
                Text("The list is truncated. Review this feature in Tear Down before deleting any changes.")
                    .font(.caption)
                    .foregroundStyle(.orange)
                    .fixedSize(horizontal: false, vertical: true)
            }
            HStack {
                Spacer()
                Button("Cancel", role: .cancel) { flow.cancelConsent() }
                    .keyboardShortcut(.cancelAction)
                Button("Delete Files and Tear Down", role: .destructive) { flow.confirmConsent() }
                    .buttonStyle(.borderedProminent)
                    .tint(.red)
                    .disabled(flow.unselectableReason(name) != nil)
            }
        }
        .padding(16)
        .frame(width: 380)
    }
}
