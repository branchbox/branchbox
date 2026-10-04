import AppKit
import BranchBoxStores
import SwiftUI

/// Settings › Notifications: when BranchBox notifies, and macOS permission.
struct NotificationsTab: View {
    @Environment(AppModel.self) private var model
    @State private var permission: Bool?

    static let systemSettingsURL = URL(string: "x-apple.systempreferences:com.apple.Notifications-Settings.extension")!

    var body: some View {
        @Bindable var settings = model.settings
        Form {
            if !model.notifier.isAvailable {
                Section {
                    Label {
                        VStack(alignment: .leading, spacing: 2) {
                            Text("Notifications are off in development builds")
                            SettingsCaption("They work in the installed BranchBox app. Your choices here are kept.")
                        }
                    } icon: {
                        Image(systemName: "info.circle.fill").foregroundStyle(.tint)
                    }
                }
            }
            Section {
                Toggle(isOn: $settings.notificationsEnabled) {
                    Text("Notify when operations finish")
                    Text("Only while you're in another app, and only for operations that take more than a few seconds.")
                }
                Toggle("Only when something goes wrong", isOn: $settings.notifyOnlyOnProblems)
                    .disabled(!settings.notificationsEnabled)
                Toggle(isOn: $settings.notifyAttentionChanges) {
                    Text("Notify when a feature needs attention")
                    Text("For example when an environment stops or a folder goes missing.")
                }
            } header: {
                Text("When to notify")
            }
            Section {
                LabeledContent("Permission") {
                    HStack {
                        switch permission {
                        case true?: Label("Allowed", systemImage: "checkmark.circle.fill").foregroundStyle(.green)
                        case false?: Label("Not allowed", systemImage: "xmark.circle").foregroundStyle(.secondary)
                        case nil: EmptyView()
                        }
                        Button("Allow Notifications…", action: requestPermission)
                            .disabled(!model.notifier.isAvailable)
                    }
                }
                Button("Open Notification Settings…") { NSWorkspace.shared.open(Self.systemSettingsURL) }
                    .buttonStyle(.link)
            } header: {
                Text("macOS")
            }
        }
        .formStyle(.grouped)
        .frame(width: AppSettingsView.tabWidth)
    }

    private func requestPermission() {
        let notifier = model.notifier
        Task { permission = await notifier.requestAuthorizationIfNeeded() }
    }
}
