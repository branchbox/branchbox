import AppKit
import BranchBoxKit
import BranchBoxStores
import SwiftUI

/// Settings › Editors & Terminal: the editor features open in (and whether as a folder or a dev container), and
/// the terminal app or command.
struct EditorsTerminalTab: View {
    enum EditorKind: String, Hashable { case vscode, cursor, custom }
    enum TerminalKind: String, Hashable { case terminal, iTerm, custom }

    @Environment(AppModel.self) private var model
    @State private var template = ""

    var body: some View {
        @Bindable var settings = model.settings
        Form {
            Section {
                Picker("Open features in", selection: editorBinding) {
                    Text(Self.appTitle("Visual Studio Code", bundleID: HostLaunchPlan.vscodeBundleID)).tag(EditorKind.vscode)
                    Text(Self.appTitle("Cursor", bundleID: HostLaunchPlan.cursorBundleID)).tag(EditorKind.cursor)
                    Text(customEditorTitle).tag(EditorKind.custom)
                }
                if case .custom(let path) = settings.preferredEditor {
                    LabeledContent("App") {
                        HStack {
                            Text(path.isEmpty ? "None chosen" : path)
                                .lineLimit(1)
                                .truncationMode(.middle)
                                .foregroundStyle(.secondary)
                            Button("Choose…", action: chooseEditor)
                        }
                    }
                }
                Picker("Open as", selection: $settings.editorOpenMode) {
                    Text("Folder").tag(EditorOpenMode.folder)
                    Text("Dev Container").tag(EditorOpenMode.devContainer)
                }
                .pickerStyle(.segmented)
                SettingsCaption(settings.editorOpenMode == .folder
                                ? "Opens the feature's folder on this Mac."
                                : "Reopens the feature inside its container (VS Code and Cursor, container runtime only).")
            } header: {
                Text("Editor")
            }
            Section {
                Picker("Terminal", selection: terminalBinding) {
                    Text("Terminal").tag(TerminalKind.terminal)
                    Text(Self.appTitle("iTerm", bundleID: HostLauncher.iTermBundleID)).tag(TerminalKind.iTerm)
                    Text("Custom command").tag(TerminalKind.custom)
                }
                if terminalBinding.wrappedValue == .custom {
                    LabeledContent("Command") {
                        TextField("Command", text: $template, prompt: Text("open -a Ghostty {command}"))
                            .textFieldStyle(.roundedBorder)
                            .font(.body.monospaced())
                            .labelsHidden()
                            .frame(width: 300)
                            .onSubmit(commitTemplate)
                            .onChange(of: template) { _, _ in commitTemplate() }
                    }
                    if let problem = Self.templateProblem(template) {
                        Label(problem, systemImage: "exclamationmark.circle.fill").font(.caption).foregroundStyle(.red)
                    } else {
                        SettingsCaption("{command} is the script to run and {path} the feature's folder; both are quoted for you.")
                    }
                }
            } header: {
                Text("Terminal")
            }
        }
        .formStyle(.grouped)
        .frame(width: AppSettingsView.tabWidth)
        .onAppear {
            if case .custom(let current) = model.settings.preferredTerminal { template = current }
        }
    }

    /// "Cursor" or "Cursor (not installed)".
    static func appTitle(_ name: String, bundleID: String) -> String {
        NSWorkspace.shared.urlForApplication(withBundleIdentifier: bundleID) == nil ? "\(name) (not installed)" : name
    }

    static func templateProblem(_ template: String) -> String? {
        if template.trimmingCharacters(in: .whitespaces).isEmpty { return "Enter the command that opens your terminal" }
        return template.contains("{command}") ? nil : "The command must include {command}"
    }

    private var customEditorTitle: String {
        if case .custom(let path) = model.settings.preferredEditor, !path.isEmpty {
            return FileManager.default.displayName(atPath: path)
        }
        return "Other App…"
    }

    private var editorBinding: Binding<EditorKind> {
        Binding(get: {
            switch model.settings.preferredEditor {
            case .vscode: .vscode
            case .cursor: .cursor
            case .custom: .custom
            }
        }, set: { kind in
            switch kind {
            case .vscode: model.settings.preferredEditor = .vscode
            case .cursor: model.settings.preferredEditor = .cursor
            case .custom: chooseEditor()
            }
        })
    }

    private func chooseEditor() {
        let panel = NSOpenPanel()
        panel.title = "Choose an Editor"
        panel.prompt = "Choose"
        panel.allowedContentTypes = [.application]
        panel.directoryURL = URL(fileURLWithPath: "/Applications")
        guard panel.runModal() == .OK, let url = panel.url else { return }
        model.settings.preferredEditor = .custom(appPath: url.path)
    }

    private var terminalBinding: Binding<TerminalKind> {
        Binding(get: {
            switch model.settings.preferredTerminal {
            case .terminal: .terminal
            case .iTerm: .iTerm
            case .custom: .custom
            }
        }, set: { kind in
            switch kind {
            case .terminal: model.settings.preferredTerminal = .terminal
            case .iTerm: model.settings.preferredTerminal = .iTerm
            case .custom: model.settings.preferredTerminal = .custom(template: template)
            }
        })
    }

    private func commitTemplate() {
        guard case .custom(let current) = model.settings.preferredTerminal, current != template else { return }
        model.settings.preferredTerminal = .custom(template: template)
    }
}
