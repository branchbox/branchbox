import BranchBoxKit
import BranchBoxStores
import SwiftUI

/// Settings › Coding Agent: which agent Launch Agent starts, its name, and whether it gets the feature's prompt.
/// A project's own `editor.default_agent` (Project Settings) takes precedence.
struct CodingAgentTab: View {
    enum Choice: String, CaseIterable, Hashable { case claude, codex, custom }

    @Environment(AppModel.self) private var model
    @State private var rebootstrapper = SettingsRebootstrapper()
    @State private var customCommand = ""

    var body: some View {
        @Bindable var settings = model.settings
        Form {
            Section {
                Picker("Agent", selection: choiceBinding) {
                    Text("Claude Code").tag(Choice.claude)
                    Text("Codex").tag(Choice.codex)
                    Text("Custom command").tag(Choice.custom)
                }
                if choiceBinding.wrappedValue == .custom {
                    LabeledContent("Command") {
                        TextField("Command", text: $customCommand, prompt: Text("aider --model sonnet"))
                            .textFieldStyle(.roundedBorder)
                            .font(.body.monospaced())
                            .labelsHidden()
                            .frame(width: 280)
                            .onSubmit(commitCustom)
                            .onChange(of: customCommand) { _, _ in commitCustom() }
                    }
                }
                LabeledContent("Name shown in BranchBox") {
                    TextField("Name shown in BranchBox", text: displayNameBinding, prompt: Text(defaultName))
                        .textFieldStyle(.roundedBorder)
                        .labelsHidden()
                        .frame(width: 280)
                }
                Toggle(isOn: $settings.passPromptToAgent) {
                    Text("Give the agent the feature's prompt")
                    Text("When a feature was started with a prompt, the agent begins with it.")
                }
            } header: {
                Text("Launch Agent")
            } footer: {
                SettingsCaption("A project can choose its own agent in Project Settings › Coding Agent; that choice wins.")
            }
        }
        .formStyle(.grouped)
        .frame(width: AppSettingsView.tabWidth)
        .onAppear {
            if case .custom(let command) = model.settings.agentChoice { customCommand = command }
        }
    }

    private var defaultName: String {
        switch model.settings.agentChoice {
        case .claude: "Claude Code"
        case .codex: "Codex"
        case .custom: "Agent"
        }
    }

    private var choiceBinding: Binding<Choice> {
        Binding(get: {
            switch model.settings.agentChoice {
            case .claude: .claude
            case .codex: .codex
            case .custom: .custom
            }
        }, set: { choice in
            switch choice {
            case .claude: model.settings.agentChoice = .claude
            case .codex: model.settings.agentChoice = .codex
            case .custom: model.settings.agentChoice = .custom(command: customCommand)
            }
            rebootstrapper.now(model.environment)
        })
    }

    private var displayNameBinding: Binding<String> {
        Binding(get: { model.settings.agentDisplayName ?? "" }, set: { name in
            model.settings.agentDisplayName = name.isEmpty ? nil : name
            rebootstrapper.schedule(model.environment)
        })
    }

    private func commitCustom() {
        guard case .custom(let current) = model.settings.agentChoice, current != customCommand else { return }
        model.settings.agentChoice = .custom(command: customCommand)
        rebootstrapper.schedule(model.environment)
    }
}
