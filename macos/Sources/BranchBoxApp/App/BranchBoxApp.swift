import AppKit
import BranchBoxKit
import BranchBoxStores
import SwiftUI

/// The app's scenes (D-7): the single main window, the menu bar extra, Settings, the Run Command windows, and the
/// Activity and Diagnostics windows. All of them read the one `AppModel` from the environment; `CompositionRoot`
/// builds it and `AppDelegate` starts it at launch.
@main
struct BranchBoxApp: App {
    @NSApplicationDelegateAdaptor(AppDelegate.self) private var appDelegate
    @State private var model: AppModel = CompositionRoot.model
    private let buildBadge = CompositionRoot.buildBadge(environment: ProcessInfo.processInfo.environment)

    var body: some Scene {
        Window("BranchBox", id: SceneID.main) {
            MainWindow(buildBadge: buildBadge)
                .environment(model)
        }
        .defaultSize(width: 1100, height: 720)
        .commands {
            SidebarCommands()
            TextEditingCommands()
            AppCommands(model: model)
        }

        MenuBarExtra(isInserted: menuBarInserted) {
            MenuBarContent()
                .environment(model)
        } label: {
            MenuBarLabel()
                .environment(model)
        }
        .menuBarExtraStyle(.menu)

        Settings {
            AppSettingsView()
                .environment(model)
        }

        WindowGroup("Run Command", id: SceneID.run, for: FeatureRef.self) { $feature in
            RunCommandWindow(feature: feature)
                .environment(model)
        }

        Window("Activity", id: SceneID.activity) {
            ActivityWindow()
                .environment(model)
        }

        Window("Diagnostics", id: SceneID.diagnostics) {
            DiagnosticsWindow()
                .environment(model)
        }
    }

    /// Settings › General › Show menu bar icon.
    private var menuBarInserted: Binding<Bool> {
        let settings = model.settings
        return Binding(get: { settings.showMenuBarIcon }, set: { settings.showMenuBarIcon = $0 })
    }

    /// The bootstrapper `environment` selects (kept from SW-0 for tests; `CompositionRoot` owns the choice).
    @MainActor static func makeBootstrapper(environment: [String: String] = ProcessInfo.processInfo.environment)
        -> any BackendBootstrapping {
        CompositionRoot.makeBootstrapper(environment: environment)
    }
}
