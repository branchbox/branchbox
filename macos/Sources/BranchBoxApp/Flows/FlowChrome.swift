import AppKit
import BranchBoxKit
import BranchBoxStores
import SwiftUI

// Shared pieces of the operation sheets (Start, Teardown, Prune, Stray): the sheet layout, section boxes, the
// dispatch helper, the Stop confirmation (D-18) and the handler that performs a result card's recoveries.

/// A sheet's frame: a header with an icon, title and subtitle; scrolling content; a footer button bar.
struct FlowSheetLayout<Content: View, Footer: View>: View {
    let title: String
    var subtitle: String?
    let systemImage: String
    var tint: Color = .accentColor
    @ViewBuilder var content: Content
    @ViewBuilder var footer: Footer

    var body: some View {
        VStack(spacing: 0) {
            HStack(alignment: .center, spacing: 12) {
                Image(systemName: systemImage)
                    .font(.system(size: 20, weight: .medium))
                    .foregroundStyle(tint)
                    .frame(width: 36, height: 36)
                    .background(tint.opacity(0.12), in: RoundedRectangle(cornerRadius: 8))
                    .accessibilityHidden(true)
                VStack(alignment: .leading, spacing: 2) {
                    Text(title)
                        .font(.title3.weight(.semibold))
                        .lineLimit(1)
                        .truncationMode(.middle)
                    if let subtitle {
                        Text(subtitle)
                            .font(.callout)
                            .foregroundStyle(.secondary)
                            .lineLimit(1)
                            .truncationMode(.middle)
                    }
                }
                Spacer(minLength: 0)
            }
            .padding(.horizontal, 20)
            .padding(.vertical, 16)
            .accessibilityElement(children: .combine)
            .accessibilityAddTraits(.isHeader)
            Divider()
            ScrollView {
                VStack(alignment: .leading, spacing: 16) {
                    content
                }
                .padding(20)
                .frame(maxWidth: .infinity, alignment: .leading)
            }
            Divider()
            HStack(spacing: 8) {
                footer
            }
            .padding(.horizontal, 20)
            .padding(.vertical, 12)
        }
    }
}

/// A titled group inside a sheet: a caption-sized heading above an inset box.
struct FlowSection<Content: View>: View {
    let title: String
    var systemImage: String?
    var trailing: String?
    /// A heading colour for sections about loss (red) or risk; nil is the usual secondary heading.
    var tint: Color?
    @ViewBuilder var content: Content

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack(spacing: 6) {
                if let systemImage {
                    Image(systemName: systemImage).accessibilityHidden(true)
                }
                Text(title)
                Spacer(minLength: 4)
                if let trailing {
                    Text(trailing).monospacedDigit()
                }
            }
            .font(.subheadline.weight(.semibold))
            .foregroundStyle(tint ?? .secondary)
            .accessibilityAddTraits(.isHeader)
            VStack(alignment: .leading, spacing: 8) {
                content
            }
            .padding(12)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(Color(nsColor: .controlBackgroundColor), in: RoundedRectangle(cornerRadius: 8))
            .overlay(RoundedRectangle(cornerRadius: 8)
                .strokeBorder(tint?.opacity(0.45) ?? Color(nsColor: .separatorColor).opacity(0.6)))
        }
    }
}

/// One fact line in a section: an icon in the given tint, the text, and an optional secondary detail.
struct FlowFactRow: View {
    let systemImage: String
    var tint: Color = .secondary
    let text: String
    var detail: String?

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: 8) {
            Image(systemName: systemImage)
                .foregroundStyle(tint)
                .frame(width: 16)
                .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: 2) {
                Text(text)
                    .fixedSize(horizontal: false, vertical: true)
                if let detail {
                    Text(detail)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .textSelection(.enabled)
                        .fixedSize(horizontal: false, vertical: true)
                }
            }
        }
        .accessibilityElement(children: .combine)
    }
}

/// A monospaced list of paths that scrolls past `visibleLines`.
struct FlowPathList: View {
    let lines: [String]
    var visibleLines = 6

    var body: some View {
        let list = VStack(alignment: .leading, spacing: 2) {
            ForEach(Array(lines.enumerated()), id: \.offset) { _, line in
                Text(line)
                    .font(.system(.callout, design: .monospaced))
                    .lineLimit(1)
                    .truncationMode(.middle)
                    .textSelection(.enabled)
                    .frame(maxWidth: .infinity, alignment: .leading)
            }
        }
        Group {
            if lines.count > visibleLines {
                ScrollView { list }.frame(height: CGFloat(visibleLines) * 18)
            } else {
                list
            }
        }
        .padding(8)
        .background(.background.opacity(0.7), in: RoundedRectangle(cornerRadius: 6))
    }
}

/// A field's own validation line: a small icon and text right under the control it is about.
struct FlowFieldMessage: View {
    enum Style { case error, info }

    let style: Style
    let text: String

    var body: some View {
        Label {
            Text(text).fixedSize(horizontal: false, vertical: true)
        } icon: {
            Image(systemName: style == .error ? "exclamationmark.circle.fill" : "info.circle")
        }
        .font(.callout)
        .foregroundStyle(style == .error ? Color.red : .secondary)
        .accessibilityLabel((style == .error ? "Problem: " : "Note: ") + text)
    }
}

/// An inline notice inside a sheet: why something can't run, a rejected dispatch, a launch failure.
struct FlowNotice: View {
    enum Style { case info, warning, error }

    let style: Style
    let text: String

    var body: some View {
        Label {
            Text(text)
                .fixedSize(horizontal: false, vertical: true)
                .textSelection(.enabled)
        } icon: {
            Image(systemName: symbol).foregroundStyle(tint)
        }
        .font(.callout)
        .padding(.horizontal, 10)
        .padding(.vertical, 8)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(tint.opacity(0.1), in: RoundedRectangle(cornerRadius: 8))
    }

    private var symbol: String {
        switch style {
        case .info: "info.circle.fill"
        case .warning: "exclamationmark.triangle.fill"
        case .error: "xmark.octagon.fill"
        }
    }

    private var tint: Color {
        switch style {
        case .info: .blue
        case .warning: .orange
        case .error: .red
        }
    }
}

// MARK: Dispatch

enum FlowDispatch {
    /// The record a dispatch started or queued, or the text explaining why nothing started.
    @MainActor static func record(of result: DispatchResult) -> Result<OperationRecord, FlowDispatchError> {
        switch result {
        case .started(let record), .queued(let record, _):
            .success(record)
        case .rejected(let reason):
            .failure(FlowDispatchError(message: reason))
        case .unavailable(let error):
            .failure(FlowDispatchError(message: error.oneLine))
        }
    }
}

extension BackendError {
    /// The presentation's title and message as one sentence pair, for inline notices.
    var oneLine: String {
        let presentation = presentation()
        guard !presentation.message.isEmpty, presentation.message != presentation.title else { return presentation.title }
        let title = presentation.title.hasSuffix(".") ? presentation.title : presentation.title + "."
        return "\(title) \(presentation.message)"
    }
}

struct FlowDispatchError: Error, Hashable {
    let message: String
}

extension OperationRecord {
    /// The record's state as the flow sheets branch on it.
    var isFinished: Bool { !isCancellable }

    /// The failure a finished record carries, if any.
    var failure: BackendError? {
        if case .failed(let error) = state { return error }
        return nil
    }
}

// MARK: Stop (D-18)

/// Holds the record whose Stop the user asked for until they confirm. Stopping never happens without the
/// confirmation, whose copy warns about a partial worktree and registry corruption on CLIs without
/// `registry-lock` and `write-ahead-start`.
@MainActor @Observable final class StopConfirmationState {
    private(set) var pending: OperationRecord?

    /// Asks before stopping `record`; nothing is cancelled yet.
    func request(_ record: OperationRecord) {
        guard record.isCancellable else { return }
        pending = record
    }

    /// The confirmation's title, message and buttons for the pending record.
    func confirmation(capabilities: Set<Capability>) -> CancelConfirmation? {
        pending.map { CancelConfirmation(kind: $0.kind, title: $0.title, capabilities: capabilities) }
    }

    func confirm(in operations: OperationStore) {
        if let pending { operations.cancel(pending.id) }
        pending = nil
    }

    func dismiss() {
        pending = nil
    }
}

extension View {
    /// Presents the D-18 Stop confirmation for `state`'s pending record.
    func stopConfirmation(_ state: StopConfirmationState, model: AppModel) -> some View {
        modifier(StopConfirmationModifier(state: state, model: model))
    }
}

private struct StopConfirmationModifier: ViewModifier {
    let state: StopConfirmationState
    let model: AppModel

    func body(content: Content) -> some View {
        let confirmation = state.confirmation(capabilities: model.environment.identity?.capabilities ?? [])
        content.confirmationDialog(confirmation?.title ?? "", isPresented: Binding(
            get: { state.pending != nil },
            set: { if !$0 { state.dismiss() } }
        ), titleVisibility: .visible) {
            Button(confirmation?.stopLabel ?? "Stop", role: .destructive) { state.confirm(in: model.operations) }
            Button(confirmation?.keepLabel ?? "Keep Running", role: .cancel) { state.dismiss() }
        } message: {
            Text(confirmation?.message ?? "")
        }
    }
}

// MARK: Recoveries and host actions

/// Performs what a result card asks for: `.retry` re-dispatches through the model (and returns the new record);
/// everything else is a host action (Finder, Terminal, Activity, Diagnostics) or a refresh.
@MainActor struct FlowActions {
    let model: AppModel
    var openWindow: (String) -> Void = { _ in }
    var launcher = HostLauncher()

    /// Runs `action`; a retry's new record or rejection comes back for the sheet to show.
    func perform(_ action: RecoveryAction) async -> Result<OperationRecord, FlowDispatchError>? {
        switch action {
        case .retry:
            guard let result = model.actions.perform(action) else { return nil }
            return FlowDispatch.record(of: result)
        case .runInTerminal(let command, let directory, _):
            return await launch(HostLaunchPlan(kind: .terminalScript(
                script: HostLaunchPlan.script(cd: directory, exec: HostLaunchPlan.shellCommand(command)),
                terminal: model.settings.preferredTerminal), workingDirectory: directory))
        case .revealInFinder(let path):
            reveal(path)
        case .copyCommand(let command, _):
            Pasteboard.general.copy(command)
        case .openDoctor:
            openWindow(SceneID.diagnostics)
        case .locateCLI:
            openWindow(SceneID.diagnostics)
        case .refresh(let project):
            model.projects.project(project)?.requestRefresh(.manual)
        case .showLog(let operation):
            ActivitySelection.select(operation)
            openWindow(SceneID.activity)
        }
        return nil
    }

    /// Opens `plan` (editor, terminal, agent); a failure comes back as text for an inline notice.
    func launch(_ plan: HostLaunchPlan) async -> Result<OperationRecord, FlowDispatchError>? {
        do {
            try await launcher.launch(plan)
            return nil
        } catch let error as HostLaunchError {
            return .failure(FlowDispatchError(message: error.message))
        } catch {
            return .failure(FlowDispatchError(message: String(describing: error)))
        }
    }

    /// Selects `path` in Finder, or its closest existing parent when it is gone.
    func reveal(_ path: String) {
        var url = URL(fileURLWithPath: path)
        while !FileManager.default.fileExists(atPath: url.path), url.pathComponents.count > 1 {
            url.deleteLastPathComponent()
        }
        NSWorkspace.shared.activateFileViewerSelecting([url])
    }

    /// A login shell in `folder`, in the user's terminal.
    func openTerminal(in folder: String) async -> Result<OperationRecord, FlowDispatchError>? {
        await launch(HostLaunchPlan(kind: .terminalScript(
            script: HostLaunchPlan.script(cd: folder, exec: Self.loginShell),
            terminal: model.settings.preferredTerminal), workingDirectory: folder))
    }

    /// The user's login shell, as feature terminals start it.
    static let loginShell = "\"${SHELL:-/bin/zsh}\" -l"

    /// The diagnostic report for a failed record, redacted.
    func diagnosticReport(for record: OperationRecord) -> String {
        DiagnosticReport(identity: model.environment.identity, environment: model.environment.summary,
                         operationTitle: record.title, error: record.failure, context: record.context,
                         secrets: Array(model.settings.extraEnvironment.values)).markdown()
    }
}

extension Pluralized {
    static func count(_ count: Int, _ singular: String, _ plural: String? = nil) -> String {
        "\(count.formatted()) \(count == 1 ? singular : (plural ?? singular + "s"))"
    }
}

/// "1 file", "3 files": counted nouns in sheet copy.
enum Pluralized {}
