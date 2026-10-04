import BranchBoxKit
import BranchBoxStores
import SwiftUI

/// What a finished teardown did, each claim as verified as BranchBox could make it: the worktree (checked on
/// disk), the branch (kept, deleted by the CLI or the app, or not deletable), the runtime cleanup (verified, a
/// residue list with commands to copy, or unverifiable), module reports and warnings. A legacy CLI's "removed
/// manually after git removal failed" gets a red flag.
struct TeardownResultView: View {
    let flow: TeardownFlow
    let outcome: TeardownOutcome
    let record: OperationRecord
    let actions: FlowActions
    let onDone: () -> Void
    @State private var confirmingForceDelete: String?

    private var summary: TeardownSummary { outcome.summary }

    var body: some View {
        FlowSheetLayout(title: title, subtitle: flow.projectStore?.displayName ?? flow.feature.project.displayName,
                        systemImage: isClean ? "checkmark.circle.fill" : "exclamationmark.circle.fill",
                        tint: isClean ? .green : .orange) {
            if manualRemoval {
                FlowNotice(style: .error, text: "git couldn't remove the worktree, so BranchBox CLI 0.13 deleted the folder "
                    + "by hand. Files git didn't know about were deleted with it.")
            }
            FlowSection(title: "Worktree", systemImage: "folder") {
                if outcome.worktreeGone {
                    FlowFactRow(systemImage: "checkmark.circle.fill", tint: .green, text: "Removed",
                                detail: "Checked on disk: the folder is gone.")
                } else {
                    FlowFactRow(systemImage: "xmark.octagon.fill", tint: .red, text: "The folder is still there",
                                detail: flow.plan?.worktree.path)
                }
                if !summary.discardedChanges.isEmpty {
                    DisclosureGroup("Discarded \(Pluralized.count(summary.discardedChanges.count, "change"))") {
                        FlowPathList(lines: summary.discardedChanges.map(\.path), visibleLines: 5)
                    }
                }
                ForEach(summary.preserved, id: \.path) { file in
                    FlowFactRow(systemImage: "arrow.turn.down.right", tint: .green, text: "Kept \(file.path)",
                                detail: file.destination.isEmpty ? nil : "Now at \(file.destination)")
                }
            }
            FlowSection(title: "Branch", systemImage: "arrow.triangle.branch") {
                branchRows
            }
            if let runtime = summary.runtimeTeardown {
                FlowSection(title: "Runtime", systemImage: "shippingbox") {
                    runtimeRows(runtime)
                }
            }
            if !summary.moduleReports.isEmpty {
                FlowSection(title: "Modules", systemImage: "checklist") {
                    ForEach(summary.moduleReports, id: \.name) { report in
                        FlowFactRow(systemImage: report.teardownOk ? "checkmark.circle.fill" : "xmark.octagon.fill",
                                    tint: report.teardownOk ? .green : .red, text: report.name,
                                    detail: report.errors.isEmpty ? nil : report.errors.joined(separator: "\n"))
                    }
                }
            }
            if !otherWarnings.isEmpty {
                FlowSection(title: "Warnings", systemImage: "exclamationmark.triangle", trailing: "\(otherWarnings.count)") {
                    ForEach(otherWarnings, id: \.self) { warning in
                        FlowFactRow(systemImage: "exclamationmark.triangle.fill", tint: .orange, text: warning)
                    }
                }
            }
            if let error = flow.dispatchError {
                FlowNotice(style: .error, text: error)
            }
        } footer: {
            Button("Show Log") { Task { _ = await actions.perform(.showLog(operation: record.id)) } }
            Spacer()
            Button("Done", action: onDone)
                .keyboardShortcut(.defaultAction)
                .buttonStyle(.borderedProminent)
        }
        .confirmationDialog("Force-delete \(confirmingForceDelete ?? "the branch")?", isPresented: Binding(
            get: { confirmingForceDelete != nil }, set: { if !$0 { confirmingForceDelete = nil } }
        ), titleVisibility: .visible, presenting: confirmingForceDelete) { branch in
            Button("Force-Delete \(branch)", role: .destructive) { flow.forceDeleteBranch(branch) }
            Button("Cancel", role: .cancel) {}
        } message: { branch in
            Text(RecoveryPlanner.unmergedConfirmationText(branch: branch, ahead: flow.plan?.branch?.ahead))
        }
    }

    private var title: String {
        isClean ? "\(flow.feature.name) is torn down" : "\(flow.feature.name) is torn down, with problems"
    }

    /// Nothing for the user to follow up on.
    private var isClean: Bool {
        guard outcome.worktreeGone, !manualRemoval, otherWarnings.isEmpty else { return false }
        if case .deleteFailed = outcome.branch { return false }
        if let runtime = summary.runtimeTeardown, !(runtime.verified && runtime.residueFree) { return false }
        return summary.moduleReports.allSatisfy(\.teardownOk)
    }

    private var manualRemoval: Bool {
        summary.warnings.contains { $0.contains(Self.manualRemovalWarning) }
    }

    /// 0.13.x's last-resort `remove_dir_all` (BranchBoxCLI `LegacyTeardown.manualRemovalWarning`).
    static let manualRemovalWarning = "removed manually after git removal failed"

    private var otherWarnings: [String] {
        (summary.warnings + summary.adapterCleanupWarnings).filter { !$0.contains(Self.manualRemovalWarning) }
    }

    // MARK: Branch

    @ViewBuilder private var branchRows: some View {
        switch outcome.branch {
        case .kept(let name):
            FlowFactRow(systemImage: "checkmark.circle", tint: .secondary, text: "\(name ?? "The branch") was kept")
        case .deleted(let name, let deleter):
            FlowFactRow(systemImage: "checkmark.circle.fill", tint: .green, text: "\(name) was deleted",
                        detail: deleter == .cli ? "Deleted by the BranchBox CLI" : "Deleted by BranchBox for Mac (git branch)")
        case .notFound(let name):
            FlowFactRow(systemImage: "minus.circle", text: "\(name) was already gone")
        case .deleteFailed(let name, let reason):
            FlowFactRow(systemImage: "exclamationmark.triangle.fill", tint: .orange, text: "\(name) couldn't be deleted",
                        detail: reason)
            if let branchRecord = flow.branchRecord {
                OperationRow(record: branchRecord)
                if let error = branchRecord.failure {
                    ResultCard(error: error, context: branchRecord.context) { action in
                        Task { flow.adoptBranchRetry(await actions.perform(action)) }
                    }
                }
            } else if flow.deleteFailedBecauseUnmerged(reason: reason) {
                Button("Force-Delete Branch…", role: .destructive) { confirmingForceDelete = name }
                    .buttonStyle(.bordered)
                    .foregroundStyle(.red)                      // .tint doesn't colour a bordered macOS button
            }
        }
    }

    // MARK: Runtime

    @ViewBuilder private func runtimeRows(_ runtime: RuntimeTeardownReport) -> some View {
        if runtime.verified, runtime.residueFree {
            FlowFactRow(systemImage: "checkmark.circle.fill", tint: .green, text: "Cleaned up",
                        detail: Self.verifiedCleanDetail(provider: runtime.provider))
        } else if !runtime.residue.isEmpty {
            FlowFactRow(systemImage: "exclamationmark.triangle.fill", tint: .orange,
                        text: "Some runtime resources were left behind", detail: runtimeName(runtime))
            FlowPathList(lines: runtime.residue.flatMap { item in item.identifiers.map { "\(item.kind)  \($0)" } })
            HStack {
                CopyButton(text: Self.cleanupCommands(runtime.residue), label: "Copy Cleanup Commands", showsTitle: true)
                Text("Review them, then run them in Terminal.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        } else {
            FlowFactRow(systemImage: "questionmark.circle", tint: .secondary, text: "Couldn't verify the cleanup",
                        detail: "BranchBox couldn't check whether \(runtimeName(runtime)) is fully gone.")
        }
    }

    /// What the verified check found, in the runtime's own terms.
    static func verifiedCleanDetail(provider: String?) -> String {
        switch provider.map(RuntimeProvider.init(raw:)) {
        case .container?: "Checked: no containers, networks or volumes are left."
        case .sbx?: "Checked: the sandbox is gone."
        case .localVM?: "Checked: the VM is gone."
        default: "Checked: nothing from the runtime is left."
        }
    }

    private func runtimeName(_ runtime: RuntimeTeardownReport) -> String {
        let provider = runtime.provider.map { RuntimeProvider(raw: $0).label } ?? "the runtime"
        return runtime.runtimeID.map { "\(provider) \($0)" } ?? provider
    }

    /// `docker rm -f` / `docker volume rm` / `docker network rm` lines for Docker residue, a comment for the rest.
    /// Copied, never run.
    static func cleanupCommands(_ residue: [ResidueItem]) -> String {
        residue.compactMap { item -> String? in
            guard !item.identifiers.isEmpty else { return nil }
            let ids = item.identifiers.map(HostLaunchPlan.shellQuote).joined(separator: " ")
            let kind = item.kind.lowercased()
            if kind.contains("volume") { return "docker volume rm \(ids)" }
            if kind.contains("network") { return "docker network rm \(ids)" }
            if kind.contains("container") { return "docker rm -f \(ids)" }
            return "# \(item.kind): \(item.identifiers.joined(separator: " "))"
        }.joined(separator: "\n")
    }
}
