import AppKit
import BranchBoxKit
import BranchBoxStores
import SwiftUI

/// Settings › Advanced: verbose CLI logs, log retention, the logs folder, the onboarding checklist and which
/// backend runs operations.
struct AdvancedTab: View {
    @Environment(AppModel.self) private var model
    @Environment(\.openWindow) private var openWindow
    @State private var rebootstrapper = SettingsRebootstrapper()

    static let retentionChoices = [25, 50, 100, 200, 500]

    var body: some View {
        @Bindable var settings = model.settings
        Form {
            Section {
                Toggle(isOn: $settings.verboseLogs) {
                    Text("Verbose command logs")
                    Text("Adds branchbox's debug output to every operation log. Useful when reporting a problem.")
                }
                Picker("Keep logs of", selection: $settings.logRetention) {
                    ForEach(Self.retentionChoices, id: \.self) { Text("The last \($0) operations").tag($0) }
                    if !Self.retentionChoices.contains(settings.logRetention) {
                        Text("The last \(settings.logRetention) operations").tag(settings.logRetention)
                    }
                }
                LabeledContent("Log files") {
                    Button("Show in Finder", action: revealLogs)
                }
            } header: {
                Text("Logs")
            }
            Section {
                LabeledContent("Onboarding") {
                    Button("Show Welcome Checklist") {
                        model.post(.select(.welcome))
                        openWindow(id: SceneID.main)
                    }
                }
                LabeledContent("Backend") {
                    Text(backendLabel).foregroundStyle(.secondary)
                }
            } header: {
                Text("App")
            }
        }
        .formStyle(.grouped)
        .frame(width: AppSettingsView.tabWidth)
        .onChange(of: settings.verboseLogs) { _, _ in rebootstrapper.now(model.environment) }
    }

    private var backendLabel: String {
        if case .preview? = model.environment.identity?.kind { return "Preview (sample data)" }
        return "BranchBox CLI"
    }

    private func revealLogs() {
        AdvancedTab.revealLogsFolder()
    }

    /// The operation logs folder, or the nearest folder above it that exists.
    static func revealLogsFolder() {
        var folder = AppModel.Configuration.standard.logsDirectory
        while !FileManager.default.fileExists(atPath: folder.path), folder.pathComponents.count > 1 {
            folder.deleteLastPathComponent()
        }
        NSWorkspace.shared.activateFileViewerSelecting([folder])
    }
}
