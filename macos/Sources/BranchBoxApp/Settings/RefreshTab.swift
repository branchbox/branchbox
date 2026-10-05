import BranchBoxKit
import BranchBoxStores
import SwiftUI

/// Settings › Refresh: how often projects are re-read, and whether file changes trigger a refresh.
struct RefreshTab: View {
    @Environment(AppModel.self) private var model

    static let intervals: [RefreshInterval] = [.s30, .m1, .m5, .m15, .manual]

    var body: some View {
        @Bindable var settings = model.settings
        Form {
            Section {
                Picker("Project you're viewing", selection: $settings.selectedProjectRefresh) {
                    ForEach(Self.intervals, id: \.self) { Text(Self.label($0)).tag($0) }
                }
                Picker("Other projects", selection: $settings.otherProjectsRefresh) {
                    ForEach(Self.intervals, id: \.self) { Text(Self.label($0)).tag($0) }
                }
            } header: {
                Text("Check for changes")
            } footer: {
                SettingsCaption("BranchBox also refreshes when you switch back to it, after every operation, and when you press ⌘R.")
            }
            Section {
                Toggle(isOn: $settings.watchProjectFiles) {
                    Text("Watch project files")
                    Text("Notices features started or removed from Terminal within a second.")
                }
            }
        }
        .formStyle(.grouped)
        .frame(width: AppSettingsView.tabWidth)
    }

    static func label(_ interval: RefreshInterval) -> String {
        switch interval {
        case .s30: "Every 30 seconds"
        case .m1: "Every minute"
        case .m5: "Every 5 minutes"
        case .m15: "Every 15 minutes"
        case .manual: "Only when asked"
        }
    }
}
