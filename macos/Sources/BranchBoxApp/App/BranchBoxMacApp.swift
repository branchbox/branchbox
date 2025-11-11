#if canImport(SwiftUI)
import SwiftUI
#if os(macOS)
import AppKit
import UserNotifications

final class AppDelegate: NSObject, NSApplicationDelegate {
    func applicationDidFinishLaunching(_ notification: Notification) {
        // Bring the app to the foreground when launched via `swift run`.
        NSApp.activate(ignoringOtherApps: true)

        // Request local notification permission for completion toasts.
        UNUserNotificationCenter.current().requestAuthorization(options: [.alert, .sound, .badge]) { _, _ in }
    }
}
#endif

@main
struct BranchBoxMacApp: App {
    @StateObject private var viewModel = FeatureListViewModel()
    #if os(macOS)
    @NSApplicationDelegateAdaptor(AppDelegate.self) var appDelegate
    #endif

    var body: some Scene {
        // Main window: split-view shell with Home, Features, Agent, Settings
        WindowGroup {
            MainAppView()
                .environmentObject(viewModel)
        }
        .commands {
            CommandGroup(after: .appInfo) {
                Button("Refresh Features") {
                    viewModel.refresh()
                }
                .keyboardShortcut("r", modifiers: [.command])

                Button("Start Feature") {
                    viewModel.commandStartRequested.toggle()
                }
                .keyboardShortcut("n", modifiers: [.command])

                Button("Command Palette…") {
                    viewModel.isCommandPalettePresented.toggle()
                }
                .keyboardShortcut("k", modifiers: [.command])
            }
        }

        // Native Settings window (⌘,)
        #if os(macOS)
        Settings {
            SettingsView().environmentObject(viewModel)
        }
        #endif

        // Menu bar companion for quick actions
        #if os(macOS)
        MenuBarExtra("BranchBox", systemImage: "shippingbox") {
            StatusMenuView()
                .environmentObject(viewModel)
        }
        .menuBarExtraStyle(.window)
        #endif
    }
}
#else
@main
struct BranchBoxMacApp {
    static func main() {
        fatalError("BranchBoxApp requires macOS 13+ and SwiftUI")
    }
}
#endif
