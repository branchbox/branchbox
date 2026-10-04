import BranchBoxKit
import BranchBoxPreview
import BranchBoxStores
import Observation
import SwiftUI

/// One worktree's line in the Update All Workspaces results.
struct SyncRowPresentation: Identifiable, Hashable {
    let feature: String
    let symbol: String
    let tint: StatusTint
    let status: String
    /// The CLI's error for this worktree, the skip reason, or the file count.
    private let error: String?
    private let note: String?
    /// The feature's folder, for [Reveal Folder] on a failed row and the path in its error (from the registry
    /// when the CLI didn't say).
    var worktreePath: String?
    var detail: String? { error.map { Self.explain($0, worktreePath: worktreePath) } ?? note }
    var id: String { feature }
    var isFailure: Bool { tint == .red }

    init(_ row: SyncReport.Row, dryRun: Bool) {
        feature = row.feature
        switch row.status {
        case .synced:
            symbol = "checkmark.circle.fill"
            tint = .green
            status = "Updated"
        case .wouldSync:
            symbol = "arrow.triangle.2.circlepath"
            tint = .blue
            status = "Will update"
        case .skipped:
            symbol = "minus.circle"
            tint = .gray
            status = "Skipped"
        case .failed:
            symbol = "xmark.octagon.fill"
            tint = .red
            status = dryRun ? "Can't update" : "Failed"
        case .unknown:
            symbol = "questionmark.circle"
            tint = .gray
            status = "Unknown"
        }
        let files = row.files.isEmpty ? nil : (row.files.count == 1 ? row.files[0] : "\(row.files.count) files")
        worktreePath = row.worktreePath
        error = row.error
        note = row.skipReason ?? files
    }

    /// The CLI's I/O errors in words: "IO error: Permission denied (os error 13)" becomes "BranchBox couldn't write
    /// to ~/…/beta/.devcontainer (Permission denied)." Other errors stay as they are.
    static func explain(_ error: String, worktreePath: String?) -> String {
        let target = worktreePath.map { ($0 as NSString).abbreviatingWithTildeInPath + "/.devcontainer" } ?? "the feature's .devcontainer folder"
        let reasons: [(String, String)] = [
            ("os error 13", "Permission denied"), ("os error 1)", "Operation not permitted"),
            ("os error 28", "the disk is full"), ("os error 30", "the disk is read-only"),
        ]
        for (code, reason) in reasons where error.contains(code) {
            return "BranchBox couldn't write to \(target) (\(reason))."
        }
        return error
    }
}

/// The Update All Workspaces sheet's state: the strategy, the preview (dry run) and the real run.
@MainActor @Observable final class SyncSheetModel {
    var strategy: SyncStrategy
    private(set) var preview: OperationRecord?
    private(set) var run: OperationRecord?
    private(set) var dispatchProblem: String?

    init(strategy: SyncStrategy = .copy) {
        self.strategy = strategy
    }

    func startPreview(project: ProjectRef, using model: AppModel) {
        if let record = dispatch(.syncDevcontainers(SyncRequest(project: project, strategy: strategy, dryRun: true)), using: model) {
            preview = record
        }
    }

    func apply(project: ProjectRef, using model: AppModel) {
        if let record = dispatch(.syncDevcontainers(SyncRequest(project: project, strategy: strategy)), using: model) {
            run = record
        }
    }

    /// A preview describes one strategy; switching Copy/Link makes it stale.
    func strategyChanged() {
        if run == nil { preview = nil }
    }

    /// The record whose results show: the run once there is one, else the preview.
    var current: OperationRecord? { run ?? preview }

    /// A dry-run preview is still useful, but replacing setup files waits until known Git damage is inspected.
    static func applyBlockedReason(features: [FeatureRecord]) -> String? {
        let broken = features.filter { $0.status == .active && $0.worktreeIssue != nil }
        guard !broken.isEmpty else { return nil }
        return broken.map { "\($0.workFeature): \($0.worktreeIssue ?? "Git worktree needs repair")" }.joined(separator: "\n")
    }

    /// Rows for a record's report; failures are rows like any other, whatever the CLI's exit status was.
    static func rows(for record: OperationRecord) -> [SyncRowPresentation] {
        guard case .sync(let report)? = record.result else { return [] }
        return rows(from: report)
    }

    static func rows(from report: SyncReport) -> [SyncRowPresentation] {
        report.rows.map { SyncRowPresentation($0, dryRun: report.dryRun) }
    }

    /// "3 workspaces will be updated", "2 updated · 1 failed".
    static func summary(of report: SyncReport) -> String {
        let failed = report.failedCount
        let skipped = report.rows.filter { $0.status == .skipped }.count
        if report.dryRun {
            let will = report.rows.filter { $0.status == .wouldSync }.count
            var parts = [will == 1 ? "1 workspace will be updated" : "\(will) workspaces will be updated"]
            if failed > 0 { parts.append("\(failed) can't be") }
            if skipped > 0 { parts.append("\(skipped) skipped") }
            return parts.joined(separator: " · ")
        }
        let updated = report.rows.filter { $0.status == .synced }.count
        var parts = ["\(updated) updated"]
        if failed > 0 { parts.append("\(failed) failed") }
        if skipped > 0 { parts.append("\(skipped) skipped") }
        return parts.joined(separator: " · ")
    }

    private func dispatch(_ request: OperationRequestContext, using model: AppModel) -> OperationRecord? {
        dispatchProblem = nil
        switch model.actions.dispatch(request) {
        case .started(let record), .queued(let record, _):
            return record
        case .rejected(let reason):
            dispatchProblem = reason
        case .unavailable(let error):
            dispatchProblem = error.presentation().message
        }
        return nil
    }
}

/// Updates every active feature's .devcontainer from main: pick copy or link, [Preview] the dry run, then
/// [Update Workspaces]. Each worktree gets a result row, and a failed one is shown even when the CLI exits 0.
struct SyncDevcontainersSheet: View {
    let project: ProjectRef

    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    @Environment(\.openWindow) private var openWindow
    @State private var sheet: SyncSheetModel
    @State private var confirmingStop = false

    init(project: ProjectRef) {
        self.project = project
        _sheet = State(initialValue: SyncSheetModel())
    }

    /// Additive: a sheet with prepared state (render tests, previews).
    init(project: ProjectRef, model: SyncSheetModel) {
        self.project = project
        _sheet = State(initialValue: model)
    }

    private var store: ProjectStore? { model.projects.project(project) }
    private var activeCount: Int { store?.features.filter { $0.status == .active }.count ?? 0 }
    private var applyBlockedReason: String? { SyncSheetModel.applyBlockedReason(features: store?.features ?? []) }

    var body: some View {
        ProjectSheetScaffold(title: "Update All Workspaces",
                             subtitle: "Give every active feature the latest dev container setup from main.",
                             systemImage: "arrow.triangle.2.circlepath") {
            VStack(alignment: .leading, spacing: 16) {
                if sheet.run == nil { options }
                if let reason = applyBlockedReason {
                    ProjectNotice(style: .warning, title: "Inspect Git worktrees before updating", message: reason)
                }
                if let problem = sheet.dispatchProblem {
                    ProjectNotice(style: .error, title: "Couldn't start", message: problem)
                }
                if let record = sheet.current {
                    results(record)
                } else {
                    Spacer()
                }
            }
            .padding(20)
        } footer: {
            footer
        }
        .frame(minWidth: 560, idealWidth: 560, minHeight: 460, idealHeight: 520)
        .onChange(of: sheet.strategy) { sheet.strategyChanged() }
        .confirmationDialog(stopConfirmation.title, isPresented: $confirmingStop, titleVisibility: .visible) {
            Button(stopConfirmation.stopLabel, role: .destructive) {
                if let run = sheet.run { model.operations.cancel(run.id) }
            }
            Button(stopConfirmation.keepLabel, role: .cancel) {}
        } message: {
            Text(stopConfirmation.message)
        }
    }

    /// D-18: a running operation can always be stopped, after a confirmation.
    private var stopConfirmation: CancelConfirmation {
        CancelConfirmation(kind: .syncDevcontainers, title: sheet.run?.title ?? "Update All Workspaces",
                           capabilities: model.environment.identity?.capabilities ?? [])
    }

    private var options: some View {
        VStack(alignment: .leading, spacing: 12) {
            Picker("Method", selection: $sheet.strategy) {
                Text("Copy files").tag(SyncStrategy.copy)
                Text("Link to main").tag(SyncStrategy.symlink)
            }
            .pickerStyle(.segmented)
            .disabled(sheet.preview.map { OperationSummary($0).isRunning } ?? false)
            Text(sheet.strategy == .copy
                 ? "Each feature gets its own copy, so you can change one without affecting the others."
                 : "Each feature links to main's files, so later changes in main apply everywhere at once.")
                .font(.callout)
                .foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)
            ProjectNotice(style: .info,
                          title: activeCount == 1 ? "1 active feature will be updated" : "\(activeCount) active features will be updated",
                          message: "Only active features are updated; each one's .devcontainer folder is replaced with main's. Changes made there by hand are lost.")
        }
    }

    @ViewBuilder private func results(_ record: OperationRecord) -> some View {
        let summary = OperationSummary(record)
        VStack(alignment: .leading, spacing: 8) {
            if summary.isRunning {
                HStack(spacing: 8) {
                    ProgressView().controlSize(.small)
                    Text(record === sheet.run ? "Updating workspaces…" : "Checking what would change…")
                }
            } else if case .sync(let report)? = record.result {
                let rows = SyncSheetModel.rows(from: report).map { row in
                    var row = row
                    if row.worktreePath == nil { row.worktreePath = store?.feature(named: row.feature)?.worktreePath }
                    return row
                }
                HStack(spacing: 6) {
                    Image(systemName: report.failedCount > 0 ? "exclamationmark.triangle.fill"
                          : (report.dryRun ? "eye" : "checkmark.circle.fill"))
                        .foregroundStyle(report.failedCount > 0 ? .orange : (report.dryRun ? .secondary : .green))
                        .accessibilityHidden(true)
                    Text(report.dryRun ? "Preview: \(SyncSheetModel.summary(of: report))" : SyncSheetModel.summary(of: report))
                        .font(.headline)
                }
                if rows.isEmpty {
                    Text(report.rawText?.isEmpty == false ? report.rawText ?? "" : "There are no active features to update.")
                        .font(report.rawText?.isEmpty == false ? .callout.monospaced() : .callout)
                        .foregroundStyle(.secondary)
                        .textSelection(.enabled)
                } else {
                    SyncResultList(rows: rows, onReveal: report.dryRun ? nil : { path in ProjectActions.reveal(path) })
                }
            } else if case .failed(let error) = summary.state {
                ResultCard(error: error, context: record.context, operationID: record.id) { action in
                    switch action {
                    case .showLog:
                        model.post(.showActivity(operation: record.id))
                        dismiss()
                    case .openDoctor:
                        openWindow(id: SceneID.diagnostics)
                    default:
                        _ = model.actions.perform(action)
                    }
                }
            } else if case .cancelled(let note) = summary.state {
                ProjectNotice(style: .warning, title: "Stopped", message: note)
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
    }

    @ViewBuilder private var footer: some View {
        let previewRunning = sheet.preview.map { OperationSummary($0).isRunning } ?? false
        if let run = sheet.run {
            Spacer()
            if OperationSummary(run).isRunning {
                Button("Run in Background") { dismiss() }
                    .keyboardShortcut(.cancelAction)
                Button("Stop", role: .destructive) { confirmingStop = true }
                    .keyboardShortcut(".", modifiers: .command)
                    .disabled(!run.isCancellable)
            } else {
                if case .sync(let report)? = run.result, report.failedCount > 0 {
                    // The CLI updates every active feature at once; running it again gives the ones that already
                    // succeeded the same files, so it is safe.
                    Button("Try Again") { sheet.apply(project: project, using: model) }
                        .help("Run the update again; features that were already updated get the same files")
                        .disabled(model.environment.identity == nil || applyBlockedReason != nil)
                        .accessibilityIdentifier("sync.retry")
                }
                Button("Done") { dismiss() }
                    .buttonStyle(.borderedProminent)
                    .keyboardShortcut(.defaultAction)
            }
        } else {
            Button("Preview") { sheet.startPreview(project: project, using: model) }
                .help("Shows what would change, without changing anything")
                .disabled(previewRunning || model.environment.identity == nil)
            Spacer()
            Button("Cancel", role: .cancel) { dismiss() }
                .keyboardShortcut(.cancelAction)
            Button("Update Workspaces") { sheet.apply(project: project, using: model) }
                .buttonStyle(.borderedProminent)
                .keyboardShortcut(.defaultAction)
                .disabled(previewRunning || activeCount == 0 || model.environment.identity == nil || applyBlockedReason != nil)
                .help(applyBlockedReason ?? "Update the active features' dev container setup")
                .accessibilityIdentifier("sync.apply")
        }
    }
}

/// Result rows: symbol, feature, status word and the error or file count; a failed row can reveal its folder.
struct SyncResultList: View {
    let rows: [SyncRowPresentation]
    var onReveal: ((String) -> Void)?

    var body: some View {
        Group {
            if rows.count > 6 {
                ScrollView { list }
            } else {
                list
            }
        }
        .background(Color(nsColor: .controlBackgroundColor), in: RoundedRectangle(cornerRadius: 8))
        .overlay(RoundedRectangle(cornerRadius: 8).strokeBorder(Color(nsColor: .separatorColor)))
    }

    private var list: some View {
            VStack(alignment: .leading, spacing: 0) {
                ForEach(Array(rows.enumerated()), id: \.element.id) { index, row in
                    if index > 0 { Divider() }
                    HStack(alignment: .firstTextBaseline, spacing: 10) {
                        Image(systemName: row.symbol)
                            .foregroundStyle(row.tint.color)
                            .frame(width: 18)
                            .accessibilityHidden(true)
                        VStack(alignment: .leading, spacing: 2) {
                            Text(row.feature)
                                .fontWeight(.medium)
                                .lineLimit(1)
                                .truncationMode(.middle)
                            if let detail = row.detail {
                                Text(detail)
                                    .font(.caption)
                                    .foregroundStyle(row.isFailure ? Color.red : .secondary)
                                    .textSelection(.enabled)
                                    .fixedSize(horizontal: false, vertical: true)
                            }
                        }
                        Spacer(minLength: 8)
                        if row.isFailure, let path = row.worktreePath, let onReveal {
                            Button("Reveal Folder") { onReveal(path) }
                                .controlSize(.small)
                                .help("Show \(row.feature)'s folder in Finder")
                        }
                        Text(row.status)
                            .font(.caption)
                            .foregroundStyle(row.isFailure ? Color.red : .secondary)
                    }
                    .padding(.vertical, 7)
                    .padding(.horizontal, 10)
                    .accessibilityElement(children: .combine)
                }
            }
    }
}

#Preview("Update all workspaces") {
    SyncDevcontainersSheet(project: PreviewSamples.project)
        .environment(ProjectsPreviewModel.model())
}
