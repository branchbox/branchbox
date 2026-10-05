import BranchBoxKit
import BranchBoxStores
import SwiftUI

/// A finished start: the RESOLVED `work_feature` (never the typed title), its branch and folder, links, the
/// setup checklist, skipped modules and every warning (core's and the adapter's, a stash warning highlighted).
/// [Open in Editor] is the default action.
struct StartResultView: View {
    let flow: StartFlow
    let summary: StartSummary
    let record: OperationRecord
    let actions: FlowActions
    let onDone: () -> Void

    var body: some View {
        FlowSheetLayout(title: "\(summary.workFeature) is ready", subtitle: subtitle,
                        systemImage: warnings.isEmpty ? "checkmark.circle.fill" : "exclamationmark.triangle.fill",
                        tint: warnings.isEmpty ? .green : .orange) {
            ForEach(stashWarnings, id: \.self) { warning in
                StashWarning(text: warning)
            }
            FlowSection(title: "Feature", systemImage: "shippingbox") {
                Grid(alignment: .leadingFirstTextBaseline, horizontalSpacing: 12, verticalSpacing: 6) {
                    fact("Name", summary.workFeature, monospaced: true)
                    fact("Branch", summary.branchName.isEmpty ? "—" : summary.branchName, monospaced: true)
                    if let path = summary.worktreePath {
                        GridRow {
                            factLabel("Folder")
                            HStack(spacing: 6) {
                                Text(path)
                                    .font(.system(.callout, design: .monospaced))
                                    .lineLimit(1)
                                    .truncationMode(.middle)
                                    .textSelection(.enabled)
                                Button {
                                    actions.reveal(path)
                                } label: {
                                    Image(systemName: "folder")
                                }
                                .buttonStyle(.borderless)
                                .help("Reveal in Finder")
                                CopyButton(text: path, label: "Copy Path")
                                    .buttonStyle(.borderless)
                            }
                        }
                    }
                    GridRow {
                        factLabel("Runtime")
                        Label((summary.runtime?.provider ?? lastRuntime).label,
                              systemImage: (summary.runtime?.provider ?? lastRuntime).symbol)
                            .font(.callout)
                    }
                    if let mode = summary.mode {
                        fact("Setup", mode == StartFeatureRequest.Mode.minimal.rawValue ? "Quick" : "Full")
                    }
                }
            }
            if hasLinks {
                FlowSection(title: "Open", systemImage: "link") {
                    if let url = summary.urls.primary {
                        Link(destination: url) { Label(url.absoluteString, systemImage: "safari") }
                    }
                    if let tunnel = summary.urls.tunnel {
                        Link(destination: tunnel) { Label(tunnel.absoluteString, systemImage: "network") }
                    }
                    if !summary.urls.ports.isEmpty {
                        PortLinks(urls: summary.urls)
                    }
                }
            }
            if !summary.moduleOutcomes.isEmpty {
                FlowSection(title: "Setup", systemImage: "checklist") {
                    ModuleChecklist(outcomes: summary.moduleOutcomes)
                }
            }
            if !summary.skippedModules.isEmpty {
                FlowSection(title: "Skipped", systemImage: "forward") {
                    ForEach(summary.skippedModules, id: \.module) { skipped in
                        FlowFactRow(systemImage: "forward.fill", text: skipped.module, detail: skipped.reason)
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
            if let error = flow.launchError {
                FlowNotice(style: .error, text: error)
            }
        } footer: {
            Button("Show Log") {
                Task { _ = await actions.perform(.showLog(operation: record.id)) }
            }
            Spacer()
            Button("Done", action: onDone)
                .keyboardShortcut(.cancelAction)
            if let agent = flow.agentPlan {
                Button("Launch Agent") { Task { await flow.launch(agent, with: actions) } }
                    .disabled(!agent.isEnabled)
                    .help(agent.disabledReason ?? "Open the coding agent in a terminal")
            }
            if let editor = flow.editorPlan {
                Button("Open in Editor") {
                    Task {
                        await flow.launch(editor, with: actions)
                        if flow.launchError == nil { onDone() }
                    }
                }
                .keyboardShortcut(.defaultAction)
                .buttonStyle(.borderedProminent)
                .disabled(!editor.isEnabled)
                .help(editor.disabledReason ?? "")
            }
        }
    }

    private var subtitle: String {
        let parts = [summary.branchName.isEmpty ? nil : summary.branchName,
                     (summary.runtime?.provider ?? lastRuntime).label,
                     flow.projectStore?.displayName]
        return parts.compactMap { $0 }.joined(separator: " · ")
    }

    private var lastRuntime: RuntimeProvider { flow.lastRequest?.runtime ?? .container }

    private var hasLinks: Bool {
        summary.urls.primary != nil || summary.urls.tunnel != nil || !summary.urls.ports.isEmpty
    }

    /// Core's warnings, the adapter's, and the CLI preamble's, without repeats.
    private var warnings: [String] {
        var all: [String] = []
        for warning in [summary.preambleWarning].compactMap({ $0 }) + summary.warnings + (summary.adapter?.warnings ?? [])
        where !all.contains(warning) {
            all.append(warning)
        }
        return all
    }

    /// Warnings about stashed changes: the user's work moved somewhere they must know about.
    private var stashWarnings: [String] { warnings.filter { $0.localizedCaseInsensitiveContains("stash") } }
    private var otherWarnings: [String] { warnings.filter { !$0.localizedCaseInsensitiveContains("stash") } }

    private func fact(_ label: String, _ value: String, monospaced: Bool = false) -> some View {
        GridRow {
            factLabel(label)
            Text(value)
                .font(monospaced ? .system(.callout, design: .monospaced).weight(.medium) : .callout)
                .lineLimit(1)
                .truncationMode(.middle)
                .textSelection(.enabled)
        }
    }

    private func factLabel(_ text: String) -> some View {
        Text(text)
            .font(.callout)
            .foregroundStyle(.secondary)
            .gridColumnAlignment(.trailing)
    }
}

/// A stash warning stands out: the user's uncommitted work was moved into `git stash`.
private struct StashWarning: View {
    let text: String

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: 10) {
            Image(systemName: "tray.and.arrow.down.fill")
                .foregroundStyle(.orange)
                .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: 2) {
                Text("Your changes were stashed").font(.headline)
                Text(text)
                    .textSelection(.enabled)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
        .padding(12)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(Color.yellow.opacity(0.18), in: RoundedRectangle(cornerRadius: 8))
        .overlay(RoundedRectangle(cornerRadius: 8).strokeBorder(Color.orange.opacity(0.5)))
        .accessibilityElement(children: .combine)
    }
}

/// A start that failed or was stopped: the cause card with its recoveries, [Edit and Retry], and [Show Feature]
/// when a partial registry entry appeared.
struct StartFailureView: View {
    let flow: StartFlow
    let record: OperationRecord
    let actions: FlowActions
    let onClose: () -> Void

    var body: some View {
        FlowSheetLayout(title: title, subtitle: flow.projectStore?.displayName, systemImage: symbol, tint: tint) {
            if let error = record.failure {
                ResultCard(error: error, context: record.context,
                           diagnosticReport: { actions.diagnosticReport(for: record) }) { action in
                    Task { flow.adopt(await actions.perform(action)) }
                }
            } else if case .cancelled(let note) = record.state {
                ResultCard(error: .cancelled(note: note), context: record.context) { _ in }
                FlowNotice(style: .info, text: "Anything the start left behind shows in the sidebar as “Interrupted” or "
                    + "“Unregistered worktree”; you can remove it from there.")
            }
            if let partial = flow.partialFeature {
                FlowNotice(style: .warning, text: "The registry has an entry for \(partial.workFeature) from this start.")
            }
            if let error = flow.dispatchError {
                FlowNotice(style: .error, text: error)
            }
            DisclosureGroup("Log") {
                LogView(lines: record.log.lines, archiveURL: record.log.archiveURL, firstIndex: record.log.droppedLines,
                        revision: record.log.revision)
                    .frame(height: 220)
                    .overlay(RoundedRectangle(cornerRadius: 6).strokeBorder(.separator))
            }
        } footer: {
            if let partial = flow.partialFeature, let project = flow.lastRequest?.project {
                Button("Show Feature") {
                    flow.model.post(.select(.feature(projectPath: project.path, name: partial.workFeature)))
                    onClose()
                }
            }
            Spacer()
            Button("Close", action: onClose)
                .keyboardShortcut(.cancelAction)
            Button("Edit and Retry") { flow.editAndRetry() }
                .keyboardShortcut(.defaultAction)
                .buttonStyle(.borderedProminent)
        }
    }

    private var title: String {
        let name = flow.lastRequest?.name ?? "the feature"
        if case .cancelled = record.state { return "Stopped starting \(name)" }
        return "Couldn't start \(name)"
    }

    /// The cause card's own symbol and colour: a refusal is orange, a failure red, a stop grey.
    private var symbol: String {
        if case .cancelled = record.state { return "stop.circle.fill" }
        return record.failure?.presentation(context: record.context).symbol ?? "xmark.octagon.fill"
    }

    private var tint: Color {
        if case .cancelled = record.state { return .secondary }
        return record.failure?.presentation(context: record.context).tint.color ?? .red
    }
}
