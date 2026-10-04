import BranchBoxKit
import BranchBoxPreview
import SwiftUI

/// An operation's outcome inline in its sheet or row: a refusal, a partial failure, a failure, or a success that
/// came with warnings (§9.4). Errors offer the `RecoveryPlanner` recoveries for them through `onRecovery`;
/// destructive ones are confirmed first in a dialog that shows what is lost. Copy recoveries copy in place.
/// [Show Log] appears with an operation id, [Copy Diagnostic Report] for failures when a report is provided.
struct ResultCard: View {
    enum Content {
        case failure(BackendError, context: OperationRequestContext?)
        case success(title: String, warnings: [String])
    }

    private let content: Content
    private let operationID: UUID?
    private let diagnosticReport: (() -> String)?
    private let onRecovery: (RecoveryAction) -> Void
    @State private var pendingConfirmation: RecoveryAction?

    /// Most file or cause lines shown before the list scrolls.
    static let visibleDetailLines = 8

    init(error: BackendError, context: OperationRequestContext?, operationID: UUID? = nil,
         diagnosticReport: (() -> String)? = nil, onRecovery: @escaping (RecoveryAction) -> Void) {
        self.content = .failure(error, context: context)
        self.operationID = operationID
        self.diagnosticReport = diagnosticReport
        self.onRecovery = onRecovery
    }

    init(successTitle: String, warnings: [String], operationID: UUID? = nil,
         onRecovery: @escaping (RecoveryAction) -> Void = { _ in }) {
        self.content = .success(title: successTitle, warnings: warnings)
        self.operationID = operationID
        self.diagnosticReport = nil
        self.onRecovery = onRecovery
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            Label {
                Text(title).font(.headline)
            } icon: {
                Image(systemName: symbol).foregroundStyle(tint.color)
            }
            if !message.isEmpty {
                Text(message)
                    .textSelection(.enabled)
                    .fixedSize(horizontal: false, vertical: true)
            }
            if !completed.isEmpty {
                VStack(alignment: .leading, spacing: 2) {
                    ForEach(completed, id: \.self) { step in
                        Label {
                            Text(step)
                        } icon: {
                            Image(systemName: "checkmark.circle.fill").foregroundStyle(.green)
                        }
                    }
                }
            }
            if !details.isEmpty {
                detailList
            }
            if !warnings.isEmpty {
                VStack(alignment: .leading, spacing: 4) {
                    ForEach(Array(warnings.enumerated()), id: \.offset) { _, warning in
                        Label(warning, systemImage: "exclamationmark.triangle")
                            .foregroundStyle(.orange)
                            .textSelection(.enabled)
                    }
                }
            }
            actions
        }
        .padding(12)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(tint.color.opacity(0.08), in: RoundedRectangle(cornerRadius: 10))
        .overlay(RoundedRectangle(cornerRadius: 10).strokeBorder(tint.color.opacity(0.3)))
        .accessibilityElement(children: .contain)
        .confirmationDialog(pendingConfirmation?.confirmationTitle ?? "", isPresented: confirmationShown,
                            titleVisibility: .visible, presenting: pendingConfirmation) { action in
            Button(action.confirmationTitle, role: .destructive) { onRecovery(action) }
            Button("Cancel", role: .cancel) {}
        } message: { action in
            Text(action.confirmationMessage ?? "")
        }
    }

    // MARK: Parts

    private var detailList: some View {
        let list = VStack(alignment: .leading, spacing: 2) {
            ForEach(Array(details.enumerated()), id: \.offset) { _, line in
                Text(line)
                    .font(.system(.callout, design: .monospaced))
                    .textSelection(.enabled)
                    .frame(maxWidth: .infinity, alignment: .leading)
            }
        }
        return Group {
            if details.count > Self.visibleDetailLines {
                ScrollView { list }.frame(maxHeight: CGFloat(Self.visibleDetailLines) * 18)
            } else {
                list
            }
        }
        .padding(8)
        .background(.background.opacity(0.6), in: RoundedRectangle(cornerRadius: 6))
    }

    @ViewBuilder private var actions: some View {
        let recoveries = self.recoveries
        let showsReport = presentation?.offersDiagnosticReport == true && diagnosticReport != nil
        if !recoveries.isEmpty || operationID != nil || showsReport {
            FlowLayout(spacing: 8) {
                ForEach(recoveries) { action in
                    recoveryButton(action)
                }
                if let operationID {
                    Button("Show Log") { onRecovery(.showLog(operation: operationID)) }
                }
                if showsReport, let diagnosticReport {
                    CopyButton(label: "Copy Diagnostic Report", showsTitle: true, text: diagnosticReport)
                }
            }
        }
    }

    @ViewBuilder private func recoveryButton(_ action: RecoveryAction) -> some View {
        if case .copyCommand(let command, let label) = action {
            CopyButton(text: command, label: label, showsTitle: true)
        } else if action.isDestructive {
            Button(action.title, role: .destructive) { pendingConfirmation = action }
                .foregroundStyle(.red)                  // `role` alone doesn't colour a bordered macOS button
        } else {
            Button(action.title) { onRecovery(action) }
        }
    }

    private var confirmationShown: Binding<Bool> {
        Binding(get: { pendingConfirmation != nil }, set: { if !$0 { pendingConfirmation = nil } })
    }

    // MARK: Content

    private var presentation: ErrorPresentation? {
        guard case .failure(let error, let context) = content else { return nil }
        return error.presentation(context: context)
    }

    /// The planner's recoveries; Show Log is its own button.
    private var recoveries: [RecoveryAction] {
        guard case .failure(let error, let context) = content else { return [] }
        return RecoveryPlanner.recoveries(for: error, after: context).filter {
            if case .showLog = $0 { return false }
            return true
        }
    }

    private var title: String {
        switch content {
        case .failure: presentation?.title ?? ""
        case .success(let title, _): title
        }
    }

    private var message: String { presentation?.message ?? "" }
    private var details: [String] { presentation?.details ?? [] }
    private var completed: [String] { presentation?.completed ?? [] }

    private var warnings: [String] {
        if case .success(_, let warnings) = content { return warnings }
        return []
    }

    private var symbol: String {
        if let presentation { return presentation.symbol }
        return warnings.isEmpty ? "checkmark.circle.fill" : "exclamationmark.triangle.fill"
    }

    private var tint: StatusTint {
        if let presentation { return presentation.tint }
        return warnings.isEmpty ? .green : .orange
    }
}

private enum ResultCardPreview {
    static let project = PreviewSamples.project
    static let feature = FeatureRef(project: project, name: "prine")
    static let request = TeardownRequest(feature: feature, recordedBranch: "feature/prine", branch: .deleteIfMerged)
    static let diagnostics = Diagnostics(summary: "Error: failed to remove worktree", causes: ["git worktree remove: Directory not empty"],
                                         exitCode: 1, logTail: ["Error: failed to remove worktree"],
                                         invocation: "branchbox feature teardown prine --json --keep-branch", cliVersion: "0.13.4")
    static let dirty = BackendError.refused(Refusal(
        cause: .uncommittedChanges(files: [ChangedFile(path: "README.md", kind: "modified", area: "other"),
                                           ChangedFile(path: "notes.txt", kind: "untracked", area: "other")]),
        message: "Refusing to tear down 'prine'; nothing was removed. 2 uncommitted changes would be lost.",
        diagnostics: diagnostics))
    static let partial = BackendError.partial(PartialFailure(
        completed: ["Worktree removed", "Runtime cleaned up"],
        remaining: Refusal(cause: .unmergedBranch(branch: "feature/prine", ahead: 3),
                           message: "Branch 'feature/prine' could not be deleted without force.", diagnostics: diagnostics)))
}

#Preview("Refusal with recoveries") {
    ResultCard(error: ResultCardPreview.dirty, context: .teardown(ResultCardPreview.request), operationID: UUID()) { _ in }
        .frame(width: 480)
        .padding()
}

#Preview("Partial and failure") {
    VStack(spacing: 16) {
        ResultCard(error: ResultCardPreview.partial, context: .teardown(ResultCardPreview.request)) { _ in }
        ResultCard(error: .commandFailed(ResultCardPreview.diagnostics), context: nil, operationID: UUID(),
                   diagnosticReport: { "# BranchBox diagnostic report" }) { _ in }
        ResultCard(error: .cancelled(note: "The worktree may be partly created"), context: nil) { _ in }
    }
    .frame(width: 480)
    .padding()
}

#Preview("Success with warnings") {
    ResultCard(successTitle: "Started \(PreviewSamples.features[0].workFeature)",
               warnings: ["Tunnel provisioning disabled in project configuration", "Adapter: no service URL detected"])
        .frame(width: 480)
        .padding()
}
