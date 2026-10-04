import BranchBoxKit
import BranchBoxStores
import SwiftUI

/// Plans and runs one teardown; a first attempt never carries discard consent.
///
/// The sheet says what will be deleted and what is kept before anything runs. A refusal or partial failure
/// shows in the same sheet with its recoveries (each destructive one confirmed in a dialog that lists exactly
/// what is lost); the result replaces the plan in place. Tear Down is destructive and has no Return shortcut.
struct TeardownSheet: View {
    let feature: FeatureRef
    let preselect: BranchPolicy?

    @Environment(AppModel.self) private var model
    @State private var flow: TeardownFlow?

    init(feature: FeatureRef, preselect: BranchPolicy?) {
        self.feature = feature
        self.preselect = preselect
    }

    /// A sheet around an existing flow (previews and render tests).
    init(flow: TeardownFlow) {
        self.feature = flow.feature
        self.preselect = flow.preselect
        _flow = State(initialValue: flow)
    }

    var body: some View {
        Group {
            if let flow {
                TeardownContent(flow: flow)
            } else {
                ProgressView().frame(maxWidth: .infinity, maxHeight: .infinity)
            }
        }
        .frame(minWidth: 560, idealWidth: 600, minHeight: 480, idealHeight: 620)
        .onAppear {
            if flow == nil { flow = TeardownFlow(model: model, feature: feature, preselect: preselect) }
        }
    }
}

private struct TeardownContent: View {
    @Bindable var flow: TeardownFlow
    @Environment(\.dismiss) private var dismiss
    @Environment(\.openWindow) private var openWindow
    @State private var confirmingSpecOverride = false

    var body: some View {
        Group {
            if let record = flow.record {
                if !record.isFinished {
                    running(record)
                } else if let outcome = flow.outcome {
                    TeardownResultView(flow: flow, outcome: outcome, record: record, actions: actions, onDone: { dismiss() })
                } else {
                    failed(record)
                }
            } else {
                planning
            }
        }
        .stopConfirmation(flow.stop, model: flow.model)
        .task {
            if flow.draft == nil { await flow.load() }
        }
        .onChange(of: flow.record?.isFinished == true) { _, finished in
            if finished, flow.record?.needsAttention == true { flow.record?.acknowledge() }
        }
    }

    private var actions: FlowActions {
        FlowActions(model: flow.model, openWindow: { openWindow(id: $0) })
    }

    private var subtitle: String {
        var parts: [String] = []
        if let branch = flow.featureRecord?.branchName, !branch.isEmpty { parts.append(branch) }
        if let runtime = flow.featureRecord?.runtime.provider { parts.append(runtime.label) }
        parts.append(flow.projectStore?.displayName ?? flow.feature.project.displayName)
        return parts.joined(separator: " · ")
    }

    // MARK: Plan

    private var planning: some View {
        FlowSheetLayout(title: "Tear Down \(flow.feature.name)", subtitle: subtitle, systemImage: "trash", tint: .red) {
            switch flow.planState {
            case .loading:
                HStack(spacing: 10) {
                    ProgressView().controlSize(.small)
                    Text("Checking for unsaved work…").foregroundStyle(.secondary)
                }
                .frame(maxWidth: .infinity, minHeight: 200)
            case .failed(let error):
                ResultCard(error: error, context: nil) { action in
                    Task { _ = await actions.perform(action) }
                }
                Button("Check Again") { Task { await flow.load() } }
            case .ready:
                if let draft = flow.draft {
                    TeardownPlanSections(flow: flow, draft: draft)
                }
            }
            if let error = flow.dispatchError {
                FlowNotice(style: .error, text: error)
            }
        } footer: {
            copyCommandButton
            Spacer()
            Button("Cancel") { dismiss() }
                .keyboardShortcut(.cancelAction)
            Button("Tear Down", role: .destructive) { flow.tearDown() }
                .buttonStyle(.borderedProminent)
                .tint(.red)
                .disabled(!flow.canTearDown)
                .help(flow.draft?.blockingReason ?? "Remove the worktree; you'll be asked before anything of yours is deleted")
                .accessibilityIdentifier("sheet.teardown.confirm")
        }
        .confirmationDialog("Force-delete \(flow.plan?.branch?.name ?? "the branch")?", isPresented: $flow.confirmingForceDelete,
                            titleVisibility: .visible) {
            Button("Force-Delete When Tearing Down", role: .destructive) { flow.confirmForceDelete() }
            Button("Cancel", role: .cancel) {}
        } message: {
            Text(flow.forceDeleteMessage)
        }
    }

    @ViewBuilder private var copyCommandButton: some View {
        let command = flow.draft.flatMap { draft in
            (try? flow.model.backend())?.previewCommandLine(.teardown(draft.makeRequest()))
        }
        if let command {
            CopyButton(text: command, label: "Copy as Command", showsTitle: true)
        } else {
            Button {} label: { Label("Copy as Command", systemImage: "doc.on.doc") }
                .disabled(true)
                .help("Only available with the BranchBox CLI")
        }
    }

    // MARK: Running and failure

    private func running(_ record: OperationRecord) -> some View {
        FlowSheetLayout(title: "Tear Down \(flow.feature.name)", subtitle: subtitle, systemImage: "trash", tint: .red) {
            OperationProgressView(record: record, capabilities: flow.model.environment.identity?.capabilities ?? [])
                .frame(minHeight: 380)
        } footer: {
            Spacer()
            Button("Run in Background") { dismiss() }
                .keyboardShortcut(.cancelAction)
            Button("Stop…", role: .destructive) { flow.stop.request(record) }
                .keyboardShortcut(".", modifiers: .command)
        }
    }

    private func failed(_ record: OperationRecord) -> some View {
        FlowSheetLayout(title: "Tear Down \(flow.feature.name)", subtitle: subtitle, systemImage: "hand.raised.fill",
                        tint: .orange) {
            if let error = record.failure {
                ResultCard(error: error, context: record.context,
                           diagnosticReport: { actions.diagnosticReport(for: record) }) { action in
                    Task { flow.adopt(await actions.perform(action)) }
                }
                if let override = flow.specOverride {
                    Button(override.title, role: .destructive) { confirmingSpecOverride = true }
                        .buttonStyle(.bordered)
                        .foregroundStyle(.red)                      // .tint doesn't colour a bordered macOS button
                        .confirmationDialog(override.confirmationTitle, isPresented: $confirmingSpecOverride,
                                            titleVisibility: .visible) {
                            Button(override.confirmationTitle, role: .destructive) {
                                flow.adopt(FlowDispatch.record(of: flow.model.actions.perform(override)
                                    ?? .rejected(reason: "Nothing to retry")))
                            }
                            Button("Cancel", role: .cancel) {}
                        } message: {
                            Text(override.confirmationMessage ?? "")
                        }
                }
            } else if case .cancelled(let note) = record.state {
                ResultCard(error: .cancelled(note: note), context: record.context) { _ in }
            }
            if !flow.earlierAttempts.isEmpty {
                Text(flow.earlierAttempts.count == 1 ? "After 1 earlier attempt in this sheet."
                                                     : "After \(flow.earlierAttempts.count) earlier attempts in this sheet.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            if let error = flow.dispatchError {
                FlowNotice(style: .error, text: error)
            }
            DisclosureGroup("Log") {
                LogView(lines: record.log.lines, archiveURL: record.log.archiveURL, firstIndex: record.log.droppedLines,
                        revision: record.log.revision)
                    .frame(height: 200)
                    .overlay(RoundedRectangle(cornerRadius: 6).strokeBorder(.separator))
            }
        } footer: {
            Spacer()
            Button("Close") { dismiss() }
                .keyboardShortcut(.cancelAction)
        }
    }
}

/// What the plan says will happen: your changes, generated files, the spec, the branch choice, the runtime and
/// tunnel, and a folder that is already gone.
private struct TeardownPlanSections: View {
    let flow: TeardownFlow
    let draft: TeardownDraft

    private var plan: TeardownPlanDocument { draft.plan }

    var body: some View {
        if !plan.worktree.exists {
            // The first attempt never forces removal (D-11), so the CLI asks first; its recovery is the forced clean-up.
            FlowNotice(style: .info, text: "The folder \(plan.worktree.path) is already gone. Tearing down cleans up what's "
                + "left (the registry entry and the runtime); BranchBox may ask you to confirm removing the missing "
                + "folder's entry first.")
        }
        if plan.worktree.exists, plan.worktree.locked {
            FlowNotice(style: .warning, text: "This worktree is locked"
                + (plan.worktree.lockReason.map { " (\($0))" } ?? "")
                + ". Git refuses to remove a locked worktree; unlock it (git worktree unlock) before tearing down.")
        }
        if plan.worktree.exists {
            changesSection
        }
        if !plan.changes.preserved.isEmpty {
            FlowSection(title: "Kept", systemImage: "doc.badge.arrow.up") {
                ForEach(plan.changes.preserved, id: \.path) { file in
                    FlowFactRow(systemImage: "arrow.turn.down.right", tint: .green, text: file.path,
                                detail: file.destination.isEmpty ? "Moved to the main worktree" : "Moved to \(file.destination)")
                }
                Toggle("Mark the spec as completed", isOn: Binding(
                    get: { draft.completeSpec }, set: { flow.setCompleteSpec($0) }
                ))
                .help("Moves the preserved spec to docs/features/completed/")
            }
        }
        branchSection
        alsoRemovedSection
        ForEach(plan.warnings, id: \.self) { warning in
            FlowNotice(style: .warning, text: warning)
        }
        if plan.source == .appPreflight {
            Text("Checked by BranchBox for Mac: this BranchBox CLI has no teardown preview.")
                .font(.caption)
                .foregroundStyle(.secondary)
        }
        if let reason = draft.blockingReason, flow.branchIsUnmerged == false || draft.branch != .deleteIfMerged {
            FlowNotice(style: .error, text: reason)
        }
    }

    // MARK: Changes

    @ViewBuilder private var changesSection: some View {
        let user = plan.changes.user
        if !plan.changes.statusAvailable {
            FlowSection(title: "Your changes", systemImage: "questionmark.folder") {
                FlowFactRow(systemImage: "exclamationmark.triangle.fill", tint: .orange,
                            text: "BranchBox couldn't check this folder for unsaved work.",
                            detail: plan.blockers.first { $0.kind == "status_unavailable" }?.cause
                                ?? "Tearing down stops, and you'll be asked before anything is removed.")
            }
        } else if user.isEmpty {
            FlowSection(title: "Your changes", systemImage: "checkmark.shield") {
                FlowFactRow(systemImage: "checkmark.circle.fill", tint: .green, text: "No user changes reported",
                            detail: "Git-ignored files are not listed; they are removed with the worktree even when you keep the branch.")
            }
        } else {
            FlowSection(title: "Will be permanently deleted", systemImage: "exclamationmark.octagon",
                        trailing: plan.changes.truncated ? "more than \(user.count)" : "\(user.count)", tint: .red) {
                FlowPathList(lines: user.map { $0.kind.isEmpty ? $0.path : "\($0.path)  (\($0.kind))" })
                if let warning = draft.pendingDiscardWarning {
                    Text(warning)
                        .font(.callout)
                        .foregroundStyle(.red)
                        .fixedSize(horizontal: false, vertical: true)
                }
            }
        }
        if !plan.changes.generated.isEmpty {
            FlowSection(title: "Generated by BranchBox (safe to delete)", systemImage: "gearshape.2",
                        trailing: "\(plan.changes.generated.count)") {
                DisclosureGroup(plan.changes.generated.count == 1 ? "1 file" : "\(plan.changes.generated.count) files") {
                    FlowPathList(lines: plan.changes.generated.map(\.path), visibleLines: 5)
                }
            }
        }
    }

    // MARK: Branch

    private var branchSection: some View {
        FlowSection(title: "Branch", systemImage: "arrow.triangle.branch") {
            if let branch = plan.branch, flow.branchIsUnmerged {
                FlowFactRow(systemImage: "exclamationmark.triangle.fill", tint: .orange,
                            text: branch.ahead > 0
                                ? "\(branch.name) has \(Pluralized.count(branch.ahead, "commit")) not in \(reference(branch))"
                                : "\(branch.name) isn't merged into \(reference(branch))")
            } else if let branch = plan.branch, !branch.exists {
                FlowFactRow(systemImage: "minus.circle", text: "\(branch.name) no longer exists")
            }
            VStack(alignment: .leading, spacing: 6) {
                ForEach(draft.visibleBranchOptions, id: \.self) { option in
                    BranchOptionRow(option: option, selected: draft.branch == option, branchName: plan.branch?.name,
                                    unmerged: flow.branchIsUnmerged) {
                        flow.choose(option)
                    }
                }
            }
            if let reason = draft.blockingReason, draft.branch == .deleteIfMerged, flow.branchIsUnmerged {
                Label(reason, systemImage: "xmark.octagon.fill")
                    .font(.callout)
                    .foregroundStyle(.red)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
    }

    private func reference(_ branch: TeardownPlanDocument.Branch) -> String {
        branch.referenceName.isEmpty ? (branch.upstream ?? "its base") : branch.referenceName
    }

    // MARK: Runtime and tunnel

    @ViewBuilder private var alsoRemovedSection: some View {
        let runtime = plan.runtime?.provider.map(RuntimeProvider.init(raw:))
        let tunnelActive = plan.tunnel?.status == .active
        if runtime != nil || tunnelActive {
            FlowSection(title: "Also removed", systemImage: "shippingbox") {
                if let runtime {
                    FlowFactRow(systemImage: runtime.symbol, text: runtimeText(runtime),
                                detail: plan.runtime?.runtimeID.map { "Runtime \($0)" })
                }
                if tunnelActive {
                    FlowFactRow(systemImage: "network", text: "The active tunnel stops sharing this feature",
                                detail: flow.featureRecord?.tunnel?.hostname)
                }
            }
        }
    }

    private func runtimeText(_ runtime: RuntimeProvider) -> String {
        switch runtime {
        case .container: "The dev container, its Compose services and volumes"
        case .sbx: "The Docker Sandbox"
        case .localVM: "The local VM"
        case .inGuest: "The in-guest environment"
        case .unknown(let raw): "The \(raw) runtime"
        }
    }
}

/// One branch policy as a radio row with what it does.
private struct BranchOptionRow: View {
    let option: BranchPolicy
    let selected: Bool
    let branchName: String?
    let unmerged: Bool
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            HStack(alignment: .firstTextBaseline, spacing: 10) {
                Image(systemName: selected ? "largecircle.fill.circle" : "circle")
                    .foregroundStyle(selected ? (option == .forceDelete ? Color.red : Color.accentColor) : .secondary)
                    .accessibilityHidden(true)
                VStack(alignment: .leading, spacing: 1) {
                    Text(title).foregroundStyle(option == .forceDelete ? Color.red : .primary)
                    Text(detail).font(.caption).foregroundStyle(.secondary)
                }
                Spacer(minLength: 0)
            }
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityAddTraits(selected ? [.isSelected] : [])
    }

    private var title: String {
        switch option {
        case .keep: "Keep the branch"
        case .deleteIfMerged: "Delete the branch if it's merged"
        case .forceDelete: "Force-delete the branch…"
        }
    }

    private var detail: String {
        let branch = branchName ?? "the branch"
        return switch option {
        case .keep: "\(branch) stays; delete it later from Git."
        case .deleteIfMerged: unmerged ? "git refuses: it isn't merged." : "Safe: git refuses if anything isn't merged."
        case .forceDelete: "Deletes \(branch) and the commits that aren't merged anywhere else."
        }
    }
}
