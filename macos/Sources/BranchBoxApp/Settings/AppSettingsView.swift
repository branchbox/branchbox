import BranchBoxKit
import BranchBoxStores
import SwiftUI

/// The Settings scene: General, Tools, Editors & Terminal, Coding Agent, Notifications, Refresh and Advanced
/// tabs, each a grouped form (macOS Settings conventions). Changes that affect the CLI rebuild the backend
/// environment at once through `environment.rebootstrap()`; nothing needs a relaunch.
struct AppSettingsView: View {
    enum Tab: String, CaseIterable, Hashable {
        case general, tools, editors, agent, notifications, refresh, advanced
    }

    @State private var selection: Tab = .general

    init() {}

    var body: some View {
        TabView(selection: $selection) {
            GeneralTab()
                .tabItem { Label("General", systemImage: "gearshape") }
                .tag(Tab.general)
            ToolsTab()
                .tabItem { Label("Tools", systemImage: "wrench.and.screwdriver") }
                .tag(Tab.tools)
            EditorsTerminalTab()
                .tabItem { Label("Editors & Terminal", systemImage: "chevron.left.forwardslash.chevron.right") }
                .tag(Tab.editors)
            CodingAgentTab()
                .tabItem { Label("Coding Agent", systemImage: "sparkles") }
                .tag(Tab.agent)
            NotificationsTab()
                .tabItem { Label("Notifications", systemImage: "bell.badge") }
                .tag(Tab.notifications)
            RefreshTab()
                .tabItem { Label("Refresh", systemImage: "arrow.clockwise") }
                .tag(Tab.refresh)
            AdvancedTab()
                .tabItem { Label("Advanced", systemImage: "slider.horizontal.3") }
                .tag(Tab.advanced)
        }
        .frame(width: 620)
        .scenePadding(.minimum)
    }

    /// The width every tab lays out at.
    static let tabWidth: CGFloat = 620
}

/// Rebootstraps the backend shortly after the last of a burst of changes (typing in a field), or at once.
@MainActor final class SettingsRebootstrapper {
    private var pending: Task<Void, Never>?

    func schedule(_ environment: EnvironmentStore, after delay: Duration = .milliseconds(600)) {
        pending?.cancel()
        pending = Task {
            do {
                try await Task.sleep(for: delay)
            } catch {
                return                                           // a newer change restarted the wait
            }
            await environment.rebootstrap()
        }
    }

    func now(_ environment: EnvironmentStore) {
        pending?.cancel()
        pending = Task { await environment.rebootstrap() }
    }
}

/// A caption under a form row.
struct SettingsCaption: View {
    let text: String
    init(_ text: String) { self.text = text }

    var body: some View {
        Text(text)
            .font(.caption)
            .foregroundStyle(.secondary)
            .fixedSize(horizontal: false, vertical: true)
    }
}
