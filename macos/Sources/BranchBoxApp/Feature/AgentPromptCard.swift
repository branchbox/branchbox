import BranchBoxKit
import BranchBoxPreview
import SwiftUI

/// The coding agent and the feature's prompt: what the CLI planned for the default agent (status and detail),
/// which agent Launch runs, and the prompt seed with [Show All], [Copy] and [Launch with Prompt].
struct AgentPromptCard: View {
    let record: FeatureRecord
    let feature: FeatureRef
    let availability: FeatureActionAvailability

    @State private var showsFullPrompt = false

    /// Lines of the prompt shown before [Show All].
    static let previewLines = 4

    var body: some View {
        FeatureCard("Coding Agent", systemImage: "sparkles") {
            if let plan = record.defaultAgent {
                Tag(title: plan.status.label, systemImage: plan.status.symbol, tint: plan.status.tint.color)
                    .help(Self.badgeHelp(plan))
                    .accessibilityLabel("Default agent: \(plan.status.label)")
            }
        } content: {
            VStack(alignment: .leading, spacing: 12) {
                agentSummary
                Divider()
                promptSection
            }
        }
    }

    private var agentSummary: some View {
        Text(Self.summary(agent: availability.preferences.agentName, plan: record.defaultAgent,
                          inContainer: record.runtime.provider == .container))
            .fixedSize(horizontal: false, vertical: true)
    }

    /// One state-dependent sentence about where and when the agent runs: what Launch does, then what happens on
    /// its own when the feature starts. The CLI's own detail is used only when it explains a problem (waiting,
    /// blocked); the command and setup hints go in the badge's help.
    static func summary(agent: String, plan: DefaultAgentPlan?, inContainer: Bool) -> String {
        let launch = "Launch \(agent) opens it in this feature's folder."
        guard let plan else { return launch }
        let place = inContainer ? " in the dev container" : ""
        switch plan.status {
        case .ready:
            return launch + " It also starts automatically\(place) when the feature starts."
        case .disabled:
            return launch + " Nothing starts automatically when the feature starts."
        case .waiting, .blocked, .unknown:
            guard let detail = plan.detail?.trimmingCharacters(in: .whitespacesAndNewlines), !detail.isEmpty else {
                return launch
            }
            return launch + " " + (detail.hasSuffix(".") ? detail : detail + ".")
        }
    }

    /// The badge's help: the command that runs automatically, or how to set it up.
    static func badgeHelp(_ plan: DefaultAgentPlan) -> String {
        if let command = plan.command, !command.isEmpty { return "Runs “\(command)” when the feature starts" }
        if let detail = plan.detail, !detail.isEmpty { return detail }
        return "Default agent: \(plan.status.label)"
    }

    @ViewBuilder private var promptSection: some View {
        if let prompt = record.promptSeed?.trimmingCharacters(in: .whitespacesAndNewlines), !prompt.isEmpty {
            VStack(alignment: .leading, spacing: 8) {
                Text("Prompt")
                    .font(.subheadline.weight(.semibold))
                Text(prompt)
                    .lineLimit(Self.previewLines)
                    .textSelection(.enabled)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(10)
                    .background(Color(nsColor: .textBackgroundColor).opacity(0.6), in: RoundedRectangle(cornerRadius: 6))
                    .overlay(RoundedRectangle(cornerRadius: 6).strokeBorder(Color(nsColor: .separatorColor).opacity(0.5)))
                FlowLayout(spacing: 8) {
                    let plan = availability.agentWithPrompt
                    Button {
                        HostLaunchFeedback.shared.launch(plan, for: feature)
                    } label: {
                        Label("Launch with Prompt", systemImage: "play.fill")
                    }
                    .disabled(!plan.isEnabled)
                    .help(plan.disabledReason ?? "Start \(availability.preferences.agentName) with this prompt")
                    .accessibilityIdentifier("feature.action.agentWithPrompt")
                    Button("Show All") { showsFullPrompt = true }
                        .popover(isPresented: $showsFullPrompt, arrowEdge: .bottom) {
                            ScrollView {
                                Text(prompt)
                                    .textSelection(.enabled)
                                    .frame(maxWidth: .infinity, alignment: .leading)
                                    .padding(16)
                            }
                            .frame(width: 420, height: 320)
                        }
                        .accessibilityIdentifier("feature.action.showPrompt")
                    CopyButton(text: prompt, label: "Copy Prompt", showsTitle: true)
                }
            }
        } else {
            Text("No prompt was given when this feature started.")
                .foregroundStyle(.secondary)
        }
    }
}

#Preview("Agent and prompt") {
    let record = PreviewSamples.features.first { $0.workFeature == "sbx-demo" } ?? PreviewSamples.features[0]
    AgentPromptCard(record: record, feature: FeatureRef(project: PreviewSamples.project, name: record.workFeature),
                    availability: FeatureActionAvailability(record: record, folderExists: true, backendReady: true,
                                                            preferences: LaunchPreferences()))
        .padding()
        .frame(width: 480)
}
