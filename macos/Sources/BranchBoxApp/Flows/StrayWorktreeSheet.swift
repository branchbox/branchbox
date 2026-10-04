import BranchBoxKit
import BranchBoxStores
import SwiftUI

/// A worktree the registry does not know: reveal, open or remove it.
///
/// [Remove Worktree…] asks first, then removes without discarding anything; a dirty worktree is refused and
/// the refusal offers "Discard N changes and remove", confirmed with the file list.
struct StrayWorktreeSheet: View {
    let project: ProjectRef
    let stray: StrayWorktree

    @Environment(AppModel.self) private var model
    @State private var flow: StrayFlow?

    init(project: ProjectRef, stray: StrayWorktree) {
        self.project = project
        self.stray = stray
    }

    /// A sheet around an existing flow (previews and render tests).
    init(flow: StrayFlow) {
        self.project = flow.project
        self.stray = flow.stray
        _flow = State(initialValue: flow)
    }

    var body: some View {
        Group {
            if let flow {
                StrayContent(flow: flow)
            } else {
                ProgressView().frame(maxWidth: .infinity, maxHeight: .infinity)
            }
        }
        .frame(minWidth: 520, idealWidth: 560, minHeight: 420, idealHeight: 480)
        .onAppear {
            if flow == nil { flow = StrayFlow(model: model, project: project, stray: stray) }
        }
    }
}

private struct StrayContent: View {
    @Bindable var flow: StrayFlow
    @Environment(\.dismiss) private var dismiss
    @Environment(\.openWindow) private var openWindow
    @State private var launchError: String?

    var body: some View {
        FlowSheetLayout(title: title, subtitle: flow.folderName, systemImage: symbol, tint: tint) {
            if let record = flow.record {
                attempt(record)
            } else {
                details
            }
            if let error = flow.dispatchError ?? launchError {
                FlowNotice(style: .error, text: error)
            }
        } footer: {
            Button("Reveal") { actions.reveal(flow.stray.path) }
                .disabled(flow.removed)
            Button("Open in Terminal") {
                Task {
                    if case .failure(let error)? = await actions.openTerminal(in: flow.stray.path) {
                        launchError = error.message
                    }
                }
            }
            .disabled(!flow.folderExists || flow.removed)
            Spacer()
            if flow.record == nil {
                Button("Cancel") { dismiss() }
                    .keyboardShortcut(.cancelAction)
                Button("Remove Worktree…", role: .destructive) { flow.confirmingRemove = true }
                    .buttonStyle(.borderedProminent)
                    .tint(.red)
            } else if flow.record?.isFinished == false {
                Button("Run in Background") { dismiss() }
                    .keyboardShortcut(.cancelAction)
                if let record = flow.record {
                    Button("Stop…", role: .destructive) { flow.stop.request(record) }
                }
            } else {
                Button("Done") { dismiss() }
                    .keyboardShortcut(.defaultAction)
                    .buttonStyle(.borderedProminent)
            }
        }
        .confirmationDialog("Remove the worktree \(flow.folderName)?", isPresented: $flow.confirmingRemove,
                            titleVisibility: .visible) {
            Button("Remove Worktree", role: .destructive) { flow.remove() }
            Button("Cancel", role: .cancel) {}
        } message: {
            Text(flow.removeConfirmationMessage)
        }
        .stopConfirmation(flow.stop, model: flow.model)
        .onChange(of: flow.record?.isFinished == true) { _, finished in
            guard finished else { return }
            if flow.record?.needsAttention == true { flow.record?.acknowledge() }
            flow.removalFinished()
        }
    }

    private var actions: FlowActions {
        FlowActions(model: flow.model, openWindow: { openWindow(id: $0) })
    }

    private var branchFailed: Bool { flow.branchRecord?.failure != nil }

    private var title: String {
        guard flow.removed else { return "Unregistered Worktree" }
        return branchFailed ? "Worktree removed, branch kept" : "Worktree removed"
    }

    private var symbol: String {
        guard flow.removed else { return "questionmark.folder.fill" }
        return branchFailed ? "exclamationmark.circle.fill" : "checkmark.circle.fill"
    }

    private var tint: Color {
        guard flow.removed else { return .purple }
        return branchFailed ? .orange : .green
    }

    // MARK: Details

    @ViewBuilder private var details: some View {
        Text("This folder is a Git worktree in the BranchBox layout, but the project's registry doesn't know it. "
             + "It's usually left behind by a start that was interrupted.")
            .foregroundStyle(.secondary)
            .fixedSize(horizontal: false, vertical: true)
        FlowSection(title: "Worktree", systemImage: "folder") {
            Grid(alignment: .leadingFirstTextBaseline, horizontalSpacing: 12, verticalSpacing: 6) {
                GridRow {
                    label("Path")
                    HStack(spacing: 6) {
                        Text(flow.stray.path)
                            .font(.system(.callout, design: .monospaced))
                            .lineLimit(2)
                            .truncationMode(.middle)
                            .textSelection(.enabled)
                        CopyButton(text: flow.stray.path, label: "Copy Path")
                            .buttonStyle(.borderless)
                    }
                }
                GridRow {
                    label("Branch")
                    Text(flow.stray.branch ?? "Detached HEAD")
                        .font(.system(.callout, design: .monospaced))
                        .textSelection(.enabled)
                }
                if let head = flow.stray.head {
                    GridRow {
                        label("Commit")
                        Text(String(head.prefix(12)))
                            .font(.system(.callout, design: .monospaced))
                            .textSelection(.enabled)
                    }
                }
            }
            if flow.stray.locked {
                FlowFactRow(systemImage: "lock.fill", tint: .orange, text: "Locked",
                            detail: "git won't remove a locked worktree; unlock it with git worktree unlock first.")
            }
            if !flow.folderExists {
                FlowFactRow(systemImage: "folder.badge.questionmark", tint: .secondary, text: "The folder is gone",
                            detail: "Removing it only clears git's record of the worktree.")
            }
        }
        FlowSection(title: "Your changes", systemImage: "doc.badge.ellipsis") {
            FlowFactRow(systemImage: "shield.lefthalf.filled", tint: .secondary,
                        text: "Checked when you remove it",
                        detail: "If the folder has uncommitted changes, nothing is removed and you'll see the files first.")
        }
        if let branch = flow.stray.branch {
            Toggle("Also delete \(branch) if it's merged", isOn: $flow.deleteBranchIfMerged)
                .help("git branch -d: refused if the branch has commits that aren't merged")
        }
    }

    private func label(_ text: String) -> some View {
        Text(text)
            .font(.callout)
            .foregroundStyle(.secondary)
            .gridColumnAlignment(.trailing)
    }

    // MARK: Attempt

    @ViewBuilder private func attempt(_ record: OperationRecord) -> some View {
        if !record.isFinished {
            OperationProgressView(record: record, capabilities: flow.model.environment.identity?.capabilities ?? [])
                .frame(minHeight: 260)
        } else if let error = record.failure {
            ResultCard(error: error, context: record.context,
                       diagnosticReport: { actions.diagnosticReport(for: record) }) { action in
                Task { flow.adopt(await actions.perform(action)) }
            }
        } else if case .cancelled(let note) = record.state {
            ResultCard(error: .cancelled(note: note), context: record.context) { _ in }
        } else {
            FlowSection(title: "Worktree", systemImage: "folder") {
                FlowFactRow(systemImage: "checkmark.circle.fill", tint: .green, text: "Removed \(flow.stray.path)")
            }
            if let branchRecord = flow.branchRecord {
                FlowSection(title: "Branch", systemImage: "arrow.triangle.branch") {
                    OperationRow(record: branchRecord)
                    if let error = branchRecord.failure {
                        ResultCard(error: error, context: branchRecord.context) { action in
                            Task { flow.adoptBranchRetry(await actions.perform(action)) }
                        }
                    }
                }
            } else if let branch = flow.stray.branch {
                FlowSection(title: "Branch", systemImage: "arrow.triangle.branch") {
                    FlowFactRow(systemImage: "checkmark.circle", text: "\(branch) was kept")
                }
            }
        }
    }
}
