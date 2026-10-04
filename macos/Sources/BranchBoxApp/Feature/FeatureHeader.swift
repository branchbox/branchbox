import BranchBoxKit
import BranchBoxPreview
import SwiftUI

/// The top of the feature detail: colour, name, status and runtime, the branch line, and the everyday actions —
/// the editor (one prominent split button), Terminal, the coding agent and the feature's URLs, with Tear Down set
/// apart at the trailing end (it opens the confirmation sheet; it has no Return shortcut).
struct FeatureHeader: View {
    let record: FeatureRecord
    let feature: FeatureRef
    let availability: FeatureActionAvailability
    let attention: AttentionReason?
    /// Opens the teardown sheet; nil hides the button (previews, the removed state).
    var onTearDown: (() -> Void)?

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            titleRow
            detailRow
            if !availability.isRemoved {
                actionBar
                    .padding(.top, 4)
            }
        }
    }

    // MARK: Title

    private var titleRow: some View {
        HStack(alignment: .center, spacing: 10) {
            ColorSwatch(hex: record.color, size: 14)
            Text(record.workFeature)
                .font(.title.weight(.semibold))
                .lineLimit(1)
                .truncationMode(.middle)
                .help(record.workFeature)
                .textSelection(.enabled)
                .accessibilityAddTraits(.isHeader)
                .layoutPriority(-1)
            StatusBadge(status: record.status)
                .fixedSize()
            if let attention, attention.label != record.status.label {
                StatusBadge(attention: attention)
                    .fixedSize()
            }
            Spacer(minLength: 0)
        }
    }

    private var detailRow: some View {
        HStack(alignment: .firstTextBaseline, spacing: 8) {
            RuntimeBadge(provider: record.runtime.provider)
                .fixedSize()
            if record.startMode == StartFeatureRequest.Mode.minimal.rawValue {
                Tag(title: "Quick", systemImage: "hare", tint: .blue)
                    .help("Started in Quick mode: only the worktree, without the full environment")
            }
            let subtitle = FeaturePresentation.subtitle(for: record)
            if !subtitle.isEmpty {
                Text("·").foregroundStyle(.tertiary).accessibilityHidden(true)
                Text(subtitle)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
                    .truncationMode(.middle)
                    .help(subtitle)
            }
        }
        .font(.callout)
    }

    // MARK: Actions

    private var actionBar: some View {
        HStack(alignment: .top, spacing: 12) {
            // One line at every usual width: full labels when they fit, then Terminal and Open URL as icons with
            // help text, and only in a very narrow pane do the buttons wrap.
            ViewThatFits(in: .horizontal) {
                actionButtons(compact: false)
                actionButtons(compact: true)
                FlowLayout(spacing: 8) {
                    editorButton
                    terminalButton(compact: true)
                    agentButton
                    urlMenu(compact: true)
                }
            }
            if let onTearDown {
                Spacer(minLength: 0)
                Divider()
                    .frame(height: 20)
                Button(role: .destructive, action: onTearDown) {
                    Label("Tear Down…", systemImage: "trash")
                        .foregroundStyle(availability.teardown.isEnabled ? Color.red : Color.secondary)
                }
                .fixedSize()
                .disabled(!availability.teardown.isEnabled)
                .help(availability.teardown.disabledReason
                      ?? "Remove the worktree and environment of \(record.workFeature); you confirm first")
                .accessibilityIdentifier("feature.header.teardown")
            }
        }
    }

    private func actionButtons(compact: Bool) -> some View {
        HStack(spacing: 8) {
            editorButton
            terminalButton(compact: compact)
            agentButton
            urlMenu(compact: compact)
        }
        .fixedSize(horizontal: true, vertical: false)
    }

    /// The preferred editor as a split button (click opens it, the chevron lists the other ways to open the folder),
    /// built like [Open URL] so the row has one split-button style. When the preferred editor can't run but another
    /// entry can, the click does nothing and the help says why; the chevron still opens the menu.
    private var editorButton: some View {
        let plan = availability.primaryEditor
        return Menu {
            Section("Open Folder In") {
                launchButton("VS Code", plan: availability.editor(.vscode), verb: "editor.vscode")
                launchButton("Cursor", plan: availability.editor(.cursor), verb: "editor.cursor")
                if case .custom(let path) = availability.preferences.editor {
                    launchButton(LaunchPreferences.editorName(.custom(appPath: path)),
                                 plan: availability.editor(.custom(appPath: path)), verb: "editor.custom")
                }
            }
            if record.runtime.provider == .container {
                Divider()
                launchButton("Open in Dev Container", plan: availability.devContainer, verb: "editor.devcontainer")
            }
        } label: {
            Label(availability.primaryEditorTitle, systemImage: "chevron.left.forwardslash.chevron.right")
        } primaryAction: {
            if plan.isEnabled { HostLaunchFeedback.shared.launch(plan, for: feature) }
        }
        .menuStyle(.button)
        .fixedSize()
        .disabled(!plan.isEnabled && !anyEditorEnabled)
        .help(plan.isEnabled ? "\(availability.primaryEditorTitle) (other editors in the menu)"
              : (plan.disabledReason ?? "No editor can open it"))
        .accessibilityLabel(availability.primaryEditorTitle)
        .accessibilityIdentifier("feature.action.editor")
    }

    /// Some entry of the editor menu can run (otherwise the chevron is disabled rather than a menu of greyed items).
    private var anyEditorEnabled: Bool {
        var plans = [availability.editor(.vscode), availability.editor(.cursor)]
        if case .custom(let path) = availability.preferences.editor { plans.append(availability.editor(.custom(appPath: path))) }
        if record.runtime.provider == .container { plans.append(availability.devContainer) }
        return plans.contains { $0.isEnabled }
    }

    @ViewBuilder private func terminalButton(compact: Bool) -> some View {
        let plan = availability.terminal
        if let command = plan.copyCommand {
            // A sandbox has no host folder shell yet; offer the command that opens one instead.
            CopyButton(text: command, label: "Copy Shell Command", showsTitle: !compact)
                .help("\(plan.disabledReason ?? ""). Copies: \(command)")
                .accessibilityIdentifier("feature.action.copyShellCommand")
        } else {
            Button {
                HostLaunchFeedback.shared.launch(plan, for: feature)
            } label: {
                Label("Terminal", systemImage: "terminal")
                    .labelStyle(ActionLabelStyle(compact: compact))
            }
            .disabled(!plan.isEnabled)
            .help(plan.disabledReason ?? "Open a shell in the feature's folder")
            .accessibilityIdentifier("feature.action.terminal")
        }
    }

    private var agentButton: some View {
        let plan = availability.agent
        let name = availability.preferences.agentName
        return Button {
            HostLaunchFeedback.shared.launch(plan, for: feature)
        } label: {
            Label("Launch \(name)", systemImage: "sparkles")
        }
        .disabled(!plan.isEnabled)
        .help(plan.disabledReason ?? (availability.preferences.passPrompt && availability.hasPrompt
            ? "Start \(name) in the feature's folder with its prompt" : "Start \(name) in the feature's folder"))
        .accessibilityIdentifier("feature.action.agent")
    }

    @ViewBuilder private func urlMenu(compact: Bool) -> some View {
        let links = FeatureLinks.menuLinks(for: record)
        Menu {
            ForEach(Array(links.enumerated()), id: \.offset) { _, link in
                Button(link.title) { HostLaunchFeedback.shared.open(link.url, for: feature) }
            }
        } label: {
            Label("Open URL", systemImage: "safari")
                .labelStyle(ActionLabelStyle(compact: compact))
        } primaryAction: {
            if let first = links.first { HostLaunchFeedback.shared.open(first.url, for: feature) }
        }
        .menuStyle(.button)
        .fixedSize()
        .disabled(links.isEmpty)
        .help(links.first.map { "Open \($0.url.absoluteString) (more in the menu)" } ?? "This feature has no URLs")
        .accessibilityIdentifier("feature.action.openURL")
    }

    private func launchButton(_ title: String, plan: HostLaunchPlan, verb: String) -> some View {
        Button(title) { HostLaunchFeedback.shared.launch(plan, for: feature) }
            .disabled(!plan.isEnabled)
            .help(plan.disabledReason ?? title)
            .accessibilityIdentifier("feature.action.\(verb)")
    }
}

/// Title and icon, or just the icon when the action row is short of room (the title stays the accessibility label).
private struct ActionLabelStyle: LabelStyle {
    let compact: Bool

    func makeBody(configuration: Configuration) -> some View {
        if compact {
            Label(configuration).labelStyle(.iconOnly)
        } else {
            Label(configuration).labelStyle(.titleAndIcon)
        }
    }
}

#Preview("Header") {
    let record = PreviewSamples.features[0]
    FeatureHeader(record: record, feature: FeatureRef(project: PreviewSamples.project, name: record.workFeature),
                  availability: FeatureActionAvailability(record: record, folderExists: true, backendReady: true,
                                                          preferences: LaunchPreferences()),
                  attention: nil)
        .padding()
        .frame(width: 760)
}
