import AppKit
import BranchBoxStores
import SwiftUI

/// Settings › General: where BranchBox shows up (menu bar, Dock; one of the two always stays on, so the app can
/// always be reached), whether the menu bar icon shows the attention count, and whether the main window reopens
/// on what it showed last.
struct GeneralTab: View {
    /// Persisted next to the other preferences; SW-4 applies it at launch (`NSApp.setActivationPolicy`).
    static let showDockIconKey = "settings.showDockIcon"
    /// The menu bar icon shows how many items need attention next to its badge (default on).
    static let menuBarShowsCountKey = "settings.menuBarShowsCount"
    /// The main window reopens on the project or feature it showed last (default on).
    static let reopenLastSelectionKey = "settings.reopenLastSelection"

    @Environment(AppModel.self) private var model
    @AppStorage(GeneralTab.showDockIconKey, store: AppSettings.defaultsForCurrentProcess()) private var showDockIcon = true
    @AppStorage(GeneralTab.menuBarShowsCountKey, store: AppSettings.defaultsForCurrentProcess()) private var menuBarShowsCount = true
    @AppStorage(GeneralTab.reopenLastSelectionKey, store: AppSettings.defaultsForCurrentProcess())
    private var reopenLastSelection = true

    var body: some View {
        @Bindable var settings = model.settings
        Form {
            Section {
                Toggle(isOn: $settings.showMenuBarIcon) {
                    Text("Show BranchBox in the menu bar")
                    Text("Your features, their status and quick actions, one click away.")
                }
                .disabled(settings.showMenuBarIcon && !showDockIcon)
                Toggle(isOn: $showDockIcon) {
                    Text("Show BranchBox in the Dock")
                    Text("When hidden, open BranchBox from its menu bar icon.")
                }
                .disabled(showDockIcon && !settings.showMenuBarIcon)
                Toggle(isOn: $menuBarShowsCount) {
                    Text("Show the attention count on the menu bar icon")
                    Text("The icon always gets a badge when something needs attention; this adds the number.")
                }
                .disabled(!settings.showMenuBarIcon)
            } header: {
                Text("Appearance")
            } footer: {
                if !settings.showMenuBarIcon || !showDockIcon {
                    SettingsCaption("BranchBox needs at least one of these so you can always reach it.")
                }
            }
            Section {
                Toggle(isOn: $reopenLastSelection) {
                    Text("Reopen the last project at launch")
                    Text("When off, BranchBox opens on the first project in the sidebar.")
                }
            } header: {
                Text("Startup")
            }
        }
        .formStyle(.grouped)
        .frame(width: AppSettingsView.tabWidth)
        .onChange(of: showDockIcon) { _, show in
            NSApplication.shared.setActivationPolicy(show ? .regular : .accessory)
        }
    }
}
