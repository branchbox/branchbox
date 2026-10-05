import AppKit
import BranchBoxKit
import BranchBoxPreview
import BranchBoxStores
import SwiftUI

/// The frame every project sheet shares: a header with a symbol, title and one-line explanation, the content, and
/// a footer bar whose trailing button is the default action (HIG: the primary action sits rightmost).
struct ProjectSheetScaffold<Content: View, Footer: View>: View {
    let title: String
    var subtitle: String?
    var systemImage: String
    @ViewBuilder var content: Content
    @ViewBuilder var footer: Footer

    var body: some View {
        VStack(spacing: 0) {
            HStack(alignment: .center, spacing: 12) {
                Image(systemName: systemImage)
                    .font(.system(size: 20, weight: .medium))
                    .foregroundStyle(.tint)
                    .frame(width: 40, height: 40)
                    .background(.tint.opacity(0.12), in: RoundedRectangle(cornerRadius: 10))
                    .accessibilityHidden(true)
                VStack(alignment: .leading, spacing: 2) {
                    Text(title)
                        .font(.title3.weight(.semibold))
                        .accessibilityAddTraits(.isHeader)
                    if let subtitle {
                        Text(subtitle)
                            .font(.callout)
                            .foregroundStyle(.secondary)
                            .lineLimit(2)
                            .truncationMode(.middle)
                            .fixedSize(horizontal: false, vertical: true)
                    }
                }
                Spacer(minLength: 0)
            }
            .padding(.horizontal, 20)
            .padding(.vertical, 16)
            Divider()
            content
                .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .top)
            Divider()
            HStack(spacing: 8) {
                footer
            }
            .controlSize(.regular)
            .padding(.horizontal, 20)
            .padding(.vertical, 12)
        }
    }
}

/// A tinted note inside a form or sheet: info, warning or success, never colour alone (each has a symbol).
struct ProjectNotice<Actions: View>: View {
    enum Style {
        case info, warning, success, error

        var symbol: String {
            switch self {
            case .info: "info.circle.fill"
            case .warning: "exclamationmark.triangle.fill"
            case .success: "checkmark.circle.fill"
            case .error: "xmark.octagon.fill"
            }
        }

        var tint: Color {
            switch self {
            case .info: .accentColor
            case .warning: .orange
            case .success: .green
            case .error: .red
            }
        }
    }

    let style: Style
    let title: String
    var message: String?
    @ViewBuilder var actions: Actions

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: 10) {
            Image(systemName: style.symbol)
                .foregroundStyle(style.tint)
                .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: 6) {
                Text(title).font(.headline)
                if let message {
                    Text(message)
                        .font(.callout)
                        .foregroundStyle(.secondary)
                        .textSelection(.enabled)
                        .fixedSize(horizontal: false, vertical: true)
                }
                let actionsView = HStack(spacing: 8) { actions }
                actionsView
            }
            Spacer(minLength: 0)
        }
        .padding(12)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(style.tint.opacity(0.08), in: RoundedRectangle(cornerRadius: 10))
        .overlay(RoundedRectangle(cornerRadius: 10).strokeBorder(style.tint.opacity(0.25)))
        .accessibilityElement(children: .contain)
    }
}

extension ProjectNotice where Actions == EmptyView {
    init(style: Style, title: String, message: String? = nil) {
        self.init(style: style, title: title, message: message) { EmptyView() }
    }
}

/// A small capsule with a title and value, e.g. "Stack Rails".
struct ProjectInfoChip: View {
    let title: String
    let value: String
    var systemImage: String?

    var body: some View {
        HStack(spacing: 4) {
            if let systemImage {
                Image(systemName: systemImage).foregroundStyle(.secondary).accessibilityHidden(true)
            }
            Text(title).foregroundStyle(.secondary)
            Text(value).fontWeight(.medium)
        }
        .font(.caption)
        .lineLimit(1)
        .padding(.horizontal, 8)
        .padding(.vertical, 4)
        .background(.quaternary.opacity(0.6), in: Capsule())
        .accessibilityElement(children: .ignore)
        .accessibilityLabel("\(title): \(value)")
    }
}

/// Opening folders, files and Terminal for project surfaces. Nothing here writes inside a repository: Terminal
/// gets a script in the app's caches folder (`HostLauncher`).
@MainActor enum ProjectActions {
    /// An open panel for one folder; nil when cancelled.
    static func chooseFolder(title: String, prompt: String, startingAt directory: URL? = nil) -> URL? {
        let panel = NSOpenPanel()
        panel.title = title
        panel.prompt = prompt
        panel.canChooseDirectories = true
        panel.canChooseFiles = false
        panel.allowsMultipleSelection = false
        panel.canCreateDirectories = false
        panel.directoryURL = directory
        return panel.runModal() == .OK ? panel.url : nil
    }

    /// An open panel for one executable file (Settings › Tools › Locate…); nil when cancelled.
    static func chooseExecutable(title: String, startingAt directory: URL?) -> URL? {
        let panel = NSOpenPanel()
        panel.title = title
        panel.prompt = "Use This CLI"
        panel.canChooseDirectories = false
        panel.canChooseFiles = true
        panel.allowsMultipleSelection = false
        panel.treatsFilePackagesAsDirectories = true
        panel.showsHiddenFiles = true
        panel.directoryURL = directory
        return panel.runModal() == .OK ? panel.url : nil
    }

    static func reveal(_ path: String) {
        NSWorkspace.shared.activateFileViewerSelecting([URL(fileURLWithPath: path)])
    }

    static func open(_ path: String) {
        NSWorkspace.shared.open(URL(fileURLWithPath: path))
    }

    /// The user's login shell in `folder`, in their chosen terminal.
    static func openTerminal(at folder: String, terminal: TerminalChoice) async throws {
        let plan = HostLaunchPlan(kind: .terminalScript(script: HostLaunchPlan.script(cd: folder, exec: loginShell),
                                                        terminal: terminal),
                                  workingDirectory: folder)
        try await HostLauncher().launch(plan)
    }

    /// Runs `command` (shell text) in the user's terminal, e.g. `sbx login`.
    static func runInTerminal(_ command: String, workingDirectory: String? = nil, terminal: TerminalChoice) async throws {
        let plan = HostLaunchPlan(kind: .terminalScript(script: HostLaunchPlan.script(cd: workingDirectory, exec: command),
                                                        terminal: terminal),
                                  workingDirectory: workingDirectory)
        try await HostLauncher().launch(plan)
    }

    /// The login shell, expanded by the script's own `sh`.
    private static let loginShell = "\"${SHELL:-/bin/zsh}\" -l"
}

/// An `AppModel` on `PreviewBackend` for Xcode previews of the project, settings and diagnostics screens. It
/// stores nothing outside a temporary folder and starts with the sample project added.
@MainActor enum ProjectsPreviewModel {
    static func model(_ scenario: PreviewScenario = .contract) -> AppModel {
        let directory = FileManager.default.temporaryDirectory
            .appendingPathComponent("branchbox-previews", isDirectory: true)
            .appendingPathComponent(scenario.name, isDirectory: true)
        let defaults = UserDefaults(suiteName: directory.appendingPathComponent("settings").path) ?? .standard
        let model = AppModel(settings: AppSettings(defaults: defaults), bootstrapper: PreviewBootstrapper(scenario: scenario),
                             notifier: NoopNotifier(), configuration: .isolated(in: directory))
        Task {
            await model.start()
            _ = await model.projects.add(folder: PreviewSamples.project.root)
        }
        return model
    }
}
